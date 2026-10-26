import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

test('runtime progress keeps download, cache and SSH stages distinct and clears on disconnect',async()=>{
  const elements=new Map(),callbacks={};let languageChange,english=false,cancels=0;
  const translations=JSON.parse(await readFile(new URL('../frontend/en.json',import.meta.url),'utf8'));
  const source=(await readFile(new URL('../frontend/environment-progress.js',import.meta.url),'utf8')).replace(/^import[^\n]+\n/gm,'');
  const get=id=>{if(!elements.has(id))elements.set(id,{hidden:id==='environmentStatus',textContent:'',value:0,classList:{toggle(){}},setAttribute(key,value){this[key]=value;},removeAttribute(key){delete this[key];}});return elements.get(id);};
  vm.runInNewContext(source,{
    document:{getElementById:get},window:{studio:{onEnvironment:h=>callbacks.progress=h,onState:h=>callbacks.state=h,cancelEnvironment:async()=>cancels++}},
    t:(key,values={})=>(english?translations[key]||key:key).replace(/\{(\w+)\}/g,(_,name)=>values[name]),onLanguageChange:h=>languageChange=h,
    showActivity:(target,busy)=>target.busy=busy,
    clearTimeout(){},setTimeout:h=>{callbacks.finish=h;return 1;}
  });
  callbacks.progress({phase:'ready',feature:'desktop'});assert.equal(get('environmentStatus').hidden,true);
  callbacks.progress({phase:'downloading',feature:'desktop',package:'x11vnc',connections:32,done:524288,total:1048576});
  assert.equal(get('environmentStatus').hidden,false);assert.equal(get('environmentProgress').value,50);
  assert.equal(get('environmentTitle').busy,true);
  assert.match(get('environmentDetail').textContent,/32.*50%/);
  english=true;languageChange();assert.match(get('environmentTitle').textContent,/Downloading components on this PC/);
  assert.match(get('environmentDetail').textContent,/32 download connections/);
  callbacks.progress({phase:'downloading',feature:'desktop',connections:0,done:1048576,total:1048576});assert.match(get('environmentDetail').textContent,/Using PC cache/);
  callbacks.progress({phase:'transferring',feature:'desktop',done:100,total:1000});assert.equal(get('environmentProgress').value,10);
  await get('cancelEnvironment').onclick();assert.equal(cancels,1);
  callbacks.progress({phase:'preparing',feature:'desktop'});assert.equal(get('environmentProgress').value,undefined);
  callbacks.progress({phase:'error',feature:'desktop',message:'failed'});assert.equal(get('cancelEnvironment').hidden,true);assert.equal(get('dismissEnvironment').hidden,false);
  callbacks.state({ssh:'disconnected'});assert.equal(get('environmentStatus').hidden,true);
  assert.equal(get('environmentTitle').busy,false);
});
