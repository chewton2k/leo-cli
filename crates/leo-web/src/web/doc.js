(function (root) {
  'use strict';

  const md = root.leoMarkdown;
  const ed = root.leoEditing;

  function mount(container, { source, onChange, placeholder, dir = '', onPictures = null, onSelection = null, outline = null }) {
    let lines = String(source || '').replace(/\r\n?/g, '\n').split('\n');
    let blocks = [];
    let editing = null;

    const MOST_UNDO = 200;
    const TYPING_PAUSE = 1000;
    const TYPING_GROUP = 4000;
    const past = [];
    const future = [];
    let current = lines.join('\n');
    let typing = { in: null, last: 0, since: 0 };

    function changed() {
      const next = lines.join('\n');
      if (next !== current) {
        const now = Date.now();
        const keystroke = Math.abs(next.length - current.length) <= 1;
        const same = keystroke && editing && typing.in === editing.area && now - typing.last < TYPING_PAUSE && now - typing.since < TYPING_GROUP;
        if (!same) {
          past.push(current);
          if (past.length > MOST_UNDO) past.shift();
          typing = { in: keystroke && editing ? editing.area : null, last: now, since: now };
        } else {
          typing.last = now;
        }
        future.length = 0;
        current = next;
      }
      onChange(next);
      outlineLater();
    }

    function restore(text) {
      const before = current;
      stop();
      typing = { in: null, last: 0, since: 0 };
      current = text;
      lines = text.split('\n');
      onChange(text);
      outlineLater();
      draw();
      let from = 0;
      while (from < before.length && from < text.length && before[from] === text[from]) from++;
      let tail = 0;
      while (tail < before.length - from && tail < text.length - from && before[before.length - 1 - tail] === text[text.length - 1 - tail]) tail++;
      editSpan(from, Math.max(from, text.length - tail));
    }

    function undo() {
      if (!past.length) return false;
      if (editing) stop();
      future.push(current);
      restore(past.pop());
      return true;
    }

    function redo() {
      if (!future.length) return false;
      if (editing) stop();
      past.push(current);
      restore(future.pop());
      return true;
    }

    function historyKey(e) {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return false;
      const key = e.key.toLowerCase();
      if (key === 'z') {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
        return true;
      }
      if (key === 'y' && e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        redo();
        return true;
      }
      return false;
    }

    function levelOf(text) {
      const lead = text.match(/^\s*/)[0].replace(/\t/g, '    ').length;
      return /^\s*([-*+]|\d+[.)])\s/.test(text) ? Math.floor(lead / 2) : 0;
    }

    function renderBlock(block) {
      const text = lines.slice(block.start, block.end).join('\n');
      if (!text.trim()) {
        const lone = lines.length === 1;
        return lone ? `<p class="placeholder">${md.escape(placeholder || 'Tap to start writing')}</p>` : '<p class="blank">&#8203;</p>';
      }
      return md.render(text.replace(/^\s+/, ''), {
        boxOffset: ed.boxesBefore(lines, block.start),
        dir,
      });
    }

    function draw() {
      blocks = ed.splitBlocks(lines);
      container.innerHTML =
        blocks
          .map((block, i) => {
            const text = lines.slice(block.start, block.end).join('\n');
            const level = block.kind === 'line' ? levelOf(text) : 0;
            const style = level ? ` style="margin-left:${level * 1.5}em"` : '';
            return `<div class="blk blk-${block.kind}" data-start="${block.start}" data-i="${i}"${style}>${renderBlock(block)}</div>`;
          })
          .join('') + '<div class="doc-tail" data-tail="1"></div>';
      outlineScroll();
    }

    let outlineTimer = 0;
    let outlineFrame = 0;
    let outlineKey = '';
    let outlineWide = null;
    let outlineRows = [];
    const outlineFolded = new Set();
    const outlineChevron = outline && outline.querySelector('summary > svg') ? outline.querySelector('summary > svg').outerHTML : '';
    const outlineNav = outline && outline.querySelector('nav');
    const outlineDetails = outline && outline.querySelector('details');
    const OUTLINE_PAUSE = 300;
    const OUTLINE_WIDE = 900;

    function outlineTop() {
      const bar = document.getElementById('bar');
      return (bar ? bar.getBoundingClientRect().bottom : 0) + 12;
    }

    function outlineActive() {
      outlineFrame = 0;
      if (!outline || !outline.isConnected || outline.hidden) return;
      const top = outlineTop();
      outline.style.setProperty('--outline-top', `${top}px`);
      let current = outlineRows[0];
      const bottom = root.scrollY > 0 && root.scrollY + root.innerHeight >= document.documentElement.scrollHeight - 2;
      for (const row of outlineRows) {
        const block = container.querySelector(`.blk[data-start="${row.line}"]`);
        if (block && block.getBoundingClientRect().top <= (bottom ? root.innerHeight : top + 24)) current = row;
      }
      let selected = current && outlineNav.querySelector(`button[data-line="${current.line}"]`);
      let hidden = selected && selected.closest('ul[hidden]');
      while (hidden) {
        selected = outlineNav.querySelector(`button[data-line="${hidden.dataset.parent}"]`);
        hidden = selected && selected.closest('ul[hidden]');
      }
      for (const button of outlineNav.querySelectorAll('button[data-line]')) {
        if (button === selected) button.setAttribute('aria-current', 'location');
        else button.removeAttribute('aria-current');
      }
    }

    function outlineScroll() {
      if (outline && !outlineFrame) outlineFrame = requestAnimationFrame(outlineActive);
    }

    function outlineList(rows, parent = null) {
      const folded = parent && outlineFolded.has(parent.key);
      const group = parent ? ` id="outline-group-${parent.line}" data-parent="${parent.line}"${folded ? ' hidden' : ''}` : '';
      return `<ul${group}>${rows.map((row) => {
        const text = md.escape(md.plain(row.text));
        const children = row.children.length > 0;
        const toggle = children ? `<button type="button" class="outline-toggle" data-fold="${row.line}" aria-label="Subheadings of ${text}" aria-expanded="${!outlineFolded.has(row.key)}" aria-controls="outline-group-${row.line}">${outlineChevron}</button>` : '<span class="outline-spacer" aria-hidden="true"></span>';
        return `<li><div class="outline-row">${toggle}<button type="button" data-line="${row.line}" title="${text}">${text}</button></div>${children ? outlineList(row.children, row) : ''}</li>`;
      }).join('')}</ul>`;
    }

    function outlineDraw(force = false) {
      if (!outlineNav || !container.isConnected) return;
      outlineRows = ed.headings(lines);
      const repeats = new Map();
      for (const row of outlineRows) {
        const label = JSON.stringify([row.level, row.text]);
        const occurrence = repeats.get(label) || 0;
        repeats.set(label, occurrence + 1);
        row.key = JSON.stringify([label, occurrence]);
      }
      const keys = new Set(outlineRows.map((row) => row.key));
      for (const key of outlineFolded) if (!keys.has(key)) outlineFolded.delete(key);
      outline.hidden = !outlineRows.length;
      const key = JSON.stringify(outlineRows);
      if (force || key !== outlineKey) {
        const active = outlineNav.contains(document.activeElement) ? document.activeElement : null;
        const focused = active ? { type: active.hasAttribute('data-fold') ? 'fold' : 'line', line: active.dataset.fold || active.dataset.line } : null;
        outlineKey = key;
        outlineNav.innerHTML = outlineList(ed.headingTree(outlineRows));
        if (focused) {
          const button = outlineNav.querySelector(`[data-${focused.type}="${focused.line}"]`) || outlineNav.querySelector('button[data-line]');
          if (button) button.focus({ preventScroll: true });
        }
      }
      outlineScroll();
    }

    function outlineLater() {
      if (!outline) return;
      clearTimeout(outlineTimer);
      outlineTimer = setTimeout(() => outlineDraw(), OUTLINE_PAUSE);
    }

    function outlineGo(e) {
      const toggle = e.target.closest('button[data-fold]');
      if (toggle && outlineNav.contains(toggle)) {
        const row = outlineRows.find((row) => row.line === Number(toggle.dataset.fold));
        if (row) {
          if (outlineFolded.has(row.key)) outlineFolded.delete(row.key);
          else outlineFolded.add(row.key);
          outlineDraw(true);
        }
        return;
      }
      const button = e.target.closest('button[data-line]');
      if (!button || !outlineNav.contains(button)) return;
      stop();
      const block = container.querySelector(`.blk[data-start="${button.dataset.line}"]`);
      if (!block) return;
      block.tabIndex = -1;
      block.focus({ preventScroll: true });
      if (!outlineWide) outlineDetails.open = false;
      const inset = outlineWide ? 0 : outlineDetails.querySelector('summary').getBoundingClientRect().height + 8;
      root.scrollTo({ top: Math.max(0, root.scrollY + block.getBoundingClientRect().top - outlineTop() - inset), behavior: 'instant' });
      outlineScroll();
    }

    const outlineResize = outline && root.ResizeObserver ? new ResizeObserver((entries) => {
      const wide = entries[0].contentRect.width >= OUTLINE_WIDE;
      if (wide !== outlineWide) {
        outlineWide = wide;
        outlineDetails.open = wide;
      }
      outlineScroll();
    }) : null;
    if (outlineNav) {
      outlineNav.addEventListener('click', outlineGo);
      root.addEventListener('scroll', outlineScroll, { passive: true });
      root.addEventListener('resize', outlineScroll);
      if (outlineResize) outlineResize.observe(outline.closest('.note-layout'));
      outlineDraw();
    }

    function blockAtLine(line) {
      return blocks.findIndex((b) => line >= b.start && line < b.end);
    }

    function fit(area) {
      area.style.height = 'auto';
      area.style.height = `${area.scrollHeight}px`;
    }

    let kept = { caret: null, picked: null, text: null };
    let told = false;

    function liveSelection() {
      if (!editing) return null;
      const { area } = editing;
      if (area.selectionStart === area.selectionEnd) return null;
      const base = lineStart(editing.start);
      return ed.snapshotOf(lines, base + area.selectionStart, base + area.selectionEnd);
    }

    function keep() {
      if (!editing) return;
      const base = lineStart(editing.start);
      kept = { caret: lineOf(base + editing.area.selectionEnd), picked: liveSelection(), text: lines.join('\n') };
    }

    function tell() {
      if (!onSelection) return;
      requestAnimationFrame(() => {
        const has = Boolean(container.isConnected && ((editing && document.activeElement === editing.area && liveSelection()) || selectedSpan()));
        if (has || told) onSelection(has);
        told = has;
      });
    }

    function caretBox(area, at) {
      const style = getComputedStyle(area);
      const mirror = document.createElement('div');
      for (const p of ['boxSizing', 'width', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth', 'fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'letterSpacing', 'lineHeight', 'textTransform', 'wordSpacing', 'tabSize', 'textIndent']) mirror.style[p] = style[p];
      Object.assign(mirror.style, { position: 'absolute', visibility: 'hidden', whiteSpace: 'pre-wrap', overflowWrap: 'break-word', top: '0', left: '-9999px' });
      mirror.textContent = area.value.slice(0, at);
      const mark = document.createElement('span');
      mark.textContent = area.value.slice(at, at + 1) || '.';
      mirror.appendChild(mark);
      document.body.appendChild(mirror);
      const box = area.getBoundingClientRect();
      const point = { x: box.left + mark.offsetLeft - area.scrollLeft, y: box.top + mark.offsetTop - area.scrollTop + mark.offsetHeight };
      mirror.remove();
      return point;
    }

    function onSelectionChange() {
      if (!container.isConnected) return;
      tell();
    }

    document.addEventListener('selectionchange', onSelectionChange);

    function stop() {
      if (!editing) return;
      keep();
      const { area } = editing;
      area.removeEventListener('input', onInput);
      area.removeEventListener('keydown', onKey);
      area.removeEventListener('blur', onBlur);
      for (const kind of ['select', 'keyup', 'mouseup']) area.removeEventListener(kind, tell);
      editing = null;
      draw();
      tell();
    }

    function edit(index, caret) {
      const block = blocks[index];
      if (!block) return;
      const length = lines.slice(block.start, block.end).join('\n').length;
      const at = caret === 'start' ? 0 : caret === 'end' || caret == null ? length : Math.max(0, Math.min(caret, length));
      open(index, index, at, at);
    }

    function open(first, last, from, to) {
      if (editing) stop();
      const block = blocks[first];
      const end = blocks[last];
      if (!block || !end) return;
      const el = container.querySelector(`.blk[data-i="${first}"]`);
      if (!el) return;
      for (let i = first + 1; i <= last; i++) {
        const covered = container.querySelector(`.blk[data-i="${i}"]`);
        if (covered) covered.remove();
      }
      const raw = lines.slice(block.start, end.end).join('\n');
      const area = document.createElement('textarea');
      area.className = 'line-edit';
      area.value = raw;
      area.rows = 1;
      area.spellcheck = true;
      area.setAttribute('autocapitalize', 'sentences');
      el.removeAttribute('style');
      el.replaceChildren(area);
      el.classList.add('editing');
      editing = { index: first, last, start: block.start, end: end.end, originalEnd: end.end, kind: first === last ? block.kind : 'span', area };
      fit(area);
      area.addEventListener('input', onInput);
      area.addEventListener('keydown', onKey);
      area.addEventListener('blur', onBlur);
      for (const kind of ['select', 'keyup', 'mouseup']) area.addEventListener(kind, tell);
      area.focus({ preventScroll: true });
      area.setSelectionRange(Math.max(0, Math.min(from, raw.length)), Math.max(0, Math.min(to, raw.length)));
      if (from === to) area.scrollIntoView({ block: 'nearest' });
    }

    function lineStart(line) {
      let at = 0;
      for (let k = 0; k < line && k < lines.length; k++) at += lines[k].length + 1;
      return at;
    }

    function lineOf(at) {
      let start = 0;
      for (let k = 0; k < lines.length; k++) {
        if (at <= start + lines[k].length) return k;
        start += lines[k].length + 1;
      }
      return lines.length - 1;
    }

    const docLength = () => lines.join('\n').length;

    function editSpan(from, to) {
      if (from > to) [from, to] = [to, from];
      const first = blockAtLine(lineOf(from));
      const last = blockAtLine(lineOf(to));
      if (first < 0 || last < 0) return;
      const base = lineStart(blocks[first].start);
      open(first, last, from - base, to - base);
    }

    function pointAt(node, offset, side) {
      if (node === container) {
        if (offset >= blocks.length) return docLength();
        if (side === 'start') return lineStart(shifted(blocks[offset].start));
        return offset > 0 ? lineStart(shifted(blocks[offset - 1].start) + blocks[offset - 1].end - blocks[offset - 1].start) - 1 : 0;
      }
      const el = node.nodeType === 1 ? node : node.parentElement;
      const blk = el && el.closest('.blk');
      if (!blk || !container.contains(blk) || blk.classList.contains('editing')) return side === 'start' ? 0 : docLength();
      const block = blocks[Number(blk.dataset.i)];
      const start = shifted(block.start);
      const raw = lines.slice(start, start + block.end - block.start).join('\n');
      let inside = side === 'start' ? 0 : raw.length;
      if (block.kind === 'line' && raw.trim()) {
        const range = document.createRange();
        range.setStart(blk, 0);
        try {
          range.setEnd(node, offset);
          inside = ed.rawOffset(raw, range.toString().replace(/\u200b/g, ''));
        } catch (e) {
          inside = side === 'start' ? 0 : raw.length;
        }
      }
      return lineStart(start) + inside;
    }

    function selectedSpan() {
      if (editing && document.activeElement === editing.area) return null;
      const selection = root.getSelection ? root.getSelection() : null;
      if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
      const range = selection.getRangeAt(0);
      if (!container.contains(range.startContainer) && !container.contains(range.endContainer)) return null;
      const from = container.contains(range.startContainer) ? pointAt(range.startContainer, range.startOffset, 'start') : 0;
      const to = container.contains(range.endContainer) ? pointAt(range.endContainer, range.endOffset, 'end') : docLength();
      return from === to ? null : [from, to];
    }

    function takeSelection() {
      const span = selectedSpan();
      if (!span) return false;
      stop();
      editSpan(span[0], span[1]);
      return true;
    }

    function onDocumentKey(e) {
      if (!container.isConnected || e.isComposing || e.defaultPrevented) return;
      if (editing && document.activeElement === editing.area) return;
      const focus = document.activeElement;
      if ((!focus || focus === document.body || container.contains(focus)) && historyKey(e)) return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a' && (!focus || focus === document.body || container.contains(focus))) {
        e.preventDefault();
        stop();
        editSpan(0, docLength());
        return;
      }
      const typing = e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey;
      if (e.key !== 'Backspace' && e.key !== 'Delete' && !typing) return;
      if (!takeSelection()) return;
      e.preventDefault();
      const area = editing.area;
      area.setRangeText(typing ? e.key : '', area.selectionStart, area.selectionEnd, 'end');
      onInput();
    }

    document.addEventListener('keydown', onDocumentKey);

    function editLine(line, caret) {
      const index = blockAtLine(line);
      if (index >= 0) edit(index, caret);
    }

    function onInput() {
      if (told) tell();
      const { area, start } = editing;
      const next = area.value.split('\n');
      lines.splice(start, editing.end - start, ...next);
      editing.end = start + next.length;
      fit(area);
      changed();
    }

    function singleRow(area) {
      const style = getComputedStyle(area);
      const lineHeight = parseFloat(style.lineHeight) || 24;
      return area.scrollHeight <= lineHeight * 1.6;
    }

    function onKey(e) {
      if (e.isComposing || e.keyCode === 229) return;
      if (historyKey(e)) return;
      const { area, start, kind } = editing;
      const pos = area.selectionStart;
      const collapsed = pos === area.selectionEnd;
      const value = area.value;

      if (e.key === 'Escape') {
        e.preventDefault();
        area.blur();
        return;
      }

      if (kind === 'line' && e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        const before = value.slice(0, pos).split('\n');
        const after = value.slice(area.selectionEnd);
        const last = before[before.length - 1];
        const next = ed.continueLine(last);
        const span = editing.end - start;
        if (next.exit && !after.trim()) {
          before[before.length - 1] = last.replace(/^(\s*)(?:[-*+]\s(?:\[[ xX]\]\s?)?|\d+[.)]\s|>\s?)$/, '$1').trimEnd();
          lines.splice(start, span, ...before);
          changed();
          stop();
          editLine(start + before.length - 1, 'end');
          return;
        }
        lines.splice(start, span, ...before, ...(next.prefix + after).split('\n'));
        changed();
        stop();
        editLine(start + before.length, next.prefix.length);
        return;
      }

      if (kind === 'line' && e.key === 'Backspace' && collapsed && pos === 0 && start > 0) {
        const previous = blocks[blockAtLine(start - 1)];
        if (previous && previous.kind === 'line') {
          e.preventDefault();
          const above = lines[start - 1];
          lines.splice(start - 1, 1 + editing.end - start, ...(above + value).split('\n'));
          changed();
          stop();
          editLine(start - 1, above.length);
        }
        return;
      }

      if (kind === 'line' && e.key === 'Tab') {
        e.preventDefault();
        const next = e.shiftKey ? ed.outdent(value) : ed.indent(value);
        const shift = next.length - value.length;
        area.value = next;
        onInput();
        area.setSelectionRange(Math.max(0, pos + shift), Math.max(0, pos + shift));
        return;
      }

      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a' && (start > 0 || editing.end < lines.length)) {
        e.preventDefault();
        stop();
        editSpan(0, docLength());
        return;
      }

      if (e.key === 'ArrowUp' && collapsed && (pos === 0 || (kind === 'line' && singleRow(area))) && start > 0) {
        e.preventDefault();
        stop();
        editLine(start - 1, 'end');
        return;
      }

      if (e.key === 'ArrowDown' && collapsed && (pos === value.length || (kind === 'line' && singleRow(area))) && editing.end < lines.length) {
        e.preventDefault();
        const below = editing.end;
        stop();
        editLine(below, 'start');
      }
    }

    function settle() {
      if (editing && document.activeElement !== editing.area) stop();
    }

    function onBlur() {
      setTimeout(() => {
        if (!pressed) settle();
      }, 0);
    }

    function caretIn(el, x, y) {
      let node = null;
      let offset = 0;
      if (document.caretPositionFromPoint) {
        const pos = document.caretPositionFromPoint(x, y);
        if (pos) {
          node = pos.offsetNode;
          offset = pos.offset;
        }
      } else if (document.caretRangeFromPoint) {
        const range = document.caretRangeFromPoint(x, y);
        if (range) {
          node = range.startContainer;
          offset = range.startOffset;
        }
      }
      if (!node || !el.contains(node)) return null;
      const range = document.createRange();
      range.setStart(el, 0);
      try {
        range.setEnd(node, offset);
      } catch (e) {
        return null;
      }
      return range.toString().replace(/\u200b/g, '');
    }

    let pressed = false;
    let release;

    let extended = false;

    container.addEventListener('pointerdown', (e) => {
      pressed = true;
      clearTimeout(release);
      if (!e.shiftKey || !editing || e.target.closest('.editing')) return;
      const blk = e.target.closest('.blk');
      if (!blk) return;
      const block = blocks[Number(blk.dataset.i)];
      const start = shifted(block.start);
      const raw = lines.slice(start, start + block.end - block.start).join('\n');
      const prefix = block.kind === 'line' ? caretIn(blk, e.clientX, e.clientY) : null;
      const anchor = lineStart(editing.start) + editing.area.selectionStart;
      const below = start >= editing.end;
      const inside = prefix !== null && raw.trim() ? ed.rawOffset(raw, prefix) : below ? raw.length : 0;
      e.preventDefault();
      extended = true;
      const target = lineStart(start) + inside;
      stop();
      editSpan(anchor, target);
    });

    container.addEventListener('pointercancel', () => {
      pressed = false;
      settle();
    });

    function onPointerUp() {
      if (!pressed) return;
      clearTimeout(release);
      release = setTimeout(() => {
        pressed = false;
        settle();
      }, 400);
    }

    document.addEventListener('pointerup', onPointerUp);

    function shifted(line) {
      if (editing && line >= editing.originalEnd) return line + editing.end - editing.originalEnd;
      return line;
    }

    function boxFor(e) {
      const direct = e.target.closest('input[data-box]');
      if (direct) return direct;
      const item = e.target.closest('.task-item');
      const input = item && item.querySelector('input[data-box]');
      const text = item && item.querySelector('label.task > span');
      if (!input || !text) return null;
      return e.clientX < text.getBoundingClientRect().left ? input : null;
    }

    let framed = null;

    function unframe() {
      if (!framed) return;
      framed.frame.remove();
      framed.img.classList.remove('picked');
      framed = null;
    }

    function innerWidth(img) {
      return img.getBoundingClientRect().width;
    }

    function placeFrame() {
      if (!framed) return;
      const box = container.getBoundingClientRect();
      const r = framed.img.getBoundingClientRect();
      Object.assign(framed.frame.style, { left: `${r.left - box.left}px`, top: `${r.top - box.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      framed.frame.querySelector('.pic-size').textContent = `${Math.round(innerWidth(framed.img))} px`;
    }

    function pictureAt(img) {
      const blk = img.closest('.blk');
      if (!blk) return null;
      const block = blocks[Number(blk.dataset.i)];
      if (!block) return null;
      return { start: shifted(block.start), length: block.end - block.start, index: [...blk.querySelectorAll('img.note-img')].indexOf(img) };
    }

    function frame(img) {
      unframe();
      const frameEl = document.createElement('div');
      frameEl.className = 'pic-frame';
      frameEl.innerHTML = '<span class="pic-size"></span><button type="button" class="pic-handle" aria-label="Resize picture: drag, or use the arrow keys" title="Drag to resize · double-click to reset"></button>';
      container.appendChild(frameEl);
      img.classList.add('picked');
      framed = { img, frame: frameEl };
      placeFrame();
      const handle = frameEl.querySelector('.pic-handle');
      handle.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        handle.setPointerCapture(e.pointerId);
        const startX = e.clientX;
        const startWidth = innerWidth(img);
        const most = Math.max(48, (img.closest('.blk') || container).clientWidth);
        let width = startWidth;
        const move = (m) => {
          if (!(m.buttons & 1)) return up();
          width = Math.round(Math.min(most, Math.max(48, startWidth + m.clientX - startX)));
          img.style.width = `${width}px`;
          placeFrame();
        };
        let done = false;
        const up = () => {
          if (done) return;
          done = true;
          handle.removeEventListener('pointermove', move);
          handle.removeEventListener('pointerup', up);
          handle.removeEventListener('pointercancel', up);
          handle.removeEventListener('lostpointercapture', up);
          if (handle.hasPointerCapture(e.pointerId)) handle.releasePointerCapture(e.pointerId);
          if (Math.abs(width - startWidth) >= 2) resizePicture(img, width);
        };
        handle.addEventListener('pointermove', move);
        handle.addEventListener('pointerup', up);
        handle.addEventListener('pointercancel', up);
        handle.addEventListener('lostpointercapture', up);
      });
      handle.addEventListener('dblclick', (e) => {
        e.preventDefault();
        e.stopPropagation();
        resizePicture(img, 0);
      });
      handle.addEventListener('keydown', (e) => {
        if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
        e.preventDefault();
        const now = innerWidth(img);
        resizePicture(img, Math.max(48, Math.round(now + (e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 100 : 20))));
      });
    }

    function resizePicture(img, width) {
      const at = pictureAt(img);
      if (!at || at.index < 0) return;
      if (editing) stop();
      const before = lines.slice(at.start, at.start + at.length).join('\n');
      const after = ed.setPictureWidth(before, at.index, width);
      if (after === before) return;
      lines.splice(at.start, at.length, ...after.split('\n'));
      changed();
      draw();
      const blk = container.querySelector(`.blk[data-start="${at.start}"]`);
      const again = blk && blk.querySelectorAll('img.note-img')[at.index];
      if (!again) return;
      const show = () => {
        frame(again);
        framed.frame.querySelector('.pic-handle').focus({ preventScroll: true });
      };
      if (again.complete) show();
      else again.addEventListener('load', show, { once: true });
    }

    container.addEventListener('dblclick', (e) => {
      const img = e.target.closest('img.note-img');
      if (!img || img.closest('.editing')) return;
      const blk = img.closest('.blk');
      unframe();
      if (blk) editLine(shifted(Number(blk.dataset.start)), 'end');
    });

    function onFrameKey(e) {
      if (e.key !== 'Escape' || !framed || !framed.frame.isConnected) return;
      e.preventDefault();
      e.stopPropagation();
      unframe();
    }
    document.addEventListener('keydown', onFrameKey, true);
    root.addEventListener('resize', placeFrame);

    container.addEventListener('click', (e) => {
      pressed = false;
      clearTimeout(release);
      if (extended) {
        extended = false;
        return;
      }
      if (e.target.closest('.pic-frame')) return;
      const pic = e.target.closest('img.note-img');
      if (pic && !pic.closest('.editing') && !e.shiftKey) {
        e.preventDefault();
        frame(pic);
        return;
      }
      unframe();
      if (!e.target.closest('input[data-box], a') && takeSelection()) {
        e.preventDefault();
        return;
      }
      const box = boxFor(e);
      if (box) {
        e.preventDefault();
        const number = Number(box.dataset.box);
        stop();
        const line = ed.boxLine(lines, number);
        if (line >= 0) {
          lines[line] = ed.toggleBox(lines[line]);
          changed();
          draw();
        }
        return;
      }
      if (e.target.closest('summary')) return;
      if (e.target.closest('label.task')) e.preventDefault();
      const blk = e.target.closest('.blk');
      const tail = !blk && e.target.closest('.doc-tail');
      if (e.target.closest('a') || (!blk && !tail) || (blk && blk.classList.contains('editing'))) {
        settle();
        return;
      }
      if (tail) {
        stop();
        if (lines[lines.length - 1].trim()) {
          lines.push('');
          changed();
          draw();
        }
        editLine(lines.length - 1, 'end');
        return;
      }
      const block = blocks[Number(blk.dataset.i)];
      const prefix = block && block.kind === 'line' ? caretIn(blk, e.clientX, e.clientY) : null;
      const line = shifted(Number(blk.dataset.start));
      stop();
      const raw = lines[line] || '';
      const caret = prefix !== null && raw.trim() ? ed.rawOffset(raw, prefix) : 'end';
      editLine(line, caret);
    });

    draw();

    function insert(text) {
      if (editing) {
        const { area } = editing;
        const at = area.selectionStart;
        const before = area.value.slice(0, at);
        const lead = before && !before.endsWith('\n') ? '\n' : '';
        area.setRangeText(`${lead}${text}\n`, at, area.selectionEnd, 'end');
        onInput();
        return;
      }
      const last = lines[lines.length - 1];
      if (last.trim()) lines.push('');
      if (lines.length === 1 && !lines[0].trim()) lines[0] = text;
      else lines.push(text);
      changed();
      draw();
    }

    const pictures = (list) => [...(list || [])].filter((f) => /^image\//.test(f.type));
    function onPaste(e) {
      const found = pictures(e.clipboardData && e.clipboardData.files);
      if (!onPictures || !found.length) return;
      e.preventDefault();
      onPictures(found);
    }
    function onDrop(e) {
      const found = pictures(e.dataTransfer && e.dataTransfer.files);
      if (!onPictures || !found.length) return;
      e.preventDefault();
      e.stopPropagation();
      onPictures(found);
    }
    container.addEventListener('paste', onPaste);
    container.addEventListener('drop', onDrop);

    return {
      destroy: () => {
        stop();
        clearTimeout(outlineTimer);
        cancelAnimationFrame(outlineFrame);
        if (outlineResize) outlineResize.disconnect();
        if (outlineNav) outlineNav.removeEventListener('click', outlineGo);
        root.removeEventListener('scroll', outlineScroll);
        root.removeEventListener('resize', outlineScroll);
        document.removeEventListener('selectionchange', onSelectionChange);
        clearTimeout(release);
        document.removeEventListener('pointerup', onPointerUp);
        document.removeEventListener('keydown', onDocumentKey);
        document.removeEventListener('keydown', onFrameKey, true);
        root.removeEventListener('resize', placeFrame);
        unframe();
        container.removeEventListener('paste', onPaste);
        container.removeEventListener('drop', onDrop);
      },
      insert,
      source: () => lines.join('\n'),
      stop,
      isEditing: () => editing !== null,
      selection() {
        const live = editing && liveSelection();
        if (live) return live;
        const span = selectedSpan();
        if (span) return ed.snapshotOf(lines, span[0], span[1]);
        return kept.picked && kept.text === lines.join('\n') ? kept.picked : null;
      },
      selectionBox() {
        if (editing && document.activeElement === editing.area && editing.area.selectionStart !== editing.area.selectionEnd) return caretBox(editing.area, editing.area.selectionEnd);
        const picked = root.getSelection ? root.getSelection() : null;
        if (!picked || picked.isCollapsed || !picked.rangeCount || !selectedSpan()) return null;
        const box = picked.getRangeAt(0).getBoundingClientRect();
        return { x: box.right, y: box.bottom };
      },
      caretLine() {
        if (editing) return lineOf(lineStart(editing.start) + editing.area.selectionEnd);
        return kept.text === lines.join('\n') ? kept.caret : null;
      },
      highlight(from, to) {
        const hits = [...container.querySelectorAll('.blk')].filter((el) => {
          const block = blocks[Number(el.dataset.i)];
          return block && block.start < to && block.end >= from;
        });
        if (!hits.length) return false;
        for (const el of container.querySelectorAll('.blk-cited')) el.classList.remove('blk-cited');
        for (const el of hits) el.classList.add('blk-cited');
        hits[0].scrollIntoView({ block: 'center', behavior: 'smooth' });
        setTimeout(() => hits.forEach((el) => el.classList.remove('blk-cited')), CITED_FOR);
        return true;
      },
      editStart: () => editLine(0, 'start'),
      editEnd: () => editLine(lines.length - 1, 'end'),
    };
  }

  const CITED_FOR = 4000;

  root.leoDoc = { mount };
})(window);
