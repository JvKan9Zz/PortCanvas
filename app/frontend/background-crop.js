import {initialCrop,moveCrop,resizeCrop} from './crop-model.js';
import {withActivity} from './connection-indicator.js';
import {t} from './i18n.js';
const $=id=>document.getElementById(id);
export class BackgroundCropDialog{
 constructor(){
  this.dialog=$('backgroundCropDialog');this.image=$('cropImage');this.video=$('cropVideo');this.stage=$('cropStage');this.selection=$('cropSelection');this.nativeHidden=false;
  document.addEventListener('visibilitychange',()=>this.setHidden(this.nativeHidden));
  $('cancelCrop').onclick=()=>this.cancel();$('resetCrop').onclick=()=>{this.crop=initialCrop(this.aspect,this.targetAspect);this.render();};
  this.dialog.addEventListener('cancel',event=>{event.preventDefault();this.cancel();});
  $('confirmCrop').onclick=async()=>{
   if(this.busy)return;this.busy=true;this.controls();$('cropError').hidden=true;
   try{const result=await withActivity($('confirmCrop'),()=>this.commit({...this.crop}));this.finish(result);}
   catch(error){$('cropError').textContent=t(error.message);$('cropError').hidden=false;}
   finally{this.busy=false;this.controls();}
  };
  this.selection.onpointerdown=event=>{
   if(this.busy||event.button!==0)return;event.preventDefault();this.selection.focus();
   this.drag={id:event.pointerId,x:event.clientX,y:event.clientY,crop:{...this.crop},corner:event.target.closest('[data-corner]')?.dataset.corner};this.selection.setPointerCapture(event.pointerId);
  };
  this.selection.onpointermove=event=>{
   if(!this.drag||this.drag.id!==event.pointerId)return;
   const dx=(event.clientX-this.drag.x)/this.box.width,dy=(event.clientY-this.drag.y)/this.box.height;
   this.crop=this.drag.corner?resizeCrop(this.drag.crop,this.drag.corner,dx,dy,this.ratio):moveCrop(this.drag.crop,dx,dy);this.render();
  };
  this.selection.onpointerup=this.selection.onpointercancel=this.selection.onlostpointercapture=()=>{this.drag=null;};
  this.selection.onkeydown=event=>{
   if(this.busy||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;event.preventDefault();
   const dx=event.key==='ArrowLeft'?-.01:event.key==='ArrowRight'?.01:0,dy=event.key==='ArrowUp'?-.01:event.key==='ArrowDown'?.01:0;
   this.crop=event.shiftKey?resizeCrop(this.crop,'se',dx,dy,this.ratio):moveCrop(this.crop,dx,dy);this.render();
  };
  new ResizeObserver(()=>{if(this.dialog.open)this.render();}).observe(this.stage);
 }
 async open(imageData,commit,video=false){
  this.image=video?this.video:$('cropImage');$('cropImage').hidden=video;this.video.hidden=!video;
  try{
   if(video){await new Promise((resolve,reject)=>{const timer=setTimeout(()=>finish(new Error('timeout')),15000),finish=error=>{clearTimeout(timer);this.video.onloadedmetadata=null;this.video.onerror=null;error?reject(error):resolve();};this.video.onloadedmetadata=()=>finish();this.video.onerror=()=>finish(new Error('decode'));this.video.src=imageData;this.video.load();});}
   else{this.image.src=imageData;await this.image.decode();}
   this.width=video?this.video.videoWidth:this.image.naturalWidth;this.height=video?this.video.videoHeight:this.image.naturalHeight;if(!this.width||!this.height)throw new Error('dimensions');
  }catch{this.release();throw new Error(t('媒体无法读取，请选择其他图片或视频。'));}
  this.aspect=this.width/this.height;this.targetAspect=document.documentElement.clientWidth/document.documentElement.clientHeight;this.ratio=this.targetAspect/this.aspect;
  this.crop=initialCrop(this.aspect,this.targetAspect);this.commit=commit;this.busy=false;$('cropError').hidden=true;this.controls();
  const result=new Promise(resolve=>this.resolve=resolve);this.dialog.showModal();this.render();this.selection.focus();this.setHidden(this.nativeHidden);return result;
 }
 controls(){for(const id of ['confirmCrop','cancelCrop','resetCrop'])$(id).disabled=Boolean(this.busy);this.selection.tabIndex=this.busy?-1:0;}
 render(){
  const scale=Math.min(this.stage.clientWidth/this.width,this.stage.clientHeight/this.height);
  this.box={width:this.width*scale,height:this.height*scale};
  const left=(this.stage.clientWidth-this.box.width)/2,top=(this.stage.clientHeight-this.box.height)/2;
  Object.assign(this.selection.style,{left:left+this.crop.x*this.box.width+'px',top:top+this.crop.y*this.box.height+'px',width:this.crop.width*this.box.width+'px',height:this.crop.height*this.box.height+'px'});
 }
 cancel(){if(!this.busy)this.finish(null);}
 setHidden(hidden){this.nativeHidden=Boolean(hidden);if(this.dialog.open&&!this.video.hidden&&!this.nativeHidden&&!document.hidden)this.video.play().catch(()=>{});else this.video.pause();}
 release(){this.video.pause();this.video.removeAttribute('src');this.video.load();$('cropImage').removeAttribute('src');}
 finish(result){this.drag=null;this.dialog.close();this.release();const resolve=this.resolve;this.resolve=null;this.commit=null;resolve?.(result);}
}
