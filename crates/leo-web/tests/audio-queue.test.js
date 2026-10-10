'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
require('../src/web/audio-queue.js');
function memory() {
  const items=[];
  return {items,put:async (item) => {items.push(item);},list:async (id,limit=Infinity) => items.filter((i) => i.id===id).sort((a,b) => a.seq-b.seq).slice(0,limit),remove:async (item) => items.splice(items.indexOf(item),1)};
}
test('a lost acknowledgement retries the same bytes and sequence, then advances',async () => {
  const db=memory(); await db.put({id:'session-1',seq:0,bytes:[1,2]}); await db.put({id:'session-1',seq:1,bytes:[3,4]});
  const heard=new Map(); const requests=[]; let lose=true;
  const send=async (item) => { requests.push(item.seq); heard.set(item.seq,item.bytes); if (lose) {lose=false;throw Error('response lost');} return item.seq+1; };
  await assert.rejects(globalThis.leoAudioQueue.drain(db,'session-1',send));
  assert.equal(db.items.length,2);
  await globalThis.leoAudioQueue.drain(db,'session-1',send);
  assert.deepEqual(requests,[0,0,1]); assert.equal(heard.size,2); assert.equal(db.items.length,0);
});
test('a failed or invalid acknowledgement leaves recovery audio and other sessions intact',async () => {
  const db=memory(); await db.put({id:'one',seq:0,bytes:[7]}); await db.put({id:'two',seq:0,bytes:[8]});
  await assert.rejects(globalThis.leoAudioQueue.drain(db,'one',async () => 0)); assert.equal(db.items.length,2);
  await globalThis.leoAudioQueue.drain(db,'one',async () => 1); assert.equal(db.items[0].id,'two');
});

test('invalid sequences and oversized audio fail before opening browser storage',async () => {
  const storage=globalThis.leoAudioQueue.indexed();
  for (const seq of [-1,100000,Infinity,1.5]) await assert.rejects(storage.put({id:'one',seq,bytes:new ArrayBuffer(2)}),/Stop and save/);
  await assert.rejects(storage.put({id:'one',seq:0,bytes:new ArrayBuffer(640001)}),/Stop and save/);
});

test('a browser without IndexedDB still queues, sends in order and forgets what leo acknowledged',async () => {
  const storage=globalThis.leoAudioQueue.pick();
  await storage.put({id:'one',seq:1,bytes:new ArrayBuffer(4)}); await storage.put({id:'one',seq:0,bytes:new ArrayBuffer(2)}); await storage.put({id:'two',seq:0,bytes:new ArrayBuffer(2)});
  assert.equal(await storage.count('one'),2);
  const sent=[];
  await globalThis.leoAudioQueue.drain(storage,'one',async (item) => { sent.push(item.seq); return item.seq+1; });
  assert.deepEqual(sent,[0,1]); assert.equal(await storage.count('one'),0); assert.equal(await storage.count('two'),1);
  await assert.rejects(storage.put({id:'one',seq:-1,bytes:new ArrayBuffer(2)}),/Stop and save/);
});

require('../src/web/recorder.js');
require('../src/web/recording.js');
function recordingFlush(storage) {
  const source=require('node:fs').readFileSync(require.resolve('../src/web/recording.js'),'utf8');
  const start=source.indexOf('    async function flush(');
  const end=source.indexOf('    function detach()',start);
  const state={view:{id:'session-1'},seq:0,sending:false,held:Array.from({length:20},(_,i) => new Int16Array(32000).fill(i))};
  let closed=0;
  const flush=require('node:vm').runInNewContext(`(function(){${source.slice(start,end)} return flush;})()`,{s:state,audioQueue:storage,root:{leoAudioQueue:{...globalThis.leoAudioQueue,drain:async () => 0}},bytesOf:globalThis.leoRecording.bytesOf,join:globalThis.leoRecording.join,closeMic:() => closed++,note:() => {},sendChunk:async () => 0});
  return {state,flush,closed:() => closed};
}
test('offline call backlogs are saved as bounded chunks instead of one oversized upload',async () => {
  const storage=memory(); storage.count=async () => storage.items.length;
  const put=storage.put; storage.put=async (item) => { if (item.bytes.byteLength>640000) throw Error('upload too large'); return put(item); };
  const {state,flush,closed}=recordingFlush(storage);
  await flush();
  assert.equal(storage.items.length,20); assert.equal(state.held.length,0); assert.equal(closed(),0);
  assert.ok(storage.items.every((i) => i.bytes.byteLength===64000));
});
test('a partial storage failure keeps only unwritten chunks and retries without duplicates',async () => {
  const storage=memory(); storage.count=async () => storage.items.length;
  const put=storage.put; let fail=true;
  storage.put=async (item) => { if (fail && storage.items.length===5) throw Error('storage full'); return put(item); };
  const {state,flush}=recordingFlush(storage);
  await flush();
  assert.equal(state.seq,5); assert.equal(state.held.length,15);
  fail=false; await flush();
  assert.equal(state.seq,20); assert.equal(state.held.length,0);
  assert.deepEqual(storage.items.map((i) => i.seq),Array.from({length:20},(_,i) => i));
});
