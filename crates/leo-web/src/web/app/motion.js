const FLIP_MS = 150;
const FLIP_TRAVEL = 40;
const FLIP_ROWS = 200;
const QUICK_FLIP_MS = 120;
const SETTLE_MS = 180;
const SETTLE_SCALE = 1.02;

const drawn = { key: '', source: null, at: 0, top: false };

function scrollToTop() {
  if (!drawn.top) return;
  drawn.top = false;
  window.scrollTo(0, 0);
}

function flipOffset(before, after, travel = FLIP_TRAVEL) {
  const dx = before.left - after.left;
  const dy = before.top - after.top;
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return null;
  const cap = (value) => Math.max(-travel, Math.min(travel, value));
  return { dx: cap(dx), dy: cap(dy) };
}

function flipMoves(before, after, travel = FLIP_TRAVEL) {
  const moves = [];
  for (const [key, to] of after) {
    const from = before.get(key);
    const offset = from && flipOffset(from, to, travel);
    if (offset) moves.push({ key, ...offset });
  }
  return moves;
}

function flipSkip({ reduced, count, changed = true, limit = FLIP_ROWS }) {
  return Boolean(reduced) || !changed || count < 2 || count > limit;
}

function flipRects(els, keyOf) {
  const rects = new Map();
  for (const el of els) {
    const key = keyOf(el);
    if (key === undefined || key === null || rects.has(key)) continue;
    const { left, top } = el.getBoundingClientRect();
    rects.set(key, { left, top });
  }
  return rects;
}

function flipPlay(els, keyOf, before, ms) {
  const after = flipRects(els, keyOf);
  const moves = new Map(flipMoves(before, after).map((move) => [move.key, move]));
  if (!moves.size) return 0;
  let played = 0;
  for (const el of els) {
    const move = moves.get(keyOf(el));
    if (!move || typeof el.animate !== 'function') continue;
    el.animate([{ transform: `translate(${move.dx}px, ${move.dy}px)` }, { transform: 'none' }], { duration: ms, easing: 'ease-out' });
    played++;
  }
  return played;
}

function flipList(root, selector, keyOf, change, { ms = FLIP_MS, changed = true } = {}) {
  const els = root ? [...root.querySelectorAll(selector)] : [];
  if (flipSkip({ reduced: stillMotion(), count: els.length, changed })) return change();
  const before = flipRects(els, keyOf);
  const result = change();
  flipPlay([...root.querySelectorAll(selector)], keyOf, before, ms);
  return result;
}

function sameKeys(a, b) {
  return a.length === b.length && a.every((key, i) => key === b[i]);
}

function transitionSkip({ reduced, sheet, same, first }) {
  if (reduced) return 'reduced';
  if (sheet) return 'sheet';
  if (first) return 'first';
  if (same) return 'same';
  return '';
}

function inView(el) {
  if (!el || !el.isConnected) return false;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
}

function morphSources(id) {
  if (!id) return [];
  const pick = drawn.source && drawn.source.isConnected && Date.now() - drawn.at < 1500 ? drawn.source : null;
  const own = pick && pick.dataset.id === id ? [pick] : [];
  const all = [...document.querySelectorAll('.card[data-id], .side-note[data-id]')].filter((el) => el.dataset.id === id);
  return [...own, ...all];
}

function morphTitle(id) {
  for (const row of morphSources(id)) {
    const title = row.matches('.card') ? row.querySelector('.card-title > span') : row.querySelector('.side-label');
    if (inView(title)) return title;
  }
  return null;
}

function textRect(el) {
  if (!el || !el.isConnected) return null;
  const range = document.createRange();
  range.selectNodeContents(el);
  const { left, top, width, height } = range.getBoundingClientRect();
  return width > 0 && height > 0 ? { left, top, width, height } : null;
}

function glideOffset(from, to) {
  if (!from || !to || !from.width || !to.width) return null;
  const scale = from.width / to.width;
  const dx = from.left - to.left;
  const dy = from.top - to.top;
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(scale - 1) < .01) return null;
  return { dx, dy, scale };
}

