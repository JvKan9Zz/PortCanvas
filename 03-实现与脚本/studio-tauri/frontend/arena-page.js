import {t,onLanguageChange} from './i18n.js';
import {sampleRates,metricPath} from './arena-model.js';
const $=id=>document.getElementById(id);
const bytes=n=>n==null?'—':n<1024?`${n.toFixed(0)} B`:n<1024**2?`${(n/1024).toFixed(1)} KiB`:n<1024**3?`${(n/1024**2).toFixed(1)} MiB`:`${(n/1024**3).toFixed(2)} GiB`;
export class ArenaPage {
  constructor(){
    this.visible=false;this.connected=false;this.samples=[];this.epoch=0;this.cameraIntent=0;this.cameraId=null;this.cameraPhase='idle';this.cameraMessage='';
    $('arenaCameraToggle').onclick=()=>this.cameraId!==null||this.cameraPhase==='connecting'?this.stopCamera():this.startCamera();
    $('arenaCameraRefresh').onclick=()=>this.loadCameras();
    $('arenaCameraDevice').onchange=()=>this.stopCamera();
    onLanguageChange(()=>this.render());
    document.addEventListener('visibilitychange',()=>{if(document.hidden){this.pause();}else if(this.visible){this.resume();}});
    this.render();
  }
  setConnection(state){
    const connected=state.ssh==='connected',identity=connected?`${state.generation}:${state.deviceId}`:'';
    if(identity===this.identity&&connected===this.connected)return;
    this.identity=identity;this.connected=connected;this.pause();this.previous=null;this.latest=null;this.samples=[];this.metricsError='';this.cameraDevices=[];
    $('arenaCameraDevice').replaceChildren();if(this.visible&&!document.hidden)this.resume();this.render();
  }
  show(){if(this.visible)return;this.visible=true;this.resume();}
  hide(){this.visible=false;this.pause();}
  pause(){this.epoch++;clearTimeout(this.metricTimer);this.stopCamera();}
  resume(){if(!this.visible||document.hidden)return;const epoch=++this.epoch;this.previous=null;this.latest=null;this.samples=[];this.metricsError='';if(this.connected){this.pollMetrics(epoch);this.loadCameras();}this.render();}
  async pollMetrics(epoch){
    try{const sample=await window.studio.arenaMetrics();if(epoch!==this.epoch||!this.visible)return;
      const rates=sampleRates(this.previous,sample);this.previous=sample;this.latest={...sample,...rates};this.metricsError='';
      this.samples.push({clock:sample.clock,cpu:rates.cpu,memory:100*(sample.memoryTotal-sample.memoryAvailable)/sample.memoryTotal});this.samples=this.samples.filter(p=>sample.clock-p.clock<=60);
    }catch(error){if(epoch!==this.epoch)return;this.metricsError=error.message;this.latest=null;this.previous=null;}
    if(epoch!==this.epoch||!this.visible)return;this.renderMetrics();this.metricTimer=setTimeout(()=>this.pollMetrics(epoch),2000);
  }
  async loadCameras(){
    if(!this.connected||!this.visible||document.hidden)return;
    const epoch=this.epoch;this.loadingDevices=true;this.renderCamera();
    try{const devices=await window.studio.arenaCameras();if(epoch!==this.epoch)return;const selected=$('arenaCameraDevice').value;this.cameraDevices=devices;const options=devices.map(d=>new Option(`${d.name} · ${d.path}`,d.path));$('arenaCameraDevice').replaceChildren(...options);if(devices.some(d=>d.path===selected))$('arenaCameraDevice').value=selected;this.cameraMessage=devices.length?'':'没有可用摄像头';
    }catch(error){if(epoch!==this.epoch)return;this.cameraDevices=[];this.cameraMessage=error.message;}
    finally{if(epoch===this.epoch){this.loadingDevices=false;this.renderCamera();}}
  }
  async startCamera(){
    if(!this.visible||!this.connected||document.hidden||!$('arenaCameraDevice').value)return;
    const intent=++this.cameraIntent;this.cameraPhase='connecting';this.cameraMessage='';this.renderCamera();
    try{const state=await window.studio.startArenaCamera($('arenaCameraDevice').value);
      if(intent!==this.cameraIntent||!this.visible||!this.connected||document.hidden){await window.studio.stopArenaCamera(state.id);return;}
      this.cameraId=state.id;this.sequence=0;this.pollCamera(intent,state.id);
    }catch(error){if(intent===this.cameraIntent){this.cameraPhase='error';this.cameraMessage=error.message;this.renderCamera();}}
  }
  async stopCamera(){
    this.cameraIntent++;clearTimeout(this.cameraTimer);const id=this.cameraId;this.cameraId=null;this.cameraPhase='idle';this.cameraMessage='';this.clearImage();this.renderCamera();
    if(id!==null){this.stopping=true;this.renderCamera();try{await window.studio.stopArenaCamera(id);}catch(error){this.cameraMessage=error.message;}finally{this.stopping=false;this.renderCamera();}}
  }
  clearImage(){
    $('arenaCameraImage').removeAttribute('src');
    if(this.frameUrl){URL.revokeObjectURL(this.frameUrl);this.frameUrl=null;}
  }
  async pollCamera(intent,id){
    try{const state=await window.studio.arenaCameraFrame(id,this.sequence);if(intent!==this.cameraIntent||!this.visible)return;
      this.cameraPhase=state.phase;this.cameraMessage=state.message;
      if(state.frame){
        const bytes=Uint8Array.from(atob(state.frame.data),c=>c.charCodeAt(0)),url=URL.createObjectURL(new Blob([bytes],{type:'image/jpeg'}));let published=false;
        try{const image=new Image();image.src=url;await image.decode();if(intent!==this.cameraIntent||!this.visible)return;const old=this.frameUrl;$('arenaCameraImage').src=url;this.frameUrl=url;published=true;this.sequence=state.frame.sequence;if(old)URL.revokeObjectURL(old);}finally{if(!published)URL.revokeObjectURL(url);}
      }
      if(state.phase==='idle'||state.phase==='error'){this.cameraId=null;this.clearImage();}
    }catch(error){if(intent!==this.cameraIntent)return;this.cameraPhase='error';this.cameraMessage=error.message;await window.studio.stopArenaCamera(id).catch(()=>{});if(intent!==this.cameraIntent)return;this.cameraId=null;this.clearImage();}
    if(intent!==this.cameraIntent||!this.visible)return;this.renderCamera();if(this.cameraId!==null)this.cameraTimer=setTimeout(()=>this.pollCamera(intent,id),125);
  }
  renderCamera(){
    const active=this.cameraId!==null||this.cameraPhase==='connecting';
    $('arenaCameraToggle').textContent=t(active?'关闭摄像头':'开启摄像头');$('arenaCameraToggle').disabled=this.stopping||(!active&&(!this.connected||this.loadingDevices||!this.cameraDevices?.length));
    $('arenaCameraToggle').className='button '+(active?'secondary':'primary')+' compact';$('arenaCameraToggle').setAttribute('aria-pressed',String(active));
    if(!this.cameraDevices?.length&&!this.loadingDevices)$('arenaCameraDevice').replaceChildren(new Option(t('未检测到摄像头'),''));
    $('arenaCameraDevice').disabled=!this.connected||active||this.loadingDevices||this.stopping||!this.cameraDevices?.length;$('arenaCameraRefresh').disabled=!this.connected||active||this.loadingDevices||this.stopping;
    const live=this.cameraPhase==='connected'&&$('arenaCameraImage').hasAttribute('src');$('arenaCameraImage').hidden=!live;$('arenaCameraEmpty').hidden=live;
    const message=t(!this.connected?'设备未连接':this.stopping?'正在关闭摄像头…':this.cameraMessage||({connecting:'正在开启摄像头…',error:'摄像头连接失败',connected:'摄像头已开启',idle:'摄像头已关闭'}[this.cameraPhase]));
    if($('arenaCameraStatus').textContent!==message)$('arenaCameraStatus').textContent=message;
    $('arenaCameraStatus').classList.toggle('error',this.cameraPhase==='error');
  }
  renderMetrics(){
    const data=this.latest,percent=n=>Number.isFinite(n)?n.toFixed(1)+'%':'—';
    $('arenaMetricsStatus').textContent=t(!this.connected?'设备未连接':this.metricsError?'读取失败':data?'实时':'正在读取…');$('arenaMetricsStatus').title=this.metricsError||'';
    const memory=data?100*(data.memoryTotal-data.memoryAvailable)/data.memoryTotal:null;
    $('arenaCpu').textContent=percent(data?.cpu);$('arenaCpuBar').value=data?.cpu||0;
    $('arenaMemory').textContent=data?`${bytes(data.memoryTotal-data.memoryAvailable)} / ${bytes(data.memoryTotal)}`:'—';$('arenaMemoryBar').value=memory||0;
    $('arenaSwap').textContent=data?(data.swapTotal?`${bytes(data.swapTotal-data.swapFree)} / ${bytes(data.swapTotal)}`:t('未启用')):'—';
    $('arenaLoad').textContent=data?data.load.map(n=>n.toFixed(2)).join(' / '):'—';
    $('arenaUptime').textContent=data?t('{days}天 {hours}小时 {minutes}分',{days:Math.floor(data.uptime/86400),hours:Math.floor(data.uptime/3600)%24,minutes:Math.floor(data.uptime/60)%60}):'—';
    $('arenaReceive').textContent=bytes(data?.received)+ (data?.received!=null?'/s':'');$('arenaSend').textContent=bytes(data?.sent)+(data?.sent!=null?'/s':'');
    $('arenaRtt').textContent=data?Math.round(data.roundTripMs)+' ms':'—';
    $('arenaCpuPlot').setAttribute('d',metricPath(this.samples,'cpu'));$('arenaMemoryPlot').setAttribute('d',metricPath(this.samples,'memory'));
    $('arenaMetricsError').hidden=!this.metricsError;$('arenaMetricsError').textContent=t(this.metricsError||'');
  }
  render(){this.renderCamera();this.renderMetrics();}
}
