const fs=require('node:fs/promises');
const path=require('node:path');
const assert=require('node:assert/strict');
const readline=require('node:readline');
const crypto=require('node:crypto');
const {WebSocket}=require('ws');
const root=path.resolve(__dirname,'../../..');
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));

(async()=>{
  const reader=readline.createInterface({input:process.stdin});
  const password=await new Promise(resolve=>reader.once('line',line=>{reader.close();resolve(line);}));process.stdin.unref?.();
  const real=JSON.parse(await fs.readFile(path.join(root,'05-测试与验证/local/v02-real-sftp.json'),'utf8'));
  const run=crypto.randomUUID().slice(0,8),folderName='界面上传-'+run;
  const upload=path.join(real.localBase,'upload'),download=path.join(real.localBase,'gui-download-'+run);await fs.mkdir(download,{recursive:true});
  const shots=path.join(root,'05-测试与验证/local/screenshots');
  const targets=await(await fetch('http://127.0.0.1:9366/json')).json();
  const target=targets.find(t=>t.type==='page'&&t.url.includes('/demo-v0.2/win-unpacked/resources/app.asar/ui/index.html'));assert.ok(target,'Packaged V0.2 renderer not found');
  const socket=new WebSocket(target.webSocketDebuggerUrl);await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
  let sequence=0;const pending=new Map();
  socket.on('message',data=>{const value=JSON.parse(data);if(!value.id)return;const p=pending.get(value.id);if(!p)return;pending.delete(value.id);clearTimeout(p.timer);value.error?p.reject(Error(value.error.message)):p.resolve(value.result);});
  const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error('CDP request timed out: '+method));},20000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>{const value=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(value.exceptionDetails)throw Error(value.exceptionDetails.exception?.description||value.exceptionDetails.text);return value.result.value;};
  const until=async(fn,check,label,timeout=16000)=>{for(let i=0;i<timeout/100;i++){const value=await fn();if(check(value))return value;await sleep(100);}throw Error('UI condition timed out: '+label);};
  const click=async(selector,count=1)=>{
    const point=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e||e.disabled)throw Error('Control unavailable');e.scrollIntoView({block:'center'});const r=e.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
    for(let i=1;i<=count;i++){await call('Input.dispatchMouseEvent',{type:'mousePressed',button:'left',clickCount:i,...point});await call('Input.dispatchMouseEvent',{type:'mouseReleased',button:'left',clickCount:i,...point});}
  };
  const screenshot=async name=>{await sleep(250);const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await fs.writeFile(path.join(shots,name+'.png'),Buffer.from(shot.data,'base64'));};
  const page=async name=>{await click(`[data-page="${name}"]`);await sleep(250);};
  const setPath=async(side,directory)=>{
    await evaluate(`(()=>{document.getElementById('${side}Path').value=${JSON.stringify(directory)};document.getElementById('${side}PathForm').requestSubmit()})()`);
    await until(()=>evaluate(`({path:document.getElementById('${side}Path').value,ready:!document.getElementById('${side}Refresh').disabled})`),v=>v.ready&&v.path===directory,side+' directory');
  };
  const row=async(side,name,double=false)=>{
    await evaluate(`(()=>{const row=[...document.querySelectorAll('#${side}Files tr[data-name]')].find(e=>e.dataset.name===${JSON.stringify(name)});if(!row)throw Error('File row not found');row.id='qa-file-row'})()`);
    await click(double?'#qa-file-row td:nth-child(2)':'#qa-file-row input',double?2:1);
    await evaluate(`document.getElementById('qa-file-row')?.removeAttribute('id')`);
  };
  const state=()=>evaluate('window.studio.fileState()');
  const terminal=()=>until(state,v=>!v.busy&&['completed','failed','canceled'].includes(v.phase),'transfer terminal');
  const idlePanels=()=>until(()=>evaluate(`!document.getElementById('localRefresh').disabled&&!document.getElementById('remoteRefresh').disabled`),Boolean,'panels ready');
  const results=[];
  try {
    await until(()=>evaluate(`Boolean(document.getElementById('name')?.value.length)`),Boolean,'device preset');
    if((await evaluate('window.studio.state()')).ssh!=='connected')await evaluate(`(()=>{document.getElementById('password').value=${JSON.stringify(password)};document.getElementById('deviceForm').requestSubmit()})()`);
    await until(()=>evaluate('window.studio.state()'),v=>{if(v.ssh==='error')throw Error(v.message);return v.ssh==='connected';},'device connection');
    await page('files');await idlePanels();await setPath('local',upload);await setPath('remote',real.remoteBase);
    await row('local','中文目录 空格',true);
    await until(()=>evaluate(`document.getElementById('localPath').value`),v=>v.endsWith('中文目录 空格'),'double-click directory');
    await click('#localUp');await idlePanels();assert.equal(await evaluate(`document.getElementById('localPath').value`),upload);results.push('Real mouse double-click/up navigation passed');
    await click('#remoteNew');await evaluate(`document.getElementById('folderName').value=${JSON.stringify(folderName)}`);await click('#folderCreate');
    await until(()=>evaluate(`document.getElementById('folderDialog').open`),v=>!v,'mkdir dialog closed');await idlePanels();await row('remote',folderName,true);
    const destination=real.remoteBase+'/'+folderName;await until(()=>evaluate(`document.getElementById('remotePath').value`),v=>v===destination,'new remote directory');
    await row('local','中文目录 空格');await click('#uploadFiles');
    await until(()=>evaluate(`!![...document.querySelectorAll('#remoteFiles tr[data-name]')].find(e=>e.dataset.name==='中文目录 空格')`),Boolean,'uploaded directory visible');
    let transfer=await terminal();assert.equal(transfer.phase,'completed',transfer.message);await idlePanels();await screenshot('v02-release-upload');results.push('Packaged UI mkdir and upload passed');
    await setPath('local',download);await row('remote','中文目录 空格');await click('#downloadFiles');
    await until(()=>fs.stat(path.join(download,'中文目录 空格/程序 示例.py')).then(()=>true,()=>false),Boolean,'download published');
    transfer=await terminal();assert.equal(transfer.phase,'completed',transfer.message);await idlePanels();
    for(const hash of real.hashes){const crypto=require('node:crypto');assert.equal(crypto.createHash('sha256').update(await fs.readFile(path.join(download,'中文目录 空格',hash.name))).digest('hex'),hash.sha256);}
    await screenshot('v02-release-download');results.push('Packaged UI download hashes passed');
    await fs.writeFile(path.join(download,'中文目录 空格/程序 示例.py'),'# GUI overwrite cancellation must preserve remote original\n');
    await row('local','中文目录 空格');await click('#uploadFiles');
    await until(state,v=>v.phase==='confirming','native overwrite confirmation');
    console.log('UI_CHECK_WAITING_OVERWRITE_CONFIRM: choose Cancel Transfer in the native dialog.');
    transfer=await until(state,v=>['canceled','completed','failed'].includes(v.phase)&&!v.busy,'overwrite dialog canceled',120000);assert.equal(transfer.phase,'canceled','Native dialog did not cancel the transfer');results.push('Native overwrite dialog cancel passed');await idlePanels();
    await page('desktop');await click('#startDesktop');
    await until(()=>evaluate('window.studio.state()'),v=>{if(v.desktop==='error')throw Error(v.message);return v.desktop==='connected';},'desktop connected');await screenshot('v02-release-desktop');
    await page('files');await setPath('local',upload);await fs.writeFile(path.join(upload,'桌面并行.txt'),'SFTP and desktop together\n');await click('#localRefresh');await idlePanels();
    await row('local','桌面并行.txt');await click('#uploadFiles');await until(()=>evaluate(`!![...document.querySelectorAll('#remoteFiles tr[data-name]')].find(e=>e.dataset.name==='桌面并行.txt')`),Boolean,'parallel upload visible');
    transfer=await terminal();assert.equal(transfer.phase,'completed',transfer.message);assert.equal((await evaluate('window.studio.state()')).desktop,'connected');await idlePanels();
    await row('local','大文件.bin');await evaluate(`document.getElementById('transferRate').value='1048576'`);await click('#uploadFiles');
    await until(state,v=>v.phase==='transferring'&&v.bytes>0,'throttled upload progress');await screenshot('v02-release-progress');await click('#cancelTransfer');
    transfer=await until(state,v=>v.phase==='canceled'&&!v.busy,'cancel finished');await idlePanels();assert.equal((await evaluate('window.studio.state()')).desktop,'connected');
    assert.equal(await evaluate(`[...document.querySelectorAll('#remoteFiles tr[data-name]')].some(e=>e.dataset.name==='大文件.bin'||e.dataset.name.endsWith('.part'))`),false);
    await screenshot('v02-release-canceled');results.push('GUI rate limit/progress/cancel retained desktop connection');
    await call('Emulation.setDeviceMetricsOverride',{width:1000,height:680,deviceScaleFactor:1,mobile:false});await screenshot('v02-release-files-small');
    assert.equal(await evaluate('document.documentElement.scrollWidth>innerWidth'),false);await call('Emulation.clearDeviceMetricsOverride');
    await page('desktop');await screenshot('v02-release-final-desktop');
    const record={passed:true,exe:target.url,results,localDownload:download,remoteDirectory:destination,desktopRetained:true};
    await fs.writeFile(path.join(root,'05-测试与验证/local/v02-ui-validation.json'),JSON.stringify(record,null,2));console.log(JSON.stringify(record,null,2));
  } finally {socket.close();}
})().catch(error=>{console.error(error.stack);process.exitCode=1;});
