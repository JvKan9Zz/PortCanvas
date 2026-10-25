import {t,setLanguage,getLanguage,onLanguageChange} from './i18n.js';
import {getTheme,setTheme} from './theme.js';
const $=id=>document.getElementById(id);
const updateError=message=>{const parts=String(message||'').match(/^(检查更新失败|下载或校验失败|安装更新失败)：(.*)$/s);return parts?t(parts[1]+'：{error}',{error:t(parts[2])}):t(message);};
export class SettingsDialog {
  constructor({toast}){
    this.toast=toast;this.acting=false;
    $('openSettings').onclick=()=>{$('settingsDialog').showModal();this.render();};
    $('closeSettings').onclick=()=>$('settingsDialog').close();
    $('settingsDialog').addEventListener('close',()=>$('openSettings').focus());
    $('settingsDialog').addEventListener('keydown',event=>{
      if(event.key!=='Tab')return;
      const controls=[...$('settingsDialog').querySelectorAll('button:not(:disabled),select:not(:disabled)')].filter(element=>element.getClientRects().length>0),first=controls[0],last=controls.at(-1);
      if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus();}
      else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}
    });
    $('settingsLanguage').onchange=async()=>{const select=$('settingsLanguage'),focused=document.activeElement===select;select.disabled=true;try{const result=await window.studio.setLanguage(select.value);setLanguage(result.language);}catch(error){select.value=getLanguage();toast(error.message);}finally{select.disabled=false;if(focused&&document.activeElement===document.body&&$('settingsDialog').open)select.focus();}};
    $('settingsTheme').onchange=async()=>{const select=$('settingsTheme'),focused=document.activeElement===select;select.disabled=true;try{const result=await window.studio.setTheme(select.value);setTheme(result.theme);}catch(error){select.value=getTheme();toast(error.message);}finally{select.disabled=false;if(focused&&document.activeElement===document.body&&$('settingsDialog').open)select.focus();}};
    $('openRepository').onclick=()=>window.studio.openRepository().catch(error=>toast(error.message));
    $('checkUpdates').onclick=()=>this.perform(()=>window.studio.checkUpdates());
    $('downloadUpdate').onclick=()=>this.perform(()=>window.studio.downloadUpdate());
    $('installUpdate').onclick=()=>this.perform(()=>window.studio.installUpdate());
    $('cancelUpdate').onclick=()=>window.studio.cancelUpdate().catch(error=>toast(error.message));
    window.studio.onUpdate(update=>{this.update=update;this.render();});
    window.studio.updateState().then(update=>{if(!this.update)this.update=update;this.render();}).catch(error=>toast(error.message));
    onLanguageChange(()=>this.render());
    window.studio.settings().then(info=>{this.info=info;setTheme(info.theme);setLanguage(info.language);this.render();}).catch(error=>toast(error.message));
  }
  async perform(action){
    if(this.acting)return;this.acting=true;this.render();
    try{this.update=await action();}catch(error){this.toast(error.message);}finally{this.acting=false;this.render();}
  }
  render(){
    $('settingsLanguage').value=getLanguage();$('settingsTheme').value=getTheme();$('appVersion').textContent=this.info?'Demo V'+this.info.version:'—';
    const update=this.update||{},status=update.status||'idle',busy=['checking','downloading','verifying','installing'].includes(status);
    $('checkUpdates').disabled=this.acting||busy;$('checkUpdates').textContent=t(status==='checking'?'正在检查…':'检查更新');
    $('downloadUpdate').hidden=!update.version||!['available','canceled','error'].includes(status);
    $('downloadUpdate').disabled=this.acting||busy;
    $('installUpdate').hidden=status!=='ready';$('installUpdate').disabled=this.acting;
    $('cancelUpdate').hidden=!['checking','downloading'].includes(status);
    const messages={checking:'正在检查…',verifying:'正在校验签名…',ready:'更新已就绪。',installing:'正在安装，即将重新启动…',current:'已是最新版本。',canceled:'已取消更新。'};
    const downloaded=(Number(update.downloaded||0)/1048576).toFixed(1),total=update.total?(Number(update.total)/1048576).toFixed(1):'—';
    let message=status==='available'?t('发现新版本 Demo V{version}',{version:update.version}):status==='ready'?t('Demo V{version} 更新已就绪。',{version:update.version}):status==='downloading'?(update.connections?t('正在下载（{connections} 路）：{downloaded} / {total} MiB',{connections:update.connections,downloaded,total}):t('正在连接下载服务…')):status==='updated'?t('已更新至 Demo V{version}。',{version:update.version}):status==='error'?updateError(update.message):t(messages[status]||'');
    $('updateStatus').hidden=!message;if($('updateStatus').textContent!==message)$('updateStatus').textContent=message;
    $('updateStatus').classList.toggle('error',status==='error');
    $('updateProgress').hidden=!['downloading','verifying','ready'].includes(status);
    if(update.total)$('updateProgress').value=Math.min(100,Number(update.downloaded||0)/Number(update.total)*100);else $('updateProgress').removeAttribute('value');
    $('updateNotes').hidden=!update.notes||!['available','downloading','verifying','ready'].includes(status);$('updateNotes').textContent=update.notes||'';
  }
}
