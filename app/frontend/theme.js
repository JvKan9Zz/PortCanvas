let theme='blue';
const listeners=new Set();
export const getTheme=()=>theme;
export function setTheme(value){
  theme=['blue','green','graphite'].includes(value)?value:'blue';
  document.documentElement.dataset.theme=theme;
  for(const callback of listeners)callback();
}
export function onThemeChange(callback){listeners.add(callback);}
export function terminalTheme(){
  const style=getComputedStyle(document.documentElement);
  return {background:'#17222b',foreground:'#e1e8ed',cursor:style.getPropertyValue('--terminal-cursor').trim(),selectionBackground:style.getPropertyValue('--terminal-selection').trim()};
}
