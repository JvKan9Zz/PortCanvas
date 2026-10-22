const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

function validateProfile(input) {
  const p = {
    id: typeof input.id === 'string' && /^[\w-]{1,80}$/.test(input.id) ? input.id : crypto.randomUUID(),
    name: String(input.name || '').trim(),
    host: String(input.host || '').trim(),
    port: Number(input.port),
    username: String(input.username || '').trim(),
    display: String(input.display || ':0').trim(),
    xauthority: String(input.xauthority || '').trim()
  };
  if (!p.name || p.name.length > 80) throw new Error('请输入设备名称（最多 80 字）。');
  if (!p.host || p.host.length > 253 || !/^[a-zA-Z0-9.:-]+$/.test(p.host)) throw new Error('请输入有效的 IP 地址或主机名。');
  if (!Number.isInteger(p.port) || p.port < 1 || p.port > 65535) throw new Error('SSH 端口应为 1–65535 的整数。');
  if (!/^[a-zA-Z0-9_.-]{1,64}$/.test(p.username)) throw new Error('请输入有效的 Linux 用户名。');
  if (!/^:\d{1,3}(\.\d)?$/.test(p.display)) throw new Error('桌面显示编号格式应为 :0 或 :1。');
  if (p.xauthority && (!p.xauthority.startsWith('/') || /[\r\n\0]/.test(p.xauthority))) throw new Error('桌面授权文件须填写 Linux 绝对路径，或留空自动选择。');
  return p;
}

class Profiles {
  constructor(directory) { this.file = path.join(directory, 'devices.json'); }
  read() {
    if (!fs.existsSync(this.file)) return {devices: [], hostKeys: {}};
    const data = JSON.parse(fs.readFileSync(this.file, 'utf8'));
    return {devices: data.devices.map(validateProfile), hostKeys: data.hostKeys || {}};
  }
  write(data) {
    fs.mkdirSync(path.dirname(this.file), {recursive: true});
    const temp = this.file + '.tmp';
    fs.writeFileSync(temp, JSON.stringify(data, null, 2), {mode: 0o600});
    fs.renameSync(temp, this.file);
  }
  save(input) {
    const p = validateProfile(input), data = this.read();
    const index = data.devices.findIndex(d => d.id === p.id);
    if (index < 0) data.devices.push(p); else data.devices[index] = p;
    this.write(data);
    return p;
  }
  remove(id) {
    const data = this.read();
    data.devices = data.devices.filter(d => d.id !== id);
    this.write(data);
  }
  trust(address, fingerprint) {
    const data = this.read();
    data.hostKeys[address] = fingerprint;
    this.write(data);
  }
}
module.exports = {Profiles, validateProfile};
