import {commandContext,completionEdit,commonPrefix} from './command-completion.js';
import {t,onLanguageChange} from './i18n.js';
import {terminalTheme,onThemeChange} from './theme.js';
const $=id=>document.getElementById(id);
export class TerminalDrawer {
  constructor({toast,goDevices}) {
    this.toast=toast;this.goDevices=goDevices;this.connected=false;this.visible=false;this.panes=new Map();this.activeKey='main';this.deviceKey='';this.request=0;this.busy=false;this.catalogReady=false;
    this.setupResize();
    onThemeChange(()=>{for(const pane of this.panes.values())if(pane.term)pane.term.options.theme=terminalTheme();});
    $('connectTerminal').onclick=()=>this.connected?this.current?.start():goDevices();
    $('disconnectTerminal').onclick=()=>this.current&&window.studio.stopTerminal(this.current.key).catch(e=>toast(e.message));
    $('clearTerminal').onclick=()=>{this.current?.term?.clear();this.current?.term?.focus();};
    $('newTerminal').onclick=()=>this.create();$('restartTerminal').onclick=()=>this.restart();
    $('logProjectName').oninput=()=>{if(this.current)this.current.projectDraft=$('logProjectName').value;};
    $('saveLogProject').onclick=async()=>{const pane=this.current;if(!pane)return;const project=$('logProjectName').value;pane.logRequest++;try{pane.log=await window.studio.logProject(project,pane.key);if(pane.projectDraft===project)delete pane.projectDraft;this.renderLog();toast('项目设置已保存，对此终端之后开始的命令生效。');}catch(error){toast(error.message);}};
    $('terminalTabs').onkeydown=event=>{
      if(event.target.getAttribute('role')!=='tab')return;
      const keys=[...this.panes.keys()],index=keys.indexOf(this.activeKey),next={ArrowLeft:(index+keys.length-1)%keys.length,ArrowRight:(index+1)%keys.length,Home:0,End:keys.length-1}[event.key];
      if(next!==undefined){event.preventDefault();this.select(keys[next],false);$('terminal-tab-'+keys[next])?.focus();}
    };
    $('terminalOutput').addEventListener('wheel',event=>{
      event.preventDefault();event.stopPropagation();const pane=this.current,term=pane?.term;
      const lineHeight=(term?.element?.clientHeight/term?.rows)||18;
      const lines=event.deltaY*(event.deltaMode===1?1:event.deltaMode===2?(term?.rows||24):1/lineHeight),buffer=term?.buffer.active;
      if(buffer&&(lines<0?buffer.viewportY>0:buffer.viewportY<buffer.baseY)){
        const previous=pane.wheelRemainder||0;pane.wheelRemainder=(Math.sign(previous)===Math.sign(lines)?previous:0)+lines;
        const count=Math.trunc(pane.wheelRemainder);if(count){term.scrollLines(count);pane.wheelRemainder-=count;}
      }else{if(pane)pane.wheelRemainder=0;$('terminalDrawer').scrollTop+=lines*lineHeight;}
    },{capture:true,passive:false});
    window.studio.onTerminalState(state=>{const pane=this.panes.get(state.key||'main');if(pane&&(!state.deviceKey||state.deviceKey===pane.deviceKey)){pane.update(state);this.render();}});
    window.studio.onTerminalData(({key,id,data,ack})=>{const pane=this.panes.get(key||'main');if(pane&&id===pane.state.id&&pane.term)pane.term.write(data,ack);else ack?.();});
    window.studio.onLogState(state=>{for(const pane of this.panes.values()){const entry=(state.terminals||[]).find(item=>item.key===pane.key&&item.deviceKey===pane.deviceKey);if(state.configError)pane.log={...pane.log,phase:'error',message:state.message,configError:true};else if(entry)pane.log=entry;else if(pane.log?.configError)pane.log={...pane.log,phase:'idle',message:state.message,configError:false};}this.renderLog();});
    onLanguageChange(()=>{this.clearCompletions();const project=$('logProjectName').value;this.render();$('logProjectName').value=project;for(const pane of this.panes.values())pane.term?.textarea?.setAttribute('aria-label',t(pane.title+'远端输入'));});
    this.setupCommand();
    this.reload();
  }
  get current(){return this.panes.get(this.activeKey);}
  setupCommand(){
    const input=$('terminalCommand');
    $('expandCommand').onclick=()=>{this.commandExpanded=!this.commandExpanded;this.renderCommandSize();if(!input.disabled)input.focus();};
    input.oninput=()=>{this.clearCompletions();if(this.current)this.current.draft=input.value;this.renderCommandSize();};
    input.onkeydown=event=>{
      if(event.isComposing||event.keyCode===229)return;
      if(this.completions){if(['ArrowUp','ArrowDown','Tab'].includes(event.key)&&!event.shiftKey){event.preventDefault();const list=this.completions;list.index=(list.index+(event.key==='ArrowUp'?-1:1)+list.items.length)%list.items.length;this.renderCompletions();return;}if(event.key==='Enter'&&!event.shiftKey&&!event.ctrlKey&&!event.metaKey){event.preventDefault();this.chooseCompletion(this.completions.index);return;}if(event.key==='Escape'){event.preventDefault();this.clearCompletions();return;}}
      if(event.key==='Tab'&&!event.shiftKey){event.preventDefault();this.completeCommand();return;}
      if(event.key==='Enter'&&(event.ctrlKey||event.metaKey||(!this.commandExpanded&&!event.shiftKey))){event.preventDefault();$('terminalCommandForm').requestSubmit();}
      else if(['ArrowUp','ArrowDown'].includes(event.key)&&!this.commandExpanded&&!input.value.includes('\n')&&this.current?.history.length){
        event.preventDefault();this.clearCompletions();const pane=this.current;
        if(pane.historyIndex===pane.history.length)pane.historyDraft=input.value;
        pane.historyIndex=Math.max(0,Math.min(pane.history.length,pane.historyIndex+(event.key==='ArrowUp'?-1:1)));
        input.value=pane.historyIndex===pane.history.length?pane.historyDraft||'':pane.history[pane.historyIndex];pane.draft=input.value;this.renderCommandSize();
      }
    };
    input.addEventListener('blur',()=>this.clearCompletions());
    $('terminalCommandForm').onsubmit=async event=>{
      event.preventDefault();this.clearCompletions();const pane=this.current;
      if(!pane||pane.sending||!this.connected||pane.state.phase!=='connected')return;
      const id=pane.state.id,command=input.value;pane.draft=command;pane.sending=true;this.renderCommand();
      try{
        const text=command.replace(/\r?\n/g,'\r'),framed=command&&pane.term?.modes.bracketedPasteMode?'\x1b[200~'+text+'\x1b[201~':text;
        await window.studio.writeTerminal(id,framed+'\r',pane.key);
        if(command.trim()){pane.history.push(command);if(pane.history.length>100)pane.history.shift();}
        pane.historyIndex=pane.history.length;pane.historyDraft='';if(pane.draft===command)pane.draft='';
      }catch(error){this.toast(error.message);}finally{pane.sending=false;this.renderCommand();if(this.current===pane&&this.visible&&!input.disabled)input.focus();}
    };
  }
  renderCommandSize(){
    const input=$('terminalCommand'),expanded=Boolean(this.commandExpanded),button=$('expandCommand');
    $('terminalCommandForm').classList.toggle('expanded',expanded);button.setAttribute('aria-expanded',String(expanded));
    button.title=t(expanded?'收起命令输入框':'展开命令输入框');button.setAttribute('aria-label',button.title);button.classList.toggle('selected',expanded);
    input.title=t(expanded?'Enter换行；Ctrl+Enter发送；Tab补全':'Enter发送；Shift+Enter换行；Tab补全');
    input.style.height='';
    if(!expanded){input.style.height='auto';input.style.height=Math.min(110,Math.max(38,input.scrollHeight))+'px';}
  }
  clearCompletions(){
    this.completionSerial=(this.completionSerial||0)+1;this.completions=null;
    $('terminalCompletions').hidden=true;$('terminalCommand').setAttribute('aria-expanded','false');$('terminalCommand').setAttribute('aria-busy','false');$('terminalCommand').removeAttribute('aria-activedescendant');
  }
  renderCompletions(){
    const list=this.completions,menu=$('terminalCompletions');menu.replaceChildren();if(!list){menu.hidden=true;return;}
    list.items.forEach((item,index)=>{const row=document.createElement('li');row.id='command-option-'+index;row.setAttribute('role','option');row.setAttribute('aria-selected',String(index===list.index));row.className=index===list.index?'selected':'';row.textContent=item.label;row.onmousedown=event=>event.preventDefault();row.onclick=()=>this.chooseCompletion(index);menu.append(row);});
    menu.hidden=false;$('terminalCommand').setAttribute('aria-expanded','true');$('terminalCommand').setAttribute('aria-activedescendant','command-option-'+list.index);menu.children[list.index]?.scrollIntoView({block:'nearest'});
  }
  chooseCompletion(index){
    const list=this.completions;if(!list)return;
    const edit=completionEdit(list.context,list.items[index]);this.clearCompletions();const input=$('terminalCommand');input.value=edit.value;input.setSelectionRange(edit.cursor,edit.cursor);if(this.current)this.current.draft=input.value;this.renderCommandSize();input.focus();
  }
  async completeCommand(){
    const pane=this.current,input=$('terminalCommand');if(!pane||input.disabled)return;
    this.clearCompletions();const serial=this.completionSerial,id=pane.state.id,context=commandContext(input.value,input.selectionStart);
    const menu=$('terminalCompletions'),loading=document.createElement('li');loading.textContent=t('正在补全…');loading.setAttribute('role','status');menu.replaceChildren(loading);menu.hidden=false;input.setAttribute('aria-busy','true');
    try{const result=await window.studio.completeTerminal(pane.key,id,context.prefix,context.commands,context.directories);
      if(serial!==this.completionSerial||this.current!==pane||pane.state.id!==id||input.value!==context.value||document.activeElement!==input)return;
      input.setAttribute('aria-busy','false');if(!result.items.length){this.clearCompletions();return;}
      this.completions={items:result.items,index:0,context};
      if(result.items.length===1){this.chooseCompletion(0);return;}
      const shared=commonPrefix(result.items);if(!result.truncated&&shared.length>context.prefix.length){const edit=completionEdit(context,{value:shared,kind:'directory'},false);input.value=edit.value;input.setSelectionRange(edit.cursor,edit.cursor);pane.draft=input.value;this.completions.context=commandContext(input.value,edit.cursor);}
      this.renderCompletions();
    }catch(error){if(serial===this.completionSerial){this.clearCompletions();this.toast(error.message);}}
  }
  renderCommand(){
    const pane=this.current,input=$('terminalCommand'),disabled=!pane||!this.connected||pane.state.phase!=='connected'||pane.sending;
    input.disabled=Boolean(disabled);$('sendCommand').disabled=Boolean(disabled);
    if(input.value!==(pane?.draft||''))input.value=pane?.draft||'';
    this.renderCommandSize();
  }

