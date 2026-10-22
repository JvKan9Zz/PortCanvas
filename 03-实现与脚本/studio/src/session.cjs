const {EventEmitter} = require('node:events');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {Client} = require('ssh2');
const {WebSocketServer, createWebSocketStream} = require('ws');
const {validateProfile} = require('./profiles.cjs');

function friendlyError(error) {
  const message = error?.message || String(error);
  if (/authentication|auth methods/i.test(message)) return '认证失败：请检查用户名和密码后重试。';
  if (/ECONNREFUSED/.test(message)) return '连接被拒绝：请确认树莓派的 SSH 服务和端口。';
  if (/ETIMEDOUT|timed out|Timeout/.test(message)) return '连接超时：请检查设备地址、供电和网络。';
  if (/ENOTFOUND|EAI_AGAIN/.test(message)) return '无法解析设备地址：请检查主机名或改用 IP。';
  if (/EHOSTUNREACH|ENETUNREACH/.test(message)) return '设备不可达：请确认电脑和树莓派在同一网络。';
  return message;
}

class DeviceSession extends EventEmitter {
  constructor({verifyHost, ClientClass = Client} = {}) {
    super();
    this.verifyHost = verifyHost;
    this.ClientClass = ClientClass;
    this.state = {ssh: 'disconnected', desktop: 'disconnected', message: '尚未连接设备'};
    this.generation = 0;
  }
  update(data) { Object.assign(this.state, data); this.emit('state', {...this.state}); }
  async connect(input, credentials) {
    const profile = validateProfile(input);
    if (!credentials.password && !credentials.privateKey) throw new Error('请输入本次连接使用的密码。');
    this.disconnect();
    const generation = this.generation;
    const client = this.client = new this.ClientClass();
    this.profile = profile;
    this.update({ssh: 'connecting', desktop: 'disconnected', message: `正在连接 ${profile.host}…`, device: profile.name});
    let verificationError;
    await new Promise((resolve, reject) => {
      client.once('ready', () => {
        if (generation !== this.generation) return reject(new Error('连接已取消。'));
        this.update({ssh: 'connected', message: '设备连接成功，可以打开远程桌面或浏览文件。'});
        resolve();
      });
      client.on('error', error => {
        if (generation === this.generation) {
          this.stopDesktop();
          this.update({ssh: 'error', desktop: 'disconnected', message: verificationError || friendlyError(error)});
        }
        reject(new Error(verificationError || friendlyError(error)));
      });
      client.on('close', () => {
        if (generation === this.generation && this.client === client) {
          this.stopDesktop(); this.client = null;
          if (this.state.ssh !== 'error') this.update({ssh: 'disconnected', desktop: 'disconnected', message: '设备连接已关闭，请重新连接。'});
        }
        reject(new Error('连接已关闭。'));
      });
      client.connect({
        host: profile.host, port: profile.port, username: profile.username,
        ...credentials, readyTimeout: 15000, keepaliveInterval: 5000, keepaliveCountMax: 2,
        hostVerifier: (key, callback) => {
          const fingerprint = 'SHA256:' + crypto.createHash('sha256').update(key).digest('base64').replace(/=+$/, '');
          Promise.resolve(this.verifyHost?.(`${profile.host}:${profile.port}`, fingerprint) ?? false)
            .then(accepted => { if (!accepted) verificationError = '主机身份未确认，已停止连接。'; callback(Boolean(accepted) && generation === this.generation); })
            .catch(() => callback(false));
        }
      });
    });
    return {...this.state};
  }
  disconnect() {
    this.generation++;
    this.stopDesktop();
    const client = this.client;
    this.client = null;
    if (client) client.destroy();
    this.update({ssh: 'disconnected', desktop: 'disconnected', message: '设备已断开'});
  }
  stopDesktop() {
    this.desktopGeneration = (this.desktopGeneration || 0) + 1;
    if (this.wss) {
      for (const ws of this.wss.clients) ws.terminate();
      this.wss.close(); this.wss = null;
    }
    if (this.desktopChannel) { this.desktopChannel.end(); this.desktopChannel = null; }
    this.update({desktop: 'disconnected'});
  }
  async startDesktop() {
    if (!this.client || this.state.ssh !== 'connected') throw new Error('请先在设备页面连接树莓派。');
    this.stopDesktop();
    const generation = this.desktopGeneration;
    const client = this.client;
    this.update({desktop: 'connecting', message: '正在共享树莓派当前桌面…'});
    const source = fs.readFileSync(path.join(__dirname, 'desktop.py'), 'utf8');
    const config = JSON.stringify({display: this.profile.display, xauthority: this.profile.xauthority});
    const code = `import sys,base64;sys.argv=['desktop',${JSON.stringify(config)}];exec(base64.b64decode('${Buffer.from(source).toString('base64')}'))`;
    const command = 'python3 -c ' + "'" + code.replace(/'/g, "'\\''") + "'";
    try {
      const info = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('桌面启动超时，请确认 Linux 桌面已登录。')), 20000);
        client.exec(command, (error, channel) => {
          if (error) { clearTimeout(timer); return reject(error); }
          if (generation !== this.desktopGeneration) { channel.end(); clearTimeout(timer); return reject(new Error('桌面连接已取消。')); }
          this.desktopChannel = channel;
          let output = '', diagnostic = '', ready = false;
          channel.stderr.on('data', data => { diagnostic = (diagnostic + data.toString()).slice(-6000); });
          channel.on('data', data => {
            output += data.toString();
            const match = output.match(/STUDIO_DESKTOP=(\{[^\r\n]+\})/);
            if (match && !ready) { ready = true; clearTimeout(timer); resolve(JSON.parse(match[1])); }
          });
          channel.on('error', reject);
          channel.on('close', () => {
            clearTimeout(timer);
            if (!ready) reject(new Error(diagnostic.trim() || '桌面服务在就绪前关闭。'));
            if (generation === this.desktopGeneration) {
              this.stopDesktop();
              this.update({desktop: 'error', message: '桌面会话已关闭，可点击重新连接桌面。'});
            }
          });
        });
      });
      if (generation !== this.desktopGeneration) throw new Error('桌面连接已取消。');
      const token = crypto.randomBytes(24).toString('hex');
      const wss = this.wss = new WebSocketServer({host: '127.0.0.1', port: 0, path: '/' + token, maxPayload: 4 * 1024 * 1024});
      await new Promise((resolve, reject) => { wss.once('listening', resolve); wss.once('error', reject); });
      if (generation !== this.desktopGeneration) { wss.close(); throw new Error('桌面连接已取消。'); }
      wss.on('connection', ws => {
        if (generation !== this.desktopGeneration || wss.clients.size > 1) return ws.close();
        client.forwardOut('127.0.0.1', 0, '127.0.0.1', info.port, (error, stream) => {
          if (error) { ws.close(1011, 'SSH tunnel failed'); return; }
          if (ws.readyState !== 1) return stream.destroy();
          const bridge = createWebSocketStream(ws);
          bridge.on('error', () => stream.destroy());
          stream.on('error', () => bridge.destroy());
          bridge.pipe(stream).pipe(bridge);
          ws.once('close', () => stream.destroy());
        });
      });
      return {url: `ws://127.0.0.1:${wss.address().port}/${token}`};
    } catch (error) {
      if (generation === this.desktopGeneration) {
        this.stopDesktop();
        this.update({desktop: 'error', message: friendlyError(error)});
      }
      throw error;
    }
  }
  desktopReady() {
    if (this.wss) this.update({desktop: 'connected', message: '远程桌面已连接'});
  }
}
module.exports = {DeviceSession, friendlyError};
