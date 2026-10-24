declare module '@novnc/novnc' {
  export default class RFB extends EventTarget {
    constructor(target:HTMLElement,url:string,options?:{shared?:boolean});
    scaleViewport:boolean;
    resizeSession:boolean;
    viewOnly:boolean;
    qualityLevel:number;
    compressionLevel:number;
    background:string;
    disconnect():void;
    focus():void;
    sendCtrlAltDel():void;
  }
}
