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
    const context = { stillMotion: () => reduced, MOTION_MS: 180, EASE_OUT: 'ease-out', app: {}, LIBRARY_ITEMS: '', libraryKey: () => '', flipList: (root, selector, key, change) => change() };
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

const motionSource = fs.readFileSync(path.resolve(__dirname, '../src/web/app/motion.js'), 'utf8');
function motion(extra = {}) {
  const context = { stillMotion: () => false, EASE_OUT: 'ease-out', document: { addEventListener() {}, querySelector: () => null, querySelectorAll: () => [], documentElement: { classList: { add() {}, remove() {} } } }, window: { scrollTo() {} }, innerHeight: 800, innerWidth: 1200, performance: { now: () => 0 }, Date, ...extra };
  vm.createContext(context);
  vm.runInContext(`${motionSource}\nglobalThis.api = { flipOffset, flipMoves, flipSkip, flipList, sameKeys, transitionSkip, swap, settleNow, settleLater, settleOffset, pillGlide, drawn, handoff, FLIP_TRAVEL, FLIP_ROWS, SETTLE_MS };`, context);
  return context.api;
}

test('list moves are capped at 40 px, skipped when nothing moved and never invented for new rows', () => {
  const m = motion();
  assert.deepEqual({ ...m.flipOffset({ left: 0, top: 300 }, { left: 0, top: 100 }) }, { dx: 0, dy: 40 });
  assert.deepEqual({ ...m.flipOffset({ left: 0, top: 100 }, { left: 90, top: 130 }) }, { dx: -40, dy: -30 });
  assert.equal(m.flipOffset({ left: 5, top: 5 }, { left: 5.4, top: 5.4 }), null);
  const before = new Map([['a', { left: 0, top: 0 }], ['b', { left: 0, top: 50 }]]);
  const after = new Map([['a', { left: 0, top: 0 }], ['b', { left: 0, top: 20 }], ['new', { left: 0, top: 90 }]]);
  assert.deepEqual([...m.flipMoves(before, after).map((x) => x.key)], ['b']);
});

test('list moves skip reduced motion, unchanged lists, single rows and big lists', () => {
  const m = motion();
  assert.equal(m.flipSkip({ reduced: false, count: 10 }), false);
  assert.equal(m.flipSkip({ reduced: true, count: 10 }), true);
  assert.equal(m.flipSkip({ reduced: false, count: 10, changed: false }), true);
  assert.equal(m.flipSkip({ reduced: false, count: 1 }), true);
  assert.equal(m.flipSkip({ reduced: false, count: m.FLIP_ROWS }), false);
  assert.equal(m.flipSkip({ reduced: false, count: m.FLIP_ROWS + 1 }), true);
  assert.equal(m.sameKeys(['a', 'b'], ['a', 'b']), true);
  assert.equal(m.sameKeys(['a', 'b'], ['b', 'a']), false);
});

test('a big or still list changes at once and reads no layout', () => {
  for (const [count, reduced] of [[201, false], [10, true]]) {
    let reads = 0, changes = 0;
    const els = Array.from({ length: count }, (_, i) => ({ dataset: { id: String(i) }, getBoundingClientRect: () => { reads++; return { left: 0, top: i }; }, animate() { throw Error('no motion'); } }));
    const m = motion({ stillMotion: () => reduced });
    assert.equal(m.flipList({ querySelectorAll: () => els }, '.x', (el) => el.dataset.id, () => ++changes), 1);
    assert.equal(reads, 0);
  }
});

test('a moved row animates from where it was, and a stable one does not', () => {
  const tops = new Map([['a', 0], ['b', 40]]);
  const played = [];
  const els = ['a', 'b'].map((id) => ({ dataset: { id }, getBoundingClientRect: () => ({ left: 0, top: tops.get(id) }), animate: (frames, options) => played.push({ id, frames, options }) }));
  const m = motion();
  m.flipList({ querySelectorAll: () => els }, '.x', (el) => el.dataset.id, () => tops.set('b', 90));
  assert.equal(played.length, 1);
  assert.equal(played[0].id, 'b');
  assert.equal(played[0].frames[0].transform, 'translate(0px, -40px)');
  assert.equal(played[0].options.duration, 150);
});

