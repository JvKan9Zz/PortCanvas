import {test} from 'node:test';
import assert from 'node:assert/strict';
import {checkStartupUpdate,hasAvailableUpdate} from '../frontend/startup-update.js';
test('startup checks on supported idle installs and does not download or install',async()=>{
 let calls=0;const result=await checkStartupUpdate({updatesSupported:true},{status:'idle'},async()=>{calls++;return {status:'available',version:'0.6.0'};});
 assert.equal(calls,1);assert.equal(result.version,'0.6.0');assert.equal(hasAvailableUpdate(result),true);
});
test('startup does not compete with active updates or run on unsupported installs',async()=>{
 const check=()=>{throw Error('must not run');};
 assert.equal(await checkStartupUpdate({updatesSupported:false},{status:'idle'},check),null);
 for(const status of ['checking','downloading','verifying','installing','ready'])assert.equal(await checkStartupUpdate({updatesSupported:true},{status},check),null);
});
test('offline startup failures are silent; notices follow known versions through update progress',async()=>{
 assert.equal(await checkStartupUpdate({updatesSupported:true},{status:'idle'},async()=>{throw Error('offline');}),null);
 for(const status of ['available','downloading','verifying','ready','canceled','error'])assert.equal(hasAvailableUpdate({status,version:'0.6.0'}),true);
 for(const status of ['idle','checking','current','manual','updated','installing'])assert.equal(hasAvailableUpdate({status,version:'0.6.0'}),false);
 assert.equal(hasAvailableUpdate({status:'error'}),false);
});
