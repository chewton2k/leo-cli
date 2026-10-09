(function (root) {
  'use strict';

  const md = root.leoMarkdown;
  const ed = root.leoEditing;

  function mount(container, { source, onChange, placeholder, dir = '', onPictures = null }) {
    let lines = String(source || '').replace(/\r\n?/g, '\n').split('\n');
    let blocks = [];
    let editing = null;

    const changed = () => onChange(lines.join('\n'));

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
    }

    function blockAtLine(line) {
      return blocks.findIndex((b) => line >= b.start && line < b.end);
    }

    function fit(area) {
      area.style.height = 'auto';
      area.style.height = `${area.scrollHeight}px`;
    }

    function stop() {
      if (!editing) return;
      const { area } = editing;
      area.removeEventListener('input', onInput);
      area.removeEventListener('keydown', onKey);
      area.removeEventListener('blur', onBlur);
      editing = null;
      draw();
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

      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a' && area.selectionStart === 0 && area.selectionEnd === value.length && (start > 0 || editing.end < lines.length)) {
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

    container.addEventListener('click', (e) => {
      pressed = false;
      clearTimeout(release);
      if (extended) {
        extended = false;
        return;
      }
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
        clearTimeout(release);
        document.removeEventListener('pointerup', onPointerUp);
        document.removeEventListener('keydown', onDocumentKey);
        container.removeEventListener('paste', onPaste);
        container.removeEventListener('drop', onDrop);
      },
      insert,
      source: () => lines.join('\n'),
      stop,
      isEditing: () => editing !== null,
      editStart: () => editLine(0, 'start'),
      editEnd: () => editLine(lines.length - 1, 'end'),
    };
  }

  root.leoDoc = { mount };
})(window);
