const { test } = require('node:test');
const assert = require('node:assert/strict');
require('../src/web/browser-storage.js');
const L = globalThis.leoBrowserStorage;
function storage(values) {
  const data = new Map(Object.entries(values));
  return { get length() { return data.size; }, key: i => [...data.keys()][i], getItem: k => data.get(k) ?? null, setItem: (k,v) => data.set(k,v), removeItem: k => data.delete(k) };
}
test('browser inventory keeps note backups private and leaves retired practice keys untouched', () => {
  const retired = JSON.stringify({'practice-one:problem-one':{answer:'private code'}});
  const store = storage({ 'leo-draft-v1:note-one': '{"body":"private note text"}', 'leo-interview-drafts': retired, 'leo-interview-split':'50', 'leo-chat-v1':'private chat', 'leo-library':'{}', 'another-app':'private elsewhere' });
  const items = L.list(store);
  assert.equal(items.length,3);
  assert.ok(items.every(i=>i.bytes>0 && i.size));
  assert.ok(!JSON.stringify(items).includes('private'));
  L.remove(store, ['leo-draft-v1:note-one']);
  assert.equal(store.getItem('leo-draft-v1:note-one'),null);
  assert.equal(store.getItem('leo-interview-drafts'),retired);
  assert.equal(store.getItem('leo-interview-split'),'50');
  assert.equal(store.getItem('another-app'),'private elsewhere');
  assert.throws(()=>L.remove(store,['another-app']),/changed/);
  assert.throws(()=>L.remove(store,['leo-interview-drafts']),/changed/);
});
test('corrupt retired practice backups are ignored and cannot be discarded through Storage', () => {
  for (const value of ['broken JSON', 'x'.repeat(8*1024*1024+1)]) {
    const store=storage({'leo-interview-drafts':value,'another-app':'keep'});
    assert.deepEqual(L.list(store),[]);
    assert.throws(()=>L.remove(store,['leo-interview-drafts']),/changed/);
    assert.equal(store.getItem('leo-interview-drafts'),value);
    assert.equal(store.getItem('another-app'),'keep');
  }
  assert.equal(L.size(0),'0 bytes');
  assert.equal(L.size(1024),'1.0 KB');
  assert.equal(L.size(Infinity),'Unknown');
});
