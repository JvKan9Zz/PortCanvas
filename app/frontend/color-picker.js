import {t,onLanguageChange} from './i18n.js';
import {normalizeHex,hexToRgb,rgbToHex,rgbToHsv,hsvToRgb} from './color-model.js';
const $=id=>document.getElementById(id);
export class ColorPicker{
 constructor(commit){
  this.commit=commit;this.panel=$('backgroundColorPicker');this.trigger=$('backgroundColorButton');this.panel.hidden=true;this.panel.inert=true;
  if(!this.panel.showPopover){this.panel.dataset.inline='true';this.panel.closest('dialog').append(this.panel);document.addEventListener('pointerdown',event=>{if(this.open&&!this.panel.contains(event.target)&&!this.trigger.contains(event.target))this.close(false);});}
  this.trigger.onclick=()=>this.open?this.close():this.show();
  $('colorPickerCancel').onclick=()=>this.close();$('colorPickerConfirm').onclick=()=>this.confirm();
  $('colorPickerHue').oninput=()=>{this.hsv.h=Number($('colorPickerHue').value);this.render();};
  $('colorPickerHex').oninput=()=>{const value=normalizeHex($('colorPickerHex').value);if(value){this.hsv=rgbToHsv(hexToRgb(value));this.render('hex');}this.validity();};
  for(const id of ['colorPickerR','colorPickerG','colorPickerB'])$(id).oninput=()=>{const values=['colorPickerR','colorPickerG','colorPickerB'].map(key=>Number($(key).value));if(values.every(value=>Number.isInteger(value)&&value>=0&&value<=255)){this.hsv=rgbToHsv(values);this.render('rgb');}this.validity();};
  const plane=$('colorPickerPlane');
  const move=event=>{const rect=plane.getBoundingClientRect();this.hsv.s=Math.max(0,Math.min(1,(event.clientX-rect.left)/rect.width));this.hsv.v=1-Math.max(0,Math.min(1,(event.clientY-rect.top)/rect.height));this.render();};
  plane.onpointerdown=event=>{if(event.button!==0)return;event.preventDefault();plane.setPointerCapture(event.pointerId);move(event);};
  plane.onpointermove=event=>{if(plane.hasPointerCapture(event.pointerId))move(event);};
  plane.onpointerup=plane.onpointercancel=event=>{if(plane.hasPointerCapture(event.pointerId))plane.releasePointerCapture(event.pointerId);};
  for(const button of this.panel.querySelectorAll('[data-color]'))button.onclick=()=>{this.hsv=rgbToHsv(hexToRgb(button.dataset.color));this.render();};
  this.panel.addEventListener('toggle',event=>{if(event.newState==='closed'&&this.open)this.close();});
  this.panel.addEventListener('keydown',event=>{
   if(event.key==='Escape'){event.preventDefault();event.stopPropagation();this.close();}
   if(event.key==='Enter'&&event.target.tagName==='INPUT'){event.preventDefault();this.confirm();}
   if(event.key==='Tab'){const controls=[...this.panel.querySelectorAll('button:not(:disabled),input:not(:disabled)')].filter(e=>e.getClientRects().length);const first=controls[0],last=controls.at(-1);if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus();}else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}event.stopPropagation();}
  });
  window.addEventListener('resize',()=>{if(this.open)this.position();});
  onLanguageChange(()=>{if(this.open)this.render();});
 }
 show(){
  this.hsv=rgbToHsv(hexToRgb($('backgroundColor').value)||[234,240,248]);this.open=true;this.panel.hidden=false;this.panel.inert=false;
  this.panel.style.visibility='hidden';if(this.panel.showPopover)this.panel.showPopover();this.render();this.position();this.panel.style.visibility='';
  this.trigger.setAttribute('aria-expanded','true');$('colorPickerHex').focus();$('colorPickerHex').select();
 }
 position(){
  const anchor=this.trigger.getBoundingClientRect(),rect=this.panel.getBoundingClientRect(),parent=this.panel.dataset.inline?this.panel.offsetParent:null;
  const bounds=parent?parent.getBoundingClientRect():{left:0,top:0,right:innerWidth,bottom:innerHeight};
  const left=Math.max(bounds.left+12,Math.min(anchor.left,bounds.right-rect.width-12)),top=anchor.bottom+rect.height+9<=bounds.bottom?anchor.bottom+8:Math.max(bounds.top+12,anchor.top-rect.height-8);
  this.panel.style.left=(left-bounds.left-(parent?.clientLeft||0))+'px';this.panel.style.top=(top-bounds.top-(parent?.clientTop||0))+'px';
 }
 close(focus=true){
  this.open=false;this.panel.inert=true;if(this.panel.hidePopover&&this.panel.matches(':popover-open'))this.panel.hidePopover();else if(!this.panel.hidePopover)this.panel.hidden=true;
  this.trigger.setAttribute('aria-expanded','false');if(focus&&this.trigger.isConnected)this.trigger.focus();
 }
 validity(){const valid=Boolean(normalizeHex($('colorPickerHex').value))&&['colorPickerR','colorPickerG','colorPickerB'].every(id=>$(id).value!==''&&$(id).checkValidity());$('colorPickerConfirm').disabled=!valid||this.saving;return valid;}
 render(editing){
  const rgb=hsvToRgb(this.hsv),hex=rgbToHex(rgb);$('colorPickerPlane').style.backgroundColor=`hsl(${this.hsv.h} 100% 50%)`;
  $('colorPickerThumb').style.left=this.hsv.s*100+'%';$('colorPickerThumb').style.top=(1-this.hsv.v)*100+'%';$('colorPickerHue').value=this.hsv.h;
  $('colorPickerSample').style.backgroundColor=hex;
  if(editing!=='hex')$('colorPickerHex').value=hex;if(editing!=='rgb')['colorPickerR','colorPickerG','colorPickerB'].forEach((id,index)=>$(id).value=rgb[index]);
  for(const button of this.panel.querySelectorAll('[data-color]')){button.setAttribute('aria-label',t('选择颜色 {color}',{color:button.dataset.color}));button.style.backgroundColor=button.dataset.color;}
  this.validity();
 }
 async confirm(){if(this.saving||!this.validity())return;this.saving=true;this.validity();try{if(await this.commit(normalizeHex($('colorPickerHex').value))!==false)this.close();}finally{this.saving=false;this.validity();}}
}
