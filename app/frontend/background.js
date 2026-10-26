export function applyBackground(color,image){
  const root=document.documentElement;
  if(color)root.style.setProperty('--workspace-background-color',color);else root.style.removeProperty('--workspace-background-color');
  if(image)root.style.setProperty('--workspace-background-image',`url("${image}")`);else root.style.removeProperty('--workspace-background-image');
  root.dataset.backgroundCustom=String(Boolean(color||image));
  root.dataset.backgroundImage=String(Boolean(image));
  const preview=document.getElementById('backgroundPreview');
  if(preview){preview.style.backgroundColor=color||'var(--workspace-base)';preview.style.backgroundImage=image?`url("${image}")`:'none';}
}