  setupResize() {
    const app=$('appRoot'),handle=$('terminalResize');
    this.preferredWidth=null;
    const limits=()=>({min:320,max:Math.max(320,app.clientWidth-560)});
    const setWidth=value=>{
      this.preferredWidth=value;
      const {min,max}=limits(),width=Math.round(Math.max(min,Math.min(max,value??Math.min(580,Math.max(360,app.clientWidth*.4)))));
      app.style.setProperty('--terminal-width',width+'px');
      handle.setAttribute('aria-valuemin',min);handle.setAttribute('aria-valuemax',max);handle.setAttribute('aria-valuenow',width);handle.setAttribute('aria-valuetext',t('终端宽度 {n} 像素',{n:width}));
    };
    const end=()=>{
      const drag=this.drag;this.drag=null;app.classList.remove('terminal-resizing');
      if(drag&&handle.hasPointerCapture(drag.id))handle.releasePointerCapture(drag.id);
    };
    this.endResize=end;
    handle.onpointerdown=event=>{
      if(event.button!==0||!this.visible)return;
      event.preventDefault();handle.focus();
      this.drag={id:event.pointerId,x:event.clientX,width:Number(handle.getAttribute('aria-valuenow'))};
      app.classList.add('terminal-resizing');handle.setPointerCapture(event.pointerId);
    };
    handle.onpointermove=event=>{if(this.drag?.id===event.pointerId)setWidth(this.drag.width+this.drag.x-event.clientX);};
    handle.onpointerup=handle.onpointercancel=handle.onlostpointercapture=end;
    handle.onkeydown=event=>{
      const {min,max}=limits(),width=Number(handle.getAttribute('aria-valuenow'));
      const value={ArrowLeft:width+24,ArrowRight:width-24,Home:min,End:max}[event.key];
      if(value!==undefined){event.preventDefault();setWidth(value);}
    };
    handle.ondblclick=()=>setWidth(null);
    window.addEventListener('resize',()=>setWidth(this.preferredWidth));
    setWidth(null);
  }
  async reload(){const request=++this.request;this.catalogReady=false;this.render();try{const catalog=await window.studio.terminalList();if(request===this.request)this.apply(catalog);}catch(error){this.toast(error.message);}}
  apply(catalog,selected){
    this.request++;this.catalogReady=Boolean(catalog.deviceKey)&&this.connected;
    if(catalog.deviceKey!==this.deviceKey){for(const pane of this.panes.values())pane.dispose();this.panes.clear();this.deviceKey=catalog.deviceKey;this.activeKey='main';}
    const keys=new Set(catalog.tabs.map(tab=>tab.key));for(const [key,pane] of this.panes)if(!keys.has(key)){pane.dispose();this.panes.delete(key);}
    for(const tab of catalog.tabs){if(!this.panes.has(tab.key))this.panes.set(tab.key,new TerminalPane(this,tab,this.deviceKey));else this.panes.get(tab.key).update(tab.state);}
    if(this.panes.size)this.select(this.panes.has(selected)?selected:this.panes.has(this.activeKey)?this.activeKey:this.panes.keys().next().value,false);else{this.activeKey='';this.render();}
  }
  select(key,focus=true){
    if(!this.panes.has(key))return;this.clearCompletions();this.activeKey=key;
    for(const pane of this.panes.values())pane.element.hidden=pane.key!==key;
    if(this.visible)this.current.show(focus);this.render();
    const pane=this.current,request=++pane.logRequest;window.studio.logState(key).then(state=>{if(!pane.disposed&&request===pane.logRequest&&(!state.deviceKey||state.deviceKey===pane.deviceKey)){pane.log=state;if(this.current===pane)this.renderLog();}}).catch(e=>this.toast(e.message));
  }
  renderTabs(){
    const list=$('terminalTabs'),focused=document.activeElement?.getAttribute('role')==='tab'?document.activeElement.dataset.key:null;list.replaceChildren();
    for(const pane of this.panes.values()){
      const wrap=document.createElement('div');wrap.className='terminal-tab'+(pane.key===this.activeKey?' selected':'');
      const tab=document.createElement('button');tab.type='button';tab.id='terminal-tab-'+pane.key;tab.dataset.key=pane.key;tab.setAttribute('role','tab');tab.setAttribute('aria-selected',String(pane.key===this.activeKey));tab.setAttribute('aria-controls',pane.element.id);tab.tabIndex=pane.key===this.activeKey?0:-1;tab.textContent=t(pane.title);tab.title=t(pane.title);tab.className=pane.state.phase==='connected'&&this.connected?'live':'';tab.onclick=()=>this.select(pane.key);wrap.append(tab);
      {const close=document.createElement('button');close.type='button';close.className='terminal-tab-close';close.textContent='×';close.setAttribute('aria-label',t('关闭'+pane.title));close.title=t('关闭'+pane.title);close.disabled=this.busy||!this.connected||!this.catalogReady;close.onclick=()=>this.close(pane);wrap.append(close);}
      list.append(wrap);
    }if(focused)$('terminal-tab-'+focused)?.focus();
  }
  renderLog(){
    const log=this.current?.log||{phase:'idle',message:'连接此终端后自动记录。',project:''};
    $('terminalLogStatus').hidden=log.phase!=='error';$('terminalLogStatus').textContent=t(log.message);$('terminalLogStatus').classList.toggle('error',log.phase==='error');
    if(document.activeElement!==$('logProjectName'))$('logProjectName').value=this.current?.projectDraft??log.project??'';
  }
  render(){
    const pane=this.current,state=pane?.state||{phase:'disconnected',message:'连接设备后可打开终端。'},live=state.phase==='connected'&&this.connected,busy=(state.phase==='connecting'||pane?.pending)&&this.connected;
    for(const item of this.panes.values())if(item.term)item.term.options.disableStdin=!(this.connected&&item.state.phase==='connected');
    this.renderTabs();$('terminalEmpty').hidden=this.panes.size>0;$('connectTerminal').disabled=this.busy||busy||live||(this.connected&&(!this.catalogReady||!pane));$('connectTerminal').textContent=t(busy?'正在连接…':live?'终端已连接':this.connected?'连接 / 恢复终端':'前往连接设备');$('connectTerminal').setAttribute('aria-busy',String(busy));
    $('disconnectTerminal').disabled=this.busy||(!live&&!busy);$('restartTerminal').disabled=this.busy||busy||!this.connected||!pane||!this.catalogReady;$('newTerminal').disabled=this.busy||!this.connected||!this.catalogReady;$('clearTerminal').disabled=!pane?.term;
    $('logProjectName').disabled=!this.connected||!pane;$('saveLogProject').disabled=this.busy||!this.connected||!this.catalogReady||!pane;
    $('terminalStatus').className='badge '+(live?'connected':state.phase==='error'?'error':'');$('terminalStatus').lastElementChild.textContent=t(live?'会话已连接':busy?'正在连接':state.phase==='error'?'连接失败':'终端未连接');
    $('terminalMessage').hidden=state.phase!=='error';$('terminalMessage').textContent=t(state.message);
    $('terminalSession').textContent=state.session?t('{host} · 会话 {session}',{host:state.host||t('远端设备'),session:state.session}):t('会话信息将在连接后显示');
    $('terminalConnectionTime').textContent=pane?.connectedAt?t('本次连接 {time}',{time:pane.connectedAt}):'';this.renderLog();this.renderCommand();
  }
  async create(){if(this.busy)return;this.busy=true;this.render();try{const result=await window.studio.newTerminal(this.deviceKey);this.apply(result.catalog,result.key);await this.current.start();}catch(error){this.toast(error.message);}finally{this.busy=false;this.render();}}
  async restart(){const pane=this.current;if(this.busy||!pane)return;this.busy=true;this.render();try{const result=await window.studio.restartTerminal(pane.key,pane.deviceKey);if(!result.canceled&&!pane.disposed){pane.term?.reset();await pane.start();}}catch(error){this.toast(error.message);}finally{this.busy=false;this.render();}}
  async close(pane){if(this.busy)return;this.busy=true;this.render();try{const result=await window.studio.closeTerminal(pane.key,pane.deviceKey);if(!result.canceled)this.apply(result.catalog);}catch(error){this.toast(error.message);}finally{this.busy=false;this.render();}}
  fitVisible(){this.current?.fitVisible();}
  show(){this.visible=true;this.current?.show(true);this.reload();}
  hide(){this.visible=false;this.clearCompletions();this.endResize();}
  setConnection(state){const before=this.connected,generation=this.generation;this.connected=state.ssh==='connected';this.generation=state.generation;if(!this.connected)this.catalogReady=false;if(this.connected&&(!before||generation!==state.generation))this.reload();this.render();}
}
class TerminalPane {
  constructor(manager,tab,deviceKey){
    this.manager=manager;this.key=tab.key;this.title=tab.title;this.deviceKey=deviceKey;this.state=tab.state;this.disposed=false;this.logRequest=0;this.draft='';this.history=[];this.historyIndex=0;
    this.element=document.createElement('div');this.element.id='terminal-pane-'+this.key;this.element.className='terminal-pane';this.element.setAttribute('role','tabpanel');this.element.setAttribute('aria-labelledby','terminal-tab-'+this.key);this.element.hidden=true;$('terminalOutput').append(this.element);
  }
  async ensure(){if(this.term||this.disposed)return;if(this.loading)return this.loading;this.loading=this.create();try{await this.loading;}finally{this.loading=null;}}
  async create(){
    const [{Terminal},{FitAddon}]=await Promise.all([import('@xterm/xterm'),import('@xterm/addon-fit')]);if(this.disposed)return;
    this.term=new Terminal({fontFamily:'"Cascadia Mono", Consolas, monospace',fontSize:14,cursorBlink:true,scrollback:5000,screenReaderMode:true,theme:terminalTheme()});
    this.fit=new FitAddon();this.term.loadAddon(this.fit);this.term.open(this.element);this.term.textarea?.setAttribute('aria-label',t(this.title+'远端输入'));
    this.term.onData(data=>{if(!this.manager.connected||this.state.phase!=='connected')return;const id=this.state.id;window.studio.writeTerminal(id,data,this.key).catch(error=>{if(!this.disposed&&id===this.state.id&&this.state.phase==='connected')this.manager.toast(error.message);});});
    this.term.onResize(({cols,rows})=>{if(this.manager.connected&&this.state.phase==='connected')window.studio.resizeTerminal(this.state.id,cols,rows,this.key).catch(()=>{});});
    this.observer=new ResizeObserver(()=>this.fitVisible());this.observer.observe(this.element);this.term.options.disableStdin=!(this.manager.connected&&this.state.phase==='connected');
  }
  update(state){if(this.disposed||Number(state.id)<Number(this.state.id))return;const changed=state.id!==this.state.id;if(state.phase==='connecting'&&changed)this.term?.reset();if(state.phase==='connected'&&(changed||this.state.phase!=='connected'))this.connectedAt=new Date().toLocaleTimeString('zh-CN',{hour12:false});this.state={...this.state,...state};if(this.term)this.term.options.disableStdin=!(this.manager.connected&&state.phase==='connected');if(state.phase==='connected')this.fitVisible();}
  fitVisible(){if(!this.disposed&&this.manager.visible&&this.manager.current===this&&this.term&&this.element.clientWidth>0)this.fit.fit();}
  show(focus){this.ensure().then(()=>{if(this.disposed||!this.manager.visible||this.manager.current!==this)return;requestAnimationFrame(()=>{this.fitVisible();if(focus&&this.state.phase==='connected')this.term?.focus();});}).catch(error=>this.manager.toast(error.message));}
  async start(){if(this.pending||this.disposed)return;this.pending=true;this.manager.render();try{await this.ensure();if(this.disposed)return;this.fitVisible();this.update(await window.studio.startTerminal(this.term.cols,this.term.rows,this.key,this.deviceKey));}catch(error){this.manager.toast(error.message);}finally{this.pending=false;this.manager.render();if(!this.disposed&&this.state.phase==='connected'&&this.manager.visible&&this.manager.current===this)this.term?.focus();}}
  dispose(){this.disposed=true;this.observer?.disconnect();this.term?.dispose();this.element.remove();}
}
