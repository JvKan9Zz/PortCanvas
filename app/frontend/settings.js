import {t,setLanguage,getLanguage,onLanguageChange} from './i18n.js';
import {getTheme,setTheme} from './theme.js';
const $=id=>document.getElementById(id);
export class SettingsDialog {
  constructor({toast}){
    this.toast=toast;this.checking=false;
    $('openSettings').onclick=()=>{$('settingsDialog').showModal();this.render();};
    $('closeSettings').onclick=()=>$('settingsDialog').close();
    $('settingsDialog').addEventListener('close',()=>$('openSettings').focus());
    $('settingsDialog').addEventListener('keydown',event=>{
      if(event.key!=='Tab')return;
      const controls=[...$('settingsDialog').querySelectorAll('button:not(:disabled),select:not(:disabled)')],first=controls[0],last=controls.at(-1);
      if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus();}
      else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}
    });
    $('settingsLanguage').onchange=async()=>{const select=$('settingsLanguage'),focused=document.activeElement===select;select.disabled=true;try{const result=await window.studio.setLanguage(select.value);setLanguage(result.language);}catch(error){select.value=getLanguage();toast(error.message);}finally{select.disabled=false;if(focused&&document.activeElement===document.body&&$('settingsDialog').open)select.focus();}};
    $('settingsTheme').onchange=async()=>{const select=$('settingsTheme'),focused=document.activeElement===select;select.disabled=true;try{const result=await window.studio.setTheme(select.value);setTheme(result.theme);}catch(error){select.value=getTheme();toast(error.message);}finally{select.disabled=false;if(focused&&document.activeElement===document.body&&$('settingsDialog').open)select.focus();}};
    $('openRepository').onclick=()=>window.studio.openRepository().catch(error=>toast(error.message));
    $('checkUpdates').onclick=async()=>{if(this.checking)return;this.checking=true;this.render();try{this.update=await window.studio.checkUpdates();}catch(error){this.update={status:'error',message:error.message};}finally{this.checking=false;this.render();}};
    onLanguageChange(()=>this.render());
    window.studio.settings().then(info=>{this.info=info;setTheme(info.theme);setLanguage(info.language);this.render();}).catch(error=>toast(error.message));
  }
  render(){
    $('settingsLanguage').value=getLanguage();$('settingsTheme').value=getTheme();$('appVersion').textContent=this.info?'Demo V'+this.info.version:'—';
    $('checkUpdates').disabled=this.checking;$('checkUpdates').textContent=t(this.checking?'正在检查…':'检查更新');
    const update=this.update;const messages={unavailable:'暂时无法检查：仓库未公开或版本信息不可访问。',limited:'GitHub 请求受限，请稍后再试。',unsupported:'当前平台暂不支持检查更新。',current:'已是最新版本。',ahead:'当前版本领先于仓库版本。'};
    $('updateStatus').hidden=!this.checking&&!update;$('updateStatus').textContent=this.checking?t('正在检查…'):update?.status==='available'?t('发现新版本 Demo V{version}',{version:update.latest}):update?.status==='error'?t(update.message):t(messages[update?.status]||'');
    $('updateStatus').classList.toggle('error',update?.status==='error');
  }
}
