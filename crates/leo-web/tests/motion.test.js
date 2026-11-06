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

test('note deletion preserves failures and never redirects another open note', async () => {
  for (const mode of ['success', 'error', 'navigate']) {
    let destroyed = 0, went = 0, failed = 0;
    const session = { note: { id: 'note' }, doc: { destroy() { destroyed++; } } };
    const box = { isConnected: true };
    const button = { dataset: { id: 'note' }, isConnected: true, disabled: false, closest: () => box };
    const context = { state: { session }, closeSheet() {}, enc: encodeURIComponent, loadSideFolders() {}, toast() {}, restore() {}, fail: () => failed++, go: () => went++ };
    context.api = async () => {
      if (mode === 'error') throw Error('offline');
      if (mode === 'navigate') context.state.session = { note: { id: 'other' } };
    };
    vm.createContext(context);
    vm.runInContext(sheets.slice(sheets.indexOf('async function deleteNow('), sheets.indexOf('async function restore(')), context);
    await context.deleteNow(button);
    assert.equal(went, mode === 'success' ? 1 : 0);
    assert.equal(destroyed, mode === 'success' ? 1 : 0);
    assert.equal(failed, mode === 'error' ? 1 : 0);
    assert.equal(button.disabled, false);
    if (mode === 'error') assert.equal(context.state.session, session);
    if (mode === 'navigate') assert.equal(context.state.session.note.id, 'other');
  }
});

const motionSource = fs.readFileSync(path.resolve(__dirname, '../src/web/app/motion.js'), 'utf8');
function motion(extra = {}) {
  const context = { stillMotion: () => false, EASE_OUT: 'ease-out', MOTION_MS: 180, document: { addEventListener() {}, querySelector: () => null, querySelectorAll: () => [], documentElement: { classList: { add() {}, remove() {} } } }, window: { scrollTo() {} }, innerHeight: 800, innerWidth: 1200, performance: { now: () => 0 }, Date, ...extra };
  vm.createContext(context);
  vm.runInContext(`${motionSource}\nglobalThis.api = { flipOffset, flipMoves, flipSkip, flipList, transitionSkip, swap, glideOffset, glideTitle, textRect, settleNow, settleLater, settleOffset, pillGlide, drawn, handoff, FLIP_TRAVEL, FLIP_ROWS, SETTLE_MS };`, context);
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

test('a page change animates only when motion is allowed, no sheet is open and the page differs', () => {
  const m = motion();
  const base = { reduced: false, sheet: false, same: false, first: false };
  assert.equal(m.transitionSkip(base), '');
  assert.equal(m.transitionSkip({ ...base, reduced: true }), 'reduced');
  assert.equal(m.transitionSkip({ ...base, sheet: true }), 'sheet');
  assert.equal(m.transitionSkip({ ...base, same: true }), 'same');
  assert.equal(m.transitionSkip({ ...base, first: true }), 'first');
});

test('a page change never starts a view transition, which would put the root element under every click, and draws at once', () => {
  let started = 0, draws = 0;
  const document = { addEventListener() {}, querySelector: () => null, querySelectorAll: () => [], documentElement: { classList: { add() {}, remove() {} } }, startViewTransition: () => { started++; } };
  const m = motion({ document });
  assert.equal(m.swap('f:a', () => draws++), false);
  assert.equal(draws, 1);
  assert.equal(m.swap('f:b', () => draws++), true);
  assert.equal(draws, 2);
  assert.equal(m.swap('f:b', () => draws++), false);
  assert.equal(started, 0);
});

test('the title glides from where its text was, only when the page changed and the text moved', () => {
  const m = motion();
  assert.deepEqual({ ...m.glideOffset({ left: 100, top: 300, width: 120, height: 18 }, { left: 360, top: 120, width: 240, height: 34 }) }, { dx: -260, dy: 180, scale: 0.5 });
  assert.equal(m.glideOffset({ left: 5, top: 5, width: 100, height: 20 }, { left: 5.2, top: 5.1, width: 100, height: 20 }), null);
  assert.equal(m.glideOffset(null, { left: 0, top: 0, width: 10, height: 10 }), null);
  const played = [];
  const range = { selectNodeContents() {}, getBoundingClientRect: () => ({ left: 360, top: 120, width: 240, height: 34 }) };
  const el = { isConnected: true, style: {}, getBoundingClientRect: () => ({ left: 340, top: 110 }), animate: (frames, options) => { const run = {}; played.push({ frames, options, run }); return run; } };
  const g = motion({ document: { addEventListener() {}, querySelector: () => null, querySelectorAll: () => [], createRange: () => range } });
  const run = g.glideTitle({ left: 100, top: 300, width: 120, height: 18 }, el);
  assert.equal(played.length, 1);
  assert.equal(played[0].frames[0].transform, 'translate(-260px, 180px) scale(0.5)');
  assert.equal(played[0].options.duration, 180);
  assert.equal(el.style.transformOrigin, '20px 10px');
  run.onfinish();
  assert.equal(el.style.transformOrigin, '');
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
