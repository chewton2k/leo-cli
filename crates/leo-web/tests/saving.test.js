'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/saving.js');

function storage() {
  const values = new Map();
  return {
    get length() { return values.size; },
    key: (index) => [...values.keys()][index],
    getItem: (key) => values.get(key) || null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
}
const note = () => ({ id: 'one', title: 'Note', body: 'Original', tags: [], directory: '', version: 'v1' });
function manager(api, local = storage(), extra = {}) {
  return leoSaving.create({ api, storage: local, schedule: () => 1, cancel: () => {}, ...extra });
}
const edit = (body) => ({ title: 'Note', body, tags: [] });

test('a failed save remains tracked and recovers after a reload', async () => {
  const local = storage();
  const failure = Object.assign(new Error('disk full'), { status: 500 });
  const m = manager(async () => { throw failure; }, local);
  const s = m.open(note());
  m.changed(s, edit('Do not lose this'));
  await assert.rejects(m.flush(s), /disk full/);
  assert.equal(m.unsaved(), true);
  const recovered = manager(async (_, { body }) => ({ ...note(), ...body, version: 'v2' }), local);
  const r = recovered.open(note());
  assert.equal(r.edit.body, 'Do not lose this');
  assert.equal(r.note.version, 'v1');
  await recovered.flush(r);
  assert.equal(recovered.unsaved(), false);
  assert.equal(local.length, 0);
});

test('edits made while a save is in flight are sent after it and retained on failure', async () => {
  let release;
  let calls = 0;
  const local = storage();
  const m = manager(async (_, { body }) => {
    calls++;
    if (calls === 1) await new Promise((resolve) => { release = resolve; });
    else throw new Error('second save failed');
    return { ...note(), ...body, version: 'v2' };
  }, local);
  const s = m.open(note());
  m.changed(s, edit('First edit'));
  const pending = m.flush(s);
  m.changed(s, edit('Latest edit'));
  release();
  await assert.rejects(pending, /second save failed/);
  const again = manager(async () => {}, local).open(note());
  assert.equal(calls, 2);
  assert.equal(again.edit.body, 'Latest edit');
  assert.equal(again.note.version, 'v2');
});

test('a recovered draft uses its original base so external edits create a conflict copy', async () => {
  const local = storage();
  const old = manager(async () => {}, local);
  old.changed(old.open(note()), edit('Phone draft'));
  const requests = [];
  const m = manager(async (path, options) => {
    requests.push({ path, ...options });
    if (options.method === 'PATCH') throw Object.assign(new Error('conflict'), { status: 409 });
    return { ...note(), ...options.body, id: 'copy', version: 'copy-v1' };
  }, local);
  const s = m.open({ ...note(), body: 'Computer edit', version: 'v9' });
  await m.flush(s);
  assert.equal(requests[0].body.base, 'v1');
  assert.equal(requests[1].body.body, 'Phone draft');
  assert.match(requests[1].body.title, /conflict from phone/);
  assert.equal(s.note.id, 'copy');
  assert.equal(local.length, 0);
});

test('offline new-note drafts recover with their folder and tags', async () => {
  const local = storage();
  const m = manager(async () => { throw new Error('offline'); }, local);
  const s = m.open({ ...note(), id: null, directory: 'cs130', version: null });
  m.changed(s, { title: 'Lecture', body: 'BFS', tags: ['exam'] });
  await assert.rejects(m.flush(s));
  const again = manager(async () => {}, local);
  const [draft] = again.drafts();
  assert.equal(draft.note.id, null);
  assert.equal(draft.note.directory, 'cs130');
  assert.deepEqual(draft.edit.tags, ['exam']);
});

test('storage failure is visible and the draft stays in memory until saved', async () => {
  let warning = 0;
  const local = storage();
  local.setItem = () => { throw new Error('quota'); };
  const m = manager(async (_, { body }) => ({ ...note(), ...body }), local, { storageError: () => warning++ });
  const s = m.open(note());
  m.changed(s, edit('First'));
  m.changed(s, edit('Latest'));
  assert.equal(warning, 1);
  assert.equal(s.persisted, false);
  assert.equal(m.unsaved(), true);
  await m.flush(s);
  assert.equal(m.unsaved(), false);
});

test('blank new notes are not created and unrelated local storage is untouched', async () => {
  const local = storage();
  local.setItem('other-app', 'keep');
  const m = manager(async () => { assert.fail('blank note was sent'); }, local);
  const s = m.open({ ...note(), id: null, title: '', body: '' });
  m.changed(s, { title: '', body: '', tags: [] });
  await m.flush(s);
  assert.equal(m.unsaved(), false);
  assert.equal(local.getItem('other-app'), 'keep');
});


test('new-note drafts work on HTTP origins without randomUUID', async () => {
  const vm = require('node:vm');
  const fs = require('node:fs');
  const sandbox = { crypto: { getRandomValues: (bytes) => require('node:crypto').webcrypto.getRandomValues(bytes) } };
  vm.runInNewContext(fs.readFileSync(require.resolve('../src/web/saving.js'), 'utf8'), sandbox);
  const local = storage();
  const m = sandbox.leoSaving.create({ api: async () => { throw new Error('offline'); }, storage: local, schedule: () => 1, cancel: () => {} });
  const one = m.open({ ...note(), id: null });
  const two = m.open({ ...note(), id: null });
  assert.notEqual(one.key, two.key);
  m.changed(one, edit('First draft'));
  m.changed(two, edit('Second draft'));
  await assert.rejects(m.flush(one));
  await assert.rejects(m.flush(two));
  assert.equal(local.length, 2);
});
