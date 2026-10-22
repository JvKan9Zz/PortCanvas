const {test,before,after}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const path=require('node:path');
const crypto=require('node:crypto');
const fixture=require('./sftp-fixture.cjs');
const {DeviceSession}=require('../src/session.cjs');
const {FileService}=require('../src/files.cjs');
const base=path.resolve(__dirname,'../../../05-测试与验证/local/sftp-fixture-'+crypto.randomUUID());
let server;
before(async()=>{await fs.mkdir(base,{recursive:true});server=await fixture(path.join(base,'remote'));});
after(async()=>{await server.close();});
async function setup(name,confirmOverwrite=async()=>false) {
  const folder=path.join(base,name);await fs.mkdir(folder,{recursive:true});
  const session=new DeviceSession({verifyHost:()=>true});
  const profile={id:name,name,host:'127.0.0.1',port:server.port,username:'demo',display:':0'};
  await session.connect(profile,{password:'fixture-password'});
  const files=new FileService(session,{localHome:folder,confirmOverwrite});
  return {folder,session,files,profile,copy:(direction,names,remotePath='/')=>files.transfer({direction,names,localPath:folder,remotePath})};
}
test('SFTP browses and copies nested Chinese paths, binary and empty files both ways',async()=>{
  const x=await setup('roundtrip');
  try {
    const tree=path.join(x.folder,'中文 目录');await fs.mkdir(path.join(tree,'空文件夹'),{recursive:true});
    const binary=crypto.randomBytes(180000);await fs.writeFile(path.join(tree,'数据.bin'),binary);await fs.writeFile(path.join(tree,'零字节.txt'),'');
    const up=await x.copy('upload',['中文 目录']);assert.equal(up.phase,'completed',up.message);assert.equal(up.completed,2);assert.equal(up.bytes,binary.length);
    assert.ok((await x.files.list('remote','/中文 目录')).entries.some(e=>e.name==='空文件夹'&&e.type==='directory'));
    assert.ok((await fs.readFile(path.join(server.root,'中文 目录/数据.bin'))).equals(binary));
    const download=path.join(x.folder,'回传');await fs.mkdir(download);
    const down=await x.files.transfer({direction:'download',names:['中文 目录'],remotePath:'/',localPath:download});
    assert.equal(down.phase,'completed',down.message);assert.ok((await fs.readFile(path.join(download,'中文 目录/数据.bin'))).equals(binary));
    assert.equal((await fs.stat(path.join(download,'中文 目录/零字节.txt'))).size,0);
  } finally {x.session.disconnect();}
});
test('refusing overwrite preserves the existing remote file',async()=>{
  let asked=0;const x=await setup('decline',async paths=>{asked=paths.length;return false;});
  try {await fs.writeFile(path.join(x.folder,'existing.txt'),'new');await fs.writeFile(path.join(server.root,'existing.txt'),'original');const result=await x.copy('upload',['existing.txt']);assert.equal(result.phase,'canceled');assert.equal(asked,1);assert.equal(await fs.readFile(path.join(server.root,'existing.txt'),'utf8'),'original');}
  finally{x.session.disconnect();}
});
test('confirmed download safely replaces an existing local file',async()=>{
  const x=await setup('overwrite',async()=>true);
  try {await fs.writeFile(path.join(server.root,'replace.txt'),'new remote bytes');await fs.writeFile(path.join(x.folder,'replace.txt'),'old');const result=await x.copy('download',['replace.txt']);assert.equal(result.phase,'completed',result.message);assert.equal(await fs.readFile(path.join(x.folder,'replace.txt'),'utf8'),'new remote bytes');}
  finally{x.session.disconnect();}
});
test('a server without atomic overwrite support preserves its original file',async()=>{
  const x=await setup('unsupported-overwrite',async()=>true);
  try {await fs.writeFile(path.join(x.folder,'unsupported.txt'),'new');await fs.writeFile(path.join(server.root,'unsupported.txt'),'keep');const result=await x.copy('upload',['unsupported.txt']);assert.equal(result.phase,'failed');assert.match(result.message,/不支持安全覆盖/);assert.equal(await fs.readFile(path.join(server.root,'unsupported.txt'),'utf8'),'keep');assert.ok(!(await fs.readdir(server.root)).some(n=>n.endsWith('.part')));}
  finally{x.session.disconnect();}
});
for(const direction of ['upload','download'])test(`canceling throttled ${direction} preserves original and removes current temporary file`,async()=>{
  const x=await setup('cancel-'+direction,async()=>true),name='cancel-'+direction+'.bin';
  try {
    const large=crypto.randomBytes(2*1024*1024),source=direction==='upload'?x.folder:server.root,dest=direction==='upload'?server.root:x.folder;
    await fs.writeFile(path.join(source,name),large);await fs.writeFile(path.join(dest,name),'preserved');
    let canceled=false;x.files.on('state',state=>{if(state.phase==='transferring'&&state.bytes>0&&!canceled){canceled=true;x.files.cancel();}});
    const result=await x.files.transfer({direction,names:[name],localPath:x.folder,remotePath:'/',rateLimit:1048576});
    assert.equal(result.phase,'canceled',result.message);assert.equal(result.completed,0);assert.equal(await fs.readFile(path.join(dest,name),'utf8'),'preserved');assert.ok(!(await fs.readdir(dest)).some(n=>n.endsWith('.part')));assert.equal(x.session.state.ssh,'connected');
  } finally{x.session.disconnect();}
});
test('disconnect during transfer reports failure, then reconnect cleans the owned temporary file',{timeout:10000},async()=>{
  const x=await setup('disconnect');
  try {
    await fs.writeFile(path.join(x.folder,'disconnect.bin'),crypto.randomBytes(2*1024*1024));let stopped=false;
    x.files.on('state',state=>{if(state.phase==='transferring'&&state.bytes>0&&!stopped){stopped=true;x.session.disconnect();}});
    const result=await x.files.transfer({direction:'upload',names:['disconnect.bin'],localPath:x.folder,remotePath:'/',rateLimit:1048576});
    assert.equal(result.phase,'failed');assert.match(result.message,/设备连接中断/);await assert.rejects(fs.stat(path.join(server.root,'disconnect.bin')));
    await x.session.connect(x.profile,{password:'fixture-password'});await x.files.list('remote','/');assert.ok(!(await fs.readdir(server.root)).some(n=>n.endsWith('.part')));
  } finally{x.session.disconnect();}
});
test('permissions and destination type conflicts fail before replacing anything',async()=>{
  const x=await setup('errors');
  try {
    await fs.mkdir(path.join(server.root,'read-only'),{recursive:true});await fs.writeFile(path.join(x.folder,'error.txt'),'dummy');
    let result=await x.copy('upload',['error.txt'],'/read-only');assert.equal(result.phase,'failed');assert.match(result.message,/没有访问权限/);
    await fs.mkdir(path.join(server.root,'error.txt'));result=await x.copy('upload',['error.txt']);assert.equal(result.phase,'failed');assert.match(result.message,/不同类型/);assert.ok((await fs.stat(path.join(server.root,'error.txt'))).isDirectory());
    await assert.rejects(x.files.list('remote','/missing-folder'),/不存在/);
  } finally{x.session.disconnect();}
});
test('symbolic links and path traversal names are rejected; mkdir remains usable',async()=>{
  const x=await setup('names');
  try {
    await fs.writeFile(path.join(server.root,'link-fixture'),'fixture');let result=await x.copy('download',['link-fixture']);assert.equal(result.phase,'failed');assert.match(result.message,/符号链接/);
    result=await x.copy('upload',['../escape.txt']);assert.equal(result.phase,'failed');assert.match(result.message,/文件名无效/);
    await x.files.mkdir('remote','/','新文件夹');assert.ok((await x.files.list('remote','/')).entries.some(e=>e.name==='新文件夹'&&e.type==='directory'));
  } finally{x.session.disconnect();}
});
