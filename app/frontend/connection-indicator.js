// Original app implementation of the 4×4 dot-matrix visual referenced by the user.
export function setConnectionIndicator(container,busy,fallback,random=Math.random){
  if(!busy){container.replaceChildren(fallback);return;}
  if(container.firstElementChild?.classList.contains('matrix-loader'))return;
  const matrix=document.createElement('span'),pattern=Math.floor(random()*4);
  matrix.className='matrix-loader';matrix.dataset.pattern=String(pattern);matrix.setAttribute('aria-hidden','true');
  const ring=[0,1,2,3,7,11,15,14,13,12,8,4,5,6,10,9];
  const shuffled=Array.from({length:16},(_,i)=>(i*7)%16);
  for(let index=0;index<16;index++){
    const row=Math.floor(index/4),column=index%4,dot=document.createElement('i');
    const order=[column*2,Math.abs(row-1.5)+Math.abs(column-1.5),ring.indexOf(index),shuffled[index]][pattern];
    dot.style.animationDelay=(order*75-1500)+'ms';matrix.append(dot);
  }
  container.replaceChildren(matrix);
}

const indicators=new WeakMap(),pending=new WeakMap();
export function showActivity(target,busy){
  busy=Boolean(busy)||(pending.get(target)||0)>0;
  target.classList.toggle('activity-busy',Boolean(busy));target.setAttribute('aria-busy',String(Boolean(busy)));
  let indicator=indicators.get(target);
  if(!busy){indicator?.remove();indicators.delete(target);return;}
  if(!indicator){indicator=document.createElement('span');indicator.className='activity-indicator';indicator.setAttribute('aria-hidden','true');setConnectionIndicator(indicator,true,null);indicators.set(target,indicator);}
  if(indicator.parentNode!==target)target.prepend(indicator);
}
export async function withActivity(target,operation){
  pending.set(target,(pending.get(target)||0)+1);showActivity(target,true);
  try{return await operation();}
  finally{const count=pending.get(target)-1;if(count)pending.set(target,count);else pending.delete(target);showActivity(target,count>0);}
}
