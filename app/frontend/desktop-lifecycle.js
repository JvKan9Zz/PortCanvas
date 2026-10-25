// Keep a user's connection intent separate from whether the desktop is visible.
// Cancel an in-flight start immediately, then serialize a subsequent resume.
export class DesktopLifecycle {
  constructor({open,close,stop,onError}){
    this.open=open;this.close=close;this.stopRemote=stop;this.onError=onError;
    this.wanted=false;this.allowed=false;this.revision=0;this.pending=Promise.resolve();
  }
  setAllowed(value){
    if(this.allowed===value)return;
    this.allowed=value;
    if(this.wanted)this.sync();
  }
  start(){this.wanted=true;return this.sync();}
  stop(){this.wanted=false;return this.sync();}
  sync(){
    const revision=++this.revision;
    if(!this.wanted||!this.allowed){
      this.close();
      const stopping=this.stopRemote().catch(error=>{if(revision===this.revision)this.onError(error);});
      this.pending=this.pending.then(()=>stopping);
    }else{
      this.pending=this.pending.then(async()=>{
        if(revision!==this.revision||!this.wanted||!this.allowed)return;
        await this.open();
      }).catch(error=>{if(revision===this.revision)this.onError(error);});
    }
    return this.pending;
  }
}
