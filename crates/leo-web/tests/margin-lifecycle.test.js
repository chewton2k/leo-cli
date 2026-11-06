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
    window: { leoMargin: kit }, document: { hidden: false, addEventListener() {}, documentElement: { addEventListener() {} } },
    chat: { isBusy: () => false, setLost() {} }, saving: { unsaved: () => false },
    flush: async () => {}, api: () => new Promise((resolve, reject) => { settle = { resolve, reject }; }),
    enc: encodeURIComponent, Locked: class extends Error {},
    setTimeout: (fn, delay) => { const id = ++next; fn.delay = delay; timers.set(id, fn); return id; },
    clearTimeout: (id) => timers.delete(id), cancelAnimationFrame() {},
    $: () => null, fail: (e) => { throw e; }, ON_MAC: true, MOTION_MS: 180, POP_FROM: .95,
    esc: (text) => String(text), choiceCards: (options) => JSON.stringify(options), segmented: () => '',
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
  for (const [why, status] of [['daily', 'Paused for today: daily limit reached'], ['felix', 'Paused while Felix answers'], ['unknown', 'Could not read this change. Check Writing AI and try again.']]) {
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

test('a stale 409 without a reason says the note changed and a retry follows the next edit', async () => {
  const h = harness();
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  const stale = new Error('leo answered 409.');
  stale.status = 409;
  h.settle().reject(stale);
  await pending;
  assert.equal(h.state.baseline.length, 0);
  assert.match(h.state.status, /note changed/i);
  assert.match(h.state.status, /next edit/i);
  assert.doesNotMatch(h.state.status, /Writing AI/);
  h.state.loaded = true;
  h.context.marginEdited(h.state.s);
  assert.equal(h.timers.get(h.state.timer).delay, 4000);
});

test('a refusal with a reason is never mistaken for a stale version', async () => {
  for (const [status, why, expected] of [[409, 'daily', 'Paused for today: daily limit reached'], [429, 'soon', 'Paused after a recent comment'], [403, 'unedited', 'Reads changes after you edit']]) {
    const h = harness();
    const pending = h.context.marginRead(h.state.s);
    await new Promise(setImmediate);
    const refused = new Error('refused');
    refused.status = status;
    refused.data = { why };
    h.settle().reject(refused);
    await pending;
    assert.equal(h.state.status, expected);
  }
});

async function silent() {
  const h = harness();
  const pending = h.context.marginRead(h.state.s);
  await new Promise(setImmediate);
  h.settle().resolve({ note: null });
  await pending;
  return h;
}

test('No comment this time clears after its bounded timer', async () => {
  const h = await silent();
  assert.equal(h.state.status, 'No comment this time');
  assert.ok(h.timers.has(h.state.statusTimer));
  h.timers.get(h.state.statusTimer)();
  assert.equal(h.state.status, '');
});

test('the bounded timer leaves a newer status, another note and a departed page alone', async () => {
  for (const change of ['newer', 'other note', 'left']) {
    const h = await silent();
    const timer = h.timers.get(h.state.statusTimer);
    if (change === 'newer') h.context.marginSetStatus('Paused for today');
    else if (change === 'other note') h.state.s = h.session();
    else h.context.marginLeave();
    const status = h.state.status;
    timer();
    assert.equal(h.state.status, status, change);
  }
});

test('a new edit clears No comment this time but not a failure', async () => {
  const h = await silent();
  h.state.loaded = true;
  h.context.marginEdited(h.state.s);
  assert.equal(h.state.status, '');
  h.context.marginSetStatus('Could not read this change. Check Writing AI and try again.');
  h.context.marginEdited(h.state.s);
  assert.equal(h.state.status, 'Could not read this change. Check Writing AI and try again.');
});

test('leaving the note drops the pending status timer', async () => {
  const h = await silent();
  const timer = h.state.statusTimer;
  h.context.marginLeave();
  assert.equal(h.timers.has(timer), false);
});

test('When you ask says how to ask, with the real shortcut and no promise of automatic reading', () => {
  for (const [mac, keys] of [[true, 'Cmd+Shift+E'], [false, 'Ctrl+Shift+E']]) {
    const h = harness();
    h.context.ON_MAC = mac;
    const html = h.context.felixSettingsCards({ gap: 90, gaps: [30, 90, 180, 300], cap: 40, caps: [20, 40, 80, 0], today: 0, default_mode: 'ask', layout: 'right', pace: 'regular' }, { tasks: [] });
    assert.ok(html.includes(`Select text, then Ask Felix or ${keys}.`));
    assert.ok(html.includes('Reads along sends the note’s title and the text you change to your writing AI.'));
    assert.ok(!/automatic/i.test(html));
  }
});

test('the page keeps only the asking affordances it really has', () => {
  const passages = fs.readFileSync(path.join(__dirname, '../src/web/app/passages.js'), 'utf8');
  assert.match(passages, /<span>Ask Felix<\/span>/);
  assert.match(passages, /e\.shiftKey/);
  assert.match(passages, /\(e\.metaKey \|\| e\.ctrlKey\)/);
});

test('the mascot, the tether and the reading stub are gone from the margin script and styles', () => {
  const script = fs.readFileSync(path.join(__dirname, '../src/web/app/margin-notes.js'), 'utf8');
  const page = fs.readFileSync(path.join(__dirname, '../src/web/index.html'), 'utf8');
  for (const dead of ['tether', 'felix.felix', 'margin-head', 'margin-face', 'readFace']) assert.equal(script.includes(dead), false, dead);
  for (const dead of ['.tether', '.margin-reading', '.margin-head', 'margin-reveal', '@keyframes margin-draw', '.chat-history-new', '.settings-layout::before']) assert.equal(page.includes(dead), false, dead);
});

test('leaving with a queued hover lets the next note schedule and paint its own hover', () => {
  const h = harness();
  const frames = new Map();
  let id = 0;
  h.context.requestAnimationFrame = fn => { frames.set(++id, fn); return id; };
  h.context.cancelAnimationFrame = id => frames.delete(id);
  h.context.marginPaint = () => {};
  const layer = () => ({ remove() {}, parentElement: { getBoundingClientRect: () => ({ left: 0, top: 0 }), style: { removeProperty() {} } } });
  const move = key => h.context.marginHover({ clientX: 2, clientY: 2, pointerType: 'mouse', target: { closest: () => ({ dataset: { id: key } }) } });
  h.state.layer = layer();
  move('old');
  assert.equal(frames.size, 1);
  h.context.marginLeave();
  assert.equal(frames.size, 0);
  h.state.layer = layer();
  move('new');
  assert.equal(frames.size, 1);
  frames.get(h.state.pointer)();
  assert.equal(h.state.hover, 'new');
  assert.equal(h.state.pointer, 0);
});

 test('preference failure keeps the remembered layout or defaults to both', async () => {
  for (const saved of [null, 'right', 'marks', 'both']) {
    const h = harness();
    h.context.localStorage = { getItem: () => saved };
    h.context.api = async () => { throw new Error('offline'); };
    assert.equal((await h.context.marginLoadPrefs()).layout, saved || 'both');
  }
});

test('switching to When you ask during the save sends nothing, and a late answer leaves no status', async () => {
  const h = harness();
  let sent = 0;
  h.context.api = () => { sent++; return new Promise(() => {}); };
  h.context.flush = async () => { h.state.mode = 'ask'; };
  await h.context.marginRead(h.state.s);
  assert.equal(sent, 0);

  const late = harness();
  const pending = late.context.marginRead(late.state.s);
  await new Promise(setImmediate);
  late.state.mode = 'ask';
  late.settle().resolve({ note: null });
  await pending;
  assert.equal(late.state.status, '');
});
