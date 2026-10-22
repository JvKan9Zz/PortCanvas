const {test, before, after} = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const net = require('node:net');
const {once} = require('node:events');
const {Server} = require('ssh2');
const {WebSocket} = require('ws');
const {DeviceSession} = require('../src/session.cjs');
const {Profiles, validateProfile} = require('../src/profiles.cjs');
const fs = require('node:fs');
const path = require('node:path');

let ssh, vnc, port;
const clients = new Set();
const hostKey = crypto.generateKeyPairSync('rsa', {modulusLength:2048}).privateKey.export({type:'pkcs1',format:'pem'});
const profile = () => ({id:'fixture',name:'本地测试设备',host:'127.0.0.1',port,username:'demo',display:':0',xauthority:''});
before(async () => {
  vnc = net.createServer(socket => {socket.write('RFB 003.008\n');socket.on('data', data => socket.write(data));socket.on('error', () => {});});
  vnc.listen(0,'127.0.0.1');await once(vnc,'listening');
  ssh = new Server({hostKeys:[hostKey]}, client => {
    clients.add(client);client.on('error', () => {});client.on('close', () => clients.delete(client));
    client.on('authentication', ctx => ctx.method === 'password' && ctx.username === 'demo' && ctx.password === 'fixture-password' ? ctx.accept() : ctx.reject());
    client.on('ready', () => {
      client.on('session', accept => {
        const channel = accept();channel.on('exec', (acceptExec) => {
          const stream = acceptExec();stream.write(`STUDIO_DESKTOP={"port":${vnc.address().port},"display":":0"}\n`);
          stream.on('end', () => {stream.exit(0);stream.end();});stream.on('error', () => {});
        });
      });
      client.on('tcpip', (accept, reject, info) => {
        const socket = net.connect(info.destPort,info.destIP, () => {const stream=accept();stream.on('error',()=>socket.destroy());socket.pipe(stream).pipe(socket);});
        socket.on('error', () => reject());
      });
    });
  });
  ssh.listen(0,'127.0.0.1');await once(ssh,'listening');port=ssh.address().port;
});
after(async () => {for(const client of clients)client.end();await Promise.all([new Promise(r=>ssh.close(r)),new Promise(r=>vnc.close(r))]);});
test('profile validation and disk store exclude credentials', () => {
  const directory=path.resolve(__dirname,'../../../05-测试与验证/local/profile-fixture');
  fs.mkdirSync(directory,{recursive:true});const store=new Profiles(directory);
  const p=store.save({...profile(),password:'must-not-persist',privateKey:'must-not-persist'});
  assert.equal(store.read().devices.at(-1).id,p.id);
  assert.ok(!fs.readFileSync(store.file,'utf8').includes('must-not-persist'));
  assert.throws(()=>validateProfile({...profile(),port:0}));
  assert.throws(()=>validateProfile({...profile(),display:':0; touch /bad'}));
  assert.throws(()=>validateProfile({...profile(),host:'127.0.0.1\ncommand'}));
});
test('real SSH handshake and desktop WebSocket tunnel round trip', async () => {
  let observedFingerprint;
  const device=new DeviceSession({verifyHost:(_address,fingerprint)=>{observedFingerprint=fingerprint;return true;}});
  try {
    await device.connect(profile(),{password:'fixture-password'});
    assert.equal(device.state.ssh,'connected');assert.match(observedFingerprint,/^SHA256:/);
    const {url}=await device.startDesktop();
    const ws=new WebSocket(url);await once(ws,'open');
    const [banner]=await once(ws,'message');assert.equal(banner.toString(),'RFB 003.008\n');
    const echo=once(ws,'message');ws.send('transport-test');assert.equal((await echo)[0].toString(),'transport-test');
    device.desktopReady();assert.equal(device.state.desktop,'connected');
    device.stopDesktop();assert.equal(device.state.desktop,'disconnected');assert.equal(device.state.ssh,'connected');
    await once(ws,'close');
    const second=await device.startDesktop();assert.notEqual(second.url,url);
    device.disconnect();assert.equal(device.state.ssh,'disconnected');assert.equal(device.wss,null);
    await device.connect(profile(),{password:'fixture-password'});assert.equal(device.state.ssh,'connected');
  } finally {device.disconnect();}
});
test('wrong password preserves actionable error', async () => {
  const device=new DeviceSession({verifyHost:()=>true});
  try {await assert.rejects(device.connect(profile(),{password:'wrong'}),/认证失败/);assert.equal(device.state.ssh,'error');}
  finally {device.disconnect();}
});
test('host identity rejection cannot become a successful connection', async () => {
  const device=new DeviceSession({verifyHost:()=>false});
  try {await assert.rejects(device.connect(profile(),{password:'fixture-password'}),/主机身份未确认/);assert.notEqual(device.state.ssh,'connected');}
  finally {device.disconnect();}
});
test('refused SSH endpoint reports a recoverable error', async () => {
  const temporary=net.createServer();temporary.listen(0,'127.0.0.1');await once(temporary,'listening');
  const unusedPort=temporary.address().port;await new Promise(resolve=>temporary.close(resolve));
  const device=new DeviceSession({verifyHost:()=>true});
  try {await assert.rejects(device.connect({...profile(),port:unusedPort},{password:'fixture-password'}),/连接被拒绝/);assert.equal(device.state.ssh,'error');}
  finally {device.disconnect();}
});
test('cancel pending connection and ignore its delayed host approval', async () => {
  let approve, reached;
  const verification=new Promise(resolve=>reached=resolve);
  const device=new DeviceSession({verifyHost:()=>{reached();return new Promise(resolve=>approve=resolve);}});
  const attempt=device.connect(profile(),{password:'fixture-password'});const rejected=assert.rejects(attempt);
  await verification;device.disconnect();approve(true);await rejected;
  assert.equal(device.state.ssh,'disconnected');
});
test('remote disconnect updates state and closes owned desktop resources', async () => {
  const device=new DeviceSession({verifyHost:()=>true});
  try {
    await device.connect(profile(),{password:'fixture-password'});await device.startDesktop();
    const closed=new Promise(resolve=>device.on('state',state=>{if(state.ssh==='disconnected')resolve();}));
    for(const client of clients)client.end();await closed;
    assert.equal(device.wss,null);assert.equal(device.desktopChannel,null);
  } finally {device.disconnect();}
});
