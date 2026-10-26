export function applyGlassOpacity(value){
 const level=Math.max(0,Math.min(100,Number(value)||0)),t=level/100,root=document.documentElement;
 const light=.22+.58*t;
 root.style.setProperty('--glass-alpha-light',String(light));root.style.setProperty('--glass-alpha-dark',String(.24+.58*t));root.style.setProperty('--glass-blur',(5+17*t)+'px');
 root.style.setProperty('--glass-workspace-alpha',String(Math.max(.48,(.8-light)/(1-light))));
 const low=[29,46,71],high=[67,85,112];root.style.setProperty('--glass-muted-light',`rgb(${low.map((v,i)=>Math.round(v+(high[i]-v)*t)).join(' ')})`);
}