test('a page change transitions only when the API exists, motion is allowed, no sheet is open and the page differs', () => {
  const m = motion();
  const base = { supported: true, reduced: false, sheet: false, same: false, first: false };
  assert.equal(m.transitionSkip(base), '');
  assert.equal(m.transitionSkip({ ...base, supported: false }), 'unsupported');
  assert.equal(m.transitionSkip({ ...base, reduced: true }), 'reduced');
  assert.equal(m.transitionSkip({ ...base, sheet: true }), 'sheet');
  assert.equal(m.transitionSkip({ ...base, same: true }), 'same');
  assert.equal(m.transitionSkip({ ...base, first: true }), 'first');
});

test('without the API, or on the same page, the draw happens at once and no transition starts', async () => {
  let started = 0, drawn = 0;
  const m = motion({ document: { addEventListener() {}, querySelector: () => null, querySelectorAll: () => [], documentElement: { classList: { add() {}, remove() {} } } } });
  assert.equal(await m.swap('f:a', () => drawn++), false);
  assert.equal(await m.swap('f:a', () => drawn++), false);
  assert.equal(drawn, 2);
  const withApi = motion({ document: { addEventListener() {}, querySelector: () => null, querySelectorAll: () => [], documentElement: { classList: { add() {}, remove() {} } }, startViewTransition: () => { started++; } } });
  await withApi.swap('f:a', () => {});
  await withApi.swap('f:a', () => {});
  assert.equal(started, 0);
});

test('a transition draws once inside its callback, never for a stale page, and clears the page tone class', async () => {
  for (const stale of [false, true]) {
    const classes = new Set();
    let draws = 0, finish;
    const document = {
      addEventListener() {}, querySelector: () => null, querySelectorAll: () => [],
      documentElement: { classList: { add: (c) => classes.add(c), remove: (c) => classes.delete(c) } },
      startViewTransition: (update) => {
        const done = Promise.resolve().then(update);
        return { ready: Promise.resolve(), finished: done.then(() => { finish?.(); }), updateCallbackDone: done };
      },
    };
    const m = motion({ document });
    await m.swap('f:a', () => draws++);
    const moved = await m.swap('f:b', () => draws++, { stale: () => stale });
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(moved, true);
    assert.equal(draws, stale ? 1 : 2);
    assert.equal(classes.has('vt'), false);
  }
});

test('a drop settles the element wherever the redraw puts it, with the time already spent, and stops after 180 ms', () => {
  let now = 1000;
  const played = [];
  const element = (dataset = {}) => ({ dataset, getBoundingClientRect: () => ({ left: 100, top: 100 }), animate: (frames, options) => { const run = { frames, options, currentTime: 0 }; played.push(run); return run; } });
  const m = motion({ performance: { now: () => now } });
  let found = element();
  m.settleLater('kanban-card', 'c1', { left: 120, top: 90 });
  m.settleNow('kanban-card', () => found);
  assert.equal(played.length, 1);
  assert.equal(played[0].frames[0].transform, 'translate(20px, -10px) scale(1.02)');
  now = 1060;
  found = element();
  m.settleNow('kanban-card', () => found);
  assert.equal(played.length, 2);
  assert.equal(played[1].currentTime, 60);
  m.settleNow('kanban-card', () => found);
  assert.equal(played.length, 2);
  m.settleNow('kanban-col', () => found);
  assert.equal(played.length, 2);
  now = 1000 + m.SETTLE_MS;
  found = element();
  m.settleNow('kanban-card', () => found);
  assert.equal(played.length, 2);
});

test('the selection pill does not glide without a previous place, under Reduced Motion or when it did not move', () => {
  const pill = (rect) => ({ getBoundingClientRect: () => rect, animate: () => 'ran' });
  const to = { left: 10, top: 100, width: 200, height: 30 };
  assert.equal(motion().pillGlide(null, pill(to)), null);
  assert.equal(motion({ stillMotion: () => true }).pillGlide({ left: 10, top: 40, width: 200, height: 30 }, pill(to)), null);
  assert.equal(motion().pillGlide({ ...to }, pill(to)), null);
  assert.equal(motion().pillGlide({ left: 10, top: 40, width: 200, height: 30 }, pill(to)), 'ran');
});
