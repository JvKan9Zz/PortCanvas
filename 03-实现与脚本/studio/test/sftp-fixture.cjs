const fs=require('node:fs');
const fsp=fs.promises;
const path=require('node:path');
const crypto=require('node:crypto');
const {once}=require('node:events');
const {Server,utils:{sftp:{OPEN_MODE:O,STATUS_CODE:S}}}=require('ssh2');

module.exports=async function fixture(root) {
  await fsp.mkdir(root,{recursive:true});
  const clients=new Set();
  const hostKey=crypto.generateKeyPairSync('rsa',{modulusLength:2048}).privateKey.export({type:'pkcs1',format:'pem'});
  const resolve=value=>{const p=path.resolve(root,'.'+path.posix.resolve('/',value));if(path.relative(root,p).startsWith('..'))throw Error('Fixture path escaped');return p;};
  const attrs=stat=>({mode:stat.mode,size:stat.size,uid:0,gid:0,atime:Math.floor(stat.atimeMs/1000),mtime:Math.floor(stat.mtimeMs/1000)});
  const server=new Server({hostKeys:[hostKey]},client=>{
    clients.add(client);client.on('error',()=>{});client.on('close',()=>clients.delete(client));
    client.on('authentication',ctx=>ctx.method==='password'&&ctx.username==='demo'&&ctx.password==='fixture-password'?ctx.accept():ctx.reject());
    client.on('ready',()=>client.on('session',accept=>accept().on('sftp',acceptSftp=>{
      const sftp=acceptSftp(),handles=new Map();let index=0,closed=false;
      sftp.on('error',()=>{});
      sftp.on('close',()=>{closed=true;for(const entry of handles.values())entry.file?.close().catch(()=>{});handles.clear();});
      const respond=fn=>{if(!closed)fn();};
      const failure=(id,error)=>respond(()=>sftp.status(id,error.code==='ENOENT'?S.NO_SUCH_FILE:error.code==='EACCES'?S.PERMISSION_DENIED:S.FAILURE,error.message));
      const work=(id,fn)=>Promise.resolve().then(fn).catch(error=>failure(id,error));
      const ok=id=>respond(()=>sftp.status(id,S.OK));
      const handle=entry=>{const b=Buffer.alloc(4);b.writeUInt32BE(++index);handles.set(index,entry);return b;};
      const entry=b=>{const e=handles.get(b.readUInt32BE());if(!e)throw Error('Invalid fixture handle');return e;};
      const deny=filename=>{if(filename.startsWith('/read-only/'))throw Object.assign(Error('Permission denied'),{code:'EACCES'});};
      sftp.on('REALPATH',(id,value)=>work(id,async()=>{await fsp.stat(resolve(value));respond(()=>sftp.name(id,[{filename:path.posix.resolve('/',value),longname:'',attrs:{}}]));}));
      for(const event of ['STAT','LSTAT'])sftp.on(event,(id,value)=>work(id,async()=>{const stat=await fsp.lstat(resolve(value));const result=attrs(stat);if(value==='/link-fixture')result.mode=0o120777;respond(()=>sftp.attrs(id,result));}));
      sftp.on('OPENDIR',(id,value)=>work(id,async()=>{const entries=await Promise.all((await fsp.readdir(resolve(value))).map(async name=>({filename:name,longname:name,attrs:attrs(await fsp.lstat(path.join(resolve(value),name)))})));respond(()=>sftp.handle(id,handle({entries})));}));
      sftp.on('READDIR',(id,b)=>work(id,()=>{const e=entry(b);if(!e.entries.length)respond(()=>sftp.status(id,S.EOF));else{const list=e.entries.splice(0,100);respond(()=>sftp.name(id,list));}}));
      sftp.on('OPEN',(id,value,flags)=>work(id,async()=>{
        if(flags&O.WRITE)deny(value);
        let mode=flags&O.WRITE?(flags&O.READ?fs.constants.O_RDWR:fs.constants.O_WRONLY):fs.constants.O_RDONLY;
        if(flags&O.CREAT)mode|=fs.constants.O_CREAT;if(flags&O.TRUNC)mode|=fs.constants.O_TRUNC;if(flags&O.EXCL)mode|=fs.constants.O_EXCL;
        const file=await fsp.open(resolve(value),mode,0o600);
        if(closed){await file.close();return;}
        respond(()=>sftp.handle(id,handle({file})));
      }));
      sftp.on('READ',(id,b,offset,length)=>work(id,async()=>{const buffer=Buffer.alloc(Math.min(length,65536));const {bytesRead}=await entry(b).file.read(buffer,0,buffer.length,offset);respond(()=>bytesRead?sftp.data(id,buffer.subarray(0,bytesRead)):sftp.status(id,S.EOF));}));
      sftp.on('WRITE',(id,b,offset,data)=>work(id,async()=>{const {bytesWritten}=await entry(b).file.write(data,0,data.length,offset);if(bytesWritten!==data.length)throw Error('Short fixture write');ok(id);}));
      sftp.on('CLOSE',(id,b)=>work(id,async()=>{const e=entry(b);handles.delete(b.readUInt32BE());await e.file?.close();ok(id);}));
      sftp.on('REMOVE',(id,value)=>work(id,async()=>{await fsp.unlink(resolve(value));ok(id);}));
      sftp.on('MKDIR',(id,value)=>work(id,async()=>{deny(value);await fsp.mkdir(resolve(value));ok(id);}));
      sftp.on('RENAME',(id,from,to)=>work(id,async()=>{await fsp.rename(resolve(from),resolve(to));ok(id);}));
    })));
  });
  server.listen(0,'127.0.0.1');await once(server,'listening');
  return {port:server.address().port,root,close:async()=>{for(const client of clients)client.end();await new Promise(resolve=>server.close(resolve));}};
};
