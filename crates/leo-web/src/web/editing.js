(function (root) {
  'use strict';

  const FENCE = /^\s*(```|~~~)/;
  const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
  const QUOTE = /^\s*>/;
  const BOX = /^- \[( |x|X)\] /;

  const isBox = (line) => BOX.test(line.trimStart());

  function splitBlocks(lines) {
    const blocks = [];
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      const fence = line.match(FENCE);
      if (fence) {
        let end = i + 1;
        while (end < lines.length && !new RegExp('^\\s*' + fence[1]).test(lines[end])) end++;
        end = Math.min(end + 1, lines.length);
        blocks.push({ start: i, end, kind: 'code' });
        i = end;
        continue;
      }
      if (line.includes('|') && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]) && lines[i + 1].includes('-')) {
        let end = i + 2;
        while (end < lines.length && lines[end].includes('|') && lines[end].trim()) end++;
        blocks.push({ start: i, end, kind: 'table' });
        i = end;
        continue;
      }
      if (QUOTE.test(line)) {
        let end = i + 1;
        while (end < lines.length && QUOTE.test(lines[end])) end++;
        blocks.push({ start: i, end, kind: 'quote' });
        i = end;
        continue;
      }
      blocks.push({ start: i, end: i + 1, kind: 'line' });
      i++;
    }
    if (!blocks.length) blocks.push({ start: 0, end: 1, kind: 'line' });
    return blocks;
  }

  function boxesBefore(lines, index) {
    let count = 0;
    for (let i = 0; i < index && i < lines.length; i++) if (isBox(lines[i])) count++;
    return count;
  }

  function boxLine(lines, number) {
    let count = 0;
    for (let i = 0; i < lines.length; i++) {
      if (isBox(lines[i]) && ++count === number) return i;
    }
    return -1;
  }

  function continueLine(line) {
    let m = line.match(/^(\s*)([-*+])\s+\[[ xX]\]\s?(.*)$/);
    if (m) return m[3].trim() ? { prefix: `${m[1]}${m[2]} [ ] `, exit: false } : { prefix: '', exit: true };
    m = line.match(/^(\s*)([-*+])\s(.*)$/);
    if (m) return m[3].trim() ? { prefix: `${m[1]}${m[2]} `, exit: false } : { prefix: '', exit: true };
    m = line.match(/^(\s*)(\d+)([.)])\s(.*)$/);
    if (m) return m[4].trim() ? { prefix: `${m[1]}${Number(m[2]) + 1}${m[3]} `, exit: false } : { prefix: '', exit: true };
    m = line.match(/^(\s*)>\s?(.*)$/);
    if (m) return m[2].trim() ? { prefix: `${m[1]}> `, exit: false } : { prefix: '', exit: true };
    return { prefix: '', exit: false };
  }

  function toggleBox(line) {
    const m = line.match(/^(\s*- \[)( |x|X)(\] .*)$/s);
    if (!m) return line;
    return m[1] + (m[2] === ' ' ? 'x' : ' ') + m[3];
  }

  function indent(line) {
    return '  ' + line;
  }

  function outdent(line) {
    return line.replace(/^ {1,2}/, '');
  }

  const PREFIX = /^(\s*)(#{1,6}\s+|>\s?|(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?)?/;

  function visibleIndices(raw) {
    const out = [];
    const prefix = raw.match(PREFIX)[0].length;
    let i = prefix;
    while (i < raw.length) {
      const c = raw[i];
      if (c === '[' || (c === '!' && raw[i + 1] === '[')) {
        const open = c === '!' ? i + 1 : i;
        const close = raw.indexOf('](', open);
        const end = close >= 0 ? raw.indexOf(')', close) : -1;
        if (close >= 0 && end >= 0) {
          for (let k = open + 1; k < close; k++) out.push(k);
          i = end + 1;
          continue;
        }
      }
      if (c === '*' || c === '~' || c === '`') {
        i++;
        continue;
      }
      out.push(i);
      i++;
    }
    return { prefix, out };
  }

  function rawOffset(raw, visiblePrefix) {
    const { prefix, out } = visibleIndices(raw);
    const k = [...visiblePrefix].length;
    if (k === 0) return out.length ? out[0] : prefix;
    if (k > out.length) return raw.length;
    return out[k - 1] + 1;
  }

  const api = { splitBlocks, boxesBefore, boxLine, continueLine, toggleBox, indent, outdent, rawOffset, isBox };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoEditing = api;
})(typeof window !== 'undefined' ? window : globalThis);
