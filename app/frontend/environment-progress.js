import {showActivity} from './connection-indicator.js';
import {t,onLanguageChange} from './i18n.js';
const $=id=>document.getElementById(id);
const phases={'board-downloading':'板端下载组件',downloading:'电脑下载组件',transferring:'通过 SSH 传输组件',preparing:'板端校验并准备组件',ready:'运行环境已就绪',error:'运行环境准备未完成',canceled:'已取消运行环境准备'};
const features={desktop:'远程桌面',terminal:'远程终端',camera:'摄像头',files:'文件传输'};
let state=null,timer;
function render(){
  const panel=$('environmentStatus');
  panel.hidden=!state;
  $('appRoot').classList.toggle('environment-active',Boolean(state));
  if(!state){showActivity($('environmentTitle'),false);return;}
  const busy=['downloading','transferring','preparing','board-downloading'].includes(state.phase);
  $('environmentTitle').textContent=`${t(features[state.feature]||'运行环境')} · ${t(phases[state.phase]||'运行环境')}`;
  showActivity($('environmentTitle'),busy);
  const measured=['downloading','transferring'].includes(state.phase)&&state.total>0;
  const done=Math.min(Number(state.total)||0,Number(state.done)||0),total=Number(state.total)||0;
  const bytes=measured?`${(done/1048576).toFixed(1)} / ${(total/1048576).toFixed(1)} MiB · ${Math.floor(done/total*100)}%`:'';
  const source=state.phase==='board-downloading'?t('设备正在访问软件源…'):state.phase==='downloading'?(state.connections===0?t('使用电脑缓存'):state.connections?t('{connections} 路下载',{connections:state.connections}):t('正在连接下载服务…')):'';
  $('environmentDetail').textContent=['error','canceled'].includes(state.phase)?`${t(state.message)} ${t('重试原操作可复用已下载的电脑缓存。')}`:[state.package,source,bytes].filter(Boolean).join(' · ');
  const progress=$('environmentProgress');progress.hidden=!busy;
  if(measured)progress.value=done/total*100;else progress.removeAttribute('value');
  progress.setAttribute('aria-label',t(phases[state.phase]||'运行环境'));
  $('cancelEnvironment').hidden=!busy;$('cancelEnvironment').disabled=false;
  $('dismissEnvironment').hidden=busy;
  panel.classList.toggle('error',state.phase==='error');
}
window.studio.onEnvironment(value=>{
  if(value.phase==='ready'&&state?.feature!==value.feature)return;
  clearTimeout(timer);state=value;render();
  if(value.phase==='ready')timer=setTimeout(()=>{state=null;render();},2000);
});
window.studio.onState(value=>{if(value.ssh!=='connected'){clearTimeout(timer);state=null;render();}});
$('cancelEnvironment').onclick=async()=>{
  $('cancelEnvironment').disabled=true;
  try{await window.studio.cancelEnvironment();}catch(error){state={...state,phase:'error',message:error.message};render();}
};
$('dismissEnvironment').onclick=()=>{state=null;render();};
onLanguageChange(render);
