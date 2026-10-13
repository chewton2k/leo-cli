const { test } = require('node:test');
const assert = require('node:assert/strict');
require('../src/web/browser-storage.js');
const L = globalThis.leoBrowserStorage;
function storage(values) {
  const data = new Map(Object.entries(values));
  return { get length() { return data.size; }, key: i => [...data.keys()][i], getItem: k => data.get(k) ?? null, setItem: (k,v) => data.set(k,v), removeItem: k => data.delete(k) };
}
test('browser inventory includes separate note and interview backups without exposing their contents', () => {
  const store = storage({ 'leo-draft-v1:note-one': '{"body":"private note text"}', 'leo-interview-drafts': JSON.stringify({'practice-one:problem-one':{answer:'private code'},'practice-two:problem-two':{answer:'other code'}}), 'leo-chat-v1':'private chat', 'leo-library':'{}', 'another-app':'private elsewhere' });
  const items = L.list(store);
  assert.equal(items.length,5);
  assert.equal(items.filter(i=>i.practice).length,2);
  assert.ok(items.every(i=>i.bytes>0 && i.size));
  assert.ok(!JSON.stringify(items).includes('private'));
  L.remove(store, ['interview@practice-one:problem-one', 'leo-draft-v1:note-one']);
  assert.equal(store.getItem('leo-draft-v1:note-one'),null);
  assert.deepEqual(Object.keys(JSON.parse(store.getItem('leo-interview-drafts'))),['practice-two:problem-two']);
  assert.equal(store.getItem('another-app'),'private elsewhere');
  assert.throws(()=>L.remove(store,['another-app']),/changed/);
});
test('deleting a practice forgets only its browser drafts, in either deletion order', () => {
  for (const ids of [['practice-one','practice-two'],['practice-two','practice-one']]) {
    const store=storage({'leo-interview-drafts':JSON.stringify({'practice-one:problem-one':{answer:'one'},'practice-two:problem-two':{answer:'two'},'keep-practice:problem-three':{answer:'keep'}})});
    for (const id of ids) L.forgetPractices(store,[id]);
    assert.deepEqual(Object.keys(JSON.parse(store.getItem('leo-interview-drafts'))),['keep-practice:problem-three']);
    L.forgetPractices(store,['keep-practice']);
    assert.equal(store.getItem('leo-interview-drafts'),null);
  }
});
test('corrupt or oversized draft backups can be discarded without harming unrelated browser data', () => {
  for (const value of ['broken JSON', 'x'.repeat(8*1024*1024+1)]) {
    const store=storage({'leo-interview-drafts':value,'another-app':'keep'});
    assert.equal(L.list(store)[0].label,'Unreadable interview draft backup');
    L.remove(store,['leo-interview-drafts']);
    assert.equal(store.getItem('another-app'),'keep');
  }
  assert.equal(L.size(0),'0 bytes');
  assert.equal(L.size(1024),'1.0 KB');
  assert.equal(L.size(Infinity),'Unknown');
});
