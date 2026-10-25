import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DesktopLifecycle} from '../frontend/desktop-lifecycle.js';
const tick=()=>new Promise(resolve=>setTimeout(resolve,0));

test('visibility alone never starts an unrequested desktop; a requested one resumes',async()=>{
 const calls=[];
 const life=new DesktopLifecycle({open:async()=>calls.push('open'),close:()=>calls.push('close'),stop:async()=>calls.push('stop'),onError:assert.fail});
 life.setAllowed(true);await life.pending;assert.deepEqual(calls,[]);
 await life.start();life.setAllowed(false);await life.pending;
 assert.deepEqual(calls,['open','close','stop']);
 life.setAllowed(true);await life.pending;assert.equal(calls.at(-1),'open');
});
test('explicit disconnect prevents automatic resume',async()=>{
 let opened=0;
 const life=new DesktopLifecycle({open:async()=>opened++,close:()=>{},stop:async()=>{},onError:assert.fail});
 life.setAllowed(true);await life.start();await life.stop();
 life.setAllowed(false);life.setAllowed(true);await life.pending;assert.equal(opened,1);
});
test('hide immediately cancels an in-flight start before rapid resume',async()=>{
 const calls=[];let complete;
 const life=new DesktopLifecycle({open:async()=>{calls.push('open');if(calls.length===1)await new Promise(resolve=>complete=resolve);},close:()=>calls.push('close'),stop:async()=>{calls.push('cancel');complete?.();},onError:assert.fail});
 life.setAllowed(true);life.start();await tick();
 life.setAllowed(false);life.setAllowed(true);await life.pending;
 assert.deepEqual(calls,['open','close','cancel','open']);
});
test('a queued start is skipped if the page is hidden before it executes',async()=>{
 let opened=0;
 const life=new DesktopLifecycle({open:async()=>opened++,close:()=>{},stop:async()=>{},onError:assert.fail});
 life.setAllowed(true);life.start();life.setAllowed(false);await life.pending;
 assert.equal(opened,0);
});
