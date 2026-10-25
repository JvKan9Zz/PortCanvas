import {invoke,Channel} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {getCurrentWebviewWindow} from '@tauri-apps/api/webviewWindow';
type Handler=(data:any)=>void;
const handlers=new Map<string,Set<Handler>>();let generation=0;
const on=(event:string,handler:Handler)=>{if(!handlers.has(event))handlers.set(event,new Set());handlers.get(event)!.add(handler);};
const ready=Promise.all([...['device-state','terminal-state','terminal-data','logs-state','logs-changed','file-state','arena-pose','arena-visibility','update-state'].map(topic=>listen(topic,event=>{
 const payload=event.payload as any;
 if(topic==='device-state'){if(payload.generation<generation)return;generation=payload.generation;}
 else if(typeof payload?.generation==='number'&&payload.generation<generation)return;
 for(const handler of handlers.get(topic)||[])handler(payload);
})),getCurrentWebviewWindow().onDragDropEvent(event=>{
 const payload=event.payload,position='position' in payload?{x:payload.position.x/window.devicePixelRatio,y:payload.position.y/window.devicePixelRatio}:null;
 for(const handler of handlers.get('file-drop')||[])handler({...payload,position});
})]);
const call=async(action:string,...args:unknown[])=>{await ready;try{return await invoke<any>('studio_call',{action,args});}catch(error){throw error instanceof Error?error:new Error(String(error));}};
const startTerminal=async(cols:number,rows:number,key:string="main",deviceKey:string="")=>{
 await ready;const decoder=new TextDecoder();const channel=new Channel<ArrayBuffer>();
 channel.onmessage=frame=>{const view=new DataView(frame),id=Number(view.getBigUint64(0,true)),sequence=Number(view.getBigUint64(8,true));const data=decoder.decode(new Uint8Array(frame,16),{stream:true});let consumed=false;const ack=()=>{if(consumed)return;consumed=true;call('terminal:ack',id,sequence,key).catch(()=>{});};const targets=handlers.get('terminal-data');if(!targets?.size){ack();return;}for(const handler of targets)handler({key,id,data,ack});};
 try{return await invoke('terminal_subscribe',{cols,rows,channel,terminalKey:key,deviceKey});}catch(error){throw error instanceof Error?error:new Error(String(error));}
};
const api={
 setDesktopMode:(mode:string)=>call('settings:desktop-mode',mode),
 settings:()=>call('settings:read'),setLanguage:(language:string)=>call('settings:language',language),setTheme:(theme:string)=>call('settings:theme',theme),checkUpdates:()=>call('settings:updates'),updateState:()=>call('settings:update-state'),downloadUpdate:()=>call('settings:update-download'),cancelUpdate:()=>call('settings:update-cancel'),installUpdate:()=>call('settings:update-install'),onUpdate:(h:Handler)=>on('update-state',h),openRepository:()=>call('settings:repository'),
 arenaMetrics:()=>call('arena:metrics'),arenaCameras:()=>call('arena:cameras'),startArenaCamera:(path:string,fps:number=20)=>call('arena:camera-start',path,fps),arenaCameraFrame:(id:number,sequence:number)=>call('arena:camera-frame',id,sequence),stopArenaCamera:(id:number|null)=>call('arena:camera-stop',id),
 startArenaPose:()=>call('arena:pose-start'),stopArenaPose:(id:number|null)=>call('arena:pose-stop',id),arenaPose:()=>call('arena:pose'),onArenaPose:(h:Handler)=>on('arena-pose',h),
 onArenaVisibility:(h:Handler)=>on('arena-visibility',h),
 profiles:()=>call('profiles:list'),save:(p:unknown,secret:string='')=>call('profiles:save',p,secret),remove:(id:string)=>call('profiles:remove',id),choosePrivateKey:()=>call('profiles:choose-key'),
 connect:(p:unknown,password:string)=>call('device:connect',p,password),disconnect:()=>call('device:disconnect'),state:()=>call('device:state'),rebootDevice:()=>call('device:reboot'),
 startDesktop:()=>call('desktop:start'),stopDesktop:()=>call('desktop:stop'),desktopReady:(id:number)=>call('desktop:ready',id),desktopLost:(id:number,clean:boolean,reason:string='')=>call('desktop:lost',id,clean,reason),fullscreen:()=>call('window:fullscreen'),
 completeTerminal:(key:string,id:number,prefix:string,commands:boolean,directories:boolean)=>call('terminal:complete',key,id,prefix,commands,directories),
 terminalList:()=>call('terminal:list'),newTerminal:(deviceKey:string='')=>call('terminal:new',deviceKey),restartTerminal:(key:string='main',deviceKey:string='')=>call('terminal:restart',key,deviceKey),closeTerminal:(key:string,deviceKey:string='')=>call('terminal:close',key,deviceKey),terminalState:(key:string='main')=>call('terminal:state',key),startTerminal,stopTerminal:(key:string='main')=>call('terminal:stop',key),writeTerminal:(id:number,data:string,key:string='main')=>call('terminal:write',id,data,key),resizeTerminal:(id:number,cols:number,rows:number,key:string='main')=>call('terminal:resize',id,cols,rows,key),
 logState:(key?:string)=>call('logs:state',key),logList:(query:unknown={})=>call('logs:page',query),readLog:(id:string)=>call('logs:read',id),logProject:(name:string,key:string='main')=>call('logs:project',name,key),chooseLogDirectory:()=>call('logs:directory'),openLogFolder:(id?:string)=>call('logs:open-folder',id),removeLogs:(ids:string[])=>call('logs:remove',ids),migrateProfiles:()=>call('profiles:import'),rollbackProfiles:()=>call('profiles:rollback'),
 listFiles:(side:string,path:string)=>call('files:list',side,path),localDrives:()=>call('files:drives'),createFolder:(side:string,parent:string,name:string)=>call('files:mkdir',side,parent,name),transfer:(options:unknown)=>call('files:transfer',options),removeRemoteFiles:(options:unknown)=>call('files:remove',options),cancelTransfer:()=>call('files:cancel'),fileState:()=>call('files:state'),chooseLocalFolder:()=>call('files:choose-local'),
 onState:(h:Handler)=>on('device-state',h),onTerminalState:(h:Handler)=>on('terminal-state',h),onTerminalData:(h:Handler)=>on('terminal-data',h),onLogState:(h:Handler)=>on('logs-state',h),onLogsChanged:(h:Handler)=>on('logs-changed',h),onFileState:(h:Handler)=>on('file-state',h),onFileDrop:(h:Handler)=>on('file-drop',h)
};
Object.defineProperty(window,'studio',{value:api,writable:false});
