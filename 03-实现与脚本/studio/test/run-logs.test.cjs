const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const {once}=require('node:events');
const {decodeOutput,parsePacket,OutputParser}=require('../src/log-protocol.cjs');
const {RunLogStore,projectOf}=require('../src/run-log-store.cjs');
const b64=s=>Buffer.from(s).toString('base64');
const event=(sequence='1')=>({kind:'begin',shell:'123-456',sequence,command:'python3 程序.py',cwd:'/home/demo/项目/debug',projectRoot:'/home/demo/项目'});
const fixture=()=>{const root=path.resolve(__dirname,'../../../05-测试与验证/local/log-tests-'+crypto.randomUUID());return new RunLogStore(root);};
test('run markers validate fields and preserve Chinese command paths',()=>{
 const packet=`begin;123-456;1;;${b64('python3 程序.py')};${b64('/home/demo/项目')};${b64('/home/demo/项目')};;`;
 assert.equal(parsePacket(packet).command,'python3 程序.py');assert.equal(parsePacket(`ready;123-456;0;;;${b64('/home/demo')};${b64('/home/demo')};;`).kind,'ready');
 assert.equal(parsePacket('end;bad;1;0;;;;;'),null);assert.equal(parsePacket('begin;123-456;1;;%%%;;;;'),null);
});
test('control-mode decoding preserves escaped controls, slash and UTF-8',()=>{
 assert.equal(decodeOutput('中文\\015\\012\\134x').toString(),'中文\r\n\\x');
});
test('stream parser separates split markers, ANSI styling and progress output',()=>{
 const parser=new OutputParser(),events=[],text=[];parser.on('marker',e=>events.push(e));parser.on('text',s=>text.push(s));
 const packet=`begin;123-456;1;;${b64('echo demo')};${b64('/tmp')};;;`;
 const source=Buffer.from('\x1b]777;WCSLOG;'+packet+'\x07\x1b[31m中文\x1b[0m\r\r\n10%\r20%\n');
 for(const byte of source)parser.push(Buffer.from([byte]));parser.flush();
 assert.equal(events.length,1);assert.equal(text.join(''),'中文\n10%\n20%\n');
});
test('completed runs are local UTF-8 files with exit code and stable project grouping',async()=>{
 const store=fixture(),writer=store.begin(event(),{device:'fixture'});writer.append('中文 traceback\n');await store.finish(writer,{code:1});
 const [record]=store.list();assert.equal(record.status,'failed');assert.equal(record.exitCode,1);assert.equal(record.complete,true);assert.equal(record.active,false);
 assert.equal(record.project.name,'项目');assert.equal(store.detail(record.id).text,'中文 traceback\n');assert.ok(fs.existsSync(path.join(record.directory,'run.json')));
 assert.equal(projectOf({...event(),cwd:'/home/demo/项目/match'}).key,record.project.key);
});
test('manual project assignment is snapshotted at command start',async()=>{
 const store=fixture(),writer=store.begin(event(),{device:'fixture',override:'中文项目 A'});await store.finish(writer,{code:0});
 assert.equal(store.list()[0].project.name,'中文项目 A');assert.ok(store.list()[0].directory.startsWith(store.root+path.sep));
 assert.ok(!projectOf(event(),'../../CON:项目').folder.includes('/'));
});
test('interruption and recovery append to one run and retain missing-output status',async()=>{
 const store=fixture(),first=store.begin(event(),{device:'fixture'});first.append('before\n');await store.finish(first,null,{interrupted:true});
 const second=store.begin(event(),{device:'fixture',recovered:true});assert.equal(second.meta.id,first.meta.id);second.append('after\n');await store.finish(second,{code:0});
 const [record]=store.list();assert.equal(record.complete,false);assert.equal(record.exitCode,0);assert.equal(record.durationMs,null);assert.match(store.detail(record.id).text,/before[\s\S]*after/);
 assert.equal(store.list().length,1);await store.recoverEnded({...event(),kind:'end',code:0},{device:'fixture'});assert.equal(store.list().length,1);
});
test('cleanup removes only selected finished runs and preserves active or unrelated data',async()=>{
 const store=fixture(),done=store.begin(event(),{device:'fixture'});done.append('done\n');await store.finish(done,{code:0});
 const active=store.begin(event('2'),{device:'fixture'}),outside=path.join(store.root,'user-document.txt');fs.writeFileSync(outside,'keep');
 assert.equal(store.remove([done.meta.id,active.meta.id]).removed,1);assert.ok(fs.existsSync(path.join(active.directory,'run.json')));assert.equal(fs.readFileSync(outside,'utf8'),'keep');
 assert.throws(()=>store.remove(['../outside']),/有效/);await store.close();
});
test('preview bounds large Chinese logs without changing complete on-disk data',async()=>{
 const store=fixture(),writer=store.begin(event(),{device:'fixture'});const text='中文'.repeat(80000);writer.append(text);await store.finish(writer,{code:0});
 const record=store.detail(writer.meta.id);assert.equal(record.truncated,true);assert.ok(!record.text.includes('\ufffd'));assert.equal(fs.readFileSync(record.file,'utf8'),text);
});
test('write errors remain visible and do not escape as unhandled stream errors',async()=>{
 const store=fixture(),writer=store.begin(event(),{device:'fixture'}),fault=once(store,'fault');writer.stream.destroy(new Error('fixture disk failure'));await fault;
 assert.equal(store.list()[0].status,'write_error');assert.equal(store.list()[0].complete,false);await store.close();
});
test('abandoned running records become incomplete after their owner exits',async()=>{
 const store=fixture(),writer=store.begin(event(),{device:'fixture'});await store.finish(writer,{code:0});
 const file=path.join(writer.directory,'run.json'),meta=JSON.parse(fs.readFileSync(file));Object.assign(meta,{status:'running',ownerPid:2147483647,endedAt:null,exitCode:null});fs.writeFileSync(file,JSON.stringify(meta));
 const [record]=new RunLogStore(store.root).list();assert.equal(record.status,'interrupted');assert.equal(record.complete,false);assert.equal(record.ownerPid,null);
 assert.equal(store.remove([record.id]).removed,1);
});
test('invalid settings stay visible while retaining default log location',async()=>{
 const {RunLogs}=require('../src/run-logs.cjs'),{EventEmitter}=require('node:events'),store=fixture();fs.mkdirSync(store.root,{recursive:true});
 const settingsFile=path.join(store.root,'settings.json');fs.writeFileSync(settingsFile,'{"root":3,"projects":null}');
 const logs=new RunLogs(new EventEmitter(),{root:store.root,settingsFile});assert.equal(logs.snapshot().phase,'error');assert.equal(logs.snapshot().root,store.root);await logs.close();
});
