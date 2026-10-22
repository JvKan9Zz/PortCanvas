// Login arrives on stdin and is never persisted in files or command arguments.
const fs=require('node:fs/promises');
const path=require('node:path');
const crypto=require('node:crypto');
const readline=require('node:readline');
const assert=require('node:assert/strict');
const {DeviceSession}=require('../src/session.cjs');
const {FileService}=require('../src/files.cjs');
const root=path.resolve(__dirname,'../../..');
const invoke=(sftp,method,...args)=>new Promise((resolve,reject)=>sftp[method](...args,(error,value)=>error?reject(error):resolve(value)));
const digest=data=>crypto.createHash('sha256').update(data).digest('hex');
async function readRemote(sftp,file){const chunks=[];for await(const chunk of sftp.createReadStream(file))chunks.push(chunk);return Buffer.concat(chunks);}

(async()=>{
  console.log('Waiting for device login on stdin.');
  const reader=readline.createInterface({input:process.stdin});
  const password=await new Promise((resolve,reject)=>{reader.once('line',line=>{reader.close();resolve(line);});reader.once('close',()=>{});reader.once('error',reject);});
  process.stdin.unref?.();
  const config=JSON.parse(await fs.readFile(path.join(root,'04-数据与素材/private/studio-data/devices.json'),'utf8'));
  const profile=config.devices[0];assert.ok(profile);
  const session=new DeviceSession({verifyHost:(address,fingerprint)=>config.hostKeys[address]===fingerprint});
  const id='sftp-tests-20261022-'+crypto.randomUUID().slice(0,8);
  const base=path.join(root,'05-测试与验证/local',id),upload=path.join(base,'upload'),download=path.join(base,'download');
  await fs.mkdir(path.join(upload,'中文目录 空格/空文件夹'),{recursive:true});await fs.mkdir(download,{recursive:true});
  const files=new FileService(session,{localHome:upload,confirmOverwrite:async paths=>{confirmations.push(paths.length);return approve;}});
  let approve=true,confirmations=[],sftp,remoteRoot,readOnly;
  const results=[];
  const copy=(direction,names,localPath=upload,remotePath=remoteRoot,rateLimit=0)=>files.transfer({direction,names,localPath,remotePath,rateLimit});
  try {
    await session.connect(profile,{password});sftp=await files.getSftp();
    remoteRoot=await files.mkdir('remote','/home/bcsh/robot-research/wheel-combat-studio',id);
    const binary=crypto.randomBytes(512*1024);
    await fs.writeFile(path.join(upload,'中文目录 空格/程序 示例.py'),"# SFTP 中文内容测试，不执行\r\nprint('demo')\r\n");
    await fs.writeFile(path.join(upload,"中文目录 空格/带空格 # 和'引号'.bin"),binary);
    await fs.writeFile(path.join(upload,'中文目录 空格/零字节.txt'),'');
    let result=await copy('upload',['中文目录 空格']);assert.equal(result.phase,'completed',result.message);assert.equal(result.completed,3);
    results.push('Chinese/spaces/quotes/binary/empty-file/empty-directory upload passed');
    const original=await readRemote(sftp,remoteRoot+'/中文目录 空格/程序 示例.py');
    await fs.writeFile(path.join(upload,'中文目录 空格/程序 示例.py'),'# 第二版，仅测试传输\n');approve=false;
    result=await copy('upload',['中文目录 空格']);assert.equal(result.phase,'canceled');assert.ok((await readRemote(sftp,remoteRoot+'/中文目录 空格/程序 示例.py')).equals(original));
    results.push('Overwrite refusal preserved original');approve=true;
    result=await copy('upload',['中文目录 空格']);assert.equal(result.phase,'completed',result.message);assert.equal(confirmations.at(-1),3);
    results.push('Confirmed OpenSSH atomic overwrite passed');
    result=await copy('download',['中文目录 空格'],download);assert.equal(result.phase,'completed',result.message);
    const hashes=[];
    for(const name of ['程序 示例.py',"带空格 # 和'引号'.bin",'零字节.txt']) {
      const originalData=await fs.readFile(path.join(upload,'中文目录 空格',name));
      const returned=await fs.readFile(path.join(download,'中文目录 空格',name));assert.equal(digest(returned),digest(originalData));
      hashes.push({name,size:returned.length,sha256:digest(returned)});
    }
    assert.ok((await fs.stat(path.join(download,'中文目录 空格/空文件夹'))).isDirectory());results.push('Download and byte hashes passed');
    const large=crypto.randomBytes(8*1024*1024);await fs.writeFile(path.join(upload,'大文件.bin'),large);
    let interrupt='cancel';
    files.on('state',state=>{if(state.phase==='transferring'&&state.bytes>0&&interrupt){const action=interrupt;interrupt=null;action==='cancel'?files.cancel():session.disconnect();}});
    result=await copy('upload',['大文件.bin'],upload,remoteRoot,1048576);assert.equal(result.phase,'canceled');
    assert.ok(!(await files.list('remote',remoteRoot)).entries.some(e=>e.name.endsWith('.part')||e.name==='大文件.bin'));results.push('Throttled upload cancellation cleaned temporary file');
    result=await copy('upload',['大文件.bin']);assert.equal(result.phase,'completed',result.message);
    await fs.writeFile(path.join(download,'大文件.bin'),'keep local original');interrupt='cancel';
    result=await copy('download',['大文件.bin'],download,remoteRoot,1048576);assert.equal(result.phase,'canceled');assert.equal(await fs.readFile(path.join(download,'大文件.bin'),'utf8'),'keep local original');
    assert.ok(!(await fs.readdir(download)).some(n=>n.endsWith('.part')));results.push('Throttled download cancellation preserved original');
    readOnly=await files.mkdir('remote',remoteRoot,'只读目录');await invoke(sftp,'chmod',readOnly,0o500);
    try {result=await copy('upload',['中文目录 空格'],upload,readOnly);assert.equal(result.phase,'failed');assert.match(result.message,/没有访问权限/);} finally {await invoke(sftp,'chmod',readOnly,0o700);readOnly=null;}
    results.push('Real remote permission denial reported');
    await fs.copyFile(path.join(upload,'大文件.bin'),path.join(upload,'断线测试.bin'));interrupt='disconnect';
    result=await copy('upload',['断线测试.bin'],upload,remoteRoot,1048576);assert.equal(result.phase,'failed');assert.match(result.message,/连接中断/);
    await session.connect(profile,{password});sftp=await files.getSftp();
    const after=await files.list('remote',remoteRoot);assert.ok(!after.entries.some(e=>e.name.endsWith('.part')||e.name==='断线测试.bin'));
    results.push('Disconnect/reconnect and owned temporary cleanup passed');
    const record={date:'2026-10-22',localBase:base,remoteBase:remoteRoot,results,hashes,passed:true};
    await fs.writeFile(path.join(root,'05-测试与验证/local/v02-real-sftp.json'),JSON.stringify(record,null,2));
    console.log(JSON.stringify(record,null,2));
  } finally {
    if(readOnly&&sftp)await invoke(sftp,'chmod',readOnly,0o700).catch(()=>{});
    session.disconnect();
  }
})().catch(error=>{console.error(error.stack);process.exitCode=1;});
