import {withActivity} from './connection-indicator.js';
import {t,locale,onLanguageChange} from './i18n.js';
const $=id=>document.getElementById(id);
const labels={running:'记录中',succeeded:'命令成功',failed:'命令失败',interrupted:'记录中断',write_error:'写入失败',session:'会话输出'};
const time=value=>value?new Date(value).toLocaleString(locale(),{hour12:false}):'—';
const bytes=size=>size<1024?size+' B':size<1048576?(size/1024).toFixed(1)+' KB':(size/1048576).toFixed(1)+' MB';
export class LogsPage{
 constructor({toast}){
  this.toast=toast;this.records=[];this.checked=new Set();this.selected=null;this.visible=false;this.cursor=null;this.history=[];this.request=0;
  $('refreshLogs').onclick=()=>this.refresh();
  for(const id of ['logFilterProject','logFilterDay','logFilterStatus'])$(id).onchange=()=>{this.reset();this.refresh();};
  $('logPageNext').onclick=()=>{if(!this.page?.hasMore)return;this.history.push(this.cursor);this.cursor=this.page.cursor;this.checked.clear();this.refresh();};
  $('logPageBack').onclick=()=>{if(!this.history.length)return;this.cursor=this.history.pop();this.checked.clear();this.refresh();};
  $('selectLogs').onchange=()=>{for(const record of this.records)if(!record.active&&record.status!=='running')$('selectLogs').checked?this.checked.add(record.id):this.checked.delete(record.id);this.renderList();};
  $('deleteLogs').onclick=async()=>{try{const result=await withActivity($('deleteLogs'),()=>window.studio.removeLogs([...this.checked]));this.toast(result.canceled?'已取消清理。':`已清理 ${result.removed} 份本地日志。`);this.checked.clear();await this.refresh();}catch(error){toast(error.message);}};
  $('openRunDirectory').onclick=()=>this.selected&&window.studio.openLogFolder(this.selected).catch(error=>toast(error.message));
  window.studio.onLogsChanged(()=>{if(this.visible){clearTimeout(this.refreshTimer);this.refreshTimer=setTimeout(()=>this.refresh(),180);}});
  onLanguageChange(()=>{if(this.visible)this.refresh();else{this.renderList();this.renderNotice();}});
  window.studio.onLogState(state=>this.renderStatus(state));window.studio.logState().then(state=>this.renderStatus(state)).catch(error=>toast(error.message));
 }
 reset(){this.cursor=null;this.history=[];this.checked.clear();this.request++;}
 renderStatus(state){
  const changed=this.root&&this.root!==state.root;this.root=state.root;
  if(changed){this.selected=null;this.reset();for(const id of ['logFilterProject','logFilterDay','logFilterStatus'])$(id).value='';if(this.visible)this.refresh();}
  this.statusMessage=state.message;this.statusError=state.phase==='error';this.statusNotice=state.notice||'';this.renderNotice();

 }

