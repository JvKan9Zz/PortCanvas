import {Terminal} from '@xterm/xterm';
import {FitAddon} from '@xterm/addon-fit';
const $=id=>document.getElementById(id);
export class TerminalDrawer {
  constructor({toast,goDevices}) {
    this.toast=toast;this.goDevices=goDevices;this.connected=false;this.visible=false;
    this.state={phase:'disconnected',id:0,message:'连接设备后可打开终端。'};
    $('connectTerminal').onclick=()=>this.connected?this.start():goDevices();
    $('disconnectTerminal').onclick=()=>window.studio.stopTerminal().catch(e=>toast(e.message));
    $('clearTerminal').onclick=()=>{this.term?.clear();this.term?.focus();};
    window.studio.onTerminalState(state=>this.render(state));
    window.studio.onTerminalData(({id,data})=>{if(id===this.state.id)this.term?.write(data);});
    window.studio.terminalState().then(state=>this.render(state)).catch(e=>toast(e.message));
  }
  ensure() {
    if(this.term)return;
    this.term=new Terminal({fontFamily:'"Cascadia Mono", Consolas, monospace',fontSize:14,cursorBlink:true,scrollback:5000,screenReaderMode:true,theme:{background:'#17222b',foreground:'#e1e8ed',cursor:'#9cddcb',selectionBackground:'#31524f'}});
    this.fit=new FitAddon();this.term.loadAddon(this.fit);this.term.open($('terminalOutput'));
    this.term.textarea?.setAttribute('aria-label','树莓派终端输入');
    this.term.onData(data=>{
      if(this.state.phase!=='connected')return;
      const id=this.state.id;
      window.studio.writeTerminal(id,data).catch(e=>{if(id===this.state.id&&this.state.phase==='connected')this.toast(e.message);});
    });
    this.term.onResize(({cols,rows})=>{
      if(this.state.phase==='connected')window.studio.resizeTerminal(this.state.id,cols,rows).catch(()=>{});
    });
    this.observer=new ResizeObserver(()=>this.fitVisible());this.observer.observe($('terminalOutput'));
    this.term.options.disableStdin=this.state.phase!=='connected';
  }
  fitVisible() {if(this.visible&&this.term&&$('terminalOutput').clientWidth>0) this.fit.fit();}
  show() {this.visible=true;this.ensure();requestAnimationFrame(()=>{this.fitVisible();if(this.state.phase==='connected')this.term.focus();});}
  hide() {this.visible=false;}
  setConnection(state) {this.connected=state.ssh==='connected';this.render(this.state);}
  render(state) {
    this.state=state;
    if(state.phase==='connecting'&&state.id!==this.displayId){this.term?.reset();this.displayId=state.id;}
    const live=state.phase==='connected',busy=state.phase==='connecting';
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
    this.ensure();this.fitVisible();
    try {await window.studio.startTerminal(this.term.cols,this.term.rows);}
    catch(error){this.toast(error.message);}
  }
}
