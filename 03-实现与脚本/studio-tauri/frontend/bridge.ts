import {invoke} from '@tauri-apps/api/core';
type Handler=(data:any)=>void;
const handlers=new Map<string,Set<Handler>>();
const on=(event:string,handler:Handler)=>{if(!handlers.has(event))handlers.set(event,new Set());handlers.get(event)!.add(handler);};
const state={ssh:'disconnected',desktop:'disconnected',message:'V0.4 M1 原型：正在验证 WebView2 与 Rust 连接能力。'};
const call=(action:string,...args:unknown[])=>invoke<any>('studio_call',{action,args});
const unavailable=async()=>{throw Error('此 M1 原型仅用于迁移可行性验证，请使用 V0.3.5 完成正式操作。');};
const api={
  profiles:()=>call('profiles:list'),save:(p:unknown)=>call('profiles:save',p),remove:unavailable,
  connect:async(p:unknown,password:string)=>{const report=await call('compat:probe',p,password);state.message='M1 连接探针通过：'+JSON.stringify(report);for(const h of handlers.get('device-state')||[])h({...state});return state;},
  disconnect:async()=>state,state:async()=>state,
  terminalState:async()=>({phase:'disconnected',id:0,message:'M1 终端组件兼容性验证'}),startTerminal:unavailable,stopTerminal:unavailable,writeTerminal:unavailable,resizeTerminal:unavailable,
  logState:async()=>({phase:'idle',root:'M1 原型未启用日志写入',project:'',message:'正式记录继续使用 V0.3.5。'}),logList:async()=>[],readLog:unavailable,logProject:unavailable,chooseLogDirectory:unavailable,openLogFolder:unavailable,removeLogs:unavailable,
  listFiles:unavailable,createFolder:unavailable,transfer:unavailable,cancelTransfer:unavailable,fileState:async()=>({phase:'idle',connection:'disconnected'}),chooseLocalFolder:unavailable,
  startDesktop:unavailable,stopDesktop:unavailable,desktopLost:unavailable,desktopReady:unavailable,fullscreen:()=>call('window:fullscreen'),
  onState:(h:Handler)=>on('device-state',h),onTerminalState:(h:Handler)=>on('terminal-state',h),onTerminalData:(h:Handler)=>on('terminal-data',h),onLogState:(h:Handler)=>on('logs-state',h),onLogsChanged:(h:Handler)=>on('logs-changed',h),onFileState:(h:Handler)=>on('file-state',h)
};
Object.defineProperty(window,'studio',{value:api,writable:false});
