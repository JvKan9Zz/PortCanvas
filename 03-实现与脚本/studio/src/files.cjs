const fs = require('node:fs');
const fsp = fs.promises;
const path = require('node:path');
const crypto = require('node:crypto');
const {EventEmitter} = require('node:events');
const {Transform} = require('node:stream');
const {pipeline} = require('node:stream/promises');
const {setTimeout: delay} = require('node:timers/promises');

const missing = error => error?.code === 'ENOENT' || error?.code === 2;
const kind = stat => stat.isSymbolicLink() ? 'link' : stat.isDirectory() ? 'directory' : stat.isFile() ? 'file' : 'other';
const mtime = stat => stat.mtimeMs ?? stat.mtime * 1000;
const call = (sftp, method, ...args) => new Promise((resolve,reject) => {
  if(sftp.readable===false || sftp.outgoing?.state!=='open')return reject(new Error('SFTP 连接已关闭。'));
  try {sftp[method](...args,(error,value)=>error?reject(error):resolve(value));} catch(error) {reject(error);}
});
function fileError(error) {
  if (error?.code === 'EACCES' || error?.code === 'EPERM' || error?.code === 3) return '没有访问权限，请检查目录或文件权限。';
  if (missing(error)) return '目录或文件不存在，请刷新后重试。';
  if (error?.code === 'ENOSPC') return '目标磁盘空间不足。';
  return error?.message || String(error);
}
function leaf(name, windows = false) {
  if (typeof name !== 'string' || !name || name === '.' || name === '..' || /[\/\0]/.test(name)) throw new Error('文件名无效。');
  if (windows && (/[\\<>:"|?*\x00-\x1f]/.test(name) || /[. ]$/.test(name) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)/i.test(name))) throw new Error(`Windows 不支持此文件名：${name}`);
  return name;
}
const local = {
  join: path.join,
  stat: p => fsp.lstat(p),
  list: async p => Promise.all((await fsp.readdir(p)).map(async name=>({name,stat:await fsp.lstat(path.join(p,name))}))),
  mkdir: p => fsp.mkdir(p),
  unlink: p => fsp.unlink(p),
  rename: (from,to) => fsp.rename(from,to),
  read: p => fs.createReadStream(p),
  write: async p => (await fsp.open(p,'wx')).createWriteStream()
};
function remote(sftp) {
  return {
    join: path.posix.join,
    stat: p => call(sftp,'lstat',p),
    list: async p => (await call(sftp,'readdir',p)).filter(e=>e.filename!=='.' && e.filename!=='..').map(e=>({name:e.filename,stat:e.attrs})),
    mkdir: p => call(sftp,'mkdir',p),
    unlink: p => call(sftp,'unlink',p),
    rename: async (from,to,overwrite) => {
      try {await call(sftp,'ext_openssh_rename',from,to);} catch(error) {
        if (error.code !== 8 && !/not support/i.test(error.message)) throw error;
        if (overwrite) throw new Error('远端不支持安全覆盖，原文件已保留，请选择其他目标目录。');
        await call(sftp,'rename',from,to);
      }
    },
    read: p => sftp.createReadStream(p),
    write: async p => {
      const handle=await call(sftp,'open',p,'wx');
      return sftp.createWriteStream(p,{handle,autoClose:true});
    }
  };
}
async function optionalStat(access,p) {
  try {return await access.stat(p);} catch(error) {if(missing(error))return null;throw error;}
}

class FileService extends EventEmitter {
  constructor(session,{localHome=process.cwd(),confirmOverwrite=async()=>false}={}) {
    super();this.session=session;this.localHome=localHome;this.confirmOverwrite=confirmOverwrite;
    this.state={connection:'disconnected',phase:'idle',message:'选择文件或文件夹，开始双向传输。',bytes:0,totalBytes:0,completed:0,totalFiles:0,speed:0};
    this.pendingCleanup=[];
    session.on('state',state=>{if(state.ssh!=='connected')this.invalidate();});
  }
  update(data) {Object.assign(this.state,data);this.emit('state',{...this.state});}
  identity() {const p=this.session.profile;return p?`${p.username}@${p.host}:${p.port}`:'';}
  invalidate() {
    if(this.job) {this.job.reason='connection';this.job.controller.abort();}
    const old=this.sftp;this.sftp=null;this.sftpPromise=null;
    if(old)old.end();
    if(this.state.connection!=='disconnected')this.update({connection:'disconnected'});
  }
  async getSftp() {
    if(this.session.state.ssh!=='connected' || !this.session.client)throw new Error('请先在设备页连接树莓派。');
    if(this.sftp)return this.sftp;
    if(this.sftpPromise)return this.sftpPromise;
    const generation=this.session.generation,client=this.session.client;
    this.update({connection:'connecting'});
    const opening=new Promise((resolve,reject)=>client.sftp((error,sftp)=>{
      if(error)return reject(error);
      if(generation!==this.session.generation){sftp.end();return reject(new Error('设备连接已变化，请重试。'));}
      this.sftp=sftp;
      const close=sftp.close.bind(sftp);
      sftp.close=(handle,callback)=>{
        if(this.sftp!==sftp || generation!==this.session.generation || sftp.readable===false || sftp.outgoing.state!=='open') {
          queueMicrotask(()=>callback(new Error('SFTP 连接已关闭。')));return;
        }
        close(handle,callback);
      };
      sftp.on('error',()=>{});
      sftp.once('close',()=>{if(this.sftp===sftp){this.sftp=null;this.sftpPromise=null;if(this.job){this.job.reason='connection';this.job.controller.abort();}this.update({connection:'disconnected'});}});
      resolve(sftp);
    }));
    this.sftpPromise=opening;
    try {
      const sftp=await opening;
      for(const entry of [...this.pendingCleanup]) {
        if(entry.identity!==this.identity())continue;
        try {await call(sftp,'unlink',entry.path);this.pendingCleanup.splice(this.pendingCleanup.indexOf(entry),1);}
        catch(error){if(missing(error))this.pendingCleanup.splice(this.pendingCleanup.indexOf(entry),1);}
      }
      if(generation!==this.session.generation||this.sftp!==sftp)throw new Error('设备连接已变化，请重试。');
      this.update({connection:'connected'});return sftp;
    } catch(error) {
      if(this.sftpPromise===opening){this.sftpPromise=null;this.update({connection:'disconnected'});}
      throw new Error(fileError(error));
    }
  }
  localPath(input) {
    const result=input || this.localHome;
    if(typeof result!=='string'||!path.isAbsolute(result)||result.includes('\0'))throw new Error('请输入本地绝对目录路径。');
    return path.normalize(result);
  }
  async remotePath(input,sftp) {
    if(input!=null && typeof input!=='string')throw new Error('远端路径必须是文字。');
    const home=await call(sftp,'realpath','.');
    const value=input==='~'||!input?home:input.startsWith('~/')?path.posix.join(home,input.slice(2)):input;
    if(typeof value!=='string'||!value.startsWith('/')||value.includes('\0'))throw new Error('请输入以 / 开头的远端目录路径。');
    return {path:path.posix.normalize(value),home};
  }
  async list(side,input) {
    if(!['local','remote'].includes(side))throw new Error('无效的文件面板。');
    try {
      const generation=this.session.generation;
      const sftp=side==='remote'?await this.getSftp():null;
      const location=side==='local'?{path:this.localPath(input),home:this.localHome}:await this.remotePath(input,sftp);
      const access=side==='local'?local:remote(sftp);
      const absolute=side==='local'?await fsp.realpath(location.path):await call(sftp,'realpath',location.path);
      if(!(await access.stat(absolute)).isDirectory())throw new Error('该路径不是文件夹。');
      const entries=(await access.list(absolute)).map(e=>({name:e.name,type:kind(e.stat),size:e.stat.size,modified:mtime(e.stat)}));
      if(side==='remote'&&generation!==this.session.generation)throw new Error('设备连接已变化，请刷新。');
      entries.sort((a,b)=>(a.type==='directory'?0:1)-(b.type==='directory'?0:1)||a.name.localeCompare(b.name,'zh-CN',{numeric:true}));
      return {...location,path:absolute,parent:side==='local'?path.dirname(absolute):path.posix.dirname(absolute),entries};
    } catch(error){throw new Error(fileError(error));}
  }
  async mkdir(side,parent,name) {
    if(this.job)throw new Error('请等待当前传输结束后再新建文件夹。');
    if(!['local','remote'].includes(side))throw new Error('无效的文件面板。');
    const access=side==='local'?local:remote(await this.getSftp());
    const root=side==='local'?this.localPath(parent):(await this.remotePath(parent,this.sftp)).path;
    const target=access.join(root,leaf(name,side==='local'&&process.platform==='win32'));
    try {await access.mkdir(target);} catch(error) {throw new Error(error.code==='EEXIST'||error.code===4?'新建文件夹失败，请检查是否已存在同名项目及写入权限。':fileError(error));}
    return target;
  }
  cancel() {if(this.job){this.job.reason='user';this.job.controller.abort();this.update({phase:'canceling',message:'正在取消传输…'});}}
  async transfer(options) {
    if(this.job)throw new Error('已有传输正在进行，请等待或取消。');
    if(!['upload','download'].includes(options?.direction))throw new Error('无效的传输方向。');
    if(!Array.isArray(options.names)||!options.names.length)throw new Error('请先选择要传输的文件或文件夹。');
    const rate=Number(options.rateLimit||0);
    if(![0,1024*1024,5*1024*1024,10*1024*1024].includes(rate))throw new Error('无效的限速设置。');
    const job=this.job={controller:new AbortController(),reason:null,temps:[],bytes:0,completed:0,totalBytes:0,totalFiles:0};
    const signal=job.controller.signal;
    const check=()=>signal.throwIfAborted();
    const identity=this.identity();
    this.update({busy:true,phase:'planning',message:'正在检查文件和目标目录…',bytes:0,totalBytes:0,completed:0,totalFiles:0,speed:0,currentFile:'',direction:options.direction});
    try {
      const sftp=await this.getSftp();check();
      const remoteRoot=(await this.remotePath(options.remotePath,sftp)).path;
      const localRoot=this.localPath(options.localPath);
      const upload=options.direction==='upload',r=remote(sftp);
      const source=upload?local:r,dest=upload?r:local;
      const from=upload?localRoot:remoteRoot,to=upload?remoteRoot:localRoot;
      if(!(await source.stat(from)).isDirectory()||!(await dest.stat(to)).isDirectory())throw new Error('传输两端必须是已存在的文件夹。');
      const directories=[],files=[],conflicts=[],plannedTargets=new Set();
      const walk=async (src,dst,name)=>{
        check();leaf(name,!upload&&process.platform==='win32');
        const key=!upload&&process.platform==='win32'?dst.toLowerCase():dst;
        if(plannedTargets.has(key))throw new Error(`多个源项目对应同一个目标路径：${dst}`);
        plannedTargets.add(key);
        const stat=await source.stat(src),type=kind(stat);
        if(type==='link'||type==='other')throw new Error(`暂不传输符号链接或特殊文件：${src}`);
        const existing=await optionalStat(dest,dst);check();
        if(existing && kind(existing)!==type)throw new Error(`目标存在不同类型的同名项目：${dst}`);
        if(type==='directory') {
          directories.push({path:dst,exists:Boolean(existing)});
          for(const entry of await source.list(src))await walk(source.join(src,entry.name),dest.join(dst,entry.name),entry.name);
        } else {
          const item={src,dst,size:stat.size,modified:mtime(stat),existing:existing?{size:existing.size,modified:mtime(existing)}:null};
          files.push(item);job.totalBytes+=stat.size;
          if(existing)conflicts.push(dst);
        }
      };
      for(const name of new Set(options.names)){leaf(name,upload&&process.platform==='win32');await walk(source.join(from,name),dest.join(to,name),name);}
      job.totalFiles=files.length;
      this.update({totalBytes:job.totalBytes,totalFiles:job.totalFiles});
      if(conflicts.length){this.update({phase:'confirming',message:`等待确认覆盖 ${conflicts.length} 个同名文件。`});if(!await this.confirmOverwrite(conflicts)){job.reason='user';job.controller.abort();}check();}
      for(const entry of directories){check();if(!entry.exists)await dest.mkdir(entry.path);}
      job.started=Date.now();let lastEvent=0;
      const progress=()=>{const now=Date.now();if(now-lastEvent<120)return;lastEvent=now;this.update({bytes:job.bytes,completed:job.completed,speed:job.bytes/Math.max((now-job.started)/1000,.001)});};
      this.update({phase:'transferring',message:'正在传输…'});
      for(const item of files) {
        check();this.update({currentFile:item.src});
        const temporary=dest.join(upload?path.posix.dirname(item.dst):path.dirname(item.dst),'.wcs-transfer-'+crypto.randomUUID()+'.part');
        const output=await dest.write(temporary);
        const owned={access:dest,path:temporary,remote:upload,identity};job.temps.push(owned);
        const started=Date.now();let fileBytes=0;
        const meter=new Transform({transform(chunk,_encoding,callback){
          fileBytes+=chunk.length;
          const wait=rate?Math.max(0,started+fileBytes/rate*1000-Date.now()):0;
          (wait>0?delay(wait,undefined,{signal}):Promise.resolve()).then(()=>{job.bytes+=chunk.length;progress();callback(null,chunk);},callback);
        }});
        await pipeline(source.read(item.src),meter,output,{signal});check();
        const sourceAfter=await source.stat(item.src),copied=await dest.stat(temporary);
        if(fileBytes!==item.size || copied.size!==item.size || sourceAfter.size!==item.size || mtime(sourceAfter)!==item.modified)throw new Error(`传输期间源文件已变化，请重新传输：${item.src}`);
        const targetNow=await optionalStat(dest,item.dst);check();
        if(targetNow && (!item.existing || kind(targetNow)!=='file' || targetNow.size!==item.existing.size || mtime(targetNow)!==item.existing.modified))throw new Error(`目标文件在确认后发生变化，请重新传输：${item.dst}`);
        await dest.rename(temporary,item.dst,Boolean(item.existing));
        job.temps.splice(job.temps.indexOf(owned),1);job.completed++;progress();
      }
      this.update({phase:'completed',message:`传输完成：${job.completed} 个文件、${directories.length} 个文件夹。`,bytes:job.bytes,completed:job.completed,currentFile:'',speed:0});
    } catch(error) {
      const canceled=signal.aborted&&job.reason==='user';
      this.update({phase:canceled?'canceled':'failed',message:canceled?`已取消，已完成 ${job.completed} 个文件。`:job.reason==='connection'?`设备连接中断，已完成 ${job.completed} 个文件。`:fileError(error),bytes:job.bytes,completed:job.completed,speed:0});
    } finally {
      for(const entry of job.temps) {
        try {await entry.access.unlink(entry.path);} catch(error) {
          if(!missing(error)) {
            if(entry.remote)this.pendingCleanup.push({identity:entry.identity,path:entry.path});
            this.update({message:this.state.message+` 临时文件待清理：${entry.path}`});
          }
        }
      }
      if(this.job===job)this.job=null;
      this.update({busy:false});
    }
    return {...this.state};
  }
}
module.exports={FileService,fileError,leaf};
