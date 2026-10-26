import {initialCrop,moveCrop,resizeCrop} from './crop-model.js';
import {withActivity} from './connection-indicator.js';
import {t} from './i18n.js';
const $=id=>document.getElementById(id);
export class BackgroundCropDialog{
 constructor(){
  this.dialog=$('backgroundCropDialog');this.image=$('cropImage');this.stage=$('cropStage');this.selection=$('cropSelection');
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
 async open(imageData,commit){
  this.image.src=imageData;
  try{await this.image.decode();}catch{throw new Error(t('图片无法读取，请选择其他图片。'));}
  this.aspect=this.image.naturalWidth/this.image.naturalHeight;this.targetAspect=document.documentElement.clientWidth/document.documentElement.clientHeight;this.ratio=this.targetAspect/this.aspect;
  this.crop=initialCrop(this.aspect,this.targetAspect);this.commit=commit;this.busy=false;$('cropError').hidden=true;this.controls();
  const result=new Promise(resolve=>this.resolve=resolve);this.dialog.showModal();this.render();this.selection.focus();return result;
 }
 controls(){for(const id of ['confirmCrop','cancelCrop','resetCrop'])$(id).disabled=Boolean(this.busy);this.selection.tabIndex=this.busy?-1:0;}
 render(){
  const scale=Math.min(this.stage.clientWidth/this.image.naturalWidth,this.stage.clientHeight/this.image.naturalHeight);
  this.box={width:this.image.naturalWidth*scale,height:this.image.naturalHeight*scale};
  const left=(this.stage.clientWidth-this.box.width)/2,top=(this.stage.clientHeight-this.box.height)/2;
  Object.assign(this.selection.style,{left:left+this.crop.x*this.box.width+'px',top:top+this.crop.y*this.box.height+'px',width:this.crop.width*this.box.width+'px',height:this.crop.height*this.box.height+'px'});
 }
 cancel(){if(!this.busy)this.finish(null);}
 finish(result){this.drag=null;this.dialog.close();const resolve=this.resolve;this.resolve=null;resolve?.(result);}
}
