import {t,onLanguageChange} from './i18n.js';
import {SettingsDialog} from './settings.js';
import {FilePage} from './files-page.js';
import {TerminalDrawer} from './terminal-drawer.js';
import {LogsPage} from './logs-page.js';
import {ArenaPage} from './arena-page.js';
import {DesktopLifecycle} from './desktop-lifecycle.js';
import {observeDesktopStartup} from './desktop-startup.js';
import {installDesktopInteractions} from './desktop-interactions.js';
installDesktopInteractions();
const $ = id => document.getElementById(id);
let devices = [], selected = null, page = 'devices', rfb = null, desktopAttempt = 0;
let current = {ssh: 'disconnected', desktop: 'disconnected', message: '就绪'}, fit = true, readonly = false;
let toastTimer,toastMessage;
const connectionSteps=[['prepare','准备认证资料'],['resolve','解析设备地址'],['tcp','建立 TCP 连接'],['handshake','SSH 密钥交换'],['identity','验证主机身份'],['secure','完成 SSH 握手'],['authenticate','登录认证'],['ready','连接完成']];
const connectionAdvice={prepare:'检查私钥文件、格式和口令。',resolve:'检查设备地址或 DNS 设置。',tcp:'检查设备是否开机、网络是否互通，以及 SSH 端口是否正确。',handshake:'确认目标端口运行 SSH 服务，并检查服务端日志。',identity:'请核对设备指纹；身份变化时不要直接信任新密钥。',secure:'确认目标端口运行 SSH 服务，并检查服务端日志。',authenticate:'检查用户名、密码，或设备上的公钥授权。'};
const icon = name => {const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');const use = document.createElementNS(svg.namespaceURI, 'use');use.setAttribute('href', '#i-' + name);svg.append(use);return svg;};
const filesPage = new FilePage({toast,icon,goDevices:()=>showPage('devices')});
let terminalExpanded=false;
const terminalDrawer = new TerminalDrawer({toast,goDevices:()=>showPage('devices')});
const logsPage = new LogsPage({toast});
const settingsDialog = new SettingsDialog({toast});
const arenaPage = new ArenaPage();
let nativeHidden=false;
let desktopPhase=0,desktopStartupCleanup=()=>{},rebootPending=false;
const desktopSteps=['准备桌面客户端','启动远端桌面服务','建立 SSH 转发与 VNC 协商','接收并绘制首屏','准备桌面交互'];
function renderDesktopLoading(){
  $('desktopProgress').value=desktopPhase;
  $('desktopProgress').setAttribute('aria-valuetext',t('已完成 {done}/{total} 个阶段',{done:desktopPhase,total:desktopSteps.length}));
  $('desktopSteps').replaceChildren(...desktopSteps.map((label,index)=>{const item=document.createElement('li');item.textContent=t(label);item.className=index<desktopPhase?'complete':index===desktopPhase?'active':'';if(index===desktopPhase)item.setAttribute('aria-current','step');return item;}));
}
const desktopLifecycle=new DesktopLifecycle({open:startDesktop,close:clearRfb,stop:()=>window.studio.stopDesktop(),onError:error=>toast(error.message)});
function updateDesktopActivity(){desktopLifecycle.setAllowed(page==='desktop'&&!nativeHidden&&!document.hidden&&current.ssh==='connected');}
function renderDesktopModeHint(){const mode=$('desktopMode');mode.title=t(mode.value==='economy'?'传输画面最高 960×540，不改变设备分辨率':'保留设备原始分辨率');}
window.studio.onArenaVisibility(({hidden})=>{nativeHidden=Boolean(hidden);updateDesktopActivity();});
document.addEventListener('visibilitychange',updateDesktopActivity);
document.querySelectorAll('.mark,.wordmark').forEach(element=>element.addEventListener('dragstart',event=>event.preventDefault()));
function toast(message) {toastMessage=message;$('toast').textContent = t(message);$('toast').hidden = false;clearTimeout(toastTimer);toastTimer = setTimeout(() => $('toast').hidden = true, 6000);}
function profile() {return {id: selected?.id, name: $('name').value, host: $('host').value, port: Number($('port').value), username: $('username').value, display: $('display').value, xauthority: $('xauthority').value,authMethod:$('authMethod').value,privateKeyPath:$('privateKeyPath').value,rememberPassword:$('rememberPassword').checked};}
function savedCredentialMatches() {
  const p=profile(),s=selected;
  return Boolean(s?.hasSavedCredential&&p.rememberPassword&&p.host.trim()===s.host&&p.port===s.port&&p.username.trim()===s.username&&p.authMethod===(s.authMethod||'password')&&(p.authMethod!=='key'||p.privateKeyPath.trim()===s.privateKeyPath));
}
function renderAuth() {
  const key=$('authMethod').value==='key',saved=savedCredentialMatches();
  $('keyFileField').hidden=!key;$('credentialLabel').textContent=t(key?'密钥口令':'密码');
  $('rememberLabel').textContent=t(key?'记住密钥口令':'记住密码');
  $('credentialHint').hidden=!saved;$('credentialHint').textContent=t(saved?'已保存':'');
  $('password').placeholder=t(saved?'已加密保存，留空使用':key?'加密私钥请输入口令，未加密可留空':'输入设备登录密码');
}
for(const id of ['host','port','username','authMethod','privateKeyPath','rememberPassword'])$(id).addEventListener('input',renderAuth);
$('choosePrivateKey').onclick=async()=>{try{const path=await window.studio.choosePrivateKey();if(path){$('privateKeyPath').value=path;renderAuth();}}catch(error){toast(error.message);}};
function fill(device) {
  selected = device || null;
  const values = device || {name: '', host: '', port: 22, username: '', display: ':0', xauthority: ''};
  for (const key of ['name', 'host', 'port', 'username', 'display', 'xauthority']) $(key).value = values[key] ?? '';
  $('password').value = '';
  $('authMethod').value=values.authMethod||'password';$('privateKeyPath').value=values.privateKeyPath||'';$('rememberPassword').checked=Boolean(values.rememberPassword);renderAuth();
  $('editorTitle').textContent = device?.name || t('添加一台设备');
  $('removeDevice').hidden = !device;
  renderDevices();
}
function renderDevices() {
  $('deviceList').replaceChildren();$('deviceCount').textContent = devices.length;
  if (!devices.length) {const text = document.createElement('p');text.className = 'no-devices';text.textContent = t('暂无设备');$('deviceList').append(text);}
  for (const device of devices) {
    const button = document.createElement('button');button.className = 'device-item' + (selected?.id === device.id ? ' active' : '');
    button.disabled = ['connected', 'connecting'].includes(current.ssh);
    const title = document.createElement('strong');title.textContent = device.name;
    const detail = document.createElement('small');detail.textContent = `${device.username}@${device.host}:${device.port}`;
    const wrap = document.createElement('div');wrap.append(title, detail);button.append(icon('device'), wrap);button.onclick = () => fill(device);$('deviceList').append(button);
  }
}
function showPage(next) {
  page = next;
  for (const button of document.querySelectorAll('[data-page]')) {button.classList.toggle('active', button.dataset.page === next);button.setAttribute('aria-current', button.dataset.page === next ? 'page' : 'false');}
  for (const section of document.querySelectorAll('.page')) section.hidden = section.id !== 'page-' + next;
  if(next==='files')filesPage.show();else filesPage.hide();
  if(next==='logs')logsPage.show();else logsPage.hide();
  if(next==='arena')arenaPage.show();else arenaPage.hide();
  renderTerminalDrawer();
  updateDesktopActivity();
}
function renderTerminalDrawer() {
  const visible=page!=='devices'&&terminalExpanded;
  $('toggleTerminal').hidden=page==='devices';
  $('toggleTerminal').setAttribute('aria-expanded',String(visible));
  $('terminalDrawer').inert=!visible;
  $('terminalDrawer').setAttribute('aria-hidden',String(!visible));
  $('appRoot').classList.toggle('terminal-open',visible);
  if(visible)terminalDrawer.show();else terminalDrawer.hide();
}
$('toggleTerminal').onclick=()=>{terminalExpanded=!terminalExpanded;renderTerminalDrawer();};
$('closeTerminalDrawer').onclick=()=>{terminalExpanded=false;renderTerminalDrawer();$('toggleTerminal').focus();};
function clearRfb() {
  desktopAttempt++;
  desktopStartupCleanup();desktopStartupCleanup=()=>{};desktopPhase=0;
  const previous = rfb;rfb = null;
  if(previous)previous.disconnect();
  $('desktopCanvas').replaceChildren();
}
function renderState(state,presentationOnly=false) {
  const revealConnectionResult=(state.ssh==='connecting'&&current.ssh!=='connecting')||(state.ssh==='error'&&current.ssh!=='error');
  current = state;
  if(!presentationOnly){filesPage.setConnection(state,selected?.id);terminalDrawer.setConnection(state);arenaPage.setConnection(state);}
  const connected = state.ssh === 'connected', connecting = state.ssh === 'connecting';
  const status = connected ? '设备已连接' : connecting ? '正在连接设备' : state.ssh === 'error' ? '连接失败' : '设备未连接';
  $('connectionBadge').className = 'badge ' + state.ssh;
  $('connectionBadge').lastElementChild.textContent = t(status);
  $('connectDevice').querySelector('span').textContent = t(connecting ? '取消连接' : connected ? '断开设备' : '连接设备');
  for (const id of ['name', 'host', 'port', 'username', 'password', 'authMethod','privateKeyPath','rememberPassword','choosePrivateKey','display', 'xauthority', 'saveDevice', 'newDevice', 'removeDevice', 'importProfiles', 'rollbackProfiles']) $(id).disabled = connected || connecting;
  $('resultTitle').textContent = t(connected ? '设备连接成功' : connecting ? '正在建立连接' : state.ssh === 'error' ? '连接失败' : '准备连接');
  const stepIndex=connectionSteps.findIndex(([key])=>key===state.connectionStage);
  $('connectionTarget').textContent=state.connectionTarget||'';
  $('connectionTarget').hidden=!state.connectionTarget;
  $('connectionSteps').replaceChildren();
  if(stepIndex>=0)for(const [index,[key,label]] of connectionSteps.entries()){
    const item=document.createElement('li');
    const result=index<stepIndex||connected?'已完成':index===stepIndex?(state.ssh==='error'?'失败':'进行中'):'等待';
    item.className=index<stepIndex||connected?'complete':index===stepIndex?(state.ssh==='error'?'failed':'active'):'';
    if(index===stepIndex)item.setAttribute('aria-current','step');
    item.append(document.createTextNode(t(label)),Object.assign(document.createElement('span'),{textContent:t(result)}));
    $('connectionSteps').append(item);
  }
  $('connectionAdvice').hidden=state.ssh!=='error'||!connectionAdvice[state.connectionStage];
  $('connectionAdvice').textContent=t(connectionAdvice[state.connectionStage]||'');
  $('resultMessage').textContent = t(state.message);
  $('connectionBadge').title=t(state.message);
  document.querySelector('.connection-result').hidden=!connected&&!connecting&&state.ssh!=='error';
  document.querySelector('.connection-result').className = 'connection-result ' + state.ssh;
  $('resultIcon').replaceChildren(icon(connected ? 'check' : 'connect'));
  $('goDesktop').hidden = !connected;
  const desktopBusy = state.desktop === 'connecting', desktopLive = state.desktop === 'connected';
  if(desktopBusy&&state.desktopStage==='service')desktopPhase=Math.max(1,desktopPhase);
  $('desktopProgress').hidden=!desktopBusy;$('desktopSteps').hidden=!desktopBusy;
  if(desktopBusy)renderDesktopLoading();
  $('desktopCanvas').setAttribute('aria-busy',String(desktopBusy));
  $('desktopEmpty').hidden=desktopLive;
  $('startDesktop').disabled = !connected || desktopBusy || Boolean(state.rebooting);
  $('startDesktop').querySelector('span').textContent = t(desktopBusy ? '正在连接…' : desktopLive ? '重新连接桌面' : '连接桌面');
  $('stopDesktop').disabled = !desktopBusy && !desktopLive;
  $('desktopMode').disabled=desktopBusy;
  for(const id of ['fitDesktop','actualSize','viewOnly'])$(id).disabled=desktopBusy;
  $('rebootDevice').disabled = !connected || desktopBusy || readonly || rebootPending || Boolean(state.rebooting);
  $('desktopStatus').className = desktopLive ? 'connected' : '';
  $('desktopStatus').replaceChildren(Object.assign(document.createElement('span'), {className:'dot'}), document.createTextNode(t(desktopLive ? '桌面已连接' : desktopBusy ? '正在连接桌面' : state.desktop === 'error' ? '桌面连接失败' : '桌面未连接')));
  $('desktopSubtitle').hidden=!connected;$('desktopSubtitle').textContent = connected ? `${state.device} · ${selected?.display || ':0'}` : '';
  if (!desktopLive) {
    $('desktopEmpty').hidden = false;
    $('desktopEmptyText').hidden=desktopBusy||state.desktop!=='error';
    $('desktopEmptyTitle').textContent = t(desktopBusy ? '正在打开远端设备桌面' : state.desktop === 'error' ? '暂时无法打开桌面' : '远程桌面未连接');
    $('desktopEmptyText').textContent = t(desktopBusy ? '正在建立加密桌面通道，请稍候。' : state.desktop === 'error' ? state.message : connected ? '设备连接已就绪，点击连接桌面即可开始。' : '先连接设备，再打开远程桌面。连接后可直接使用鼠标和键盘操作。');
    $('desktopEmptyAction').hidden = desktopBusy;
    $('desktopEmptyAction').replaceChildren(document.createTextNode(t(connected ? '连接远程桌面' : '前往连接设备')), icon('arrow'));
  }
  if (!connected && rfb) clearRfb();
  renderDevices();
  updateDesktopActivity();
  if(revealConnectionResult)requestAnimationFrame(()=>document.querySelector('.connection-result').scrollIntoView({block:'nearest',behavior:matchMedia('(prefers-reduced-motion:reduce)').matches?'instant':'smooth'}));
}
async function startDesktop() {
  clearRfb();
  renderState({...current,desktop:'connecting',desktopStage:'prepare'});
  const attempt = desktopAttempt;
  try {
    const {default:RFB} = await import('@novnc/novnc');
    if (attempt !== desktopAttempt) return;
    const {url,id,tuning} = await window.studio.startDesktop();
    if(attempt!==desktopAttempt)return;
    desktopPhase=2;renderDesktopLoading();
    const remote = new RFB($('desktopCanvas'), url, {shared: true});
    rfb = remote;
    remote.scaleViewport = fit;remote.resizeSession = false;remote.viewOnly = true;
    remote.qualityLevel = tuning?.quality??6;remote.compressionLevel = tuning?.compression??2;remote.background = '#17222b';
    const fail=message=>{if(rfb!==remote)return;clearRfb();window.studio.desktopLost(id,false,message).catch(error=>toast(error.message));};
    remote.addEventListener('connect', () => {
      if(rfb!==remote)return;
      desktopPhase=3;renderDesktopLoading();
      desktopStartupCleanup=observeDesktopStartup(remote,{
        onFrame:()=>{if(rfb===remote){desktopPhase=4;renderDesktopLoading();}},
        onReady:()=>{if(rfb!==remote)return;window.studio.desktopReady(id).then(()=>{if(rfb===remote){remote.viewOnly=readonly;remote.focus();}}).catch(error=>fail(error.message));},
        onError:fail
      });
    });
    remote.addEventListener('disconnect', event => {
      if (rfb !== remote) return;
      clearRfb();
      window.studio.desktopLost(id,Boolean(event.detail.clean)).catch(error => toast(error.message));
    });
    remote.addEventListener('securityfailure', () => toast('桌面握手失败，请断开并重新连接。'));
    remote.addEventListener('credentialsrequired', () => toast('桌面服务要求额外认证，请检查组件配置。'));
  } catch (error) {
    if (attempt === desktopAttempt) {renderState({...current, desktop:'error', message:error.message});toast(error.message);}
  }
}
document.querySelectorAll('[data-page]').forEach(button => button.onclick = () => showPage(button.dataset.page));
async function migrateProfiles(rollback=false){
 for(const id of ['connectDevice','saveDevice','newDevice','removeDevice','importProfiles','rollbackProfiles'])$(id).disabled=true;
 try{const result=await (rollback?window.studio.rollbackProfiles():window.studio.migrateProfiles());if(result.canceled){toast('已取消资料操作。');return;}devices=await window.studio.profiles();fill(devices[0]||null);toast(rollback?'已恢复导入前资料，日志文件保留。':`资料导入完成，新增 ${result.addedDevices} 台设备。备份：${result.backup}`);}catch(error){toast(error.message);}finally{renderState(await window.studio.state());}
}
$('importProfiles').onclick=()=>migrateProfiles(false);$('rollbackProfiles').onclick=()=>migrateProfiles(true);
$('newDevice').onclick = () => {fill(null);$('name').focus();};
$('saveDevice').onclick = async () => {try {const saved = await window.studio.save(profile(),$('password').value);devices = await window.studio.profiles();fill(saved);toast(saved.rememberPassword?'配置及加密凭据已保存。':'配置已保存，密码/密钥口令未保存。');} catch (error) {toast(error.message);}};
$('removeDevice').onclick = async () => {if (selected && await window.studio.remove(selected.id)) {devices = await window.studio.profiles();fill(devices[0] || null);}};
$('deviceForm').onsubmit = async event => {
  event.preventDefault();
  if (['connected', 'connecting'].includes(current.ssh)) {clearRfb();await window.studio.disconnect();return;}
  const password = $('password').value;
  if ($('authMethod').value==='password'&&!password&&!savedCredentialMatches()) {toast('请输入设备登录密码。');$('password').focus();return;}
  try {
    const saved = await window.studio.save(profile(),password);selected = saved;devices = await window.studio.profiles();renderAuth();
    await window.studio.connect(saved, password);
    $('password').value='';renderAuth();
  } catch (error) {toast(error.message);}
};
$('goDesktop').onclick = () => {showPage('desktop');if (current.desktop !== 'connected'){desktopLifecycle.start();}};

$('desktopEmptyAction').onclick = () => {if(current.ssh==='connected'){desktopLifecycle.start();}else showPage('devices');};
$('startDesktop').onclick = () => {desktopLifecycle.start();};
$('stopDesktop').onclick = () => desktopLifecycle.stop();
$('desktopMode').onchange=async()=>{const select=$('desktopMode');select.disabled=true;try{await window.studio.setDesktopMode(select.value);renderDesktopModeHint();if(desktopLifecycle.wanted)await desktopLifecycle.start();}catch(error){toast(error.message);}finally{select.disabled=current.desktop==='connecting';}};
window.studio.settings().then(info=>{$('desktopMode').value=info.desktopMode||'smooth';renderDesktopModeHint();}).catch(error=>toast(error.message));
function setFit(value) {fit = value;if (rfb) rfb.scaleViewport = fit;$('fitDesktop').classList.toggle('selected', fit);$('actualSize').classList.toggle('selected', !fit);}
$('fitDesktop').onclick = () => setFit(true);$('actualSize').onclick = () => setFit(false);
$('viewOnly').onclick = () => {readonly = !readonly;if (rfb) rfb.viewOnly = readonly||current.desktop!=='connected';$('viewOnly').classList.toggle('selected', readonly);$('viewOnly').setAttribute('aria-pressed', String(readonly));$('rebootDevice').disabled = current.ssh!=='connected' || readonly || rebootPending || Boolean(current.rebooting);};
$('rebootDevice').onclick=async()=>{if(rebootPending)return;rebootPending=true;renderState(current,true);try{const result=await window.studio.rebootDevice();if(result.requested){await desktopLifecycle.stop();toast('重启请求已提交，请稍后重新连接设备。');}}catch(error){toast(error.message);}finally{rebootPending=false;renderState(await window.studio.state(),true);}};
$('fullscreen').onclick = () => window.studio.fullscreen();
window.studio.onState(renderState);
onLanguageChange(()=>{renderAuth();renderState(current,true);renderDesktopModeHint();if(!selected)$('editorTitle').textContent=t('添加一台设备');if(toastMessage)$('toast').textContent=t(toastMessage);});
(async () => {
  try {devices = await window.studio.profiles();fill(devices[0] || null);renderState(await window.studio.state());} catch (error) {toast('读取设备配置失败：' + error.message);}
})();
