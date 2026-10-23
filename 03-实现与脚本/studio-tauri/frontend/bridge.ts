import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
type Handler=(data:any)=>void;
const handlers=new Map<string,Set<Handler>>();let generation=0;
const on=(event:string,handler:Handler)=>{if(!handlers.has(event))handlers.set(event,new Set());handlers.get(event)!.add(handler);};
const ready=Promise.all(['device-state','terminal-state','terminal-data','logs-state','logs-changed','file-state'].map(topic=>listen(topic,event=>{
 const payload=event.payload as any;
 if(topic==='device-state'){if(payload.generation<generation)return;generation=payload.generation;}
 else if(typeof payload?.generation==='number'&&payload.generation<generation)return;
 for(const handler of handlers.get(topic)||[])handler(payload);
})));
const call=async(action:string,...args:unknown[])=>{await ready;try{return await invoke<any>('studio_call',{action,args});}catch(error){throw error instanceof Error?error:new Error(String(error));}};
const unavailable=async()=>{throw Error('此功能仍在后续迁移工作包中，正式操作请使用 V0.3.5。');};
const api={
 profiles:()=>call('profiles:list'),save:(p:unknown)=>call('profiles:save',p),remove:(id:string)=>call('profiles:remove',id),
 connect:(p:unknown,password:string)=>call('device:connect',p,password),disconnect:()=>call('device:disconnect'),state:()=>call('device:state'),
 startDesktop:()=>call('desktop:start'),stopDesktop:()=>call('desktop:stop'),desktopReady:(id:number)=>call('desktop:ready',id),desktopLost:(id:number,_clean:boolean)=>call('desktop:lost',id),fullscreen:()=>call('window:fullscreen'),
 terminalState:async()=>({phase:'disconnected',id:0,message:'终端迁移在 M3 工作包中'}),startTerminal:unavailable,stopTerminal:unavailable,writeTerminal:unavailable,resizeTerminal:unavailable,
 logState:async()=>({phase:'idle',root:'日志迁移在 M4 工作包中',project:'',message:'正式日志继续使用 V0.3.5。'}),logList:async()=>[],readLog:unavailable,logProject:unavailable,chooseLogDirectory:unavailable,openLogFolder:unavailable,removeLogs:unavailable,
 listFiles:unavailable,createFolder:unavailable,transfer:unavailable,cancelTransfer:unavailable,fileState:async()=>({phase:'idle',connection:'disconnected'}),chooseLocalFolder:unavailable,
 onState:(h:Handler)=>on('device-state',h),onTerminalState:(h:Handler)=>on('terminal-state',h),onTerminalData:(h:Handler)=>on('terminal-data',h),onLogState:(h:Handler)=>on('logs-state',h),onLogsChanged:(h:Handler)=>on('logs-changed',h),onFileState:(h:Handler)=>on('file-state',h)
};
Object.defineProperty(window,'studio',{value:api,writable:false});
