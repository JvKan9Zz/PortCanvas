// Parse only the current shell word. This never evaluates or executes input.
export function commandContext(value,cursor){
  let quote='',escaped=false,start=0,word='',command=true,head='';
  const commit=()=>{if(word){if(command&&!/^[A-Za-z_]\w*=/.test(word)){head=word;command=false;}}word='';};
  for(let i=0;i<cursor;i++){
    const c=value[i];
    if(escaped){word+=c;escaped=false;continue;}
    if(c==='\\'&&quote!=="'"){escaped=true;continue;}
    if(quote){if(c===quote)quote='';else word+=c;continue;}
    if(c==='"'||c==="'"){quote=c;continue;}
    if(/\s/.test(c)){commit();if(c==='\n'||c==='\r'){command=true;head='';}start=i+1;continue;}
    if(';|&()'.includes(c)){commit();command=true;head='';start=i+1;continue;}
    if('<>'.includes(c)){commit();command=false;start=i+1;continue;}
    word+=c;
  }
  let end=cursor,q=quote,escape=escaped;
  for(;end<value.length;end++){
    const c=value[end];if(escape){escape=false;continue;}if(c==='\\'&&q!=="'"){escape=true;continue;}if(q){if(c===q)q='';continue;}if(c==='"'||c==="'"){q=c;continue;}if(/\s/.test(c)||';|&()<>'.includes(c))break;
  }
  return {value,start,end,prefix:word,commands:command,directories:!command&&['cd','pushd'].includes(head)};
}
export function quoteWord(word){
  if(/^[\w@%+=:,./~-]+$/u.test(word))return word;
  const home=word.match(/^~[\w-]*\//u)?.[0]||'';
  return home+"'"+word.slice(home.length).replaceAll("'","'\\''")+"'";
}
export function completionEdit(context,item,finish=true){
  const insert=quoteWord(item.value),tail=context.value.slice(context.end),space=finish&&item.kind!=='directory'&&!/^\s/.test(tail)?' ':'';
  return {value:context.value.slice(0,context.start)+insert+space+tail,cursor:context.start+insert.length+space.length};
}
export function commonPrefix(items){if(!items.length)return '';let value=items[0].value;for(const item of items){while(!item.value.startsWith(value)&&value)value=value.slice(0,-1);}return value;}
