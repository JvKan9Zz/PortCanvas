// Keep the browser's editing menu where it is useful, without a page menu.
export function installDesktopInteractions(root=document){
  root.addEventListener('contextmenu',event=>{
    const target=event.target;
    if(!(target instanceof Element)||!target.closest('input:not([type=checkbox]):not([type=radio]),textarea,[contenteditable=true]'))event.preventDefault();
  });
}
