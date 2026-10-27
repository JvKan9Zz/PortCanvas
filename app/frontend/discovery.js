import {t,onLanguageChange} from './i18n.js';
import {showActivity} from './connection-indicator.js';
const $=id=>document.getElementById(id);
export class DeviceDiscovery{
 constructor({profiles,connect,connection,toast}){
  Object.assign(this,{profiles,connect,connection,toast});this.state={status:'idle',devices:[]};this.dialog=$('quickConnectDialog');
  $('refreshDiscovery').onclick=()=>this.search();
  $('quickConnectForm').onsubmit=event=>{event.preventDefault();this.submit();};
  $('cancelQuickConnect').onclick=()=>this.cancel();this.dialog.addEventListener('cancel',event=>{event.preventDefault();this.cancel();});
  this.dialog.addEventListener('close',()=>{$('quickPassword').value='';$('quickError').hidden=true;});
  window.studio.onDiscovery(state=>{if((state.scanId||0)>=(this.state.scanId||0)){this.state=state;this.render();}});
  window.studio.discoveryState().then(state=>{this.state=state;this.render();if(state.status==='idle')this.search();}).catch(error=>toast(error.message));
  onLanguageChange(()=>this.render());
 }
 async search(){try{this.state=await (this.state.status==='searching'?window.studio.cancelDiscovery():window.studio.discover());this.render();}catch(error){this.toast(error.message);}}
 refresh(){this.render();}
 async choose(device){
  if(this.busy||['connected','connecting'].includes(this.connection()))return;
  const saved=this.profiles().find(profile=>profile.host===device.host&&Number(profile.port)===Number(device.port));
  if(saved&&(saved.hasSavedCredential||saved.authMethod==='key')){
   this.busy=true;this.render();try{await this.connect(saved,'');return;}catch(error){this.toast(error.message);}finally{this.busy=false;this.render();}
  }
  this.profile=saved||{name:'',host:device.host,port:device.port,username:'',display:':0',xauthority:'',authMethod:'password',privateKeyPath:'',rememberPassword:false};
  $('quickTarget').textContent=`${device.host}:${device.port}`;$('quickName').value=this.profile.name;$('quickUsername').value=this.profile.username;
  $('quickCredentialLabel').textContent=t(this.profile.authMethod==='key'?'密钥口令':'密码');$('quickRemember').checked=Boolean(this.profile.rememberPassword);
  $('quickPassword').value='';this.dialog.showModal();$(saved?'quickPassword':'quickName').focus();
 }
 async submit(){
  if(this.busy||!$('quickConnectForm').reportValidity())return;
  this.busy=true;$('quickError').hidden=true;this.render();
  try{await this.connect({...this.profile,name:$('quickName').value.trim(),username:$('quickUsername').value.trim(),rememberPassword:$('quickRemember').checked},$('quickPassword').value);if(this.dialog.open)this.dialog.close();}
  catch(error){if(this.dialog.open){$('quickError').textContent=t(error.message);$('quickError').hidden=false;}}
  finally{this.busy=false;this.render();}
 }
 async cancel(){if(this.busy){try{await window.studio.disconnect();}catch(error){this.toast(error.message);}}if(this.dialog.open)this.dialog.close();}
 render(){
  const scanning=this.state.status==='searching',disabled=this.busy||['connected','connecting'].includes(this.connection());
  $('refreshDiscovery').textContent=t(scanning?'停止搜索':'重新搜索');$('refreshDiscovery').disabled=this.busy;
  $('discoveryCount').textContent=String(this.state.devices?.length||0);showActivity($('discoveryTitle'),scanning||this.busy);
  const message=scanning?t('正在搜索 {checked}/{total}',{checked:this.state.checked||0,total:this.state.total||'—'}):this.state.status==='error'?t(this.state.message):this.state.limited?t('已搜索本机附近网段，其它地址可手动填写。'):this.state.status==='canceled'?t('搜索已停止'):'';
  $('discoveryStatus').textContent=message;$('discoveryStatus').hidden=!message;
  const list=$('discoveryList'),existing=new Map([...list.querySelectorAll('[data-address]')].map(button=>[button.dataset.address,button]));
  if(!this.state.devices?.length){let empty=list.querySelector('.no-devices');if(!empty){empty=document.createElement('p');empty.className='no-devices';list.append(empty);}empty.textContent=t(scanning?'正在查找设备…':'未发现设备');}else list.querySelector('.no-devices')?.remove();
  for(const device of this.state.devices||[]){const address=`${device.host}:${device.port}`;let button=existing.get(address);existing.delete(address);if(!button){button=document.createElement('button');button.type='button';button.dataset.address=address;button.className='device-item discovered-device';const wrap=document.createElement('div'),title=document.createElement('strong'),detail=document.createElement('small');title.textContent=device.host;detail.textContent=`SSH · ${device.port}`;wrap.append(title,detail);const action=document.createElement('span');action.className='discovery-connect';button.append(wrap,action);button.onclick=()=>this.choose(device);list.append(button);}button.disabled=disabled;button.lastElementChild.textContent=t('连接');}
  for(const button of existing.values())button.remove();
  for(const id of ['quickName','quickUsername','quickPassword','quickRemember','submitQuickConnect'])$(id).disabled=this.busy;
  $('cancelQuickConnect').textContent=t(this.busy?'取消连接':'取消');showActivity($('quickConnectTitle'),this.busy);
 }
}
