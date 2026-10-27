import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source=readFileSync(new URL('../frontend/discovery.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replaceAll('export ','');
test('discovery fills only IP and port; new device name and credentials stay blank',async()=>{
 const nodes=Object.fromEntries(['quickName','quickUsername','quickPassword','quickTarget','quickCredentialLabel','quickRemember'].map(id=>[id,{focus(){this.focused=true;}}]));let connected=0;
 const {DeviceDiscovery}=vm.runInNewContext(source+'\n({DeviceDiscovery})',{document:{getElementById:id=>nodes[id]},t:value=>value});
 const context={connection:()=> 'disconnected',profiles:()=>[],connect:()=>connected++,dialog:{showModal(){this.open=true;}}};
 await DeviceDiscovery.prototype.choose.call(context,{host:'192.168.1.3',port:22});assert.equal(nodes.quickName.value,'');assert.equal(nodes.quickUsername.value,'');assert.equal(nodes.quickPassword.value,'');assert.equal(nodes.quickTarget.textContent,'192.168.1.3:22');assert.equal(connected,0);assert.equal(nodes.quickName.focused,true);
});
test('saved credentials can connect directly and an active connection blocks another login',async()=>{
 const {DeviceDiscovery}=vm.runInNewContext(source+'\n({DeviceDiscovery})',{document:{getElementById:()=>null},t:value=>value});
 const saved={host:'192.168.1.3',port:22,hasSavedCredential:true};const calls=[];const context={connection:()=> 'disconnected',profiles:()=>[saved],connect:async(...args)=>calls.push(args),render(){}};
 await DeviceDiscovery.prototype.choose.call(context,{host:saved.host,port:22});assert.equal(calls.length,1);assert.equal(calls[0][0],saved);assert.equal(calls[0][1],'');assert.equal(context.busy,false);
 context.connection=()=> 'connected';await DeviceDiscovery.prototype.choose.call(context,{host:saved.host,port:22});assert.equal(calls.length,1);
});
