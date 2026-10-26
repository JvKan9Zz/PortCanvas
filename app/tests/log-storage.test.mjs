import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

// Exercise the real settings controller with DOM/IPC stubs, without a webview or user data.
const source=readFileSync(new URL('../frontend/storage-settings.js',import.meta.url),'utf8')
  .replace(/^import .*;\r?\n/gm,'').replace('export class StorageSettings','class StorageSettings');
function fixture(setLimit=async maxBytes=>({root:'/fixture',maxBytes,phase:'idle'})){
  const elements=new Map(),messages=[],calls=[],callbacks={};
  const get=id=>{if(!elements.has(id))elements.set(id,{value:'',textContent:'',disabled:false,classList:{toggle(){}},reportValidity:()=>true});return elements.get(id);};
  const document={getElementById:get,activeElement:null};
  const studio={setLogLimit:async value=>{calls.push(value);return setLimit(value);},logState:async()=>({root:'/fixture',maxBytes:null,phase:'idle'}),logList:async()=>({usageBytes:0}),onLogsChanged(){},onLogState:callback=>{callbacks.state=callback;}};
  const {StorageSettings}=runInNewContext(source+'\n({StorageSettings})',{document,window:{studio},withActivity:async(_target,operation)=>operation(),t:value=>value,onLanguageChange(){},locale:()=> 'en-US',setTimeout,clearTimeout});
  const page=new StorageSettings({toast:message=>messages.push(message)});
  page.refresh=async()=>{};
  return {page,get,calls,messages,callbacks,submit:()=>get('logStorageForm').onsubmit({preventDefault(){}})};
}
const tick=()=>new Promise(resolve=>setImmediate(resolve));
test('saving a storage limit sends bytes; clearing it disables automatic retention',async()=>{
  const f=fixture();await tick();
  f.get('logStorageLimit').value='1024';f.get('logStorageLimit').oninput();await f.submit();
  assert.deepEqual(f.calls,[1073741824]);assert.equal(f.get('saveLogStorage').disabled,false);
  f.get('logStorageLimit').value='';f.get('logStorageLimit').oninput();await f.submit();
  assert.deepEqual(f.calls,[1073741824,null]);
});
test('invalid input never invokes the backend',async()=>{
  const f=fixture();await tick();f.get('logStorageLimit').reportValidity=()=>false;await f.submit();
  assert.deepEqual(f.calls,[]);
});
test('failure keeps the draft and releases the save button',async()=>{
  const f=fixture(async()=>{throw new Error('synthetic save error');});await tick();
  f.get('logStorageLimit').value='512';f.get('logStorageLimit').oninput();await f.submit();
  assert.equal(f.get('logStorageLimit').value,'512');assert.equal(f.get('saveLogStorage').disabled,false);
  assert.equal(f.messages.at(-1),'synthetic save error');
});
test('late save responses and status events cannot erase a newer draft or submit twice',async()=>{
  let finish;const f=fixture(value=>new Promise(resolve=>{finish=()=>resolve({root:'/fixture',maxBytes:value,phase:'idle'});}));await tick();
  f.get('logStorageLimit').value='512';f.get('logStorageLimit').oninput();const saving=f.submit();
  await f.submit();assert.equal(f.calls.length,1);
  f.get('logStorageLimit').value='256';f.get('logStorageLimit').oninput();
  f.callbacks.state({root:'/fixture',maxBytes:1073741824,phase:'idle'});finish();await saving;
  assert.equal(f.get('logStorageLimit').value,'256');assert.equal(f.page.limitDirty,true);
});
