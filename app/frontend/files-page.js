import {t,locale,onLanguageChange} from './i18n.js';
import {FileDrag} from './file-drag.js';
const $=id=>document.getElementById(id);
const bytes=value=>value==null?t('未知'):value<1024?`${value} B`:value<1024**2?`${(value/1024).toFixed(1)} KiB`:value<1024**3?`${(value/1024**2).toFixed(1)} MiB`:`${(value/1024**3).toFixed(2)} GiB`;
export class FilePage {
  constructor({toast,icon,goDevices}) {
    this.toast=toast;this.icon=icon;this.connected=false;this.visible=false;this.busy=false;
    this.panels={local:{entries:[],selected:new Set(),path:'',request:0},remote:{entries:[],selected:new Set(),path:'',request:0}};
    for(const side of ['local','remote']) {
      $(side+'PathForm').onsubmit=e=>{e.preventDefault();this.load(side,$(side+'Path').value);};
      $(side+'Up').onclick=()=>this.load(side,this.panels[side].parent);
      $(side+'Home').onclick=()=>this.load(side,this.panels[side].home||'');
      $(side+'Refresh').onclick=()=>this.load(side,this.panels[side].path);
      $(side+'New').onclick=()=>this.newFolder(side);
      $(side+'All').onchange=()=>{const panel=this.panels[side];panel.selected=new Set($(side+'All').checked?panel.entries.map(e=>e.name):[]);this.renderSelection(side);};
    }
    $('localChoose').onclick=async()=>{try{const directory=await window.studio.chooseLocalFolder();if(directory)this.load('local',directory);}catch(error){toast(error.message);}};
    $('localDrive').onchange=()=>this.load('local',$('localDrive').value);
    $('filesConnect').onclick=goDevices;
    $('remoteDelete').onclick=()=>this.removeRemote();
    $('uploadFiles').onclick=()=>this.transfer('upload');$('downloadFiles').onclick=()=>this.transfer('download');
    $('cancelTransfer').onclick=()=>window.studio.cancelTransfer().catch(error=>toast(error.message));
    $('folderCancel').onclick=()=>$('folderDialog').close();
    $('folderForm').onsubmit=async e=>{e.preventDefault();$('folderCreate').disabled=true;try{await window.studio.createFolder(this.folderSide,this.panels[this.folderSide].path,$('folderName').value);$('folderDialog').close();await this.load(this.folderSide,this.panels[this.folderSide].path);}catch(error){$('folderError').textContent=t(error.message);}finally{$('folderCreate').disabled=false;}};
    window.studio.onFileState(state=>this.transferState(state));
    window.studio.fileState().then(state=>this.transferState(state)).catch(error=>toast(error.message));
    this.drag=new FileDrag(this);
    onLanguageChange(()=>{this.render('local');this.render('remote');this.transferState(this.lastTransferState||{phase:'idle'});this.refreshDrives();$('folderDialogTitle').textContent=t((this.folderSide==='remote'?'远端':'本地')+'新建文件夹');});
  }
  show() {this.visible=true;this.refreshDrives();if(!this.localLoaded)this.load('local','');if(this.connected&&!this.remoteLoaded)this.load('remote',this.panels.remote.path);}
  async refreshDrives(){try{const drives=await window.studio.localDrives(),placeholder=new Option(t('盘符'),'');placeholder.disabled=true;$('localDrive').replaceChildren(placeholder,...drives.map(d=>new Option(d.label.replace(/(可移动磁盘|本地磁盘|网络磁盘|光驱|内存盘|磁盘)$/,kind=>t(kind)),d.path)));this.selectDrive();}catch(error){this.toast(error.message);}}
  selectDrive(){const path=this.panels.local.path,drive=/^[a-z]:[\\/]/i.test(path)?path.slice(0,2).toUpperCase()+'\\':'/';$('localDrive').value=[...$('localDrive').options].some(o=>o.value===drive)?drive:'';}
  hide() {this.visible=false;this.drag.clear();}
  setConnection(state,profileId) {
    const was=this.connected;this.connected=state.ssh==='connected';
    if(this.profileId!==profileId){this.profileId=profileId;this.panels.remote.path='';this.remoteLoaded=false;}
    if(!this.connected) {
      this.panels.remote.request++;this.panels.remote.loading=false;this.panels.remote.entries=[];this.panels.remote.selected.clear();this.remoteLoaded=false;
      this.drag.clear();this.render('remote');$('remoteMessage').textContent='';
    } else if(!was&&this.visible)this.load('remote',this.panels.remote.path);
    $('filesConnect').hidden=this.connected;
    $('remoteMessage').hidden=!this.connected;
    this.controls();
  }
  controls() {
    for(const side of ['local','remote']) {
      const panel=this.panels[side],disabled=this.busy||this.removing||panel.loading||(side==='remote'&&!this.connected);
      for(const id of ['Up','Home','Refresh','New','Path','All'])$(side+id).disabled=Boolean(disabled);
      $(side+'PathForm').querySelector('button').disabled=Boolean(disabled);
      $(side+'Files').querySelectorAll('input').forEach(input=>input.disabled=Boolean(this.busy||this.removing));
    }
    $('localChoose').disabled=Boolean(this.busy||this.removing);
    $('localDrive').disabled=Boolean(this.busy||this.removing||this.panels.local.loading);
    const loading=this.panels.local.loading||this.panels.remote.loading;
    $('uploadFiles').disabled=Boolean(this.busy||this.removing||loading||!this.connected||!this.panels.local.selected.size||!this.remoteLoaded);
    $('downloadFiles').disabled=Boolean(this.busy||this.removing||loading||!this.connected||!this.panels.remote.selected.size||!this.localLoaded);
    $('transferRate').disabled=this.busy;
    $('cancelTransfer').disabled=!this.busy;
    $('remoteDelete').disabled=Boolean(this.busy||this.removing||!this.connected||!this.remoteLoaded||this.panels.remote.loading||!this.panels.remote.selected.size);
  }
  async load(side,directory) {
    const panel=this.panels[side];if(this.busy)return;
    const request=++panel.request;panel.loading=true;this.controls();$(side+'Message').textContent=t('正在读取目录…');
    try {
      const data=await window.studio.listFiles(side,directory);
      if(request!==panel.request)return;
      data.entries.sort((a,b)=>(a.type==='directory'?0:1)-(b.type==='directory'?0:1)||a.name.localeCompare(b.name,locale(),{numeric:true}));
      Object.assign(panel,data);panel.selected.clear();$(side+'Path').value=data.path;
      if(side==='local')this.localLoaded=true;else this.remoteLoaded=true;
      panel.error=null;this.render(side);
    } catch(error) {
      if(request!==panel.request)return;
      panel.error=error.message;$(side+'Path').value=panel.path;$(side+'Message').textContent=t(error.message);this.toast(error.message);
    } finally {if(request===panel.request){panel.loading=false;if(side==='local')this.selectDrive();this.controls();}}
  }
  render(side) {
    const panel=this.panels[side],body=$(side+'Files');body.replaceChildren();
    if(!panel.entries.length) {
      const row=document.createElement('tr'),cell=document.createElement('td');cell.colSpan=4;cell.className='file-empty';
      cell.textContent=t(side==='remote'&&!this.connected?'设备未连接':'空文件夹');row.append(cell);body.append(row);
    }
    for(const entry of panel.entries) {
      const row=document.createElement('tr');row.tabIndex=0;row.dataset.name=entry.name;row.classList.toggle('selected',panel.selected.has(entry.name));
      const checkbox=document.createElement('input');checkbox.type='checkbox';checkbox.checked=panel.selected.has(entry.name);checkbox.setAttribute('aria-label',t('选择 {name}',{name:entry.name}));checkbox.disabled=this.busy;
      const choose=add=>{if(this.busy||this.removing)return;if(!add)panel.selected.clear();if(add&&panel.selected.has(entry.name))panel.selected.delete(entry.name);else panel.selected.add(entry.name);this.renderSelection(side);};
      checkbox.onclick=e=>{e.stopPropagation();choose(true);};
      row.onclick=e=>{if(performance.now()<(this.suppressClickUntil||0))return;choose(e.ctrlKey||e.metaKey);};
      const open=()=>{if(entry.type==='directory'&&!this.busy)this.load(side,this.join(side,panel.path,entry.name));};
      row.ondblclick=open;row.onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();open();}else if(e.key===' '){e.preventDefault();choose(true);}};
      const checkCell=document.createElement('td');checkCell.append(checkbox);
      const name=document.createElement('td'),nameContent=document.createElement('span'),text=document.createElement('span');
      nameContent.className='file-name';text.textContent=entry.name;nameContent.append(this.icon(entry.type==='directory'?'folder':entry.type==='link'?'connect':'file'),text);name.append(nameContent);name.title=entry.name;
      const size=document.createElement('td');size.textContent=entry.type==='directory'?t('文件夹'):entry.type==='link'?t('链接'):bytes(entry.size);
      const date=document.createElement('td');date.textContent=entry.modified==null?t('未知'):new Date(entry.modified).toLocaleString(locale(),{month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',hour12:false});
      row.append(checkCell,name,size,date);body.append(row);
    }
    this.renderSelection(side);
  }
  renderSelection(side) {
    const panel=this.panels[side];
    for(const row of $(side+'Files').querySelectorAll('tr[data-name]')){const checked=panel.selected.has(row.dataset.name);row.classList.toggle('selected',checked);row.querySelector('input').checked=checked;}
    $(side+'All').checked=Boolean(panel.entries.length)&&panel.selected.size===panel.entries.length;
    $(side+'All').indeterminate=panel.selected.size>0&&panel.selected.size<panel.entries.length;
    if(!panel.error)$(side+'Message').textContent=t('{n} 个项目 · 已选 {selected} 个',{n:panel.entries.length,selected:panel.selected.size});
    this.controls();
  }
  join(side,parent,name) {return parent.replace(side==='local'?/[\\/]$/:/\/$/,'')+(side==='local'?'\\':'/')+name;}
  newFolder(side) {this.folderSide=side;$('folderDialogTitle').textContent=t((side==='local'?'本地':'远端')+'新建文件夹');$('folderName').value='';$('folderError').textContent='';$('folderDialog').showModal();$('folderName').focus();}
  async removeRemote(){
    const panel=this.panels.remote;
    if(this.busy||this.removing||!this.connected||panel.loading||!panel.selected.size)return;
    this.removing=true;this.drag.clear();this.controls();
    try{await window.studio.removeRemoteFiles({parent:panel.path,names:[...panel.selected],generation:panel.generation});}
    catch(error){this.toast(error.message);}
    finally{this.removing=false;this.controls();}
  }
  async transfer(direction,drop={}) {
    if(this.submitting||this.busy||this.removing)return;
    this.submitting=true;this.drag.clear();
    const side=direction==='upload'?'local':'remote';
    try {
      await window.studio.transfer({direction,names:[...this.panels[side].selected],localPath:this.panels.local.path,remotePath:this.panels.remote.path,rateLimit:Number($('transferRate').value),...drop});
    } catch(error){this.toast(error.message);}finally{this.submitting=false;}
  }
  transferState(state) {
    this.lastTransferState=state;
    const wasBusy=this.busy;this.busy=Boolean(state.busy);
    $('sftpBadge').className='badge '+state.connection;
    $('sftpBadge').lastElementChild.textContent=t(state.connection==='connected'?'SFTP 已连接':state.connection==='connecting'?'正在连接 SFTP':'SFTP 未连接');
    const deleting=state.operation==='delete';
    const titles=deleting?{planning:'检查删除目标',confirming:'确认删除',deleting:'正在删除',canceling:'正在停止删除…',completed:'删除完成',canceled:'已取消删除',failed:'删除失败'}:{idle:'准备传输',planning:'检查文件',confirming:'确认覆盖',transferring:state.direction==='upload'?'正在上传':'正在下载',canceling:'正在取消',completed:'传输完成',canceled:'已取消传输',failed:'传输失败'};
    $('transferTitle').textContent=t(titles[state.phase]||'准备传输');
    document.querySelector('.transfer-status').hidden=state.phase==='idle';
    $('transferMessage').textContent=deleting&&state.phase==='failed'?t('删除未完成，已确认删除 {n} 个项目。{error} 请刷新目录核对。',{n:state.completed||0,error:t(state.message)}):t(state.message);
    $('transferProgress').value=state.phase==='completed'?100:deleting?(state.totalFiles?state.completed/state.totalFiles*100:0):state.totalBytes?Math.min(100,state.bytes/state.totalBytes*100):0;
    $('transferNumbers').textContent=deleting?t('{done}/{total} 个项目',{done:state.completed||0,total:state.totalFiles||0}):state.phase==='idle'?'':t('{size} · {done}/{total} 个文件',{size:bytes(state.bytes||0)+' / '+bytes(state.totalBytes||0),done:state.completed||0,total:state.totalFiles||0})+(state.speed?` · ${bytes(state.speed)}/s`:'');
    $('cancelTransfer').textContent=t(deleting?'取消删除':'取消传输');
    const target=state.direction==='upload'?this.panels.remote.path:this.panels.local.path;
    $('transferCurrent').textContent=state.currentFile?t('当前：{path}',{path:state.currentFile}):deleting||state.phase==='idle'?'':t('目标：{path}',{path:target});
    $('transferCurrent').title=$('transferCurrent').textContent;
    document.querySelector('.transfer-status').classList.toggle('failed',state.phase==='failed');
    this.controls();
    if(wasBusy&&!this.busy){for(const side of ['local','remote'])if(side==='local'||this.connected)this.load(side,this.panels[side].path);}
  }
}