function glideTitle(from, el, ms = MOTION_MS) {
  const to = textRect(el);
  const move = glideOffset(from, to);
  if (!move || typeof el.animate !== 'function') return null;
  const box = el.getBoundingClientRect();
  el.style.transformOrigin = `${to.left - box.left}px ${to.top - box.top}px`;
  const run = el.animate([{ transform: `translate(${move.dx}px, ${move.dy}px) scale(${move.scale})` }, { transform: 'none' }], { duration: ms, easing: EASE_OUT });
  const done = () => { el.style.transformOrigin = ''; };
  run.onfinish = done;
  run.oncancel = done;
  return run;
}

function swap(key, draw, { from = null, to = null } = {}) {
  const skip = transitionSkip({
    reduced: stillMotion(),
    sheet: Boolean(document.querySelector('.scrim')),
    same: drawn.key === key,
    first: !drawn.key,
  });
  drawn.key = key;
  const was = skip || !from ? null : textRect(from());
  draw();
  if (skip) return false;
  if (was && to) glideTitle(was, to());
  return true;
}

document.addEventListener('click', (e) => {
  const row = e.target.closest ? e.target.closest('[data-action="open-note"]') : null;
  drawn.source = row;
  drawn.at = Date.now();
}, true);

function settleOffset(from, to) {
  if (!from) return { dx: 0, dy: 0 };
  return { dx: from.left - to.left, dy: from.top - to.top };
}

function settle(el, { from = null, elapsed = 0, ms = SETTLE_MS } = {}) {
  if (!el || stillMotion() || typeof el.animate !== 'function' || elapsed >= ms) return null;
  const { dx, dy } = settleOffset(from, el.getBoundingClientRect());
  const run = el.animate([
    { transform: `translate(${dx}px, ${dy}px) scale(${SETTLE_SCALE})`, boxShadow: '0 10px 28px rgba(0, 0, 0, .18), 0 2px 6px rgba(0, 0, 0, .1)' },
    { transform: 'none', boxShadow: '0 0 0 rgba(0, 0, 0, 0)' },
  ], { duration: ms, easing: EASE_OUT });
  run.currentTime = elapsed;
  return run;
}

const handoff = { pending: null };

function settleLater(kind, id, from) {
  handoff.pending = { kind, id, from, at: performance.now() };
}

function settleNow(kind, find) {
  const pending = handoff.pending;
  if (!pending || pending.kind !== kind) return;
  const el = find(pending.id);
  const elapsed = performance.now() - pending.at;
  if (elapsed >= SETTLE_MS) {
    handoff.pending = null;
    return;
  }
  if (!el) return;
  if (el.dataset.settled !== String(pending.at)) {
    el.dataset.settled = String(pending.at);
    settle(el, { from: pending.from, elapsed });
  }
}

const PILL_MS = 220;

function pillKey(pill) {
  const row = pill && pill.parentElement;
  return row ? row.dataset.id || row.dataset.refreshKey || '' : '';
}

function pillGlide(from, pill, ms = PILL_MS) {
  if (!from || !pill || stillMotion() || typeof pill.animate !== 'function') return null;
  const to = pill.getBoundingClientRect();
  if (!to.width || !to.height) return null;
  const dy = from.top - to.top;
  const dx = from.left - to.left;
  const sx = from.width / to.width;
  const sy = from.height / to.height;
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(sx - 1) < .01 && Math.abs(sy - 1) < .01) return null;
  return pill.animate([
    { transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` },
    { transform: 'none' },
  ], { duration: ms, easing: 'cubic-bezier(.22, 1, .36, 1)' });
}

function lift(el) {
  if (!el || stillMotion() || typeof el.animate !== 'function') return null;
  return el.animate([{ scale: '1', boxShadow: 'none' }], { duration: 140, easing: EASE_OUT });
}

function liftedRect(el) {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { left: r.left + (r.width - r.width / SETTLE_SCALE) / 2, top: r.top + (r.height - r.height / SETTLE_SCALE) / 2 };
}
