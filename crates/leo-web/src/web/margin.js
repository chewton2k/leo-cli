(function (root) {
  'use strict';

  const MARGIN_WIDTH = 240;
  const MARGIN_GAP = 24;
  const TICK_GUTTER = 48;
  const END_GUTTER = 16;
  const LEAST_COLUMN = 600;
  const TAIL = 24;
  const LEAST_TAIL = 6;
  const READ_BLOCKS = 6;
  const READ_BLOCK_CHARS = 4000;
  const READ_CHARS = 8000;
  const LEAST_BLOCK_CHARS = 20;
  const NEAR = 120;
  const AROUND = 48;

  function occurrences(source, text) {
    const hits = [];
    let at = source.indexOf(text);
    while (at >= 0) {
      hits.push(at);
      at = source.indexOf(text, at + 1);
    }
    return hits;
  }

  function sharedEnd(a, b) {
    let n = 0;
    while (n < a.length && n < b.length && a[a.length - 1 - n] === b[b.length - 1 - n]) n++;
    return n;
  }

  function sharedStart(a, b) {
    let n = 0;
    while (n < a.length && n < b.length && a[n] === b[n]) n++;
    return n;
  }

  function byContext(source, quote, prefix, suffix) {
    const before = prefix.slice(-TAIL);
    const after = suffix.slice(0, TAIL);
    const room = quote.length * 2 + NEAR;
    const enough = (tail, whole) => !whole || tail.length >= LEAST_TAIL;
    if (!enough(before, prefix) || !enough(after, suffix)) return null;
    if (!before && !after) return null;
    const starts = before ? occurrences(source, before).map((at) => at + before.length) : [0];
    for (const from of starts) {
      const to = after ? source.indexOf(after, from + 1) : source.length;
      if (to > from && to - from <= room && source.slice(from, to).trim()) return { from, to, how: 'context' };
    }
    return null;
  }

  function find(source, { quote = '', prefix = '', suffix = '' }) {
    if (!quote) return null;
    const hits = occurrences(source, quote);
    if (hits.length === 1) return { from: hits[0], to: hits[0] + quote.length, how: 'exact' };
    if (hits.length) {
      const score = (at) => sharedEnd(source.slice(Math.max(0, at - prefix.length), at), prefix) + sharedStart(source.slice(at + quote.length, at + quote.length + suffix.length), suffix);
      const best = hits.reduce((a, b) => (score(b) > score(a) ? b : a));
      return { from: best, to: best + quote.length, how: 'exact' };
    }
    return byContext(source, quote, prefix, suffix);
  }

  function around(source, quote, line = 1) {
    const start = source.split('\n').slice(0, Math.max(0, line - 1)).join('\n').length;
    const first = source.indexOf(quote, line > 1 ? start : 0);
    const at = first >= 0 ? first : source.indexOf(quote);
    if (at < 0) return null;
    const low = (code) => code >= 0xdc00 && code <= 0xdfff;
    const high = (code) => code >= 0xd800 && code <= 0xdbff;
    let from = Math.max(0, at - AROUND);
    let to = Math.min(source.length, at + quote.length + AROUND);
    if (from > 0 && low(source.charCodeAt(from))) from++;
    if (to < source.length && high(source.charCodeAt(to - 1))) to--;
    return { prefix: source.slice(from, at), suffix: source.slice(at + quote.length, to) };
  }

  function lineAfter(source, at) {
    return source.slice(0, at).split('\n').length;
  }

  function stack(items, { gap = 16, floor = 0, pinned = null } = {}) {
    const sorted = [...items].sort((a, b) => a.want - b.want);
    const tops = [];
    let next = floor;
    for (const item of sorted) {
      const top = Math.max(item.want, next);
      tops.push(top);
      next = top + item.height + gap;
    }
    const held = sorted.findIndex((item) => item.id === pinned);
    if (held >= 0) {
      tops[held] = Math.max(sorted[held].want, floor);
      for (let i = held + 1; i < sorted.length; i++) tops[i] = Math.max(sorted[i].want, tops[i - 1] + sorted[i - 1].height + gap);
      for (let i = held - 1; i >= 0; i--) tops[i] = Math.min(tops[i], tops[i + 1] - sorted[i].height - gap);
      if (sorted.length && tops[0] < floor) {
        tops[0] = floor;
        for (let i = 1; i < sorted.length; i++) tops[i] = Math.max(tops[i], tops[i - 1] + sorted[i - 1].height + gap);
      }
    }
    return Object.fromEntries(sorted.map((item, i) => [item.id, tops[i]]));
  }

  function sides(items, gap = 16) {
    const ends = { left: -Infinity, right: -Infinity };
    const out = {};
    for (const item of [...items].sort((a, b) => a.want - b.want)) {
      const side = ends.right <= item.want ? 'right' : ends.left <= item.want ? 'left' : ends.right <= ends.left ? 'right' : 'left';
      out[item.id] = side;
      ends[side] = Math.max(ends[side], item.want) + item.height + gap;
    }
    return out;
  }

  const ROOM_RIGHT = TICK_GUTTER + LEAST_COLUMN + MARGIN_GAP + MARGIN_WIDTH + END_GUTTER;
  const ROOM_BOTH = ROOM_RIGHT + MARGIN_WIDTH + MARGIN_GAP;

  function arrangement({ width, setting = 'right', noteWidth = 'wide', panel = false, threads = 0, along = false }) {
    if (!threads && !along) return 'none';
    if (setting === 'marks' || noteWidth === 'full' || panel || width < ROOM_RIGHT) return 'marks';
    return setting === 'both' && width >= ROOM_BOTH ? 'both' : 'right';
  }

  function blocksOf(source) {
    return source.split(/\n\s*\n/).map((block) => block.trim()).filter(Boolean);
  }

  function changedBlocks(before, source) {
    const known = new Map();
    for (const block of before) known.set(block, (known.get(block) || 0) + 1);
    const fresh = [];
    for (const block of blocksOf(source)) {
      const left = known.get(block) || 0;
      if (left) known.set(block, left - 1);
      else if ([...block].length >= LEAST_BLOCK_CHARS) fresh.push(block);
    }
    const out = [];
    let used = 0;
    for (const block of fresh.slice(-READ_BLOCKS).reverse()) {
      const clipped = [...block].slice(-READ_BLOCK_CHARS).join('').trim();
      const size = [...clipped].length;
      if (used + size > READ_CHARS) break;
      used += size;
      out.unshift(clipped);
    }
    return out;
  }

  function wave(left, right, y) {
    const count = Math.max(1, Math.round((right - left) / 8));
    const step = (right - left) / count;
    let path = `M${left.toFixed(1)} ${y.toFixed(1)}`;
    for (let i = 0; i < count; i++) path += ` q${(step / 2).toFixed(1)} ${i % 2 ? 3 : -3} ${step.toFixed(1)} 0`;
    return path;
  }

  function tether(from, to, clear = from.x) {
    const middle = (clear + to.x) / 2;
    const lead = clear === from.x ? '' : ` L${clear.toFixed(1)} ${from.y.toFixed(1)}`;
    return `M${from.x.toFixed(1)} ${from.y.toFixed(1)}${lead} C${middle.toFixed(1)} ${(from.y + 2).toFixed(1)} ${(middle - 4).toFixed(1)} ${to.y.toFixed(1)} ${to.x.toFixed(1)} ${to.y.toFixed(1)}`;
  }

  const api = { MARGIN_WIDTH, MARGIN_GAP, ROOM_RIGHT, ROOM_BOTH, find, around, lineAfter, stack, sides, arrangement, blocksOf, changedBlocks, wave, tether };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoMargin = api;
})(typeof window !== 'undefined' ? window : globalThis);
