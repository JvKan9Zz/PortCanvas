import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
test('image-only crop cleanup never initializes an unused video player',()=>{
 const source=readFileSync(new URL('../frontend/background-crop.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replaceAll('export ','');
 let loads=0,pauses=0,cleared=0;const attributes={};const video={getAttribute:key=>attributes[key],removeAttribute:key=>delete attributes[key],pause:()=>pauses++,load:()=>loads++};
 const {BackgroundCropDialog}=vm.runInNewContext(source+'\n({BackgroundCropDialog})',{document:{getElementById:()=>({removeAttribute:()=>cleared++})}});
 const crop={video,dialog:{open:false}};BackgroundCropDialog.prototype.setHidden.call(crop,true);BackgroundCropDialog.prototype.release.call(crop);assert.equal(loads,0);assert.equal(pauses,0);assert.equal(cleared,1);
 attributes.src='http://127.0.0.1/video.mp4';BackgroundCropDialog.prototype.release.call(crop);assert.equal(loads,1);assert.equal(pauses,1);assert.equal(attributes.src,undefined);
});
