// noVNC 1.7 has no public first-frame event. Observe a complete pixel-bearing
// update, including its asynchronous image queue, then remove both hooks.
export function observeDesktopStartup(remote,{onFrame,onReady,onError,settleMs=150,timeoutMs=15000}){
  const rect=remote._handleRect,update=remote._framebufferUpdate;
  let active=true,pixels=false,waiting=false,settleTimer;
  const restore=()=>{if(remote._handleRect===handleRect)remote._handleRect=rect;if(remote._framebufferUpdate===framebufferUpdate)remote._framebufferUpdate=update;};
  const dispose=()=>{active=false;clearTimeout(timeout);clearTimeout(settleTimer);restore();};
  const fail=()=>{if(!active)return;dispose();onError('桌面首屏加载超时，请重新连接。');};
  function handleRect(...args){
    const {encoding,width,height}=this._FBU,result=rect.apply(this,args);
    if(result&&width>0&&height>0&&(encoding>=0||encoding===-260))pixels=true;
    return result;
  }
  function framebufferUpdate(...args){
    const result=update.apply(this,args);
    if(result&&pixels&&!waiting){
      waiting=true;
      Promise.resolve(remote._display.flush()).then(()=>{
        if(!active)return;
        clearTimeout(timeout);restore();onFrame();
        settleTimer=setTimeout(()=>{if(active){dispose();onReady();}},settleMs);
      }).catch(fail);
    }
    return result;
  }
  const timeout=setTimeout(fail,timeoutMs);
  remote._handleRect=handleRect;remote._framebufferUpdate=framebufferUpdate;
  return dispose;
}
