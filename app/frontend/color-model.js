export function normalizeHex(value){const text=String(value).trim().replace(/^#/,'');return /^[\da-f]{6}$/i.test(text)?'#'+text.toUpperCase():null;}
export function hexToRgb(value){const hex=normalizeHex(value);return hex?[1,3,5].map(at=>parseInt(hex.slice(at,at+2),16)):null;}
export function rgbToHex(rgb){return '#'+rgb.map(value=>Math.max(0,Math.min(255,Math.round(value))).toString(16).padStart(2,'0')).join('').toUpperCase();}
export function rgbToHsv([red,green,blue]){
 const r=red/255,g=green/255,b=blue/255,max=Math.max(r,g,b),min=Math.min(r,g,b),delta=max-min;
 let h=0;if(delta)h=60*(max===r?((g-b)/delta+6)%6:max===g?(b-r)/delta+2:(r-g)/delta+4);
 return {h,s:max?delta/max:0,v:max};
}
export function hsvToRgb({h,s,v}){
 const chroma=v*s,x=chroma*(1-Math.abs((h/60)%2-1)),m=v-chroma;
 const part=h<60?[chroma,x,0]:h<120?[x,chroma,0]:h<180?[0,chroma,x]:h<240?[0,x,chroma]:h<300?[x,0,chroma]:[chroma,0,x];
 return part.map(value=>Math.round((value+m)*255));
}
