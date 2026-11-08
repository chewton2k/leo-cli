const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('compact note selection honors the latest click in either response order', async () => {
  for (const reverse of [false, true]) {
    const pending = new Map(), opened = [];
    const box = { isConnected: true };
    const context = {
      seq: 1, state: { session: null }, openingNote: null,
      api: url => new Promise(resolve => pending.set(url.split('/').pop(), resolve)),
      enc: encodeURIComponent, sideWide: () => false, noteHash: id => `#/n/${id}`,
      go: async hash => { opened.push(hash.split('/').pop()); return true; },
      showNote: async id => {
        opened.push(id);
        context.seq++;
        context.state.session = { note: { id }, doc: { editStart() {} } };
      },
      history: { pushState() {} }, closeSheet: () => { box.isConnected = false; },
      rememberPlace() {}, loadSideFolders: async () => {}, fail: e => { throw e; },
    };
    const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
    vm.createContext(context);
    vm.runInContext('let explorerOpenSeq = 0;\n' + source.slice(source.indexOf('async function openExplorerNote('), source.indexOf('function explorerTarget(')), context);
    const row = id => ({ dataset: { id }, closest: () => box });
    const first = context.openExplorerNote(row('first'));
    const latest = context.openExplorerNote(row('latest'));
    for (const id of reverse ? ['latest', 'first'] : ['first', 'latest']) {
      pending.get(id)({ id });
      await (id === 'first' ? first : latest);
    }
    assert.deepEqual(opened, ['latest']);
  }
});

test('reopening the open note from the Folders sheet waits for its save instead of drawing an earlier copy', async () => {
  const fetched = [], went = [];
  let drawn = 0;
  const context = {
    seq: 1, state: { session: { note: { id: 'open' } } },
    api: async url => { fetched.push(url); return { id: 'open' }; },
    enc: encodeURIComponent, sideWide: () => false, noteHash: id => `#/n/${id}`,
    go: async (hash, options) => { went.push([hash, options]); }, fail: e => { throw e; },
    side: { target: 'Folder' }, drawSide() { drawn++; },
  };
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
  vm.createContext(context);
  vm.runInContext('let explorerOpenSeq = 0;\n' + source.slice(source.indexOf('async function openExplorerNote('), source.indexOf('function explorerTarget(')), context);
  await context.openExplorerNote({ dataset: { id: 'open' }, closest: () => ({ isConnected: true }) });
  assert.deepEqual(fetched, []);
  assert.deepEqual(went, [['#/n/open', undefined]]);
  assert.equal(context.side.target, null);
  assert.equal(drawn, 1);
});
