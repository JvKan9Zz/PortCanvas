export function animateDisclosure(details,reduce=()=>window.matchMedia('(prefers-reduced-motion: reduce)').matches){
  const summary=details.querySelector(':scope > summary');if(!summary)return()=>{};
  let expanded=details.open,active=null;
  details.dataset.expanded=String(expanded);
  const clear=()=>{details.style.removeProperty('height');details.style.removeProperty('overflow');};
  const click=event=>{
    event.preventDefault();
    const start=details.getBoundingClientRect().height;
    expanded=!expanded;details.dataset.expanded=String(expanded);
    active?.cancel();active=null;clear();
    details.open=expanded;
    if(reduce()||!details.animate)return;
    const end=details.getBoundingClientRect().height;
    details.open=true;details.style.height=start+'px';details.style.overflow='hidden';
    const animation=details.animate([{height:start+'px'},{height:end+'px'}],{duration:260,easing:'cubic-bezier(.16,1,.3,1)'});
    active=animation;
    animation.onfinish=()=>{if(active!==animation)return;details.open=expanded;active=null;clear();};
  };
  const toggle=()=>{if(!active){expanded=details.open;details.dataset.expanded=String(expanded);}};
  summary.addEventListener('click',click);details.addEventListener('toggle',toggle);
  return()=>{summary.removeEventListener('click',click);details.removeEventListener('toggle',toggle);active?.cancel();clear();};
}
export function installDisclosureMotion(){document.querySelectorAll('details').forEach(details=>animateDisclosure(details));}
