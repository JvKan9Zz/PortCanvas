const $=id=>document.getElementById(id);
export class TerminalDrawer {
  constructor({toast,goDevices}) {
    this.toast=toast;this.goDevices=goDevices;this.connected=false;this.visible=false;
    this.state={phase:'disconnected',id:0,message:'连接设备后可打开终端。'};
    this.setupResize();
    $('connectTerminal').onclick=()=>this.connected?this.start():goDevices();
    $('disconnectTerminal').onclick=()=>window.studio.stopTerminal().catch(e=>toast(e.message));
    $('clearTerminal').onclick=()=>{this.term?.clear();this.term?.focus();};
    window.studio.onTerminalState(state=>this.render(state));
    window.studio.onTerminalData(({id,data,ack})=>{if(id===this.state.id&&this.term)this.term.write(data,ack);else ack?.();});
    window.studio.terminalState().then(state=>this.render(state)).catch(e=>toast(e.message));
  }
  setupResize() {
    const app=$('appRoot'),handle=$('terminalResize');
    this.preferredWidth=null;
    const limits=()=>({min:320,max:Math.max(320,app.clientWidth-560)});
    const setWidth=value=>{
      this.preferredWidth=value;
      const {min,max}=limits(),width=Math.round(Math.max(min,Math.min(max,value??Math.min(580,Math.max(360,app.clientWidth*.4)))));
      app.style.setProperty('--terminal-width',width+'px');
      handle.setAttribute('aria-valuemin',min);handle.setAttribute('aria-valuemax',max);handle.setAttribute('aria-valuenow',width);handle.setAttribute('aria-valuetext',`终端宽度 ${width} 像素`);
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
  async ensure() {
    if(this.term)return;
    if(this.loading)return this.loading;
    this.loading=this.create();
    try{await this.loading;}finally{this.loading=null;}
  }
  async create() {
    const [{Terminal},{FitAddon}]=await Promise.all([import('@xterm/xterm'),import('@xterm/addon-fit')]);
    this.term=new Terminal({fontFamily:'"Cascadia Mono", Consolas, monospace',fontSize:14,cursorBlink:true,scrollback:5000,screenReaderMode:true,theme:{background:'#17222b',foreground:'#e1e8ed',cursor:'#9cddcb',selectionBackground:'#31524f'}});
    this.fit=new FitAddon();this.term.loadAddon(this.fit);this.term.open($('terminalOutput'));
    // Handle wheel input before xterm can turn it into PTY keys or mouse reports.
    $('terminalOutput').addEventListener('wheel',event=>{
      event.preventDefault();event.stopPropagation();
      const lineHeight=this.term.element.clientHeight/this.term.rows;
      const lines=event.deltaY*(event.deltaMode===1?1:event.deltaMode===2?this.term.rows:1/lineHeight);
      const buffer=this.term.buffer.active;
      if(lines<0?buffer.viewportY>0:buffer.viewportY<buffer.baseY){
        const previous=this.wheelRemainder||0;
        this.wheelRemainder=(Math.sign(previous)===Math.sign(lines)?previous:0)+lines;
        const count=Math.trunc(this.wheelRemainder);
        if(count){this.term.scrollLines(count);this.wheelRemainder-=count;}
      }else{
        this.wheelRemainder=0;$('terminalDrawer').scrollTop+=lines*lineHeight;
      }
    },{capture:true,passive:false});
    this.term.textarea?.setAttribute('aria-label','远端设备终端输入');
    this.term.onData(data=>{
      if(this.state.phase!=='connected'||!this.connected)return;
      const id=this.state.id;
      window.studio.writeTerminal(id,data).catch(e=>{if(id===this.state.id&&this.state.phase==='connected'&&this.connected)this.toast(e.message);});
    });
    this.term.onResize(({cols,rows})=>{
      if(this.state.phase==='connected')window.studio.resizeTerminal(this.state.id,cols,rows).catch(()=>{});
    });
    this.observer=new ResizeObserver(()=>this.fitVisible());this.observer.observe($('terminalOutput'));
    this.term.options.disableStdin=this.state.phase!=='connected';
  }
  fitVisible() {if(this.visible&&this.term&&$('terminalOutput').clientWidth>0) this.fit.fit();}
  show() {this.visible=true;this.ensure().then(()=>{if(!this.visible)return;requestAnimationFrame(()=>{this.fitVisible();if(this.state.phase==='connected')this.term.focus();});}).catch(error=>this.toast(error.message));}
  hide() {this.visible=false;this.endResize();}
  setConnection(state) {this.connected=state.ssh==='connected';$('logProjectName').disabled=!this.connected;$('saveLogProject').disabled=!this.connected;this.render(this.state);}
  render(state) {
    this.state=state;
    if(state.phase==='connecting'&&state.id!==this.displayId){this.term?.reset();this.displayId=state.id;}
    const live=state.phase==='connected'&&this.connected,busy=state.phase==='connecting'&&this.connected;
    if(this.term)this.term.options.disableStdin=!live;
    $('connectTerminal').disabled=busy||live;
    $('connectTerminal').textContent=busy?'正在连接…':live?'终端已连接':this.connected?'连接 / 恢复终端':'前往连接设备';
    $('connectTerminal').setAttribute('aria-busy',String(busy));
    $('disconnectTerminal').disabled=!live&&!busy;
    $('clearTerminal').disabled=!this.term;
    $('terminalStatus').className='badge '+(live?'connected':state.phase==='error'?'error':'');
    $('terminalStatus').lastElementChild.textContent=live?'会话已连接':busy?'正在连接':state.phase==='error'?'连接失败':'终端未连接';
    $('terminalMessage').textContent=state.message;
    $('terminalSession').textContent=state.session?'会话 '+state.session:'';
    if(live&&this.visible){this.fitVisible();this.term?.focus();}
  }
  async start() {
    try {await this.ensure();this.fitVisible();await window.studio.startTerminal(this.term.cols,this.term.rows);}
    catch(error){this.toast(error.message);}
  }
}
