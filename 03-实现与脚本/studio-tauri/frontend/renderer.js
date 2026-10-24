import {t,onLanguageChange} from './i18n.js';
import {SettingsDialog} from './settings.js';
import {FilePage} from './files-page.js';
import {TerminalDrawer} from './terminal-drawer.js';
import {LogsPage} from './logs-page.js';
import {ArenaPage} from './arena-page.js';
const $ = id => document.getElementById(id);
let devices = [], selected = null, page = 'devices', rfb = null, desktopAttempt = 0;
let current = {ssh: 'disconnected', desktop: 'disconnected', message: '就绪'}, fit = true, readonly = false;
let toastTimer,toastMessage;
const icon = name => {const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');const use = document.createElementNS(svg.namespaceURI, 'use');use.setAttribute('href', '#i-' + name);svg.append(use);return svg;};
const filesPage = new FilePage({toast,icon,goDevices:()=>showPage('devices')});
let terminalExpanded=false;
const terminalDrawer = new TerminalDrawer({toast,goDevices:()=>showPage('devices')});
const logsPage = new LogsPage({toast});
const settingsDialog = new SettingsDialog({toast});
const arenaPage = new ArenaPage();
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
  const previous = rfb;rfb = null;
  if (previous) previous.disconnect();
  $('desktopCanvas').replaceChildren();
}
function renderState(state,presentationOnly=false) {
  current = state;
  if(!presentationOnly){filesPage.setConnection(state,selected?.id);terminalDrawer.setConnection(state);arenaPage.setConnection(state);}
  const connected = state.ssh === 'connected', connecting = state.ssh === 'connecting';
  const status = connected ? '设备已连接' : connecting ? '正在连接设备' : state.ssh === 'error' ? '连接失败' : '设备未连接';
  $('connectionBadge').className = 'badge ' + state.ssh;
  $('connectionBadge').lastElementChild.textContent = t(status);
  $('connectDevice').querySelector('span').textContent = t(connecting ? '取消连接' : connected ? '断开设备' : '连接设备');
  for (const id of ['name', 'host', 'port', 'username', 'password', 'authMethod','privateKeyPath','rememberPassword','choosePrivateKey','display', 'xauthority', 'saveDevice', 'newDevice', 'removeDevice', 'importProfiles', 'rollbackProfiles']) $(id).disabled = connected || connecting;
  $('resultTitle').textContent = t(connected ? '设备连接成功' : connecting ? '正在建立连接' : state.ssh === 'error' ? '连接失败' : '准备连接');
  $('resultMessage').textContent = t(state.message);
  $('connectionBadge').title=t(state.message);
  document.querySelector('.connection-result').hidden=!connected&&!connecting&&state.ssh!=='error';
  document.querySelector('.connection-result').className = 'connection-result ' + state.ssh;
  $('resultIcon').replaceChildren(icon(connected ? 'check' : 'connect'));
  $('goDesktop').hidden = !connected;
  const desktopBusy = state.desktop === 'connecting', desktopLive = state.desktop === 'connected';
  $('startDesktop').disabled = !connected || desktopBusy;
  $('startDesktop').querySelector('span').textContent = t(desktopBusy ? '正在连接…' : desktopLive ? '重新连接桌面' : '连接桌面');
  $('stopDesktop').disabled = !desktopBusy && !desktopLive;
  $('sendCAD').disabled = !desktopLive || readonly;
  $('desktopStatus').className = desktopLive ? 'connected' : '';
  $('desktopStatus').replaceChildren(Object.assign(document.createElement('span'), {className:'dot'}), document.createTextNode(t(desktopLive ? '桌面已连接' : desktopBusy ? '正在连接桌面' : state.desktop === 'error' ? '桌面连接失败' : '桌面未连接')));
  $('desktopSubtitle').hidden=!connected;$('desktopSubtitle').textContent = connected ? `${state.device} · ${selected?.display || ':0'}` : '';
  if (!desktopLive) {
    $('desktopEmpty').hidden = false;
    $('desktopEmptyText').hidden=!desktopBusy&&state.desktop!=='error';
    $('desktopEmptyTitle').textContent = t(desktopBusy ? '正在打开远端设备桌面' : state.desktop === 'error' ? '暂时无法打开桌面' : '远程桌面未连接');
    $('desktopEmptyText').textContent = t(desktopBusy ? '正在建立加密桌面通道，请稍候。' : state.desktop === 'error' ? state.message : connected ? '设备连接已就绪，点击连接桌面即可开始。' : '先连接设备，再打开远程桌面。连接后可直接使用鼠标和键盘操作。');
    $('desktopEmptyAction').hidden = desktopBusy;
    $('desktopEmptyAction').replaceChildren(document.createTextNode(t(connected ? '连接远程桌面' : '前往连接设备')), icon('arrow'));
  }
  if (!connected && rfb) clearRfb();
  renderDevices();
}
async function startDesktop() {
  clearRfb();
  const attempt = desktopAttempt;
  try {
    const {default:RFB} = await import('@novnc/novnc');
    if (attempt !== desktopAttempt) return;
    const {url,id} = await window.studio.startDesktop();
    if (attempt !== desktopAttempt) return;
    const remote = new RFB($('desktopCanvas'), url, {shared: true});
    rfb = remote;
    remote.scaleViewport = fit;remote.resizeSession = false;remote.viewOnly = readonly;
    remote.qualityLevel = 6;remote.compressionLevel = 2;remote.background = '#17222b';
    remote.addEventListener('connect', async () => {if (rfb !== remote) return;$('desktopEmpty').hidden = true;await window.studio.desktopReady(id);remote.focus();});
    remote.addEventListener('disconnect', event => {
      if (rfb !== remote) return;
      rfb = null;$('desktopCanvas').replaceChildren();
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
$('goDesktop').onclick = () => {showPage('desktop');if (current.desktop !== 'connected') startDesktop();};

$('desktopEmptyAction').onclick = () => current.ssh === 'connected' ? startDesktop() : showPage('devices');
$('startDesktop').onclick = startDesktop;
$('stopDesktop').onclick = async () => {clearRfb();await window.studio.stopDesktop();};
function setFit(value) {fit = value;if (rfb) rfb.scaleViewport = fit;$('fitDesktop').classList.toggle('selected', fit);$('actualSize').classList.toggle('selected', !fit);}
$('fitDesktop').onclick = () => setFit(true);$('actualSize').onclick = () => setFit(false);
$('viewOnly').onclick = () => {readonly = !readonly;if (rfb) rfb.viewOnly = readonly;$('viewOnly').classList.toggle('selected', readonly);$('viewOnly').setAttribute('aria-pressed', String(readonly));$('sendCAD').disabled = !rfb || readonly;};
$('sendCAD').onclick = () => rfb?.sendCtrlAltDel();
$('fullscreen').onclick = () => window.studio.fullscreen();
window.studio.onState(renderState);
onLanguageChange(()=>{renderAuth();renderState(current,true);if(!selected)$('editorTitle').textContent=t('添加一台设备');if(toastMessage)$('toast').textContent=t(toastMessage);});
(async () => {
  try {devices = await window.studio.profiles();fill(devices[0] || null);renderState(await window.studio.state());} catch (error) {toast('读取设备配置失败：' + error.message);}
})();
