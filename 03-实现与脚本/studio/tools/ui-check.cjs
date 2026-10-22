// Inspect/export only this application's own renderer. Never read form secrets.
const fs = require('node:fs');
const path = require('node:path');
const {WebSocket} = require('ws');

(async () => {
  const targets=await (await fetch(`http://127.0.0.1:${Number(process.env.STUDIO_DEBUG_PORT||9355)}/json`)).json();
  const target=targets.find(t=>t.type==='page' && t.url.startsWith('file:') && (t.url.endsWith('/studio/ui/index.html') || t.url.endsWith('/resources/app.asar/ui/index.html')));
  if(!target) throw new Error('Studio renderer not found');
  const socket=new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
  let id=0;const waiting=new Map();
  socket.on('message',data=>{const r=JSON.parse(data);if(r.id){const item=waiting.get(r.id);waiting.delete(r.id);r.error?item.reject(new Error(r.error.message)):item.resolve(r.result);}});
  const call=(method,params={})=>new Promise((resolve,reject)=>{const request=++id;waiting.set(request,{resolve,reject});socket.send(JSON.stringify({id:request,method,params}));});
  try {
    if (process.argv[3]) {
      const page=process.argv[3];
      if(!['devices','desktop','files','arena'].includes(page))throw new Error('Invalid page');
      await call('Runtime.evaluate',{expression:`document.querySelector('[data-page="${page}"]').click()`});
      await new Promise(resolve=>setTimeout(resolve,250));
    }
    if (process.argv[4]) {
      const width=Number(process.argv[4]),height=Number(process.argv[5]);
      if(!Number.isInteger(width)||!Number.isInteger(height)||width<1000||height<680)throw new Error('Invalid window size');
      await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      await new Promise(resolve=>setTimeout(resolve,250));
    }
    await call('Runtime.evaluate',{expression:`new Promise(resolve=>{const link=document.querySelector('link[rel="stylesheet"]');link.onload=()=>resolve(true);link.href='style.css?qa='+Date.now()})`,awaitPromise:true});
    const expression=`JSON.stringify({title:document.title,viewport:{width:innerWidth,height:innerHeight},page:document.querySelector('.page:not([hidden])')?.id,activeNav:document.querySelector('.nav-item.active')?.dataset.page,status:document.getElementById('globalStatus').textContent,desktop:document.getElementById('desktopStatus').textContent,canvas:[...document.querySelectorAll('canvas')].map(c=>({width:c.width,height:c.height,visible:c.getBoundingClientRect().width>0})),overflow:document.documentElement.scrollWidth>innerWidth})`;
    const result=await call('Runtime.evaluate',{expression,returnByValue:true});
    const record=JSON.parse(result.result.value);console.log(JSON.stringify(record,null,2));
    const output=path.resolve(__dirname,'../../../05-测试与验证/local/screenshots');fs.mkdirSync(output,{recursive:true});
    const label=process.argv[2]||'studio';
    if(!/^[\w-]+$/.test(label))throw new Error('Invalid screenshot label');
    const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
    fs.writeFileSync(path.join(output,label+'.png'),Buffer.from(shot.data,'base64'));
    fs.writeFileSync(path.join(output,label+'.json'),JSON.stringify(record,null,2));
  } finally {socket.close();}
})().catch(error=>{console.error(error.message);process.exitCode=1;});
