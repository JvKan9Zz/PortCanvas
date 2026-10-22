const {test,before,after}=require('node:test');
const assert=require('node:assert/strict');
const {once}=require('node:events');
const crypto=require('node:crypto');
const {Server}=require('ssh2');
const {DeviceSession}=require('../src/session.cjs');
const {TerminalSession,terminalName}=require('../src/terminal.cjs');
const key=crypto.generateKeyPairSync('rsa',{modulusLength:2048}).privateKey.export({type:'pkcs1',format:'pem'});
let server,mode='ready',lastPty,lastResize,commands=[];
before(async()=>{
  server=new Server({hostKeys:[key]},client=>{
    client.on('error',()=>{});
    client.on('authentication',ctx=>ctx.username==='fixture'&&ctx.password==='fixture-password'?ctx.accept():ctx.reject());
    client.on('ready',()=>client.on('session',accept=>{
      const session=accept();
      session.on('pty',(accept,_reject,info)=>{lastPty=info;accept();});
      session.on('window-change',(_accept,_reject,info)=>{lastResize=info;});
      session.on('exec',(accept,_reject,info)=>{
        commands.push(info.command);
        if(mode==='wait')return;
        const stream=accept();stream.on('error',()=>{});
        if(mode==='missing'){stream.stderr.write('tmux: not found');stream.exit(127);stream.end();return;}
        const text=Buffer.from('终端就绪\r\n$ ');stream.write(text.subarray(0,1));setImmediate(()=>stream.write(text.subarray(1)));
        stream.on('data',data=>stream.write('echo:'+data.toString()));
      });
    }));
  });server.listen(0,'127.0.0.1');await once(server,'listening');
});
after(()=>new Promise(resolve=>server.close(resolve)));
async function setup(options){
  mode='ready';lastResize=null;
  const session=new DeviceSession({verifyHost:()=>true});
  await session.connect({id:'fixture',name:'测试设备',host:'127.0.0.1',port:server.address().port,username:'fixture'},{password:'fixture-password'});
  const terminal=new TerminalSession(session,options),output=[];terminal.on('data',event=>output.push(event));
  return {session,terminal,output};
}
const wait=async predicate=>{for(let i=0;i<100;i++){if(predicate())return;await new Promise(r=>setTimeout(r,5));}throw Error('fixture condition timeout');};
test('embedded terminal allocates an SSH PTY and decodes split UTF-8 output',async()=>{
  const x=await setup();try{
    const state=await x.terminal.start(90,25);await wait(()=>x.output.map(o=>o.data).join('').includes('终端就绪'));
    assert.equal(state.phase,'connected');assert.equal(lastPty.term,'xterm-256color');assert.equal(lastPty.cols,90);assert.equal(lastPty.rows,25);
    assert.match(commands.at(-1),/^exec tmux -L wcs-terminal -f \/dev\/null new-session -A -s wcs-[a-f0-9]{16}$/);
    assert.ok(x.output.every(o=>o.id===state.id));
  }finally{x.session.disconnect();}
});
test('input and terminal resizes reach the same SSH channel',async()=>{
  const x=await setup();try{
    const state=await x.terminal.start();x.terminal.write(state.id,'中文 input');x.terminal.resize(state.id,80,20);
    await wait(()=>x.output.some(o=>o.data.includes('echo:中文 input'))&&lastResize);
    assert.equal(lastResize.cols,80);assert.equal(lastResize.rows,20);
    assert.throws(()=>x.terminal.resize(state.id,NaN,20),/尺寸/);
  }finally{x.session.disconnect();}
});
test('detach keeps device connection and reopen uses the same tmux session',async()=>{
  const x=await setup();try{
    x.session.state.desktop='connected';const first=await x.terminal.start(),command=commands.at(-1);
    x.terminal.stop();assert.equal(x.session.state.ssh,'connected');assert.equal(x.session.state.desktop,'connected');
    const second=await x.terminal.start();assert.equal(commands.at(-1),command);assert.notEqual(second.id,first.id);
    assert.throws(()=>x.terminal.write(first.id,'stale input'),/失效/);
    assert.throws(()=>x.terminal.resize(first.id,80,20),/失效/);
  }finally{x.session.disconnect();}
});
test('disconnected devices cannot open a terminal',async()=>{
  const x=await setup();x.session.disconnect();await assert.rejects(x.terminal.start(),/请先连接设备/);
});
test('missing tmux reports an error without substituting an unprotected shell',async()=>{
  const x=await setup();mode='missing';try{
    await x.terminal.start();await wait(()=>x.terminal.state.phase==='error');assert.match(x.terminal.state.message,/未安装 tmux/);
    assert.equal(x.session.state.ssh,'connected');
  }finally{x.session.disconnect();}
});
test('losing SSH detaches terminal and rejects later keystrokes',async()=>{
  const x=await setup();try{
    const state=await x.terminal.start();x.session.client.destroy();await wait(()=>x.session.state.ssh!=='connected');
    assert.equal(x.terminal.state.phase,'disconnected');assert.match(x.terminal.state.message,/恢复/);
    assert.throws(()=>x.terminal.write(state.id,'late input'),/失效/);
  }finally{x.session.disconnect();}
});
test('canceling a pending attachment cannot later activate it',async()=>{
  const x=await setup();mode='wait';try{
    const pending=x.terminal.start(), rejected=assert.rejects(pending,/取消/);x.terminal.stop();await rejected;
    assert.equal(x.terminal.state.phase,'disconnected');assert.equal(x.session.state.ssh,'connected');
  }finally{x.session.disconnect();}
});
test('attachment has a deadline and repeated starts share one channel',async()=>{
  const x=await setup({timeout:30});mode='wait';try{await assert.rejects(x.terminal.start(),/超时/);assert.equal(x.terminal.state.phase,'error');}finally{x.session.disconnect();}
  const y=await setup();try{const count=commands.length;await Promise.all([y.terminal.start(),y.terminal.start()]);assert.equal(commands.length,count+1);}finally{y.session.disconnect();}
});
test('session identity survives profile renames and separates accounts',()=>{
  const p={host:'192.0.2.1',port:22,username:'demo'};assert.equal(terminalName(p),terminalName({...p,id:'renamed',name:'new'}));assert.notEqual(terminalName(p),terminalName({...p,username:'other'}));assert.notEqual(terminalName(p),terminalName({...p,port:2222}));
});
