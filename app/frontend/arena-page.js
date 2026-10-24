import {t,onLanguageChange} from './i18n.js';
import {sampleRates,metricPath,worldToSvg} from './arena-model.js';
const $=id=>document.getElementById(id);
const bytes=n=>n==null?'—':n<1024?`${n.toFixed(0)} B`:n<1024**2?`${(n/1024).toFixed(1)} KiB`:n<1024**3?`${(n/1024**2).toFixed(1)} MiB`:`${(n/1024**3).toFixed(2)} GiB`;
export class ArenaPage {
  constructor(){
    this.visible=false;this.connected=false;this.samples=[];this.epoch=0;this.cameraIntent=0;this.cameraId=null;this.cameraPhase='idle';this.cameraMessage='';this.poseId=null;this.poseIntent=0;this.poseValue={phase:'unavailable'};
    window.studio.onArenaPose(state=>{if(state.id===this.poseId){this.poseValue=state;this.poseReceived=performance.now();this.renderPose();}});
    window.studio.onArenaVisibility(({hidden})=>{if(hidden===Boolean(this.nativeHidden))return;this.nativeHidden=hidden;if(hidden)this.pause();else if(this.visible)this.resume();});
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
  pause(){this.epoch++;clearTimeout(this.metricTimer);clearInterval(this.poseTimer);this.stopCamera();this.stopPose();}
  resume(){if(!this.visible||document.hidden||this.nativeHidden)return;clearTimeout(this.metricTimer);clearInterval(this.poseTimer);const epoch=++this.epoch;this.previous=null;this.latest=null;this.samples=[];this.metricsError='';if(this.connected){this.pollMetrics(epoch);this.loadCameras();this.startPose();this.poseTimer=setInterval(()=>this.renderPose(),250);}this.render();}
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
    try{const state=await window.studio.startArenaCamera($('arenaCameraDevice').value,Number($('arenaCameraRate').value));
      if(intent!==this.cameraIntent||!this.visible||!this.connected||document.hidden){await window.studio.stopArenaCamera(state.id);return;}
      this.cameraId=state.id;this.previewUrl=state.streamUrl;$('arenaCameraImage').src=state.streamUrl;this.displayFps=null;this.pollCamera(intent,state.id);
    }catch(error){if(intent===this.cameraIntent){this.cameraPhase='error';this.cameraMessage=error.message;this.renderCamera();}}
  }
  async stopCamera(){
    this.cameraIntent++;clearTimeout(this.cameraTimer);const id=this.cameraId;this.cameraId=null;this.cameraPhase='idle';this.cameraMessage='';this.clearImage();this.renderCamera();
    if(id!==null){this.stopping=true;this.renderCamera();try{await window.studio.stopArenaCamera(id);}catch(error){this.cameraMessage=error.message;}finally{this.stopping=false;this.renderCamera();}}
  }
  clearImage(){
    $('arenaCameraImage').removeAttribute('src');this.previewUrl=null;this.displayFps=null;
  }
  async pollCamera(intent,id){
    try{const state=await window.studio.arenaCameraFrame(id,0);if(intent!==this.cameraIntent||!this.visible)return;
      this.cameraPhase=state.phase;this.cameraMessage=state.message;
      this.displayFps=state.streamFps;

      if(state.phase==='idle'||state.phase==='error'){this.cameraId=null;this.clearImage();}
    }catch(error){if(intent!==this.cameraIntent)return;this.cameraPhase='error';this.cameraMessage=error.message;await window.studio.stopArenaCamera(id).catch(()=>{});if(intent!==this.cameraIntent)return;this.cameraId=null;this.clearImage();}
    if(intent!==this.cameraIntent||!this.visible)return;this.renderCamera();if(this.cameraId!==null)this.cameraTimer=setTimeout(()=>this.pollCamera(intent,id),250);
  }
  renderCamera(){
    const active=this.cameraId!==null||this.cameraPhase==='connecting';
    $('arenaCameraToggle').textContent=t(active?'关闭摄像头':'开启摄像头');$('arenaCameraToggle').disabled=this.stopping||(!active&&(!this.connected||this.loadingDevices||!this.cameraDevices?.length));
    $('arenaCameraToggle').className='button '+(active?'secondary':'primary')+' compact';$('arenaCameraToggle').setAttribute('aria-pressed',String(active));
    if(!this.cameraDevices?.length&&!this.loadingDevices)$('arenaCameraDevice').replaceChildren(new Option(t('未检测到摄像头'),''));
    $('arenaCameraRate').disabled=active||this.stopping;
    $('arenaCameraDevice').disabled=!this.connected||active||this.loadingDevices||this.stopping||!this.cameraDevices?.length;$('arenaCameraRefresh').disabled=!this.connected||active||this.loadingDevices||this.stopping;
    const live=this.cameraPhase==='connected'&&this.previewUrl&&$('arenaCameraImage').naturalWidth>0;$('arenaCameraImage').hidden=!live;$('arenaCameraEmpty').hidden=live;
    let message=t(!this.connected?'设备未连接':this.stopping?'正在关闭摄像头…':this.cameraMessage||({connecting:'正在开启摄像头…',error:'摄像头连接失败',connected:'摄像头已开启',idle:'摄像头已关闭'}[this.cameraPhase]));
    if(live&&this.displayFps!=null)message+=' · '+this.displayFps.toFixed(1)+' FPS';
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
  async startPose(){
    const intent=++this.poseIntent;
    try{const state=await window.studio.startArenaPose();if(intent!==this.poseIntent||!this.visible||!this.connected){await window.studio.stopArenaPose(state.id);return;}this.poseId=state.id;this.poseValue=state;this.poseReceived=performance.now();this.renderPose();}
    catch{if(intent===this.poseIntent){this.poseValue={phase:'unavailable'};this.renderPose();}}
  }
  stopPose(){this.poseIntent++;const id=this.poseId;this.poseId=null;this.poseValue={phase:'unavailable'};this.renderPose();if(id!==null)window.studio.stopArenaPose(id).catch(()=>{});}
  renderPose(){
    const data=this.poseValue||{phase:'unavailable'},expired=data.phase==='live'&&performance.now()-(this.poseReceived||0)+(data.ageMs||0)>2000;
    const phase=!this.connected?'offline':expired?'stale':data.phase,live=phase==='live';
    const status=t({offline:'设备未连接',unavailable:'定位未接入',invalid:'定位数据无效',stale:'定位数据已过期',live:'定位已接入'}[phase]||'定位未接入');
    if($('arenaPoseStatus').textContent!==status)$('arenaPoseStatus').textContent=status;
    $('arenaPoseX').textContent=live?data.x.toFixed(2):'—';$('arenaPoseY').textContent=live?data.y.toFixed(2):'—';$('arenaRobot').style.display=live?'':'none';
    if(live){const point=worldToSvg(data.x,data.y);$('arenaRobot').setAttribute('transform','translate('+point.x+' '+point.y+')');$('arenaRobotHeading').style.display=data.heading==null?'none':'';if(data.heading!=null)$('arenaRobotHeading').setAttribute('transform','rotate('+(-data.heading)+')');$('arenaRobotLabel').setAttribute('text-anchor',data.x>2.9?'end':'start');$('arenaRobotLabel').setAttribute('x',data.x>2.9?'-90':'90');}
  }
  render(){this.renderCamera();this.renderMetrics();this.renderPose();}
}
