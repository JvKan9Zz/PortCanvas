const {app, BrowserWindow, ipcMain, dialog, shell} = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const {Profiles, validateProfile} = require('./profiles.cjs');
const {DeviceSession} = require('./session.cjs');
const {FileService} = require('./files.cjs');
const {TerminalSession} = require('./terminal.cjs');
const {RunLogs} = require('./run-logs.cjs');

const root = path.resolve(__dirname, '../../..');
const dataDir = app.isPackaged ? path.join(path.dirname(process.execPath), 'data') : path.join(root, '04-数据与素材/private/studio-data');
app.setPath('userData', dataDir);
let window, store, session, files, logs, quitting=false;

function safeHandler(name, handler) {
  ipcMain.handle(name, async (event, ...args) => {
    if (event.sender !== window?.webContents) throw new Error('Invalid caller');
    try { return {ok: true, value: await handler(...args)}; }
    catch (error) { return {ok: false, error: error.message}; }
  });
}

app.whenReady().then(() => {
  store = new Profiles(dataDir);
  session = new DeviceSession({verifyHost: async (address, fingerprint) => {
    const known = store.read().hostKeys[address];
    if (known === fingerprint) return true;
    if (known) {
      await dialog.showMessageBox(window, {type: 'error', title: '主机身份已变化', message: '为避免连错设备，已停止连接。', detail: `设备：${address}\n已保存：${known}\n当前：${fingerprint}\n请核实设备是否重装或地址是否变化，再移除该设备配置重新添加。`, buttons: ['返回']});
      return false;
    }
    const result = await dialog.showMessageBox(window, {type: 'question', title: '首次连接设备', message: '确认这台设备的 SSH 身份', detail: `${address}\n${fingerprint}\n请与设备或已信任客户端的指纹核对。`, buttons: ['取消', '信任并连接'], defaultId: 0, cancelId: 0, noLink: true});
    if (result.response !== 1) return false;
    store.trust(address, fingerprint); return true;
  }});
  window = new BrowserWindow({
    width: 1360, height: 880, minWidth: 1000, minHeight: 680,
    title: 'Wheel Combat Studio · Demo V0.3.5', backgroundColor: '#eef1f3',
    webPreferences: {preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true}
  });
  window.setMenuBarVisibility(false);
  window.webContents.setWindowOpenHandler(() => ({action: 'deny'}));
  window.webContents.on('will-navigate', event => event.preventDefault());
  files = new FileService(session,{localHome:app.getPath('documents'),confirmOverwrite:async paths=>{
    const detail=paths.slice(0,10).join('\n')+(paths.length>10?`\n另有 ${paths.length-10} 个文件。`:'');
    const result=await dialog.showMessageBox(window,{type:'question',title:'确认覆盖文件',message:`目标中已有 ${paths.length} 个同名文件，是否覆盖？`,detail,buttons:['取消传输','覆盖这些文件'],defaultId:0,cancelId:0,noLink:true});
    return result.response===1;
  }});
  files.on('state',state=>{if(!window.isDestroyed())window.webContents.send('file-state',state);});
  session.on('state', state => { if (!window.isDestroyed()) window.webContents.send('device-state', state); });
  window.on('closed', () => session.disconnect());
  safeHandler('profiles:list', () => store.read().devices);
  safeHandler('profiles:save', input => store.save(input));
  safeHandler('profiles:remove', async id => {
    const result = await dialog.showMessageBox(window, {type: 'question', message: '移除这个设备配置？', detail: '这只移除本机连接配置。', buttons: ['取消', '移除配置'], cancelId: 0, defaultId: 0});
    if (result.response !== 1) return false;
    if (session.profile?.id === id) session.disconnect();
    const data = store.read();
    const removed = data.devices.find(p => p.id === id);
    store.remove(id);
    if (removed && !data.devices.some(p => p.id !== id && p.host === removed.host && p.port === removed.port)) {
      const updated = store.read(); delete updated.hostKeys[`${removed.host}:${removed.port}`]; store.write(updated);
    }
    return true;
  });
  safeHandler('device:connect', (input, password) => session.connect(validateProfile(input), {password: String(password)}));
  safeHandler('device:disconnect', () => session.disconnect());
  safeHandler('device:state', () => session.state);
  logs = new RunLogs(session,{root:path.join(app.getPath('documents'),'Wheel Combat Studio','Logs'),settingsFile:path.join(dataDir,'log-settings.json')});
  logs.on('state',state=>{if(!window.isDestroyed())window.webContents.send('logs-state',state);});
  logs.on('change',()=>{if(!window.isDestroyed())window.webContents.send('logs-changed');});
  const terminal = new TerminalSession(session,{prepare:(client,profile)=>logs.prepare(client,profile),sessionName:profile=>logs.sessionName(profile)});
  terminal.on('state', state => {if(!window.isDestroyed())window.webContents.send('terminal-state',state);});
  terminal.on('data', data => {if(!window.isDestroyed())window.webContents.send('terminal-data',data);});
  safeHandler('terminal:start', (cols,rows) => terminal.start(cols,rows));
  safeHandler('terminal:stop', () => terminal.stop());
  safeHandler('terminal:state', () => terminal.state);
  safeHandler('terminal:write', (id,data) => terminal.write(id,data));
  safeHandler('terminal:resize', (id,cols,rows) => terminal.resize(id,cols,rows));
  safeHandler('logs:state',()=>logs.snapshot());
  safeHandler('logs:list',()=>logs.store.list());
  safeHandler('logs:read',id=>logs.store.detail(id));
  safeHandler('logs:project',value=>logs.setProject(value));
  safeHandler('logs:directory',async()=>{
    if(logs.store.writers.size||logs.store.pending.size)throw new Error('请等待当前记录完成，或先断开设备再更换日志位置。');
    const result=await dialog.showOpenDialog(window,{title:'选择电脑本地日志目录',defaultPath:logs.store.root,properties:['openDirectory','createDirectory']});
    return result.canceled?logs.snapshot():logs.setRoot(result.filePaths[0]);
  });
  safeHandler('logs:open-folder',async id=>{const directory=id?logs.store.detail(id).directory:logs.store.root;if(!id)fs.mkdirSync(directory,{recursive:true});const error=await shell.openPath(directory);if(error)throw new Error(error);});
  safeHandler('logs:remove',async ids=>{
    const records=logs.store.removable(ids);if(!records.length)return {removed:0};
    const result=await dialog.showMessageBox(window,{type:'warning',title:'清理本地日志',message:`删除选中的 ${records.length} 份已结束或中断的日志？`,detail:records.slice(0,8).map(r=>`${r.project.name} / ${r.day} / ${r.command.slice(0,70)}`).join('\n')+'\n只处理电脑本地日志，正在写入的记录保留。',buttons:['取消','删除这些日志'],defaultId:0,cancelId:0,noLink:true});
    return result.response===1?logs.store.remove(ids):{removed:0,canceled:true};
  });
  safeHandler('files:list', (side,directory) => files.list(side,directory));
  safeHandler('files:mkdir', (side,parent,name) => files.mkdir(side,parent,name));
  safeHandler('files:transfer', options => files.transfer(options));
  safeHandler('files:cancel', () => files.cancel());
  safeHandler('files:state', () => files.state);
  safeHandler('files:choose-local', async () => {
    const selected=await dialog.showOpenDialog(window,{title:'选择本地文件夹',properties:['openDirectory']});
    return selected.canceled?null:selected.filePaths[0];
  });
  safeHandler('desktop:start', () => session.startDesktop());
  safeHandler('desktop:stop', () => {session.stopDesktop();session.update({message: '桌面已断开，设备连接保留。'});});
  safeHandler('desktop:lost', clean => {session.stopDesktop();session.update({desktop:'error', message: clean ? '桌面连接已关闭，可重新连接。' : '桌面连接中断，请检查网络后重新连接。'});});
  safeHandler('desktop:ready', () => session.desktopReady());
  safeHandler('window:fullscreen', () => {window.setFullScreen(!window.isFullScreen()); return window.isFullScreen();});
  window.loadFile(path.join(__dirname, '../ui/index.html'));
});
app.on('window-all-closed', () => app.quit());
app.on('before-quit', event => {
  if(quitting)return;
  event.preventDefault();session?.disconnect();
  Promise.resolve(logs?.close()).finally(()=>{quitting=true;app.quit();});
});
