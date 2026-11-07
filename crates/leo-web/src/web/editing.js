(function (root) {
  'use strict';

  const FENCE = /^\s*(```|~~~)/;
  const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
  const QUOTE = /^\s*>/;
  const BOX = /^(?:[-*+]|\d+[.)]) {1,4}\[( |x|X)\](?: |$)/;

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
      const math = line.trim();
      const env = math.match(/^\\begin\{([a-z]+\*?)\}/);
      const closer = env ? `\\end{${env[1]}}` : math.startsWith('\\[') && !math.slice(2).includes('\\]') ? '\\]' : null;
      if (closer && !(env && math.includes(closer))) {
        let end = i + 1;
        while (end < lines.length && !lines[end].includes(closer)) end++;
        end = Math.min(end + 1, lines.length);
        blocks.push({ start: i, end, kind: 'math' });
        i = end;
        continue;
      }
      if (math.startsWith('$$') && !(math.length > 4 && math.endsWith('$$'))) {
        let end = i + 1;
        while (end < lines.length && !lines[end].trim().endsWith('$$')) end++;
        end = Math.min(end + 1, lines.length);
        blocks.push({ start: i, end, kind: 'math' });
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

  const MOST_HEADINGS = 300;

  function headings(lines) {
    const out = [];
    for (const block of splitBlocks(lines)) {
      if (block.kind !== 'line') continue;
      const found = lines[block.start].match(/^\s*(#{1,6})\s+(.+?)\s*$/);
      if (!found) continue;
      const text = found[2].replace(/\s+#+\s*$/, '').trim();
      if (text) out.push({ line: block.start, level: found[1].length, text });
      if (out.length === MOST_HEADINGS) break;
    }
    return out;
  }

  function headingTree(rows) {
    const tree = [];
    const stack = [];
    for (const row of rows.slice(0, MOST_HEADINGS)) {
      if (!Number.isInteger(row.level) || row.level < 1 || row.level > 6) continue;
      const node = { ...row, children: [] };
      while (stack.length && stack[stack.length - 1].level >= row.level) stack.pop();
      if (stack.length) stack[stack.length - 1].children.push(node);
      else tree.push(node);
      stack.push(node);
    }
    return tree;
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

  const ITEM = /^([^\S\n]*)([-*+]|\d+[.)]|([a-zA-Z])\3*[.)])([^\S\n]+)(\[[ xX]\](?:[^\S\n]+|$))?/;

  function item(line) {
    const m = line.match(ITEM);
    return m && { lead: m[1], marker: m[2], box: m[5] || '', head: m[0], text: line.slice(m[0].length) };
  }

  const itemHead = (line) => item(line)?.head || '';
  const leadWidth = (lead) => lead.replace(/\t/g, '    ').length;

  function listLevel(line) {
    const found = item(line);
    return found ? Math.floor(leadWidth(found.lead) / 2) : 0;
  }

  function listPrefix(line) {
    const found = item(line);
    return found && !/[a-zA-Z]/.test(found.marker) ? found.head : '';
  }

  function nextMarker(marker) {
    const number = marker.match(/^(\d+)(.)$/);
    if (number) return `${Number(number[1]) + 1}${number[2]}`;
    const letters = marker.match(/^([a-zA-Z]+)(.)$/);
    if (!letters) return marker;
    const [, run, end] = letters;
    if (/[zZ]/.test(run)) return (run[0] === 'z' ? 'a' : 'A').repeat(run.length + 1) + end;
    return String.fromCharCode(run.charCodeAt(0) + 1).repeat(run.length) + end;
  }

  const firstMarker = (marker) => marker.replace(/\d+/, '1').replace(/[a-z]+/, 'a').replace(/[A-Z]+/, 'A');

  function renumber(lines, at) {
    if (!item(lines[at] ?? '')) return;
    let from = at;
    while (from > 0 && item(lines[from - 1])) from--;
    const open = [];
    for (let i = from; i < lines.length; i++) {
      const found = item(lines[i]);
      if (!found) break;
      const depth = leadWidth(found.lead);
      while (open.length && open[open.length - 1].depth > depth) open.pop();
      const level = open[open.length - 1]?.depth === depth ? open.pop() : null;
      const kind = firstMarker(found.marker);
      const marker = level ? (firstMarker(level.marker) === kind ? nextMarker(level.marker) : found.marker) : open.length ? kind : found.marker;
      open.push({ depth, marker });
      if (marker !== found.marker) lines[i] = found.lead + marker + lines[i].slice(found.lead.length + found.marker.length);
    }
  }

  function closeList(lines, at) {
    if (item(lines[at] ?? '')) return renumber(lines, at);
    const rest = lines.slice();
    rest.splice(at, 1);
    renumber(rest, at);
    rest.splice(at, 0, lines[at]);
    lines.splice(0, lines.length, ...rest);
  }

  function stepBack(line) {
    const found = item(line);
    return found.lead ? outdent(line) : found.text;
  }

  function continueLine(line) {
    const found = item(line);
    if (found) {
      const prefix = `${found.lead}${nextMarker(found.marker)} ${found.box ? '[ ] ' : ''}`;
      return found.text.trim() ? { prefix, exit: false } : { prefix, exit: true, back: stepBack(line) };
    }
    const m = line.match(/^(\s*)>\s?(.*)$/);
    if (m) return m[2].trim() ? { prefix: `${m[1]}> `, exit: false } : { prefix: '', exit: true, back: '' };
    return { prefix: '', exit: false };
  }

  function toggleBox(line) {
    const m = line.match(/^(\s*(?:[-*+]|\d+[.)]) {1,4}\[)( |x|X)(\](?: .*)?)$/s);
    if (!m) return line;
    return m[1] + (m[2] === ' ' ? 'x' : ' ') + m[3];
  }

  function indent(line) {
    // shortcut: two spaces under `1.` nest in leo but read flat in strict CommonMark, widen to the parent's text column if notes must nest elsewhere.
    return '  ' + line;
  }

  function outdent(line) {
    return line.replace(/^(?: {1,2}|\t)/, '');
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
    const k = visiblePrefix.length;
    if (k === 0) return out.length ? out[0] : prefix;
    if (k > out.length) return raw.length;
    return out[k - 1] + 1;
  }

  const MOST_PASSAGE = 8000;

  function lineAtOffset(lines, pos) {
    let start = 0;
    for (let k = 0; k < lines.length; k++) {
      if (pos <= start + lines[k].length) return k;
      start += lines[k].length + 1;
    }
    return lines.length - 1;
  }

  function snapshotOf(lines, from, to) {
    const text = lines.join('\n');
    let a = Math.max(0, Math.min(from, text.length));
    let b = Math.max(0, Math.min(to, text.length));
    if (a > b) [a, b] = [b, a];
    const picked = text.slice(a, b);
    if (!picked.trim()) return null;
    return {
      from: lineAtOffset(lines, a) + 1,
      to: lineAtOffset(lines, Math.max(a, b - 1)) + 1,
      text: [...picked].slice(0, MOST_PASSAGE).join(''),
    };
  }

  const PICTURE = /!\[\[([^\]|]+)(?:\|([^\]]*))?\]\]|!\[([^\]]*)\]\((<[^>]+>|[^)\s]+)(\s+"[^"]*")?\)/g;

  const outside = (target) => {
    const src = String(target || '').trim().replace(/^<(.*)>$/, '$1');
    return !/^data:image\/svg\+xml[;,]/i.test(src) && (/^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//'));
  };

  function setPictureWidth(text, index, width) {
    let n = -1;
    const size = width ? `|${Math.round(width)}` : '';
    const code = [...String(text).matchAll(/`[^`]+`/g)].map((m) => [m.index, m.index + m[0].length]);
    return String(text).replace(PICTURE, (whole, name, option, alt, target, title, at) => {
      if (code.some(([from, to]) => at >= from && at < to) || outside(name ?? target)) return whole;
      n += 1;
      if (n !== index) return whole;
      if (name !== undefined) {
        const keep = option && !/^\s*\d{1,4}(?:\s*x\s*\d{1,4})?\s*$/.test(option) ? `|${option}` : '';
        return `![[${name}${size || keep}]]`;
      }
      return `![${alt.replace(/\|\s*\d{1,4}(?:\s*x\s*\d{1,4})?\s*$/, '')}${size}](${target}${title || ''})`;
    });
  }

  const api = { listPrefix, itemHead, listLevel, renumber, closeList, stepBack, headingTree, headings, setPictureWidth, splitBlocks, boxesBefore, boxLine, continueLine, toggleBox, indent, outdent, rawOffset, isBox, snapshotOf, lineAtOffset, MOST_PASSAGE };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoEditing = api;
})(typeof window !== 'undefined' ? window : globalThis);
