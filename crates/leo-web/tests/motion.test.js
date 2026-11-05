const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const core = fs.readFileSync(path.resolve(__dirname, '../src/web/app/core.js'), 'utf8');
const sheets = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');

test('removal motion finishes before removing content, and reduced motion skips the wait', async () => {
  for (const reduced of [false, true]) {
    let complete, removed = 0, animated = 0;
    const element = { isConnected: true, remove: () => removed++, animate: () => { animated++; return { finished: new Promise(resolve => { complete = resolve; }) }; } };
    const context = { stillMotion: () => reduced, MOTION_MS: 180, EASE_OUT: 'ease-out' };
    vm.createContext(context);
    vm.runInContext(core.slice(core.indexOf('async function dismissContent'), core.indexOf('function stopSlide')), context);
    const pending = context.dismissContent([element]);
    assert.equal(animated, reduced ? 0 : 1);
    if (!reduced) { assert.equal(removed, 0); complete(); }
    await pending;
    assert.equal(removed, 1);
  }
});

test('note deletion preserves the session on failure and never redirects a newer page', async () => {
  for (const mode of ['success', 'error', 'navigate-api', 'navigate-animation']) {
    const session = { note: { id: 'note', directory: 'Folder' }, doc: { destroy() {} } };
    let went = 0, dismissed = 0, failed = 0;
    const context = { state: { session }, seq: 1, closeSheet() {}, enc: encodeURIComponent, app: { querySelectorAll: () => [] }, loadSideFolders() {}, toast() {}, folderHash: () => '#/f/Folder', restore() {}, fail: () => failed++, go: () => went++ };
    context.api = async () => { if (mode === 'error') throw Error('offline'); if (mode === 'navigate-api') context.seq++; };
    context.dismissContent = async () => { dismissed++; if (mode === 'navigate-animation') context.seq++; };
    vm.createContext(context);
    vm.runInContext(sheets.slice(sheets.indexOf('async function deleteNow()'), sheets.indexOf('async function restore(')), context);
    await context.deleteNow();
    assert.equal(went, mode === 'success' ? 1 : 0);
    assert.equal(failed, mode === 'error' ? 1 : 0);
    assert.equal(dismissed, mode === 'success' || mode === 'navigate-animation' ? 1 : 0);
    assert.equal(context.state.session, mode === 'success' ? null : session);
  }
  const empty = { state: { session: null } };
  vm.createContext(empty);
  vm.runInContext(sheets.slice(sheets.indexOf('async function deleteNow()'), sheets.indexOf('async function restore(')), empty);
  await empty.deleteNow();
});
