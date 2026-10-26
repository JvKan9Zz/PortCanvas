import {t,onLanguageChange} from './i18n.js';
const $=id=>document.getElementById(id);
const bytes=value=>value<1048576?(value/1024).toFixed(1)+' KiB':(value/1048576).toFixed(1)+' MiB';
export class StorageSettings{
  constructor({toast}){
    this.toast=toast;this.visible=false;this.request=0;
    $('logStorageLimit').oninput=()=>{this.limitDirty=true;};
    $('logStorageForm').onsubmit=async event=>{
      event.preventDefault();const input=$('logStorageLimit'),button=$('saveLogStorage');
      if(this.savingLimit||!input.reportValidity())return;
      const value=input.value,limit=value===''?null:Number(value)*1048576;
      this.savingLimit=true;button.disabled=true;
      try{const state=await window.studio.setLogLimit(limit);if(input.value===value)this.limitDirty=false;this.renderStatus(state);await this.refreshUsage();toast(state.notice||'日志上限已保存。');}
      catch(error){toast(error.message);}finally{this.savingLimit=false;button.disabled=false;}
    };
    $('chooseLogDirectory').onclick=async()=>{try{this.renderStatus(await window.studio.chooseLogDirectory());await this.refreshUsage();}catch(error){toast(error.message);}};
    $('openLogDirectory').onclick=()=>window.studio.openLogFolder().catch(error=>toast(error.message));
    window.studio.onLogState(state=>this.renderStatus(state));
    window.studio.onLogsChanged(()=>{if(this.visible){clearTimeout(this.timer);this.timer=setTimeout(()=>this.refreshUsage(),180);}});
    window.studio.logState().then(state=>this.renderStatus(state)).catch(error=>toast(error.message));
    onLanguageChange(()=>{this.renderUsage();if(this.state)this.renderStatus(this.state);});
  }
  renderStatus(state){
    this.state=state;$('logRootPath').textContent=state.root;
    if(!this.limitDirty&&document.activeElement!==$('logStorageLimit'))$('logStorageLimit').value=state.maxBytes==null?'':String(state.maxBytes/1048576);
    $('storageSettingsStatus').textContent=t(state.notice||state.configError&&state.message||'');
    $('storageSettingsStatus').hidden=!state.notice&&!state.configError;
  }
  renderUsage(){$('logStorageUsage').textContent=this.usage==null?'':t('已用 {size}',{size:bytes(this.usage)});}
  async refreshUsage(){const request=++this.request;try{const page=await window.studio.logList({});if(request===this.request){this.usage=page.usageBytes;this.renderUsage();}}catch(error){this.toast(error.message);}}
  show(){this.visible=true;window.studio.logState().then(state=>this.renderStatus(state)).catch(error=>this.toast(error.message));this.refreshUsage();}
  hide(){this.visible=false;clearTimeout(this.timer);}
}
