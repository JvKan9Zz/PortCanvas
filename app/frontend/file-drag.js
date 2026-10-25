import {t} from './i18n.js';
const $=id=>document.getElementById(id);
export class FileDrag {
  constructor(page){
    this.page=page;this.label=document.createElement('div');this.label.className='file-drag-label';this.label.hidden=true;this.label.setAttribute('aria-hidden','true');document.body.append(this.label);
    for(const side of ['local','remote'])$(side+'Files').addEventListener('pointerdown',event=>{
      const row=event.target.closest('tr[data-name]');
      if(event.button!==0||!event.isPrimary||!row||event.target.closest('input')||!this.available())return;
      this.pointer={id:event.pointerId,x:event.clientX,y:event.clientY,side,row,name:row.dataset.name,path:page.panels[side].path};
    });
    window.addEventListener('pointermove',event=>this.move(event));
    window.addEventListener('pointerup',event=>this.drop(event));
    window.addEventListener('pointercancel',()=>this.clear());
    window.addEventListener('blur',()=>this.clear());
    window.addEventListener('keydown',event=>{if(event.key==='Escape')this.clear();});
    window.studio.onFileDrop(event=>this.external(event));
  }
  available(){const p=this.page;return p.visible&&p.connected&&!p.busy&&!p.submitting&&!p.removing&&p.localLoaded&&p.remoteLoaded&&!p.panels.local.loading&&!p.panels.remote.loading;}
  target(x,y,side){
    if(!this.available())return null;
    const element=document.elementFromPoint(x,y),pane=element?.closest('[data-file-side]');
    if(!pane||pane.dataset.fileSide!==side)return null;
    const panel=this.page.panels[side],row=element.closest('tr[data-name]'),entry=panel.entries.find(item=>item.name===row?.dataset.name);
    return {pane,row:entry?.type==='directory'?row:null,path:entry?.type==='directory'?this.page.join(side,panel.path,entry.name):panel.path,side};
  }
  highlight(target){
    this.clearHighlight();if(!target)return;
    this.highlighted=target;target.pane.classList.add('drop-target');target.pane.dataset.dropLabel=t(target.side==='remote'?'松开以上传':'松开以下载');target.row?.classList.add('drop-folder');
  }
  clearHighlight(){if(this.highlighted){this.highlighted.pane.classList.remove('drop-target');this.highlighted.row?.classList.remove('drop-folder');this.highlighted=null;}}
  move(event){
    const drag=this.pointer;if(!drag||drag.id!==event.pointerId)return;
    if(!this.available()){this.clear();return;}
    if(!drag.active){
      if(Math.hypot(event.clientX-drag.x,event.clientY-drag.y)<7)return;
      drag.active=true;const selected=this.page.panels[drag.side].selected;drag.names=selected.has(drag.name)?[...selected]:[drag.name];
      try{drag.row.setPointerCapture(drag.id);}catch{}
      document.body.classList.add('file-dragging');this.label.textContent=t('{n} 项 · {direction}',{n:drag.names.length,direction:t(drag.side==='local'?'上传':'下载')});this.label.hidden=false;
    }
    event.preventDefault();this.label.style.left=Math.min(event.clientX+14,window.innerWidth-130)+'px';this.label.style.top=Math.min(event.clientY+14,window.innerHeight-45)+'px';
    this.highlight(this.target(event.clientX,event.clientY,drag.side==='local'?'remote':'local'));
  }
  drop(event){
    const drag=this.pointer;if(!drag||drag.id!==event.pointerId)return;
    const target=drag.active?this.target(event.clientX,event.clientY,drag.side==='local'?'remote':'local'):null;
    this.clear();if(!drag.active)return;
    this.page.suppressClickUntil=performance.now()+250;event.preventDefault();
    if(target)this.page.transfer(drag.side==='local'?'upload':'download',{names:drag.names,localPath:drag.side==='local'?drag.path:target.path,remotePath:drag.side==='remote'?drag.path:target.path});
  }
  external(event){
    if(event.type==='leave'){this.clearHighlight();return;}
    const target=event.position?this.target(event.position.x,event.position.y,'remote'):null;
    this.highlight(target);
    if(event.type==='drop'){
      this.clearHighlight();
      if(target&&event.paths?.length)this.page.transfer('upload',{names:[],localItems:event.paths,remotePath:target.path});
    }
  }
  clear(){
    const drag=this.pointer;this.pointer=null;
    if(drag?.row.hasPointerCapture(drag.id))drag.row.releasePointerCapture(drag.id);
    this.label.hidden=true;document.body.classList.remove('file-dragging');this.clearHighlight();
  }
}
