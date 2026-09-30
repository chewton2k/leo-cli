(function (root) {
  'use strict';

  const md = root.leoMarkdown;
  const ed = root.leoEditing;

  function mount(container, { source, onChange, placeholder }) {
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
      if (editing) stop();
      const block = blocks[index];
      if (!block) return;
      const el = container.querySelector(`.blk[data-i="${index}"]`);
      if (!el) return;
      const raw = lines.slice(block.start, block.end).join('\n');
      const area = document.createElement('textarea');
      area.className = 'line-edit';
      area.value = raw;
      area.rows = 1;
      area.spellcheck = true;
      area.setAttribute('autocapitalize', 'sentences');
      el.removeAttribute('style');
      el.replaceChildren(area);
      el.classList.add('editing');
      editing = { index, start: block.start, end: block.end, originalEnd: block.end, kind: block.kind, area };
      fit(area);
      area.addEventListener('input', onInput);
      area.addEventListener('keydown', onKey);
      area.addEventListener('blur', onBlur);
      area.focus({ preventScroll: true });
      const at = caret === 'start' ? 0 : caret === 'end' || caret == null ? raw.length : Math.max(0, Math.min(caret, raw.length));
      area.setSelectionRange(at, at);
      area.scrollIntoView({ block: 'nearest' });
    }

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

      if (e.key === 'ArrowUp' && collapsed && (pos === 0 || (kind === 'line' && singleRow(area))) && editing.index > 0) {
        e.preventDefault();
        const target = editing.index - 1;
        stop();
        edit(target, 'end');
        return;
      }

      if (e.key === 'ArrowDown' && collapsed && (pos === value.length || (kind === 'line' && singleRow(area))) && editing.index < blocks.length - 1) {
        e.preventDefault();
        const target = editing.index + 1;
        stop();
        edit(target, 'start');
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

    container.addEventListener('pointerdown', () => {
      pressed = true;
      clearTimeout(release);
    });

    container.addEventListener('pointercancel', () => {
      pressed = false;
      settle();
    });

    document.addEventListener('pointerup', () => {
      if (!pressed) return;
      clearTimeout(release);
      release = setTimeout(() => {
        pressed = false;
        settle();
      }, 400);
    });

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

    return {
      source: () => lines.join('\n'),
      stop,
      isEditing: () => editing !== null,
      editStart: () => editLine(0, 'start'),
      editEnd: () => editLine(lines.length - 1, 'end'),
    };
  }

  root.leoDoc = { mount };
})(window);
