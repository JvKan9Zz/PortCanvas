import english from './en.json';
let language='zh-CN';const listeners=new Set();const bindings=[];
const patterns=Object.entries(english).filter(([key])=>/\{\w+\}/.test(key)).sort(([a],[b])=>b.replace(/\{\w+\}/g,'').length-a.replace(/\{\w+\}/g,'').length).map(([key,value])=>{
  const names=[];let at=0,source='';for(const match of key.matchAll(/\{(\w+)\}/g)){source+=escape(key.slice(at,match.index))+'(.*?)';names.push(match[1]);at=match.index+match[0].length;}source+=escape(key.slice(at));return{regex:new RegExp('^'+source+'$','s'),names,value};
});
function escape(value){return value.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');}
export const locale=()=>language==='en'?'en-US':'zh-CN';
export const getLanguage=()=>language;
export function t(value,variables={}){
  const source=String(value??'');let text=source,values=variables;
  if(language==='en'){
    text=english[source];if(text===undefined){for(const pattern of patterns){const match=source.match(pattern.regex);if(match){text=pattern.value;values={...variables};pattern.names.forEach((name,i)=>values[name]=match[i+1]);break;}}}text??=source;
  }
  return text.replace(/\{(\w+)\}/g,(all,name)=>Object.hasOwn(values,name)?String(values[name]):all);
}
export function onLanguageChange(callback){listeners.add(callback);}
export function setLanguage(value){
  language=value==='en'?'en':'zh-CN';document.documentElement.lang=language;
  for(const binding of bindings){if(!binding.element.isConnected)continue;if(binding.attribute)binding.element.setAttribute(binding.attribute,t(binding.source));else binding.element.nodeValue=binding.raw.replace(binding.source,t(binding.source));}
  for(const callback of listeners)callback();
}
// Bind initial interface text only. File names, project names and terminal output are data.
const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);let node;
while((node=walker.nextNode())){const source=node.nodeValue.trim();if(english[source]&&!node.parentElement.closest('script,style'))bindings.push({element:node,source,raw:node.nodeValue});}
for(const element of document.querySelectorAll('[title],[placeholder],[aria-label]'))for(const attribute of ['title','placeholder','aria-label']){const source=element.getAttribute(attribute);if(source&&english[source])bindings.push({element,attribute,source});}
