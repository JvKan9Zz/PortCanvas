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
 const source=readFileSync(new URL('../frontend/background.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replaceAll('export ','');
 const {applyBackground}=vm.runInNewContext(source+'\n({applyBackground})',{projectCrop,atob,Blob,URL,clearInterval,document:{documentElement:{dataset,style:{setProperty:(key,value)=>values.set(key,value),removeProperty:key=>values.delete(key)}},getElementById:id=>id==='backgroundPreview'?preview:null}});
 applyBackground('#123456','http://127.0.0.1:12345/token/image.png');assert.equal(dataset.backgroundCustom,'true');assert.match(values.get('--workspace-background-image'),/^url\("http:\/\/127\.0\.0\.1:/);
 applyBackground(null,null);assert.equal(dataset.backgroundCustom,'false');assert.equal(values.size,0);assert.equal(dataset.appearance,'dark');assert.equal(dataset.theme,'green');
});
test('file-backed backgrounds keep short CSS URLs and use the same image in the preview',()=>{
 const values=new Map(),preview={style:{}};
 const source=readFileSync(new URL('../frontend/background.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replaceAll('export ','');
 const {applyBackground}=vm.runInNewContext(source+'\n({applyBackground})',{projectCrop,atob,Blob,URL,clearInterval,document:{documentElement:{dataset:{},style:{setProperty:(key,value)=>values.set(key,value),removeProperty:key=>values.delete(key)}},getElementById:id=>id==='backgroundPreview'?preview:null}});
 const image='http://127.0.0.1:12345/token/image.png';
 applyBackground(null,image);
 const css=values.get('--workspace-background-image');assert.ok(css.length<200);assert.equal(preview.style.backgroundImage,css);
 applyBackground(null,'http://127.0.0.1:12345/token/next.jpg');assert.match(values.get('--workspace-background-image'),/next\.jpg/);
 applyBackground(null,null);assert.equal(preview.style.backgroundImage,'none');
});
test('video shares one decoder and releases preview pixels when hidden or minimized',()=>{
 const values=new Map(),dataset={},nodes={backgroundPreview:{style:{},clientWidth:600,clientHeight:80},settingsDialog:{open:true},'settings-panel-general':{hidden:false},enableBackgroundAudio:{hidden:true}};
 class Video{constructor(){this.attributes={};this.style={removeProperty(){}};this.hidden=true;this.paused=true;this.loads=0;this.videoWidth=1920;this.videoHeight=1080;}set src(value){this.attributes.src=value;}getAttribute(key){return this.attributes[key]||null;}removeAttribute(key){delete this.attributes[key];}load(){this.loads++;}pause(){this.paused=true;}play(){this.paused=false;return Promise.resolve();}}
 nodes.workspaceVideo=new Video();nodes.workspaceVideo.readyState=2;
 const draws=[],timers=new Set();nodes.backgroundVideoPreview={hidden:true,width:1,height:1,getContext:()=>({drawImage:(...args)=>draws.push(args)})};
 const source=readFileSync(new URL('../frontend/background.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replaceAll('export ','');
 const api=vm.runInNewContext(source+'\n({applyBackground,setBackgroundVolume,setBackgroundHidden,refreshBackgroundPlayback})',{projectCrop,t:value=>value,setInterval:callback=>{timers.add(callback);return callback;},clearInterval:callback=>timers.delete(callback),document:{hidden:false,documentElement:{dataset,clientWidth:1360,clientHeight:880,style:{setProperty:(key,value)=>values.set(key,value),removeProperty:key=>values.delete(key)}},getElementById:id=>nodes[id]||null}});
 api.applyBackground(null,null);assert.equal(nodes.workspaceVideo.loads,0);assert.equal(timers.size,0);
 const url='http://127.0.0.1:12345/token/clip.mp4';api.applyBackground(null,url,{x:0,y:0,width:1,height:1},true,25);nodes.workspaceVideo.onloadedmetadata();
 assert.equal(dataset.backgroundVideo,'true');assert.equal(values.has('--workspace-background-image'),false);assert.equal(nodes.workspaceVideo.volume,.25);assert.equal(nodes.workspaceVideo.muted,false);assert.equal(nodes.workspaceVideo.paused,false);
 assert.equal(nodes.backgroundVideoPreview.width,600);assert.equal(nodes.backgroundVideoPreview.height,80);assert.equal(timers.size,1);assert.ok(draws.length);assert.equal(draws[0][0],nodes.workspaceVideo);assert.equal(nodes.backgroundVideoPreview.src,undefined);
 api.setBackgroundHidden(true);assert.equal(nodes.workspaceVideo.paused,true);assert.equal(nodes.backgroundVideoPreview.hidden,true);assert.equal(nodes.backgroundVideoPreview.width,1);assert.equal(timers.size,0);
 api.setBackgroundHidden(false);assert.equal(nodes.workspaceVideo.paused,false);assert.equal(nodes.backgroundVideoPreview.hidden,false);assert.equal(timers.size,1);
 nodes['settings-panel-general'].hidden=true;api.refreshBackgroundPlayback();assert.equal(nodes.backgroundVideoPreview.hidden,true);nodes['settings-panel-general'].hidden=false;api.refreshBackgroundPlayback();assert.equal(nodes.backgroundVideoPreview.hidden,false);assert.equal(timers.size,1);
 nodes.settingsDialog.open=false;api.refreshBackgroundPlayback();assert.equal(nodes.backgroundVideoPreview.hidden,true);assert.equal(timers.size,0);
 api.setBackgroundVolume(0);assert.equal(nodes.workspaceVideo.muted,true);
 nodes.settingsDialog.open=true;api.refreshBackgroundPlayback();assert.equal(timers.size,1);
 api.applyBackground(null,null);assert.equal(nodes.workspaceVideo.hidden,true);assert.equal(nodes.workspaceVideo.getAttribute('src'),null);assert.equal(nodes.backgroundVideoPreview.hidden,true);assert.equal(timers.size,0);
});
