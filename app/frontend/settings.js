import {showActivity,withActivity} from './connection-indicator.js';
import {ColorPicker} from './color-picker.js';
import {BackgroundCropDialog} from './background-crop.js';
import {applyGlassOpacity} from './glass.js';
import {StorageSettings} from './storage-settings.js';
import {checkStartupUpdate,hasAvailableUpdate} from './startup-update.js';
import {applyBackground,setBackgroundVolume,setBackgroundHidden,refreshBackgroundPlayback} from './background.js';
import {t,setLanguage,getLanguage,onLanguageChange} from './i18n.js';
import {getTheme,setTheme,getAppearance,setAppearance,onThemeChange} from './theme.js';
const $=id=>document.getElementById(id);
const updateError=message=>{const parts=String(message||'').match(/^(检查更新失败|下载或校验失败|安装更新失败)：(.*)$/s);return parts?t(parts[1]+'：{error}',{error:t(parts[2])}):t(message);};
export class SettingsDialog {
  constructor({toast}){
    this.toast=toast;this.acting=false;this.storage=new StorageSettings({toast});this.category='general';
    for(const button of document.querySelectorAll('[data-settings-page]'))button.onclick=()=>this.selectCategory(button.dataset.settingsPage);
    document.querySelector('.settings-tabs').onkeydown=event=>{
      const tabs=[...document.querySelectorAll('[data-settings-page]')],index=tabs.findIndex(tab=>tab.dataset.settingsPage===this.category);
      const next={ArrowUp:(index+tabs.length-1)%tabs.length,ArrowDown:(index+1)%tabs.length,Home:0,End:tabs.length-1}[event.key];
      if(next!==undefined){event.preventDefault();this.selectCategory(tabs[next].dataset.settingsPage);tabs[next].focus();}
    };
    $('openSettings').onclick=()=>this.open();
    $('updateNotice').onclick=()=>this.open('about');
    $('closeSettings').onclick=()=>this.close();
    $('settingsDialog').addEventListener('cancel',event=>{event.preventDefault();this.close();});
    $('settingsDialog').addEventListener('close',()=>{this.storage.hide();refreshBackgroundPlayback();$('openSettings').focus();});
    $('settingsDialog').addEventListener('keydown',event=>{
      if(event.key!=='Tab')return;
      const controls=[...$('settingsDialog').querySelectorAll('button:not(:disabled),select:not(:disabled),input:not(:disabled)')].filter(element=>element.tabIndex>=0&&!element.closest('[inert]')&&element.getClientRects().length>0),first=controls[0],last=controls.at(-1);
      if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus();}
      else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}
    });
    $('settingsLanguage').onchange=async()=>{const select=$('settingsLanguage'),focused=document.activeElement===select;select.disabled=true;try{const result=await withActivity($('settingsPageTitle'),()=>window.studio.setLanguage(select.value));setLanguage(result.language);}catch(error){select.value=getLanguage();toast(error.message);}finally{select.disabled=false;if(focused&&document.activeElement===document.body&&$('settingsDialog').open)select.focus();}};
    for(const option of document.querySelectorAll('input[name="environmentSource"]'))option.onchange=async()=>{
      if(!option.checked||this.savingSource)return;
      this.savingSource=true;this.render();
      try{this.info=await window.studio.setEnvironmentSource(option.value);}catch(error){toast(error.message);}
      finally{this.savingSource=false;this.render();}
    };
    this.colorPicker=new ColorPicker(value=>this.backgroundAction(()=>window.studio.setBackgroundColor(value)));
    for(const id of ['glassOpacity','glassOpacityNumber']){
      $(id).oninput=()=>{if($(id).checkValidity()){const value=$(id).value;$('glassOpacity').value=value;$('glassOpacityNumber').value=value;applyGlassOpacity(Number(value));}};
      $(id).onchange=()=>this.saveGlass($(id));
    }
    $('glassOpacityNumber').onkeydown=event=>{if(event.key==='Enter'){event.preventDefault();this.saveGlass($('glassOpacityNumber'));}};
    this.backgroundCrop=new BackgroundCropDialog();
    $('chooseBackgroundImage').onclick=()=>this.chooseBackground();
    $('chooseBackgroundVideo').onclick=()=>this.chooseBackground(true);
    $('cancelBackgroundImport').onclick=()=>window.studio.cancelBackgroundImport().catch(error=>toast(error.message));
    window.studio.onBackgroundProgress(({copied,total})=>{if(!this.backgroundBusy)return;$('backgroundImport').hidden=false;$('backgroundImportProgress').value=total?copied/total*100:0;$('backgroundImportText').textContent=t('正在导入 {copied} / {total} MiB',{copied:(copied/1048576).toFixed(1),total:(total/1048576).toFixed(1)});});
    window.studio.onArenaVisibility(({hidden})=>{setBackgroundHidden(hidden);this.backgroundCrop.setHidden(hidden);});
    $('backgroundVolume').oninput=()=>{setBackgroundVolume($('backgroundVolume').value);$('backgroundVolumeValue').value=$('backgroundVolume').value+'%';};
    $('backgroundVolume').onchange=async()=>{const input=$('backgroundVolume');setBackgroundVolume(input.value);input.disabled=true;try{const result=await withActivity($('backgroundVolumeRow'),()=>window.studio.setBackgroundVolume(Number(input.value)));this.info={...this.info,backgroundVolume:result.backgroundVolume};}catch(error){toast(error.message);setBackgroundVolume(this.info?.backgroundVolume??0);}finally{input.disabled=false;input.value=String(this.info?.backgroundVolume??0);$('backgroundVolumeValue').value=input.value+'%';}};
    $('enableBackgroundAudio').onclick=()=>setBackgroundVolume(this.info?.backgroundVolume??0);
    $('removeBackgroundImage').onclick=()=>this.backgroundAction(()=>window.studio.removeBackgroundImage());
    $('resetBackground').onclick=()=>this.backgroundAction(()=>window.studio.resetBackground());
    for(const option of document.querySelectorAll('input[name="appearance"]'))option.onchange=async()=>{
      if(!option.checked||this.savingAppearance)return;this.savingAppearance=true;
      try{this.info=await window.studio.setAppearance(option.value);setAppearance(this.info.appearance);}catch(error){toast(error.message);}
      finally{this.savingAppearance=false;this.render();}
    };
    $('openInstallDirectory').onclick=()=>window.studio.openInstallDirectory().catch(error=>toast(error.message));
    $('settingsTheme').onchange=async()=>{const select=$('settingsTheme'),focused=document.activeElement===select;select.disabled=true;try{const result=await withActivity($('settingsPageTitle'),()=>window.studio.setTheme(select.value));setTheme(result.theme);}catch(error){select.value=getTheme();toast(error.message);}finally{select.disabled=false;if(focused&&document.activeElement===document.body&&$('settingsDialog').open)select.focus();}};
    $('openRepository').onclick=()=>window.studio.openRepository().catch(error=>toast(error.message));
    $('checkUpdates').onclick=()=>this.perform(()=>window.studio.checkUpdates());
    $('downloadUpdate').onclick=()=>this.perform(()=>window.studio.downloadUpdate());
    $('installUpdate').onclick=()=>this.perform(()=>window.studio.installUpdate());
    $('cancelUpdate').onclick=()=>window.studio.cancelUpdate().catch(error=>toast(error.message));
    window.studio.onUpdate(update=>{this.update=update;this.render();});
    const initialUpdate=window.studio.updateState().then(update=>{if(!this.update)this.update=update;this.render();return update;}).catch(error=>{toast(error.message);});
    onLanguageChange(()=>this.render());
    onThemeChange(()=>this.render());
    const initialSettings=window.studio.settings().then(info=>{this.info=info;applyGlassOpacity(info.glassOpacity??100);setTheme(info.theme);setAppearance(info.appearance);setLanguage(info.language);this.refreshBackground();this.render();return info;}).catch(error=>{toast(error.message);});
    Promise.all([initialSettings,initialUpdate]).then(async([info,update])=>{if(!info||!update)return;const result=await checkStartupUpdate(info,this.update,()=>window.studio.checkUpdates());if(result){this.update=result;this.render();}});
  }
  open(category=this.category){if(!$('settingsDialog').open)$('settingsDialog').showModal();this.selectCategory(category);this.render();refreshBackgroundPlayback();}
  selectCategory(category){
    this.category=category;
    const tabs=[...document.querySelectorAll('[data-settings-page]')];
    tabs.forEach((tab,index)=>{const selected=tab.dataset.settingsPage===category;tab.setAttribute('aria-selected',String(selected));tab.tabIndex=selected?0:-1;$('settings-panel-'+tab.dataset.settingsPage).hidden=!selected;if(selected)document.querySelector('.settings-tabs').style.setProperty('--settings-index',index);});
    $('settingsPageTitle').textContent=t({general:'通用',storage:'存储',about:'关于'}[category]);
    if(category==='storage')this.storage.show();else this.storage.hide();
    refreshBackgroundPlayback();
  }
  async refreshBackground(){
    const request=(this.backgroundRequest||0)+1;this.backgroundRequest=request;
    const info=this.info||{};let image=null;
    if(info.backgroundImage||info.backgroundVideo)try{image=await withActivity($('backgroundPreview'),()=>window.studio.backgroundImageData());}catch(error){this.toast(error.message);}
    if(request===this.backgroundRequest)applyBackground(info.backgroundColor,image,info.backgroundCrop,Boolean(info.backgroundVideo),info.backgroundVolume??0);
  }
  async chooseBackground(video=false){
    if(this.backgroundBusy)return;this.backgroundBusy=true;this.render();let draft;
    try{
      draft=await withActivity($('settingsPageTitle'),()=>video?window.studio.chooseBackgroundVideo():window.studio.chooseBackgroundImage());if(draft.canceled)return;
      $('backgroundImport').hidden=true;
      const result=await this.backgroundCrop.open(draft.imageData,crop=>window.studio.commitBackgroundImage(draft.draftId,crop),Boolean(draft.video));
      if(result){this.info=result;await this.refreshBackground();}
    }catch(error){this.toast(error.message);}
    finally{if(draft?.draftId)await window.studio.cancelBackgroundImage(draft.draftId).catch(error=>this.toast(error.message));this.backgroundBusy=false;$('backgroundImport').hidden=true;this.render();if($('settingsDialog').open)$(video?'chooseBackgroundVideo':'chooseBackgroundImage').focus();}
  }
  async saveGlass(input){
    if(this.savingGlass)return;if(!input.reportValidity())return;
    const value=Number(input.value);if(value===(this.info?.glassOpacity??100))return;
    const focused=document.activeElement===input;this.savingGlass=true;this.render();
    try{this.info=await window.studio.setGlassOpacity(value);}catch(error){this.toast(error.message);}
    finally{this.savingGlass=false;$('glassOpacity').value=String(this.info?.glassOpacity??100);$('glassOpacityNumber').value=$('glassOpacity').value;applyGlassOpacity(this.info?.glassOpacity??100);this.render();if(focused&&document.activeElement===document.body&&$('settingsDialog').open)input.focus();}
  }
  async backgroundAction(action){
    if(this.backgroundBusy)return;this.backgroundBusy=true;this.render();
    try{const result=await action();if(!result.canceled){this.info=result;await this.refreshBackground();return true;}return false;}catch(error){this.toast(error.message);return false;}
    finally{this.backgroundBusy=false;this.render();}
  }
  close(){
    const dialog=$('settingsDialog');if(this.closing||!dialog.open)return;this.colorPicker.close(false);
    if(window.matchMedia('(prefers-reduced-motion: reduce)').matches){dialog.close();return;}
    this.closing=true;dialog.dataset.closing='true';
    const current=getComputedStyle(dialog);
    const animation=dialog.animate([{opacity:current.opacity,transform:current.transform},{opacity:0,transform:'translateY(8px) scale(.98)'}],{duration:180,easing:'cubic-bezier(.4,0,1,1)'});
    animation.onfinish=()=>{dialog.close();delete dialog.dataset.closing;this.closing=false;};
    animation.oncancel=()=>{dialog.close();delete dialog.dataset.closing;this.closing=false;};
  }
  async perform(action){
    if(this.acting)return;this.acting=true;this.render();
    try{this.update=await action();}catch(error){this.toast(error.message);}finally{this.acting=false;this.render();}
  }
  render(){
    $('settingsPageTitle').textContent=t({general:'通用',storage:'存储',about:'关于'}[this.category]);
    if(document.activeElement!==$('backgroundColor'))$('backgroundColor').value=this.info?.backgroundColor||(document.documentElement.dataset.appearance==='dark'?'#111927':'#eaf0f8');
    for(const id of ['backgroundColor','chooseBackgroundImage','chooseBackgroundVideo','resetBackground'])$(id).disabled=Boolean(this.backgroundBusy);
    $('removeBackgroundImage').hidden=!(this.info?.backgroundImage||this.info?.backgroundVideo);$('removeBackgroundImage').disabled=Boolean(this.backgroundBusy);
    $('backgroundVolumeRow').hidden=!this.info?.backgroundVideo;if(document.activeElement!==$('backgroundVolume')&&!$('backgroundVolume').disabled){$('backgroundVolume').value=String(this.info?.backgroundVolume??0);$('backgroundVolumeValue').value=$('backgroundVolume').value+'%';}
    showActivity($('settingsPageTitle'),this.savingSource||this.savingAppearance||this.savingGlass||this.backgroundBusy);
    const appearanceIndex=['light','dark','system'].indexOf(getAppearance());$('settingsAppearance').style.setProperty('--appearance-index',Math.max(0,appearanceIndex));
    for(const option of document.querySelectorAll('input[name="appearance"]')){option.checked=option.value===getAppearance();option.disabled=Boolean(this.savingAppearance);}
    if(!['glassOpacity','glassOpacityNumber'].includes(document.activeElement?.id)&&!this.savingGlass){$('glassOpacity').value=String(this.info?.glassOpacity??100);$('glassOpacityNumber').value=$('glassOpacity').value;}
    for(const id of ['glassOpacity','glassOpacityNumber'])$(id).disabled=Boolean(this.savingGlass);
    $('backgroundColorSwatch').style.backgroundColor=$('backgroundColor').value;
    $('backgroundColorButton').disabled=Boolean(this.backgroundBusy);
    $('settingsLanguage').value=getLanguage();$('settingsTheme').value=getTheme();$('appVersion').textContent=this.info?'Demo V'+this.info.version:'—';
    for(const option of document.querySelectorAll('input[name="environmentSource"]')){
      option.checked=option.value===(this.info?.environmentSource||'computer');option.disabled=Boolean(this.savingSource);
    }
    const update=this.update||{},status=update.status||'idle',busy=['checking','downloading','verifying','installing'].includes(status);
    $('updateNotice').hidden=!hasAvailableUpdate(update);
    $('updateNotice').querySelector('span').textContent=t('新版本 V{version}',{version:update.version||''});
    $('checkUpdates').disabled=this.acting||busy||this.info?.updatesSupported===false;$('checkUpdates').textContent=t(status==='checking'?'正在检查…':'检查更新');
    $('downloadUpdate').hidden=!update.version||!['available','canceled','error'].includes(status);
    $('downloadUpdate').disabled=this.acting||busy;
    $('installUpdate').hidden=status!=='ready';$('installUpdate').disabled=this.acting;
    $('cancelUpdate').hidden=!['checking','downloading'].includes(status);
    const messages={checking:'正在检查…',verifying:'正在校验签名…',ready:'更新已就绪。',installing:'正在安装，即将重新启动…',current:'已是最新版本。',canceled:'已取消更新。'};
    const downloaded=(Number(update.downloaded||0)/1048576).toFixed(1),total=update.total?(Number(update.total)/1048576).toFixed(1):'—';
    let message=status==='available'?t('发现新版本 Demo V{version}',{version:update.version}):status==='ready'?t('Demo V{version} 更新已就绪。',{version:update.version}):status==='downloading'?(update.connections?t('正在下载（{connections} 路）：{downloaded} / {total} MiB',{connections:update.connections,downloaded,total}):t('正在连接下载服务…')):status==='updated'?t('已更新至 Demo V{version}。',{version:update.version}):status==='error'?updateError(update.message):t(messages[status]||'');
    if(status==='manual')message=t(update.message);
    $('updateStatus').hidden=!message;if($('updateStatus').textContent!==message)$('updateStatus').textContent=message;
    showActivity($('updateStatus'),this.acting||busy);
    $('updateStatus').classList.toggle('error',status==='error');
    $('updateProgress').hidden=!['downloading','verifying','ready'].includes(status);
    if(update.total)$('updateProgress').value=Math.min(100,Number(update.downloaded||0)/Number(update.total)*100);else $('updateProgress').removeAttribute('value');
    $('updateNotes').hidden=!update.notes||!['available','downloading','verifying','ready'].includes(status);$('updateNotes').textContent=update.notes||'';
  }
}
