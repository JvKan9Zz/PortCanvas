import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {projectCrop} from '../frontend/crop-model.js';

test('system appearance follows media changes only in system mode and keeps the accent choice',()=>{
 const dataset={},callbacks={};const media={matches:true,addEventListener:(name,handler)=>callbacks[name]=handler};
 const source=readFileSync(new URL('../frontend/theme.js',import.meta.url),'utf8').replaceAll('export ','');
 const api=vm.runInNewContext(source+'\n({setAppearance,getAppearance,setTheme,getTheme})',{window:{matchMedia:()=>media},document:{documentElement:{dataset}},getComputedStyle:()=>({getPropertyValue:()=>''})});
 api.setTheme('green');api.setAppearance('system');assert.equal(dataset.appearance,'dark');assert.equal(api.getAppearance(),'system');
 media.matches=false;callbacks.change();assert.equal(dataset.appearance,'light');assert.equal(api.getTheme(),'green');
 api.setAppearance('dark');media.matches=false;callbacks.change();assert.equal(dataset.appearance,'dark');
 api.setAppearance('light');media.matches=true;callbacks.change();assert.equal(dataset.appearance,'light');
});
test('custom background removal clears the image and color while preserving other root state',()=>{
 const values=new Map(),dataset={appearance:'dark',theme:'green'},preview={style:{}};
 const source=readFileSync(new URL('../frontend/background.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replace('export ','');
 const {applyBackground}=vm.runInNewContext(source+'\n({applyBackground})',{projectCrop,document:{documentElement:{dataset,style:{setProperty:(key,value)=>values.set(key,value),removeProperty:key=>values.delete(key)}},getElementById:()=>preview}});
 applyBackground('#123456','data:image/png;base64,Zml4dHVyZQ==');assert.equal(dataset.backgroundCustom,'true');assert.match(values.get('--workspace-background-image'),/^url\("data:image\/png/);
 applyBackground(null,null);assert.equal(dataset.backgroundCustom,'false');assert.equal(values.size,0);assert.equal(dataset.appearance,'dark');assert.equal(dataset.theme,'green');
});
