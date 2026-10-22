const {EventEmitter} = require('node:events');
const crypto = require('node:crypto');

function terminalName(profile) {
  return 'wcs-' + crypto.createHash('sha256').update(JSON.stringify([profile.host, profile.port, profile.username])).digest('hex').slice(0, 16);
}
function dimensions(cols, rows) {
  if (!Number.isInteger(cols) || !Number.isInteger(rows) || cols < 2 || cols > 1000 || rows < 1 || rows > 500) throw new Error('终端尺寸无效。');
  return {cols, rows};
}
class TerminalSession extends EventEmitter {
  constructor(session, {timeout = 15000, prepare = null, sessionName = terminalName} = {}) {
    super(); this.session = session; this.timeout = timeout; this.prepare=prepare; this.sessionName=sessionName;
    this.state = {phase:'disconnected', id:0, message:'连接设备后可打开终端。'};
    session.on('state', state => {
      if (state.ssh !== 'connected' && ['connecting','connected'].includes(this.state.phase)) this.stop('设备连接已断开。重新连接设备后，可恢复远端终端会话。');
    });
  }
  update(patch) { Object.assign(this.state,patch); this.emit('state',{...this.state}); }
  stop(message = '终端已断开，远端会话保留。再次连接可继续。') {
    this.state.id++; this.cancelStart?.(); this.cancelStart = null;
    const channel=this.channel; this.channel=null;
    if (channel) { channel.end(); channel.close(); }
    this.update({phase:'disconnected',message});
  }
  async start(cols = 100, rows = 30) {
    dimensions(cols,rows);
    if (this.pending) return this.pending;
    if (!this.session.client || this.session.state.ssh !== 'connected') throw new Error('请先连接设备，再打开终端。');
    if (this.state.phase === 'connected') return {...this.state};
    this.stop();
    const {client,profile,generation}=this.session, id=this.state.id, name=this.sessionName(profile);
    this.update({phase:'connecting',session:name,message:'正在恢复远端终端会话…'});
    const operation=new Promise((resolve,reject) => {
      let settled=false;
      const finish=error => {
        if (settled) return; settled=true; clearTimeout(timer); this.cancelStart=null;
        error ? reject(error) : resolve({...this.state});
      };
      const fail=error => {
        if (id === this.state.id) this.update({phase:'error',message:error.message});
        finish(error);
      };
      const timer=setTimeout(() => {fail(new Error('终端连接超时，请重试。'));this.channel?.close();},this.timeout);
      this.cancelStart=() => finish(new Error('终端连接已取消。'));
      try {
        const open=command=>{
        if(id!==this.state.id||generation!==this.session.generation||settled)return;
        client.exec(command, {pty:{term:'xterm-256color',cols,rows}}, (error,channel) => {
          if (id !== this.state.id || generation !== this.session.generation || settled) {channel?.close();return;}
          if (error) return fail(error);
          this.channel=channel; channel.setEncoding('utf8'); channel.stderr.setEncoding('utf8');
          const output=data => {if(id===this.state.id)this.emit('data',{id,data});};
          channel.on('data',output);channel.stderr.on('data',output);
          channel.on('error',fail);
          channel.on('close',code => {
            if (id !== this.state.id) return;
            this.channel=null;
            if (!settled) return fail(new Error('终端在就绪前关闭，请重试。'));
            if (this.state.phase === 'error') return;
            this.update({phase:code===0?'disconnected':'error',message:code===127?'设备未安装 tmux，无法保留断线会话。请安装 tmux 后重试。':code===0?'终端已退出，可重新打开。':'终端连接中断，可重新连接恢复会话。'});
          });
          this.update({phase:'connected',message:'终端已连接。切换页面不会断开；关闭软件或断网后可恢复会话。'});finish();
        });};
        if(this.prepare)Promise.resolve().then(()=>this.prepare(client,profile)).then(open).catch(fail);
        else open(`exec tmux -L wcs-terminal -f /dev/null new-session -A -s ${name}`);
      } catch(error) {fail(error);}
    });
    this.pending=operation;
    try {return await operation;} finally {if(this.pending===operation)this.pending=null;}
  }
  active(id) {
    if (id !== this.state.id || this.state.phase !== 'connected' || !this.channel) throw new Error('终端连接已失效，请重新连接。');
    return this.channel;
  }
  write(id,data) {
    if (typeof data !== 'string' || Buffer.byteLength(data)>65536) throw new Error('单次输入过长，请分段粘贴。');
    this.active(id).write(data);
  }
  resize(id,cols,rows) {dimensions(cols,rows);this.active(id).setWindow(rows,cols,0,0);}
}
module.exports={TerminalSession,terminalName};
