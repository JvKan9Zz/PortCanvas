import {projectCrop} from './crop-model.js';
let layout=null,generation=0;
function refreshLayout(){
 if(!layout)return;const {image,crop}=layout,root=document.documentElement,preview=document.getElementById('backgroundPreview');
 const view=projectCrop(image.naturalWidth,image.naturalHeight,crop,root.clientWidth,root.clientHeight);
 root.style.setProperty('--workspace-image-size',`${view.width}px ${view.height}px`);root.style.setProperty('--workspace-image-position',`${view.x}px ${view.y}px`);
 if(preview){const sample=projectCrop(image.naturalWidth,image.naturalHeight,crop,preview.clientWidth,preview.clientHeight);preview.style.backgroundSize=`${sample.width}px ${sample.height}px`;preview.style.backgroundPosition=`${sample.x}px ${sample.y}px`;}
}
if(typeof window!=='undefined'){window.addEventListener('resize',refreshLayout);new ResizeObserver(refreshLayout).observe(document.getElementById('backgroundPreview'));}
export function applyBackground(color,image,crop=null){
  const current=++generation;layout=null;
  const root=document.documentElement;
  root.style.removeProperty('--workspace-image-size');root.style.removeProperty('--workspace-image-position');
  if(color)root.style.setProperty('--workspace-background-color',color);else root.style.removeProperty('--workspace-background-color');
  if(image)root.style.setProperty('--workspace-background-image',`url("${image}")`);else root.style.removeProperty('--workspace-background-image');
  root.dataset.backgroundCustom=String(Boolean(color||image));
  root.dataset.backgroundImage=String(Boolean(image));
  const preview=document.getElementById('backgroundPreview');
  if(preview){preview.style.backgroundColor=color||'var(--workspace-base)';preview.style.backgroundImage=image?`url("${image}")`:'none';preview.style.backgroundSize='cover';preview.style.backgroundPosition='center';}
  if(image&&crop){const picture=new Image();picture.onload=()=>{if(current!==generation)return;layout={image:picture,crop};refreshLayout();};picture.src=image;}
}
