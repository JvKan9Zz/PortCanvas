import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source=readFileSync(new URL('../frontend/color-picker.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replace('export class','class');
const {ColorPicker}=vm.runInNewContext(source+'\n({ColorPicker})',{innerWidth:1360,innerHeight:880});
test('legacy WebKit picker uses dialog coordinates and stays inside the modal',()=>{
 const parent={clientLeft:1,clientTop:1,getBoundingClientRect:()=>({left:260,top:100,right:1100,bottom:780})};
 const panel={dataset:{inline:'true'},offsetParent:parent,style:{},getBoundingClientRect:()=>({width:286,height:390})};
 ColorPicker.prototype.position.call({panel,trigger:{getBoundingClientRect:()=>({left:570,top:690,bottom:723})}});
 assert.equal(panel.style.left,'309px');assert.equal(panel.style.top,'191px');
});
test('native popovers retain viewport coordinates and avoid the right and lower edges',()=>{
 const panel={dataset:{},style:{},getBoundingClientRect:()=>({width:286,height:390})};
 ColorPicker.prototype.position.call({panel,trigger:{getBoundingClientRect:()=>({left:1250,top:700,bottom:733})}});
 assert.equal(panel.style.left,'1062px');assert.equal(panel.style.top,'302px');
});