 renderNotice(){const notice=this.page?.notice||this.statusNotice||'';$('logRecordingStatus').textContent=t(this.statusMessage||'')+(notice?' '+t(notice):'');$('logRecordingStatus').classList.toggle('error',this.statusError||Boolean(notice));$('logRecordingStatus').hidden=!this.statusError&&!notice;}
 show(){this.visible=true;this.refresh();this.tick();}
 hide(){this.visible=false;clearTimeout(this.timer);clearTimeout(this.refreshTimer);}
 tick(){clearTimeout(this.timer);if(!this.visible)return;this.timer=setTimeout(async()=>{if(this.selected)await this.readSelected();this.tick();},1200);}
 async refresh(){const request=++this.request;$('logPageNext').disabled=true;$('logPageBack').disabled=true;
  try{const page=await withActivity($('refreshLogs'),()=>window.studio.logList({project:$('logFilterProject').value,day:$('logFilterDay').value,status:$('logFilterStatus').value,cursor:this.cursor}));if(request!==this.request)return;this.page=page;this.records=page.records;
   for(const id of [...this.checked])if(!this.records.some(r=>r.id===id&&!r.active&&r.status!=='running'))this.checked.delete(id);
   this.options('logFilterProject',page.projects,'全部项目');this.options('logFilterDay',page.days.map(day=>[day,day]),'全部日期');this.renderList();this.renderNotice();
   $('logPageBack').disabled=!this.history.length;$('logPageNext').disabled=!page.hasMore;$('logPageNumber').textContent=t('第 {page} 页 · 共 {n} 份',{page:this.history.length+1,n:page.total});
   if(this.selected)await this.readSelected();else this.emptyDetail();
  }catch(error){if(request===this.request)this.toast(error.message);}
 }
 options(id,values,empty){const select=$(id),current=select.value;const entries=[...values];if(current&&!entries.some(([value])=>value===current))entries.push([current,current]);select.replaceChildren(new Option(t(empty),''),...entries.map(([value,label])=>new Option(label,value)));select.value=current;}
 renderList(){const list=$('logRunList');list.replaceChildren();for(const record of this.records){
   const row=document.createElement('li');row.className='log-row'+(record.id===this.selected?' selected':'');row.dataset.id=record.id;
   const check=document.createElement('input');check.type='checkbox';check.disabled=record.active||record.status==='running';check.checked=this.checked.has(record.id);check.setAttribute('aria-label',t('选择日志 {command}',{command:record.command.slice(0,60)}));check.onchange=()=>{check.checked?this.checked.add(record.id):this.checked.delete(record.id);this.renderSelection();};
   const button=document.createElement('button');button.className='log-row-button';const title=document.createElement('strong');title.textContent=record.command;const meta=document.createElement('small');meta.textContent=`${record.project.name} · ${time(record.startedAt)} · ${bytes(record.bytes)}`;const state=document.createElement('span');state.className='log-result '+record.status;state.textContent=t(record.kind==='session'&&record.active?'会话记录中':labels[record.status]||record.status)+(record.complete?'':t(' · 有缺失'));button.append(title,meta,state);button.onclick=()=>{this.selected=record.id;this.renderList();this.readSelected(true);};row.append(check,button);list.append(row);
  }$('logsEmpty').hidden=this.records.length>0;$('logCount').textContent=t('本页 {n} / {total} 份 · {size}',{n:this.records.length,total:this.page?.total||0,size:bytes(this.records.reduce((n,r)=>n+r.bytes,0))});this.renderSelection();
 }
 renderSelection(){const eligible=this.records.filter(r=>!r.active&&r.status!=='running'),count=eligible.filter(r=>this.checked.has(r.id)).length;$('selectLogs').checked=eligible.length>0&&count===eligible.length;$('selectLogs').indeterminate=count>0&&count<eligible.length;$('selectLogs').disabled=!eligible.length;$('deleteLogs').disabled=!this.checked.size;$('deleteLogs').textContent=this.checked.size?t('清理所选 ({n})',{n:this.checked.size}):t('清理所选');}
 emptyDetail(){$('logDetailEmpty').hidden=false;$('logDetailBody').hidden=true;}
 async readSelected(showLoading=false){const id=this.selected;if(!id)return;try{const read=()=>window.studio.readLog(id),record=await (showLoading?withActivity(document.querySelector('.log-detail'),read):read());if(id!==this.selected||!this.visible)return;$('logDetailEmpty').hidden=true;$('logDetailBody').hidden=false;$('logCommand').textContent=record.command;$('logProjectDetail').textContent=record.project.name;$('logCwd').textContent=record.cwd;$('logStart').textContent=time(record.startedAt);$('logEnd').textContent=time(record.endedAt);$('logExit').textContent=record.exitCode===null?t('未确认'):String(record.exitCode);$('logDuration').textContent=record.durationMs==null?t('未确认'):t('{n} 秒',{n:(record.durationMs/1000).toFixed(2)});$('logFilePath').textContent=record.file;$('logCompleteness').hidden=record.complete;$('logCompleteness').textContent=record.complete?'':t('此记录包含缺失输出或未正常封存，不能视为完整运行记录。');$('logTailNotice').hidden=!record.truncated;$('logOutput').textContent=record.text||t('当前尚无文本输出。');$('logBackgroundNotice').hidden=!record.backgroundJobs?.length;}catch(error){if(id===this.selected){this.selected=null;this.emptyDetail();this.toast(error.message);}}}
}
