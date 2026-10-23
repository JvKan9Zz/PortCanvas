import {FileDrag} from './file-drag.js';
const $=id=>document.getElementById(id);
const bytes=value=>value==null?'未知':value<1024?`${value} B`:value<1024**2?`${(value/1024).toFixed(1)} KiB`:value<1024**3?`${(value/1024**2).toFixed(1)} MiB`:`${(value/1024**3).toFixed(2)} GiB`;
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
    $('uploadFiles').onclick=()=>this.transfer('upload');$('downloadFiles').onclick=()=>this.transfer('download');
    $('cancelTransfer').onclick=()=>window.studio.cancelTransfer().catch(error=>toast(error.message));
    $('folderCancel').onclick=()=>$('folderDialog').close();
    $('folderForm').onsubmit=async e=>{e.preventDefault();$('folderCreate').disabled=true;try{await window.studio.createFolder(this.folderSide,this.panels[this.folderSide].path,$('folderName').value);$('folderDialog').close();await this.load(this.folderSide,this.panels[this.folderSide].path);}catch(error){$('folderError').textContent=error.message;}finally{$('folderCreate').disabled=false;}};
    window.studio.onFileState(state=>this.transferState(state));
    window.studio.fileState().then(state=>this.transferState(state)).catch(error=>toast(error.message));
    this.drag=new FileDrag(this);
  }
  show() {this.visible=true;this.refreshDrives();if(!this.localLoaded)this.load('local','');if(this.connected&&!this.remoteLoaded)this.load('remote',this.panels.remote.path);}
  async refreshDrives(){try{const drives=await window.studio.localDrives(),placeholder=new Option('盘符','');placeholder.disabled=true;$('localDrive').replaceChildren(placeholder,...drives.map(d=>new Option(d.label,d.path)));this.selectDrive();}catch(error){this.toast(error.message);}}
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
      const panel=this.panels[side],disabled=this.busy||panel.loading||(side==='remote'&&!this.connected);
      for(const id of ['Up','Home','Refresh','New','Path','All'])$(side+id).disabled=Boolean(disabled);
      $(side+'PathForm').querySelector('button').disabled=Boolean(disabled);
      $(side+'Files').querySelectorAll('input').forEach(input=>input.disabled=this.busy);
    }
    $('localChoose').disabled=this.busy;
    $('localDrive').disabled=this.busy||this.panels.local.loading;
    const loading=this.panels.local.loading||this.panels.remote.loading;
    $('uploadFiles').disabled=this.busy||loading||!this.connected||!this.panels.local.selected.size||!this.remoteLoaded;
    $('downloadFiles').disabled=this.busy||loading||!this.connected||!this.panels.remote.selected.size||!this.localLoaded;
    $('transferRate').disabled=this.busy;
    $('cancelTransfer').disabled=!this.busy;
  }
  async load(side,directory) {
    const panel=this.panels[side];if(this.busy)return;
    const request=++panel.request;panel.loading=true;this.controls();$(side+'Message').textContent='正在读取目录…';
    try {
      const data=await window.studio.listFiles(side,directory);
      if(request!==panel.request)return;
      data.entries.sort((a,b)=>(a.type==='directory'?0:1)-(b.type==='directory'?0:1)||a.name.localeCompare(b.name,'zh-CN',{numeric:true}));
      Object.assign(panel,data);panel.selected.clear();$(side+'Path').value=data.path;
      if(side==='local')this.localLoaded=true;else this.remoteLoaded=true;
      panel.error=null;this.render(side);
    } catch(error) {
      if(request!==panel.request)return;
      panel.error=error.message;$(side+'Path').value=panel.path;$(side+'Message').textContent=error.message;this.toast(error.message);
    } finally {if(request===panel.request){panel.loading=false;if(side==='local')this.selectDrive();this.controls();}}
  }
  render(side) {
    const panel=this.panels[side],body=$(side+'Files');body.replaceChildren();
    if(!panel.entries.length) {
      const row=document.createElement('tr'),cell=document.createElement('td');cell.colSpan=4;cell.className='file-empty';
      cell.textContent=side==='remote'&&!this.connected?'设备未连接':'空文件夹';row.append(cell);body.append(row);
    }
    for(const entry of panel.entries) {
      const row=document.createElement('tr');row.tabIndex=0;row.dataset.name=entry.name;row.classList.toggle('selected',panel.selected.has(entry.name));
      const checkbox=document.createElement('input');checkbox.type='checkbox';checkbox.checked=panel.selected.has(entry.name);checkbox.setAttribute('aria-label',`选择 ${entry.name}`);checkbox.disabled=this.busy;
      const choose=add=>{if(this.busy)return;if(!add)panel.selected.clear();if(add&&panel.selected.has(entry.name))panel.selected.delete(entry.name);else panel.selected.add(entry.name);this.renderSelection(side);};
      checkbox.onclick=e=>{e.stopPropagation();choose(true);};
      row.onclick=e=>{if(performance.now()<(this.suppressClickUntil||0))return;choose(e.ctrlKey||e.metaKey);};
      const open=()=>{if(entry.type==='directory'&&!this.busy)this.load(side,this.join(side,panel.path,entry.name));};
      row.ondblclick=open;row.onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();open();}else if(e.key===' '){e.preventDefault();choose(true);}};
      const checkCell=document.createElement('td');checkCell.append(checkbox);
      const name=document.createElement('td'),nameContent=document.createElement('span'),text=document.createElement('span');
      nameContent.className='file-name';text.textContent=entry.name;nameContent.append(this.icon(entry.type==='directory'?'folder':entry.type==='link'?'connect':'file'),text);name.append(nameContent);name.title=entry.name;
      const size=document.createElement('td');size.textContent=entry.type==='directory'?'文件夹':entry.type==='link'?'链接':bytes(entry.size);
      const date=document.createElement('td');date.textContent=entry.modified==null?'未知':new Date(entry.modified).toLocaleString('zh-CN',{month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',hour12:false});
      row.append(checkCell,name,size,date);body.append(row);
    }
    this.renderSelection(side);
  }
  renderSelection(side) {
    const panel=this.panels[side];
    for(const row of $(side+'Files').querySelectorAll('tr[data-name]')){const checked=panel.selected.has(row.dataset.name);row.classList.toggle('selected',checked);row.querySelector('input').checked=checked;}
    $(side+'All').checked=Boolean(panel.entries.length)&&panel.selected.size===panel.entries.length;
    $(side+'All').indeterminate=panel.selected.size>0&&panel.selected.size<panel.entries.length;
    if(!panel.error)$(side+'Message').textContent=`${panel.entries.length} 个项目 · 已选 ${panel.selected.size} 个`;
    this.controls();
  }
  join(side,parent,name) {return parent.replace(side==='local'?/[\\/]$/:/\/$/,'')+(side==='local'?'\\':'/')+name;}
  newFolder(side) {this.folderSide=side;$('folderDialogTitle').textContent=(side==='local'?'本地':'远端')+'新建文件夹';$('folderName').value='';$('folderError').textContent='';$('folderDialog').showModal();$('folderName').focus();}
  async transfer(direction,drop={}) {
    if(this.submitting||this.busy)return;
    this.submitting=true;this.drag.clear();
    const side=direction==='upload'?'local':'remote';
    try {
      await window.studio.transfer({direction,names:[...this.panels[side].selected],localPath:this.panels.local.path,remotePath:this.panels.remote.path,rateLimit:Number($('transferRate').value),...drop});
    } catch(error){this.toast(error.message);}finally{this.submitting=false;}
  }
  transferState(state) {
    const wasBusy=this.busy;this.busy=Boolean(state.busy);
    $('sftpBadge').className='badge '+state.connection;
    $('sftpBadge').lastElementChild.textContent=state.connection==='connected'?'SFTP 已连接':state.connection==='connecting'?'正在连接 SFTP':'SFTP 未连接';
    const titles={idle:'准备传输',planning:'检查文件',confirming:'确认覆盖',transferring:state.direction==='upload'?'正在上传':'正在下载',canceling:'正在取消',completed:'传输完成',canceled:'已取消传输',failed:'传输失败'};
    $('transferTitle').textContent=titles[state.phase]||'准备传输';
    document.querySelector('.transfer-status').hidden=state.phase==='idle';
    $('transferMessage').textContent=state.message;
    $('transferProgress').value=state.phase==='completed'?100:state.totalBytes?Math.min(100,state.bytes/state.totalBytes*100):0;
    $('transferNumbers').textContent=state.phase==='idle'?'':`${bytes(state.bytes||0)} / ${bytes(state.totalBytes||0)} · ${state.completed||0}/${state.totalFiles||0} 个文件`+(state.speed?` · ${bytes(state.speed)}/s`:'');
    const target=state.direction==='upload'?this.panels.remote.path:this.panels.local.path;
    $('transferCurrent').textContent=state.currentFile?`当前：${state.currentFile}`:state.phase==='idle'?'':`目标：${target}`;
    $('transferCurrent').title=$('transferCurrent').textContent;
    document.querySelector('.transfer-status').classList.toggle('failed',state.phase==='failed');
    this.controls();
    if(wasBusy&&!this.busy){for(const side of ['local','remote'])if(side==='local'||this.connected)this.load(side,this.panels[side].path);}
  }
}
