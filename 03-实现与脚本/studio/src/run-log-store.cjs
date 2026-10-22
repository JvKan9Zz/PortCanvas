const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const {EventEmitter}=require('node:events');
const {StringDecoder}=require('node:string_decoder');
const idPattern=/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const dayOf=date=>`${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,'0')}-${String(date.getDate()).padStart(2,'0')}`;
const atomic=(file,data)=>{const temp=file+'.tmp';fs.writeFileSync(temp,JSON.stringify(data,null,2),{mode:0o600});fs.renameSync(temp,file);};
function projectOf(event,override=''){
  const source=override?'manual:'+override:'auto:'+(event.projectRoot||event.cwd||'unknown');
  const name=override||path.posix.basename(event.projectRoot||event.cwd||'')||'未分类';
  const key=crypto.createHash('sha256').update(source).digest('hex').slice(0,12);
  const slug=name.replace(/[<>:"/\\|?*\x00-\x1f]/g,'_').trim().replace(/[. ]+$/,'').slice(0,50)||'未分类';
  return {name,source,key,folder:'project-'+slug+'-'+key};
}
class RunLogStore extends EventEmitter {
  constructor(root){super();this.root=path.resolve(root);this.writers=new Map();this.pending=new Set();}
  records(){
    if(!fs.existsSync(this.root))return [];
    const result=[];
    const dirs=dir=>fs.readdirSync(dir,{withFileTypes:true}).filter(e=>e.isDirectory()&&!e.isSymbolicLink());
    for(const project of dirs(this.root)){
      if(!project.name.startsWith('project-'))continue;
      const projectPath=path.join(this.root,project.name);
      for(const day of dirs(projectPath)){
        if(!/^\d{4}-\d{2}-\d{2}$/.test(day.name))continue;
        for(const run of dirs(path.join(projectPath,day.name))){
          if(!idPattern.test(run.name))continue;
          const directory=path.join(projectPath,day.name,run.name),file=path.join(directory,'run.json');
          try{
            if(fs.lstatSync(file).isSymbolicLink())continue;
            const meta=JSON.parse(fs.readFileSync(file,'utf8'));
            if(meta.schema!=='wcs.run/1'||meta.id!==run.name||typeof meta.command!=='string'||typeof meta.project?.name!=='string')continue;
            const output=path.join(directory,'output.log');
            if(fs.existsSync(output)&&fs.lstatSync(output).isSymbolicLink())continue;
            if(meta.status==='running'&&!this.writers.has(meta.id)){
              let alive=false;if(Number.isInteger(meta.ownerPid)&&meta.ownerPid>0){try{process.kill(meta.ownerPid,0);alive=true;}catch(error){alive=error.code!=='ESRCH';}}
              if(!alive){
                meta.status='interrupted';meta.complete=false;meta.message='上次记录未正常结束，输出可能不完整。';meta.ownerPid=null;
                try{atomic(file,meta);}catch(error){const notice='中断记录状态无法写回：'+error.message;if(this.notice!==notice){this.notice=notice;this.emit('notice',notice);}}
              }
            }
            result.push({...meta,directory,bytes:fs.existsSync(output)?fs.statSync(output).size:0,active:this.writers.has(meta.id)});
          }catch(error){if(!['ENOENT','ENOTDIR'].includes(error.code)&&!(error instanceof SyntaxError))throw error;}
        }
      }
    }
    return result.sort((a,b)=>b.startedAt.localeCompare(a.startedAt));
  }
  list(){return this.records().map(record=>({...record,command:record.command.slice(0,200)}));}
  find(key){return this.records().find(record=>record.sourceKey===key);}
  begin(event,{device,override='',recovered=false,kind='command'}={}){
    const sourceKey=device+':'+event.shell+':'+event.sequence;
    const old=this.find(sourceKey);
    if(old&&this.writers.has(old.id))return this.writers.get(old.id);
    if(old?.complete&&old.status!=='running')return null;
    const now=new Date(),project=old?.project||projectOf(event,override);
    const meta=old?{...old}:{schema:'wcs.run/1',id:crypto.randomUUID(),sourceKey,kind,project,day:dayOf(now),command:event.command,cwd:event.cwd,device,startedAt:now.toISOString(),gaps:[],complete:true};
    delete meta.directory;delete meta.active;delete meta.bytes;
    if(this.writers.has(meta.id))return this.writers.get(meta.id);
    if(old?.ownerPid&&old.ownerPid!==process.pid){try{process.kill(old.ownerPid,0);throw new Error('这份日志正在由另一软件实例记录。');}catch(error){if(error.code!=='ESRCH')throw error;}}
    const directory=old?.directory||path.join(this.root,project.folder,meta.day,meta.id);
    fs.mkdirSync(directory,{recursive:true});
    meta.status='running';meta.ownerPid=process.pid;meta.endedAt=null;meta.exitCode=null;
    if(recovered||old){meta.complete=false;meta.gaps.push({at:now.toISOString(),reason:'重新连接后恢复记录，连接中断期间的输出未采集。'});}
    atomic(path.join(directory,'run.json'),meta);
    const stream=fs.createWriteStream(path.join(directory,'output.log'),{flags:'a',encoding:'utf8',mode:0o600,flush:true});
    const writer={meta,directory,stream,failed:false,closing:false,append:text=>{
      if(writer.failed||writer.closing)return;
      if(stream.writableLength>8*1024*1024){this.fail(writer,new Error('日志输出过快，本地记录已中断。'));return;}
      stream.write(text);
    }};
    stream.on('error',error=>this.fail(writer,error));this.writers.set(meta.id,writer);
    if(recovered||old)writer.append('\n[记录恢复：中断期间的输出缺失]\n');
    this.emit('change');return writer;
  }
  fail(writer,error){
    if(writer.failed)return;writer.failed=true;writer.meta.complete=false;writer.meta.status='write_error';writer.meta.message=error.message;
    try{atomic(path.join(writer.directory,'run.json'),writer.meta);}catch{}
    writer.stream.destroy();this.writers.delete(writer.meta.id);this.emit('fault',error);this.emit('change');
  }
  finish(writer,event,{interrupted=false,reason='连接中断，日志不完整。'}={}){
    if(!writer||writer.closing||writer.failed)return Promise.resolve();
    writer.closing=true;
    Object.assign(writer.meta,{endedAt:new Date().toISOString(),endTimeSource:'computer_observed',exitCode:interrupted?null:event?.code??null,status:interrupted?'interrupted':event?.code===0?'succeeded':'failed',ownerPid:null,backgroundJobs:event?.backgroundJobs||[]});
    if(writer.meta.kind==='session')writer.meta.status='session';
    if(interrupted){writer.meta.complete=false;writer.meta.gaps.push({at:writer.meta.endedAt,reason});writer.stream.write('\n[记录中断：'+reason+']\n');}
    writer.meta.durationMs=writer.meta.complete?Date.parse(writer.meta.endedAt)-Date.parse(writer.meta.startedAt):null;
    const task=new Promise(resolve=>{
      let done=false;const complete=()=>{if(done)return;done=true;this.writers.delete(writer.meta.id);try{atomic(path.join(writer.directory,'run.json'),writer.meta);}catch(error){this.emit('fault',error);}this.emit('change');resolve();};
      writer.stream.once('close',complete);writer.stream.end();
    });
    this.pending.add(task);task.finally(()=>this.pending.delete(task));return task;
  }
  recoverEnded(event,options){
    const sourceKey=options.device+':'+event.shell+':'+event.sequence,old=this.find(sourceKey);
    if(old?.complete||old?.exitCode!==undefined&&old?.exitCode!==null)return;
    const writer=this.begin(event,{...options,recovered:true});if(writer)return this.finish(writer,event);
  }
  detail(id){
    if(!idPattern.test(String(id)))throw new Error('日志编号无效。');
    const record=this.records().find(r=>r.id===id);if(!record)throw new Error('日志已不存在，请刷新列表。');
    const file=path.join(record.directory,'output.log');let text='',truncated=false;
    if(fs.existsSync(file)){
      const size=fs.statSync(file).size,start=Math.max(0,size-256*1024),buffer=Buffer.alloc(size-start),fd=fs.openSync(file,'r');
      try{fs.readSync(fd,buffer,0,buffer.length,start);}finally{fs.closeSync(fd);}
      let offset=0;if(start)while(offset<buffer.length&&(buffer[offset]&0xc0)===0x80)offset++;
      text=new StringDecoder('utf8').write(buffer.subarray(offset));truncated=start>0;
    }
    return {...record,text,truncated,file};
  }
  removable(ids){
    if(!Array.isArray(ids)||ids.length>10000||ids.some(id=>!idPattern.test(String(id))))throw new Error('请选择有效的日志记录。');
    const selected=new Set(ids);return this.records().filter(r=>selected.has(r.id)&&!r.active&&r.status!=='running');
  }
  remove(ids){
    const records=this.removable(ids);let removed=0;
    for(const record of records){
      const directory=path.resolve(record.directory);
      if(!directory.startsWith(this.root+path.sep)||fs.lstatSync(directory).isSymbolicLink())throw new Error('日志路径校验失败。');
      if(this.writers.has(record.id))continue;
      fs.rmSync(directory,{recursive:true,force:false});removed++;
    }
    this.emit('change');return {removed};
  }
  async close(){for(const writer of [...this.writers.values()])this.finish(writer,null,{interrupted:true,reason:'软件或设备连接已关闭。'});await Promise.all([...this.pending]);}
}
module.exports={RunLogStore,projectOf};
