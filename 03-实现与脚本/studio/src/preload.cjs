const {contextBridge, ipcRenderer} = require('electron');
const invoke = async (name, ...args) => {
  const result = await ipcRenderer.invoke(name, ...args);
  if (!result.ok) throw new Error(result.error);
  return result.value;
};
contextBridge.exposeInMainWorld('studio', {
  profiles: () => invoke('profiles:list'),
  save: profile => invoke('profiles:save', profile),
  remove: id => invoke('profiles:remove', id),
  connect: (profile, password) => invoke('device:connect', profile, password),
  disconnect: () => invoke('device:disconnect'),
  state: () => invoke('device:state'),
  listFiles: (side,directory) => invoke('files:list',side,directory),
  createFolder: (side,parent,name) => invoke('files:mkdir',side,parent,name),
  transfer: options => invoke('files:transfer',options),
  cancelTransfer: () => invoke('files:cancel'),
  fileState: () => invoke('files:state'),
  chooseLocalFolder: () => invoke('files:choose-local'),
  onFileState: handler => {ipcRenderer.on('file-state', (_event,state)=>handler(state));},
  startDesktop: () => invoke('desktop:start'),
  stopDesktop: () => invoke('desktop:stop'),
  desktopLost: clean => invoke('desktop:lost', clean),
  desktopReady: () => invoke('desktop:ready'),
  fullscreen: () => invoke('window:fullscreen'),
  onState: handler => {ipcRenderer.on('device-state', (_event, state) => handler(state));}
});
