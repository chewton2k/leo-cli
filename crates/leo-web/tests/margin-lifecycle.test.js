const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const kit = require('../src/web/margin.js');

function harness() {
  let settle;
  const timers = new Map();
  let next = 0;
  const context = {
    window: { leoMargin: kit }, document: { hidden: false, addEventListener() {} },
    chat: { isBusy: () => false, setLost() {} }, saving: { unsaved: () => false },
    flush: async () => {}, api: () => new Promise((resolve, reject) => { settle = { resolve, reject }; }),
    enc: encodeURIComponent, Locked: class extends Error {},
    setTimeout: (fn) => { const id = ++next; timers.set(id, fn); return id; },
    clearTimeout: (id) => timers.delete(id), cancelAnimationFrame() {},
    $: () => null, fail: (e) => { throw e; },
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/margin-notes.js'), 'utf8'), context);
  const state = vm.runInContext('marginPage', context);
  const session = () => ({ note: { id: 'a', version: 'abc' }, doc: { source: () => 'A changed paragraph long enough to read.' } });
  Object.assign(state, { s: session(), mode: 'along', baseline: [] });
  context.marginShowReading = () => {};
  context.marginRedraw = () => {};
  return { context, state, session, timers, settle: () => settle };
}

test('an obsolete read cannot clear a newer note reading state or timer', async () => {
  for (const rejected of [false, true]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    h.state.s = h.session();
    h.state.reading = true;
    h.state.faceTimer = h.context.setTimeout(() => {});
    const timer = h.state.faceTimer;
    if (rejected) h.settle().reject(new Error('provider failed'));
    else h.settle().resolve({ note: null });
    await pending;
    assert.equal(h.state.reading, true);
    assert.ok(h.timers.has(timer));
  }
});

test('read completion distinguishes silence from failure without losing failed blocks', async () => {
  for (const failed of [false, true]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    h.settle().resolve({ note: null, failed });
    await pending;
    assert.equal(h.state.reading, false);
    assert.equal(h.state.baseline.length, failed ? 0 : 1);
    assert.equal(h.state.status, failed ? 'Could not read this change. Check Writing AI and try again.' : 'No comment this time');
  }
});

test('mode updates retain confirmed state after failure and ignore departed notes', async () => {
  for (const departed of [false, true]) {
    const h = harness();
    h.state.mode = 'ask';
    h.context.marginControl = () => {};
    h.context.marginModeMenu = () => {};
    const pending = h.context.marginChangeMode('along');
    if (departed) {
      h.state.s = h.session();
      h.state.changing = true;
      h.settle().resolve({ mode: 'along' });
    } else h.settle().reject(new Error('save failed'));
    await pending;
    assert.equal(h.state.mode, 'ask');
    assert.equal(h.state.changing, departed);
    if (!departed) assert.equal(h.state.status, 'Could not save this mode. Try again.');
  }
});

test('refused reads keep pending changes and show a safe pause reason', async () => {
  for (const [why, status] of [['daily', 'Paused for today'], ['felix', 'Paused while Felix answers'], ['unknown', 'Could not read this change. Check Writing AI and try again.']]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    const error = new Error('private provider detail');
    error.data = { why };
    h.settle().reject(error);
    await pending;
    assert.equal(h.state.baseline.length, 0);
    assert.equal(h.state.status, status);
  }
});
