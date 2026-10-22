const $=id=>document.getElementById(id);
const labels={running:'记录中',succeeded:'命令成功',failed:'命令失败',interrupted:'记录中断',write_error:'写入失败',session:'会话输出'};
const time=value=>value?new Date(value).toLocaleString('zh-CN',{hour12:false}):'—';
const bytes=size=>size<1024?size+' B':size<1048576?(size/1024).toFixed(1)+' KB':(size/1048576).toFixed(1)+' MB';
export class LogsPage {
  constructor({toast}){
    this.toast=toast;this.records=[];this.checked=new Set();this.selected=null;this.visible=false;
    $('refreshLogs').onclick=()=>this.refresh();
    for(const id of ['logFilterProject','logFilterDay','logFilterStatus'])$(id).onchange=()=>this.renderList();
    $('selectLogs').onchange=()=>{for(const r of this.filtered())if(!r.active&&r.status!=='running')$('selectLogs').checked?this.checked.add(r.id):this.checked.delete(r.id);this.renderList();};
    $('deleteLogs').onclick=async()=>{try{const result=await window.studio.removeLogs([...this.checked]);this.toast(result.canceled?'已取消清理。':`已清理 ${result.removed} 份本地日志。`);await this.refresh();}catch(e){toast(e.message);}};
    $('chooseLogDirectory').onclick=async()=>{try{this.renderStatus(await window.studio.chooseLogDirectory());this.selected=null;this.checked.clear();await this.refresh();}catch(e){toast(e.message);}};
    $('openLogDirectory').onclick=()=>window.studio.openLogFolder().catch(e=>toast(e.message));
    $('openRunDirectory').onclick=()=>this.selected&&window.studio.openLogFolder(this.selected).catch(e=>toast(e.message));
    $('saveLogProject').onclick=async()=>{try{this.renderStatus(await window.studio.logProject($('logProjectName').value));toast('项目设置已保存，对之后开始的命令生效。');}catch(e){toast(e.message);}};
    window.studio.onLogsChanged(()=>{if(this.visible)this.refresh();});
    window.studio.onLogState(state=>this.renderStatus(state));
    window.studio.logState().then(state=>this.renderStatus(state)).catch(e=>toast(e.message));
  }
  renderStatus(state){
    $('logRootPath').textContent=state.root;
    $('logRecordingStatus').textContent=state.message+(state.notice?' '+state.notice:'');$('logRecordingStatus').classList.toggle('error',state.phase==='error'||Boolean(state.notice));
    $('terminalLogStatus').textContent=state.phase==='ready'?'本地日志 · 自动记录':state.phase==='error'?'日志异常 · 查看日志页':'本地日志 · '+state.message;
    $('terminalLogStatus').classList.toggle('error',state.phase==='error');
    if(document.activeElement!==$('logProjectName'))$('logProjectName').value=state.project||'';
  }
  show(){this.visible=true;this.refresh();this.tick();}
  hide(){this.visible=false;clearTimeout(this.timer);}
  tick(){clearTimeout(this.timer);if(!this.visible)return;this.timer=setTimeout(async()=>{if(this.selected)await this.readSelected();this.tick();},1200);}
  async refresh(){
    try{
      this.records=await window.studio.logList();
      for(const id of [...this.checked])if(!this.records.some(r=>r.id===id&&!r.active&&r.status!=='running'))this.checked.delete(id);
      this.options('logFilterProject',[...new Map(this.records.map(r=>[r.project.key,r.project.name])).entries()],'全部项目');
      this.options('logFilterDay',[...new Set(this.records.map(r=>r.day))].sort().reverse().map(day=>[day,day]),'全部日期');
      if(this.selected&&!this.records.some(r=>r.id===this.selected))this.selected=null;
      this.renderList();if(this.selected)await this.readSelected();else this.emptyDetail();
    }catch(e){this.toast(e.message);}
  }
  options(id,values,empty){const select=$(id),current=select.value;select.replaceChildren(new Option(empty,''),...values.map(([value,label])=>new Option(label,value)));if([...select.options].some(o=>o.value===current))select.value=current;}
  filtered(){return this.records.filter(r=>(!$('logFilterProject').value||r.project.key===$('logFilterProject').value)&&(!$('logFilterDay').value||r.day===$('logFilterDay').value)&&(!$('logFilterStatus').value||r.status===$('logFilterStatus').value));}
  renderList(){
    const list=$('logRunList'),records=this.filtered();list.replaceChildren();
    for(const record of records){
      const row=document.createElement('li');row.className='log-row'+(record.id===this.selected?' selected':'');row.dataset.id=record.id;
      const check=document.createElement('input');check.type='checkbox';check.disabled=record.active||record.status==='running';check.checked=this.checked.has(record.id);check.setAttribute('aria-label','选择日志 '+record.command.slice(0,60));
      check.onchange=()=>{check.checked?this.checked.add(record.id):this.checked.delete(record.id);this.renderSelection();};
      const button=document.createElement('button');button.className='log-row-button';const title=document.createElement('strong');title.textContent=record.command;
      const meta=document.createElement('small');meta.textContent=`${record.project.name} · ${time(record.startedAt)} · ${bytes(record.bytes)}`;
      const state=document.createElement('span');state.className='log-result '+record.status;state.textContent=(record.kind==='session'&&record.active?'会话记录中':labels[record.status]||record.status)+(record.complete?'':' · 有缺失');
      button.append(title,meta,state);button.onclick=()=>{this.selected=record.id;this.renderList();this.readSelected();};row.append(check,button);list.append(row);
    }
    $('logsEmpty').hidden=records.length>0;$('logCount').textContent=`${records.length} 份记录 · ${bytes(records.reduce((n,r)=>n+r.bytes,0))}`;this.renderSelection();
  }
  renderSelection(){const selectable=this.filtered().filter(r=>!r.active&&r.status!=='running'),count=selectable.filter(r=>this.checked.has(r.id)).length;$('selectLogs').checked=selectable.length>0&&count===selectable.length;$('selectLogs').indeterminate=count>0&&count<selectable.length;$('selectLogs').disabled=!selectable.length;$('deleteLogs').disabled=!this.checked.size;$('deleteLogs').textContent=this.checked.size?`清理所选 (${this.checked.size})`:'清理所选';}
  emptyDetail(){$('logDetailEmpty').hidden=false;$('logDetailBody').hidden=true;}
  async readSelected(){
    const id=this.selected;if(!id)return;
    try{
      const record=await window.studio.readLog(id);if(id!==this.selected||!this.visible)return;
      $('logDetailEmpty').hidden=true;$('logDetailBody').hidden=false;$('logCommand').textContent=record.command;
      $('logProjectDetail').textContent=record.project.name;$('logCwd').textContent=record.cwd;$('logStart').textContent=time(record.startedAt);$('logEnd').textContent=time(record.endedAt);
      $('logExit').textContent=record.exitCode===null?'未确认':String(record.exitCode);$('logDuration').textContent=record.durationMs===null||record.durationMs===undefined?'未确认':(record.durationMs/1000).toFixed(2)+' 秒';
      $('logFilePath').textContent=record.file;$('logCompleteness').hidden=record.complete;$('logCompleteness').textContent=record.complete?'':'此记录包含缺失输出或未正常封存，不能视为完整运行记录。';
      $('logTailNotice').hidden=!record.truncated;$('logOutput').textContent=record.text||'当前尚无文本输出。';
      $('logBackgroundNotice').hidden=!record.backgroundJobs?.length;
    }catch(e){if(id===this.selected){this.emptyDetail();this.toast(e.message);}}
  }
}
