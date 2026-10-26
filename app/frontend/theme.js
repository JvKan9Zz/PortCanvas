let theme='blue';
let appearance='light';
const systemAppearance=window.matchMedia('(prefers-color-scheme: dark)');
const listeners=new Set();
export const getTheme=()=>theme;
export const getAppearance=()=>appearance;
function applyAppearance(){
  document.documentElement.dataset.appearance=appearance==='system'?(systemAppearance.matches?'dark':'light'):appearance;
  for(const callback of listeners)callback();
}
export function setAppearance(value){appearance=['light','dark','system'].includes(value)?value:'light';applyAppearance();}
systemAppearance.addEventListener('change',()=>{if(appearance==='system')applyAppearance();});
export function setTheme(value){
  theme=['blue','green','graphite'].includes(value)?value:'blue';
  document.documentElement.dataset.theme=theme;
  for(const callback of listeners)callback();
}
export function onThemeChange(callback){listeners.add(callback);}
export function terminalTheme(){
  const style=getComputedStyle(document.documentElement);
  return {background:style.getPropertyValue('--terminal-background').trim()||'#172331',foreground:'#e1e8ed',cursor:style.getPropertyValue('--terminal-cursor').trim(),selectionBackground:style.getPropertyValue('--terminal-selection').trim()};
}
