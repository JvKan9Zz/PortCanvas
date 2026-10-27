export class ConnectionDetails {
 constructor(element,reduce=()=>window.matchMedia('(prefers-reduced-motion: reduce)').matches){
  this.element=element;this.reduce=reduce;this.visible=!element.hidden;this.epoch=0;
 }
 setVisible(visible){
  if(visible===this.visible)return this.pending||Promise.resolve(visible);
  const element=this.element,start=element.hidden?0:element.getBoundingClientRect().height;
  const opacity=element.hidden?0:Number.parseFloat(typeof getComputedStyle==='function'?getComputedStyle(element).opacity:element.style.opacity||'1');
  this.visible=visible;const epoch=++this.epoch;this.active?.cancel();this.active=null;
  element.hidden=false;element.inert=!visible;element.setAttribute('aria-hidden',String(!visible));
  const finish=()=>{if(epoch!==this.epoch)return;element.hidden=!visible;element.style.removeProperty('height');element.style.removeProperty('overflow');this.active=null;};
  if(this.reduce()||!element.animate){finish();return this.pending=Promise.resolve(visible);}
  element.style.height='auto';const end=visible?element.getBoundingClientRect().height:0;
  element.style.height=start+'px';element.style.overflow='hidden';
  const animation=element.animate([{height:start+'px',opacity},{height:end+'px',opacity:visible?1:0}],{duration:visible?260:180,easing:visible?'cubic-bezier(.16,1,.3,1)':'cubic-bezier(.4,0,1,1)',fill:'both'});
  this.active=animation;animation.onfinish=()=>{if(epoch!==this.epoch)return;finish();animation.cancel();};
  return this.pending=animation.finished.then(()=>epoch===this.epoch&&this.visible,()=>false);
 }
}
