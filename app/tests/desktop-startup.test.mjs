import {test} from 'node:test';
import assert from 'node:assert/strict';
import {observeDesktopStartup} from '../frontend/desktop-startup.js';
function fixture(){return {_FBU:{encoding:-223,width:1280,height:720},finished:true,_display:{flush:()=>Promise.resolve()},_handleRect(){return true;},_framebufferUpdate(){this._handleRect();return this.finished;}};}
test('wait for a complete pixel update and asynchronous rendering before settling',async t=>{
 t.mock.timers.enable({apis:['setTimeout']});const remote=fixture(),events=[];let finish;
 remote._display.flush=()=>new Promise(resolve=>finish=resolve);
 const original=remote._handleRect;
 observeDesktopStartup(remote,{onFrame:()=>events.push('frame'),onReady:()=>events.push('ready'),onError:assert.fail});
 remote._framebufferUpdate();assert.deepEqual(events,[]);
 remote._FBU.encoding=7;remote.finished=false;remote._framebufferUpdate();assert.deepEqual(events,[]);
 remote.finished=true;remote._framebufferUpdate();assert.deepEqual(events,[]);
 finish();await Promise.resolve();assert.deepEqual(events,['frame']);assert.equal(remote._handleRect,original);
 t.mock.timers.tick(149);assert.deepEqual(events,['frame']);t.mock.timers.tick(1);assert.deepEqual(events,['frame','ready']);
});
test('cancel while decoding restores hooks and suppresses late readiness',async t=>{
 t.mock.timers.enable({apis:['setTimeout']});const remote=fixture();remote._FBU.encoding=0;let finish;const events=[];
 remote._display.flush=()=>new Promise(resolve=>finish=resolve);const original=remote._framebufferUpdate;
 const close=observeDesktopStartup(remote,{onFrame:()=>events.push('frame'),onReady:()=>events.push('ready'),onError:assert.fail});
 remote._framebufferUpdate();close();finish();await Promise.resolve();t.mock.timers.tick(20000);
 assert.deepEqual(events,[]);assert.equal(remote._framebufferUpdate,original);
});
test('cancel during settling cannot expose the previous desktop',async t=>{
 t.mock.timers.enable({apis:['setTimeout']});const remote=fixture();remote._FBU.encoding=-260;const events=[];
 const close=observeDesktopStartup(remote,{onFrame:()=>events.push('frame'),onReady:()=>events.push('ready'),onError:assert.fail});
 remote._framebufferUpdate();await Promise.resolve();close();t.mock.timers.tick(20000);assert.deepEqual(events,['frame']);
});
test('missing first screen times out with hooks removed',t=>{
 t.mock.timers.enable({apis:['setTimeout']});const remote=fixture(),original=remote._handleRect,errors=[];
 observeDesktopStartup(remote,{onFrame:assert.fail,onReady:assert.fail,onError:error=>errors.push(error)});
 remote._framebufferUpdate();t.mock.timers.tick(15000);assert.equal(errors.length,1);assert.match(errors[0],/首屏加载超时/);assert.equal(remote._handleRect,original);
});
