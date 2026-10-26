import {test} from 'node:test';
import assert from 'node:assert/strict';
import {animateDisclosure} from '../frontend/disclosure-motion.js';
function fixture(){
 const listeners={},summary={addEventListener:(n,h)=>listeners[n]=h,removeEventListener:n=>delete listeners[n]},animations=[];
 const element={open:false,dataset:{},style:{removeProperty(key){delete this[key];}},querySelector:()=>summary,addEventListener(){},removeEventListener(){},getBoundingClientRect(){return {height:this.displayedHeight??(this.open?140:36)};},animate(frames,options){const animation={frames,options,canceled:false,cancel(){this.canceled=true;}};animations.push(animation);return animation;}};
 return {element,animations,click:()=>listeners.click({preventDefault(){}})};
}
test('opening and closing animate layout height and restore natural sizing',()=>{
 const f=fixture();animateDisclosure(f.element,()=>false);f.click();assert.equal(f.element.open,true);assert.deepEqual(f.animations[0].frames,[{height:'36px'},{height:'140px'}]);
 f.animations[0].onfinish();assert.equal(f.element.style.height,undefined);f.click();assert.equal(f.element.open,true);assert.deepEqual(f.animations[1].frames,[{height:'140px'},{height:'36px'}]);f.animations[1].onfinish();assert.equal(f.element.open,false);assert.equal(f.element.style.overflow,undefined);
});
test('rapid reverse starts at the current height and ignores a late old finish',()=>{
 const f=fixture();animateDisclosure(f.element,()=>false);f.click();f.element.displayedHeight=80;f.click();assert.equal(f.animations[0].canceled,true);assert.equal(f.animations[1].frames[0].height,'80px');f.animations[0].onfinish();assert.equal(f.element.open,true);f.animations[1].onfinish();assert.equal(f.element.open,false);
});
test('reduced motion preserves immediate native disclosure behavior',()=>{
 const f=fixture();animateDisclosure(f.element,()=>true);f.click();assert.equal(f.element.open,true);f.click();assert.equal(f.element.open,false);assert.equal(f.animations.length,0);
});
