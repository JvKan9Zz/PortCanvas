import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

function fixture(){
 const element=()=>({children:[],className:'',dataset:{},style:{},attributes:{},get firstElementChild(){return this.children[0];},classList:{contains(name){return this.owner.className.split(' ').includes(name);},toggle(name,on){const names=new Set(this.owner.className.split(' ').filter(Boolean));on?names.add(name):names.delete(name);this.owner.className=[...names].join(' ');}},setAttribute(key,value){this.attributes[key]=value;},append(child){this.children.push(child);child.parentNode=this;},prepend(child){child.remove();this.children.unshift(child);child.parentNode=this;},remove(){if(this.parentNode){this.parentNode.children=this.parentNode.children.filter(child=>child!==this);this.parentNode=null;}},replaceChildren(...children){for(const child of this.children)child.parentNode=null;this.children=[];for(const child of children)this.append(child);}});
 const create=()=>{const node=element();node.classList.owner=node;return node;};
 const source=readFileSync(new URL('../frontend/connection-indicator.js',import.meta.url),'utf8').replaceAll('export ','');
 const api=vm.runInNewContext(source+'\n({setConnectionIndicator,showActivity,withActivity})',{document:{createElement:create}});
 return {...api,update:api.setConnectionIndicator,container:create(),icon:create()};
}
test('all four random variants create sixteen decorative dots with distinct timing',()=>{
 const timings=[];
 for(let pattern=0;pattern<4;pattern++){
  const {update,container,icon}=fixture();update(container,true,icon,()=>pattern/4);
  const matrix=container.firstElementChild;assert.equal(matrix.dataset.pattern,String(pattern));assert.equal(matrix.children.length,16);assert.equal(matrix.attributes['aria-hidden'],'true');
  timings.push(matrix.children.map(dot=>dot.style.animationDelay).join(','));
 }
 assert.equal(new Set(timings).size,4);
});
test('overlapping operations keep one indicator until both settle and preserve results/errors',async()=>{
 const {container,icon,withActivity,showActivity}=fixture();container.append(icon);
 let finishFirst,failSecond;const error=new Error('synthetic failure');
 const first=withActivity(container,()=>new Promise(resolve=>finishFirst=resolve));
 const second=withActivity(container,()=>new Promise((_resolve,reject)=>failSecond=reject));
 const indicator=container.firstElementChild;assert.equal(container.children.length,2);
 finishFirst(42);assert.equal(await first,42);showActivity(container,false);
 assert.equal(container.firstElementChild,indicator);assert.equal(container.attributes['aria-busy'],'true');
 failSecond(error);await assert.rejects(second,error);
 assert.equal(container.attributes['aria-busy'],'false');assert.equal(container.children.length,1);assert.equal(container.firstElementChild,icon);
});
test('a text rerender reuses the active pattern and an immediate failure clears it',async()=>{
 const {container,icon,withActivity,showActivity}=fixture();let finish;
 const task=withActivity(container,()=>new Promise(resolve=>finish=resolve));const indicator=container.firstElementChild;
 container.replaceChildren(icon);showActivity(container,true);assert.equal(container.firstElementChild,indicator);
 finish();await task;assert.equal(container.firstElementChild,icon);
 await assert.rejects(withActivity(container,()=>{throw new Error('synchronous failure');}),/synchronous failure/);
 assert.equal(container.attributes['aria-busy'],'false');assert.equal(container.children.length,1);
});
test('stage refresh keeps the same running animation and does not reroll',()=>{
 const {update,container,icon}=fixture();let rolls=0;const random=()=>{rolls++;return .6;};
 update(container,true,icon,random);const matrix=container.firstElementChild;
 update(container,true,icon,random);assert.equal(container.firstElementChild,matrix);assert.equal(rolls,1);
 update(container,false,icon,random);assert.equal(container.firstElementChild,icon);
 update(container,true,icon,random);assert.notEqual(container.firstElementChild,matrix);assert.equal(rolls,2);
});
