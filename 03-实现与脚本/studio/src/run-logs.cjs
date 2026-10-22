const fs=require('node:fs');
const path=require('node:path');
const {EventEmitter}=require('node:events');
const {RunLogStore}=require('./run-log-store.cjs');
const {decodeOutput,parsePacket,OutputParser}=require('./log-protocol.cjs');
const {terminalName}=require('./terminal.cjs');
const quote=text=>"'"+text.replace(/'/g,"'\\''")+"'";
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));

function exec(client,command,timeout=12000){
  return new Promise((resolve,reject)=>{
    let settled=false,channel,out='',err='';
    const finish=(error,value)=>{if(settled)return;settled=true;clearTimeout(timer);client.off('close',closed);error?reject(error):resolve(value);};
    const closed=()=>finish(new Error('设备连接已中断。'));
    const timer=setTimeout(()=>{finish(new Error('日志终端准备超时。'));channel?.close();},timeout);client.once('close',closed);
    try{client.exec(command,(error,stream)=>{
      if(settled){stream?.close();return;}if(error)return finish(error);channel=stream;stream.setEncoding('utf8');stream.stderr.setEncoding('utf8');
      stream.on('data',data=>{out=(out+data).slice(-250000);});stream.stderr.on('data',data=>{err=(err+data).slice(-2000);});stream.on('error',finish);
      stream.on('close',code=>finish(code===0?null:new Error(err||out||'设备端日志组件准备失败。'),out.trim()));
    });}catch(error){finish(error);}
  });
}
class RunLogs extends EventEmitter {
  constructor(session,{root,settingsFile,sessionName=profile=>terminalName(profile)+'-logs'}){
    super();this.session=session;this.settingsFile=settingsFile;this.settings={root,projects:{}};this.sessionName=sessionName;this.serial=0;this.segment=0;
    let problem;
    if(fs.existsSync(settingsFile)){try{const data=JSON.parse(fs.readFileSync(settingsFile,'utf8'));if(!data||typeof data.root!=='string'||!path.isAbsolute(data.root)||!data.projects||typeof data.projects!=='object'||Array.isArray(data.projects))throw Error('Invalid log settings');this.settings={...this.settings,...data};}catch{problem='日志设置读取失败，当前使用默认目录；原日志文件保留。';}}
    this.state={phase:problem?'error':'idle',message:problem||'连接终端后自动记录前台命令。'};this.useStore(this.settings.root);
    session.on('state',state=>{if(state.ssh!=='connected'&&this.channel)this.detach('SSH 连接已关闭，未收到的输出会保留为缺失。');});
  }
  useStore(root){
    this.store=new RunLogStore(root);this.store.on('change',()=>this.emit('change'));this.store.on('notice',()=>this.update({}));this.store.on('fault',error=>{this.detach('本地日志写入失败：'+error.message);this.update({phase:'error',message:'本地日志写入失败：'+error.message+'。终端仍可继续使用。'});});
  }
  key(){return this.session.profile?terminalName(this.session.profile):'';}
  snapshot(){return {...this.state,notice:this.store.notice||'',root:this.store.root,project:this.settings.projects[this.key()]||'',active:this.store.writers.size};}
  update(patch){Object.assign(this.state,patch);this.emit('state',this.snapshot());}
  saveSettings(){fs.mkdirSync(path.dirname(this.settingsFile),{recursive:true});const temp=this.settingsFile+'.tmp';fs.writeFileSync(temp,JSON.stringify(this.settings,null,2),{mode:0o600});fs.renameSync(temp,this.settingsFile);}
  setProject(value){
    if(!this.key())throw new Error('请先连接设备，再指定日志项目。');
    const project=String(value).trim();if(project.length>100||/[\x00-\x1f]/.test(project))throw new Error('项目名最多 100 字，不能包含控制字符。');
    this.settings.projects[this.key()]=project;this.saveSettings();this.update({});return this.snapshot();
  }
  setRoot(root){
    if(this.store.writers.size||this.store.pending.size)throw new Error('请等待当前记录完成，或先断开设备再更换日志位置。');
    const stat=fs.statSync(root);if(!stat.isDirectory())throw new Error('请选择本地文件夹。');
    this.settings.root=path.resolve(root);this.useStore(this.settings.root);this.saveSettings();this.update({});this.emit('change');return this.snapshot();
  }
  sourceKey(event){return this.key()+':'+event.shell+':'+event.sequence;}
  options(){return {device:this.key(),override:this.settings.projects[this.key()]||''};}
  handleMarker(event,recovered=false){
    if(event.kind==='ready'){this.context={...event,projectRoot:''};this.contextOverride=this.settings.projects[this.key()]||'';return;}
    this.context=event;if(event.kind==='begin')this.contextOverride=this.settings.projects[this.key()]||'';
    try{
      const key=this.sourceKey(event);
      if(event.kind==='begin'){
        if(this.current?.meta.sourceKey===key)return;
        if(this.current)this.store.finish(this.current,null,{interrupted:true,reason:'未收到上一命令的结束标记。'});
        this.current=this.store.begin(event,{...this.options(),recovered});
      }else if(this.current?.meta.sourceKey===key){
        this.store.finish(this.current,event);this.current=null;
      }else if(recovered)this.store.recoverEnded(event,this.options());
    }catch(error){this.detach('日志记录失败：'+error.message);this.update({phase:'error',message:'日志记录失败：'+error.message+'。终端仍可继续使用。'});}
  }
  detach(reason='日志连接已关闭。'){
    this.serial++;
    const channel=this.channel;this.channel=null;this.parser?.flush();this.parser=null;
    if(this.current){this.store.finish(this.current,null,{interrupted:true,reason});this.current=null;}
    if(this.sessionWriter){this.store.finish(this.sessionWriter,null,{interrupted:true,reason});this.sessionWriter=null;}
    if(channel){channel.end();channel.close();}
    this.update({phase:'idle',message:reason});
  }
  async prepare(client,profile){
    const generation=this.session.generation,name=this.sessionName(profile);
    if(this.channel&&this.target?.session===name)return `exec tmux -L wcs-terminal -f /dev/null attach-session -t ${name}`;
    this.detach();this.context=null;this.contextOverride='';const serial=this.serial;this.update({phase:'connecting',message:'正在准备本地运行记录…'});
    const source=fs.readFileSync(path.join(__dirname,'log-shell.bash'),'utf8').replace(/\r\n/g,'\n');
    const bootstrap=fs.readFileSync(path.join(__dirname,'log-bootstrap.py'),'utf8');
    const code=`import sys,base64;sys.argv=['logs',${JSON.stringify(JSON.stringify({session:name,source}))}];exec(base64.b64decode('${Buffer.from(bootstrap).toString('base64')}'))`;
    let info;
    try{info=JSON.parse(await exec(client,'python3 -c '+quote(code)));}
    catch(error){if(serial===this.serial)this.update({phase:'error',message:'日志终端准备失败：'+error.message});throw error;}
    if(info.session!==name||!/^%\d+$/.test(info.pane))throw new Error('日志终端身份校验失败。');
    if(generation!==this.session.generation||this.session.client!==client||serial!==this.serial)throw new Error('连接已变化，请重新打开终端。');
    this.target=info;
    this.parser=new OutputParser();this.parser.on('text',text=>{
      this.current?.append(text);
      try{
        const context=this.context||{cwd:'',projectRoot:''},override=this.contextOverride||'',group=override||context.projectRoot||context.cwd;
        if(this.sessionWriter?.group!==group&&this.sessionWriter){this.store.finish(this.sessionWriter,{code:null});this.sessionWriter=null;}
        if(!this.sessionWriter){this.sessionWriter=this.store.begin({...context,shell:'session-'+Date.now(),sequence:String(++this.segment),command:'[终端会话输出]'},{...this.options(),override,kind:'session'});if(this.sessionWriter)this.sessionWriter.group=group;}
        this.sessionWriter?.append(text);
      }catch(error){this.detach('会话日志写入失败：'+error.message);this.update({phase:'error',message:error.message});}
    });this.parser.on('marker',event=>this.handleMarker(event));
    try{
      await this.observe(client,info,generation,serial);
      let packet=await exec(client,`tmux -L wcs-terminal show-option -wqv -t ${info.pane} @wcs_run | base64 -d`);
      for(let n=0;n<24&&!parsePacket(packet);n++){
        if(this.session.client!==client||generation!==this.session.generation||serial!==this.serial)throw new Error('设备连接已关闭。');
        packet=await exec(client,`tmux -L wcs-terminal show-option -wqv -t ${info.pane} @wcs_run | base64 -d`);if(!parsePacket(packet))await delay(200);
      }
      if(serial!==this.serial||generation!==this.session.generation)throw new Error('日志连接已取消。');
      const state=parsePacket(packet);if(!state)throw new Error('日志 Shell 状态未就绪，请重新连接终端。');
      this.handleMarker(state,true);
      if(this.state.phase!=='error')this.update({phase:'ready',message:'自动记录已就绪，日志保存在电脑本地。'});
    }catch(error){if(serial!==this.serial)throw error;this.detach(error.message);this.update({phase:'error',message:'自动记录未就绪：'+error.message+'。本次终端仍可使用。'});}
    return `exec tmux -L wcs-terminal -f /dev/null attach-session -t ${name}`;
  }
  observe(client,info,generation,serial){
    return new Promise((resolve,reject)=>{
      let settled=false,buffer='';
      const finish=error=>{if(settled)return;settled=true;clearTimeout(timer);error?reject(error):resolve();};
      const timer=setTimeout(()=>finish(new Error('日志通道连接超时。')),8000);
      try{client.exec(`exec tmux -L wcs-terminal -C attach-session -t ${info.session}`,(error,channel)=>{
        if(error)return finish(error);
        if(generation!==this.session.generation||serial!==this.serial){channel.close();return finish(new Error('日志连接已取消。'));}
        this.channel=channel;channel.setEncoding('utf8');channel.stderr.on('data',()=>{});
        channel.on('data',data=>{
          buffer+=data;
          if(buffer.length>2*1024*1024){finish(new Error('日志协议行过长。'));this.detach('日志协议异常，记录已中断。');return;}
          for(let end;(end=buffer.indexOf('\n'))>=0;){
            const line=buffer.slice(0,end).replace(/\r$/,'');buffer=buffer.slice(end+1);
            if(line.startsWith('%end '))finish();
            else if(line.startsWith('%error '))finish(new Error('设备拒绝日志通道。'));
            else{
              const output=line.match(/^%output (%\d+) (.*)$/s);
              if(output&&output[1]===info.pane&&this.channel===channel)this.parser?.push(decodeOutput(output[2]));
            }
          }
        });
        channel.on('error',error=>{finish(error);if(this.channel===channel)this.detach('日志连接中断：'+error.message);});
        channel.on('close',()=>{finish(new Error('日志连接已关闭。'));if(this.channel===channel)this.detach('日志通道已关闭；缺失输出不会补造。');});
      });}catch(error){finish(error);}
    });
  }
  async close(){this.detach('软件已关闭，记录已封存。');await this.store.close();}
}
module.exports={RunLogs,exec,quote};
