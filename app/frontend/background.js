import {projectCrop} from './crop-model.js';
import {t} from './i18n.js';
let layout=null,generation=0,nativeHidden=false,volume=0,videoSource=null;
const $=id=>document.getElementById(id);
function refreshLayout(){
 if(!layout)return;const {image,crop}=layout,root=document.documentElement,preview=$('backgroundPreview');
 const width=image.naturalWidth||image.videoWidth,height=image.naturalHeight||image.videoHeight;
 const view=projectCrop(width,height,crop,root.clientWidth,root.clientHeight);
 root.style.setProperty('--workspace-image-size',`${view.width}px ${view.height}px`);root.style.setProperty('--workspace-image-position',`${view.x}px ${view.y}px`);
 if(image===$('workspaceVideo'))Object.assign(image.style,{width:view.width+'px',height:view.height+'px',left:view.x+'px',top:view.y+'px'});
 if(preview){const sample=projectCrop(width,height,crop,preview.clientWidth,preview.clientHeight);preview.style.backgroundSize=`${sample.width}px ${sample.height}px`;preview.style.backgroundPosition=`${sample.x}px ${sample.y}px`;const video=$('backgroundVideoPreview');if(video&&!video.hidden)Object.assign(video.style,{width:sample.width+'px',height:sample.height+'px',left:sample.x+'px',top:sample.y+'px'});}
}
function feedback(message){const status=$('backgroundMediaStatus');if(status){status.textContent=t(message);status.hidden=!message;}}
export function refreshBackgroundPlayback(){
 const video=$('workspaceVideo'),preview=$('backgroundVideoPreview'),active=video&&!video.hidden&&!nativeHidden&&!document.hidden;
 if(video?.getAttribute('src')){if(!active)video.pause();else if(video.paused){const current=generation;video.play().catch(()=>{if(current!==generation||video.hidden)return;if(!video.muted){video.muted=true;$('enableBackgroundAudio').hidden=false;video.play().catch(()=>feedback('视频无法播放，请选择使用 H.264 编码的 MP4 或 WebM。'));}else feedback('视频无法播放，请选择使用 H.264 编码的 MP4 或 WebM。');});}}
 if(preview){if(active&&$('settingsDialog')?.open&&!$('settings-panel-general')?.hidden){if(!preview.getAttribute('src')){preview.src=videoSource;preview.load();preview.onloadedmetadata=()=>{if(videoSource&&video.currentTime<preview.duration)preview.currentTime=video.currentTime;};}preview.play().catch(()=>{});}else if(preview.getAttribute('src')){preview.pause();preview.removeAttribute('src');preview.load();}}
}
export function setBackgroundHidden(hidden){nativeHidden=Boolean(hidden);refreshBackgroundPlayback();}
export function setBackgroundVolume(value){
 volume=Math.max(0,Math.min(100,Number(value)||0));const video=$('workspaceVideo');
 if(video?.getAttribute('src')){video.volume=volume/100;video.muted=volume===0;const prompt=$('enableBackgroundAudio');if(prompt)prompt.hidden=true;refreshBackgroundPlayback();}
}
if(typeof window!=='undefined'){window.addEventListener('resize',refreshLayout);document.addEventListener('visibilitychange',refreshBackgroundPlayback);new ResizeObserver(refreshLayout).observe($('backgroundPreview'));}
export function applyBackground(color,image,crop=null,isVideo=false,sound=0){
 const nextUrl=image,current=++generation;layout=null;feedback('');
 videoSource=isVideo?nextUrl:null;
 const root=document.documentElement;
 root.style.removeProperty('--workspace-image-size');root.style.removeProperty('--workspace-image-position');
 if(color)root.style.setProperty('--workspace-background-color',color);else root.style.removeProperty('--workspace-background-color');
 if(nextUrl&&!isVideo)root.style.setProperty('--workspace-background-image',`url("${nextUrl}")`);else root.style.removeProperty('--workspace-background-image');
 root.dataset.backgroundCustom=String(Boolean(color||image));root.dataset.backgroundImage=String(Boolean(image&&!isVideo));root.dataset.backgroundVideo=String(Boolean(image&&isVideo));
 const preview=$('backgroundPreview');
 if(preview){preview.style.backgroundColor=color||'var(--workspace-base)';preview.style.backgroundImage=nextUrl&&!isVideo?`url("${nextUrl}")`:'none';preview.style.backgroundSize='cover';preview.style.backgroundPosition='center';}
 for(const id of ['workspaceVideo','backgroundVideoPreview']){const video=$(id);if(!video)continue;const hadSource=Boolean(video.getAttribute('src'));if(hadSource){video.pause();video.removeAttribute('src');}video.hidden=!isVideo||!nextUrl;for(const property of ['width','height','left','top'])video.style.removeProperty(property);if(!video.hidden)video.src=nextUrl;if(hadSource||!video.hidden)video.load();}
 if(nextUrl&&isVideo){const video=$('workspaceVideo');video.onloadedmetadata=()=>{if(current!==generation)return;if(crop){layout={image:video,crop};refreshLayout();}refreshBackgroundPlayback();};video.onerror=()=>{if(current===generation)feedback('视频无法播放，请选择使用 H.264 编码的 MP4 或 WebM。');};}
 else if(nextUrl&&crop){const picture=new Image();picture.onload=()=>{if(current!==generation)return;layout={image:picture,crop};refreshLayout();};picture.onerror=()=>{if(current===generation)feedback('背景图片无法读取，请重新选择。');};picture.src=nextUrl;}
 setBackgroundVolume(sound);
}
