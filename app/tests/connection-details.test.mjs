import {test} from 'node:test';
import assert from 'node:assert/strict';
import {ConnectionDetails} from '../frontend/connection-details.js';
function fixture(){
 const animations=[],attributes={};const element={hidden:true,inert:true,style:{removeProperty(key){delete this[key];}},setAttribute:(key,value)=>attributes[key]=value,getBoundingClientRect(){return {height:this.hidden?0:this.displayedHeight??140};},animate(frames,options){let resolve,reject;const animation={frames,options,finished:new Promise((yes,no)=>{resolve=yes;reject=no;}),cancel(){this.canceled=true;reject(new Error('canceled'));},finish(){resolve();this.onfinish?.();}};animations.push(animation);return animation;}};
 return {element,animations,attributes};
}
test('connection details reveal and collapse keep layout until the exit completes',async()=>{
 const f=fixture(),view=new ConnectionDetails(f.element,()=>false);const opening=view.setVisible(true);assert.equal(f.element.hidden,false);assert.equal(f.element.inert,false);assert.equal(f.animations[0].frames[0].height,'0px');f.animations[0].finish();assert.equal(await opening,true);assert.equal(f.element.style.height,undefined);
 const closing=view.setVisible(false);assert.equal(f.element.hidden,false);assert.equal(f.element.inert,true);assert.equal(f.attributes['aria-hidden'],'true');f.animations[1].finish();assert.equal(await closing,false);assert.equal(f.element.hidden,true);assert.equal(f.element.style.overflow,undefined);
});
test('reversals start at the visible height and stale completion cannot hide a new reveal',async()=>{
 const f=fixture(),view=new ConnectionDetails(f.element,()=>false);const first=view.setVisible(true);f.element.displayedHeight=65;const closing=view.setVisible(false);assert.equal(f.animations[1].frames[0].height,'65px');f.element.displayedHeight=30;const reopened=view.setVisible(true);assert.equal(f.animations[2].frames[0].height,'30px');f.animations[0].onfinish();f.animations[1].onfinish();assert.equal(f.element.hidden,false);f.animations[2].finish();assert.equal(await first,false);assert.equal(await closing,false);assert.equal(await reopened,true);assert.equal(f.element.hidden,false);
});
test('progress refresh reuses the current reveal instead of replaying it',async()=>{
 const f=fixture(),view=new ConnectionDetails(f.element,()=>false);const first=view.setVisible(true);assert.equal(view.setVisible(true),first);assert.equal(f.animations.length,1);f.animations[0].finish();assert.equal(await first,true);
});
test('reduced motion and unavailable animation preserve immediate visibility and interaction',async()=>{
 for(const reduce of [true,false]){const f=fixture();if(!reduce)delete f.element.animate;const view=new ConnectionDetails(f.element,()=>reduce);assert.equal(await view.setVisible(true),true);assert.equal(f.element.hidden,false);assert.equal(await view.setVisible(false),false);assert.equal(f.element.hidden,true);assert.equal(f.element.inert,true);assert.equal(f.animations.length,0);}
});
