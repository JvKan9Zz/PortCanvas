import {test} from 'node:test';
import assert from 'node:assert/strict';
import {initialCrop,moveCrop,resizeCrop,projectCrop} from '../frontend/crop-model.js';
const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-9,`${a} != ${b}`);
const bounded=crop=>{assert.ok(crop.x>=-1e-9&&crop.y>=-1e-9&&crop.width>0&&crop.height>0);assert.ok(crop.x+crop.width<=1+1e-9&&crop.y+crop.height<=1+1e-9);};
test('wide, portrait and square images start with a centered crop matching the window',()=>{
 for(const imageAspect of [4,1,9/16])for(const target of [16/9,1,4/3]){
  const crop=initialCrop(imageAspect,target);bounded(crop);close(crop.x+crop.width/2,.5);close(crop.y+crop.height/2,.5);close(crop.width/crop.height*imageAspect,target);
 }
});
test('moving and resizing every corner keeps the chosen area inside the original image',()=>{
 const crop=initialCrop(2,4/3),ratio=2/3;
 for(const corner of ['nw','ne','sw','se'])for(const delta of [-5,-.05,.05,5]){
  const resized=resizeCrop(crop,corner,delta,delta,ratio);bounded(resized);close(resized.width/resized.height,ratio);
 }
 for(const delta of [-5,5])bounded(moveCrop(crop,delta,delta));
});
test('background projection preserves image proportions and never shows outside the selected crop',()=>{
 const crop={x:.2,y:.1,width:.6,height:.8};
 for(const [width,height] of [[1360,880],[1000,680],[420,920],[640,80]]){
  const view=projectCrop(2400,1600,crop,width,height);close(view.width/view.height,1.5);
  assert.ok(view.x+crop.x*view.width<=1e-8);assert.ok(view.y+crop.y*view.height<=1e-8);
  assert.ok(view.x+(crop.x+crop.width)*view.width>=width-1e-8);assert.ok(view.y+(crop.y+crop.height)*view.height>=height-1e-8);
 }
});
