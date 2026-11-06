const MARGIN_KINDS = ['add_margin_note', 'remove_margin_note'];
const READ_PAUSE = { regular: 4000, slow: 8000 };
const READ_GAP_SECONDS = 90;
const READ_FACE_AFTER_MS = 200;
const MARGIN_SETTLE_MS = 1400;
const MARGIN_LAYOUT_KEY = 'leo-margin-layout';
const GLOW_MS = 2200;
const NO_COMMENT_MS = 4000;
const WIPE_MS = 500;
const MENU_MS = 170;
const MENU_SLIDE = [{ clipPath: 'inset(0 0 100% 0)', opacity: 0, transform: 'translateY(-4px)' }, { clipPath: 'inset(0 0 0 0)', opacity: 1, transform: 'none' }];
const MOST_ASKED = 600;
const marginKit = window.leoMargin;
const LEAST_ASKED = 3;
const marginPage = { adding: new Set(), again: false, s: null, mode: 'ask', threads: [], layer: null, svg: null, els: new Map(), paths: new Map(), marks: new Map(), rects: new Map(), open: null, hover: null, focus: null, draft: null, arriving: new Set(), timer: 0, reading: false, faceTimer: 0, status: '', brief: false, statusTimer: 0, statusToken: null, slide: null, gutter: 0, pointer: 0, changing: false, baseline: [], nextRead: 0, observer: null, frame: 0, roomy: null };
let marginPrefs = null;

function marginRemembered() {
  try {
    return localStorage.getItem(MARGIN_LAYOUT_KEY);
  } catch (e) {
    return null;
  }
}

function marginRemember(prefs) {
  marginPrefs = prefs;
  try {
    localStorage.setItem(MARGIN_LAYOUT_KEY, prefs.layout);
  } catch (e) {}
}

function marginLayout(columns) {
  return marginKit.arrangement({ width: columns.clientWidth, setting: marginPrefs?.layout || marginRemembered() || 'both', noteWidth: window.leoTheme.noteWidth(), panel: chat.isOpen() });
}

async function marginLoadPrefs() {
  if (marginPrefs) return marginPrefs;
  try {
    marginRemember(await api('/api/margin'));
  } catch (e) {
    return { layout: marginRemembered() || 'both', pace: 'regular' };
  }
  return marginPrefs;
}

function marginLeave() {
  clearTimeout(marginPage.timer);
  clearTimeout(marginPage.faceTimer);
  clearTimeout(marginPage.statusTimer);
  cancelAnimationFrame(marginPage.frame);
  cancelAnimationFrame(marginPage.pointer);
  marginPage.slide?.cancel();
  marginPage.observer?.disconnect();
  marginPage.observer = null;
  const columns = marginPage.layer?.parentElement;
  columns?.style.removeProperty('--anchor-gutter');
  marginPage.layer?.remove();
  marginPage.roomy?.classList.remove('margin-room');
  chat.setLost([]);
  Object.assign(marginPage, { loaded: false, s: null, mode: 'ask', threads: [], layer: null, svg: null, open: null, hover: null, focus: null, draft: null, reading: false, status: '', brief: false, statusToken: null, slide: null, gutter: 0, pointer: 0, changing: false, baseline: [], roomy: null });
  marginPage.els.clear();
  marginPage.paths.clear();
  marginPage.marks.clear();
  marginPage.rects.clear();
  marginPage.arriving.clear();
}

async function marginMount(s, mine) {
  marginLeave();
  const columns = $('.note-columns');
  if (!columns) return;
  const layer = document.createElement('div');
  layer.className = 'margin-layer';
  layer.setAttribute('aria-label', 'Felix’s margin notes');
  layer.innerHTML = '<svg class="margin-ink" aria-hidden="true"></svg><div class="margin-said-aloud" aria-live="polite"></div>';
  columns.appendChild(layer);
  columns.dataset.margin = marginLayout(columns);
  Object.assign(marginPage, { s, layer, svg: layer.firstChild, baseline: marginKit.blocksOf(s.doc.source()), nextRead: 0 });
  layer.addEventListener('click', marginClick);
  layer.addEventListener('keydown', marginKey);
  layer.addEventListener('focusin', marginFocus);
  layer.addEventListener('focusout', marginFocus);
  layer.addEventListener('submit', (e) => marginSubmit(e).catch(fail));
  if (window.ResizeObserver) {
    marginPage.observer = new ResizeObserver(() => marginRedraw());
    marginPage.observer.observe(columns);
    marginPage.observer.observe($('#doc'));
  }
  if (s.note.id) await marginLoad(s, () => mine === seq);
}

async function marginLoad(s, current = () => true) {
  let view = null;
  try {
    [, view] = await Promise.all([marginLoadPrefs(), api(`/api/notes/${enc(s.note.id)}/margin`)]);
  } catch (e) {
    if (e instanceof Locked) throw e;
    return;
  }
  if (!current() || marginPage.s !== s || !view) return;
  marginPage.mode = view.mode;
  marginPage.threads = view.threads || [];
  marginControl();
  marginRedraw();
}

function marginSetStatus(text, brief = false) {
  clearTimeout(marginPage.statusTimer);
  const token = {};
  const s = marginPage.s;
  marginPage.status = text;
  marginPage.statusToken = token;
  marginPage.brief = Boolean(brief && text);
  const status = $('#margin-status');
  if (status) status.textContent = text;
  if (!marginPage.brief) return;
  marginPage.statusTimer = setTimeout(() => {
    if (marginPage.s === s && marginPage.statusToken === token && marginPage.status === text) marginSetStatus('');
  }, NO_COMMENT_MS);
}

function marginAskKeys() {
  return ON_MAC ? 'Cmd+Shift+E' : 'Ctrl+Shift+E';
}

function marginMenuSlide(menu, open) {
  let run = marginPage.slide;
  menu.inert = !open;
  if (stillMotion() || typeof menu.animate !== 'function') {
    run?.cancel();
    marginPage.slide = null;
    menu.hidden = !open;
    return;
  }
  if (!run) {
    if (open === !menu.hidden) return;
    run = menu.animate(MENU_SLIDE, { duration: MENU_MS, easing: 'cubic-bezier(.2, .8, .2, 1)', fill: 'both' });
    run.pause();
    run.currentTime = open ? 0 : MENU_MS;
    marginPage.slide = run;
    run.onfinish = () => {
      const shown = run.playbackRate > 0;
      run.onfinish = null;
      run.cancel();
      if (marginPage.slide === run) marginPage.slide = null;
      menu.hidden = !shown;
    };
  }
  menu.hidden = false;
  run.playbackRate = open ? 1 : -1;
  run.play();
}

function marginModeMenu(open, focus = false) {
  const menu = $('#margin-mode-menu');
  const button = $('#margin-mode');
  if (!menu || !button) return;
  button.setAttribute('aria-expanded', String(open));
  marginMenuSlide(menu, open);
  if (focus) {
    if (open) menu.querySelector('[aria-checked="true"]')?.focus({ preventScroll: true });
    else button.focus({ preventScroll: true });
  }
}

function marginControl() {
  const s = marginPage.s;
  const button = $('#margin-mode');
  const menu = $('#margin-mode-menu');
  if (!s || !button || !menu || !s.note.id) return;
  if (!button.dataset.ready) {
    button.dataset.ready = '1';
    button.addEventListener('click', () => marginModeMenu(button.getAttribute('aria-expanded') !== 'true', true));
    menu.addEventListener('click', (e) => {
      const option = e.target.closest('[data-margin-mode]');
      if (option) marginChangeMode(option.dataset.marginMode).catch(fail);
    });
    button.parentElement.addEventListener('keydown', (e) => {
      if (e.key === 'Tab' && !menu.hidden) {
        const wrap = button.parentElement;
        const controls = [...document.querySelectorAll('a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [contenteditable], [tabindex]:not([tabindex="-1"])')].filter(el => el.getClientRects().length && (el === button || !wrap.contains(el)) && !el.closest('[inert]'));
        const next = controls[controls.indexOf(button) + (e.shiftKey ? -1 : 1)];
        marginModeMenu(false);
        if (next) { e.preventDefault(); next.focus({ preventScroll: true }); }
      } else if (e.key === 'Escape' && !menu.hidden) {
        e.preventDefault();
        e.stopPropagation();
        marginModeMenu(false, true);
      } else if ((e.key === 'Enter' || e.key === ' ') && e.target.closest('[data-margin-mode]')) {
        e.preventDefault();
        e.stopPropagation();
        e.target.closest('[data-margin-mode]').click();
      } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
        e.preventDefault();
        e.stopPropagation();
        const items = [...menu.querySelectorAll('[data-margin-mode]')];
        const at = items.indexOf(document.activeElement);
        marginModeMenu(true);
        const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : (at + (e.key === 'ArrowUp' ? items.length - 1 : 1)) % items.length;
        items[next]?.focus({ preventScroll: true });
      }
    });
    button.parentElement.addEventListener('focusout', (e) => {
      if (!menu.hidden && e.relatedTarget && !button.parentElement.contains(e.relatedTarget)) marginModeMenu(false);
    });
  }
  const along = marginPage.mode === 'along';
  button.disabled = marginPage.changing;
  button.dataset.mode = marginPage.mode;
  const icon = $('#margin-mode-icon');
  if (icon) icon.innerHTML = along ? ICON.pen : ICON.chat;
  $('#margin-mode-label').textContent = along ? 'Reads along' : 'When you ask';
  button.title = along ? 'Reads changes after a pause. The text goes to your writing AI.' : `Select a passage, then choose Ask Felix or press ${marginAskKeys()}.`;
  for (const option of menu.querySelectorAll('[data-margin-mode]')) {
    option.setAttribute('aria-checked', String(option.dataset.marginMode === marginPage.mode));
    option.disabled = marginPage.changing;
  }
  marginPage.loaded = true;
}

async function marginChangeMode(mode) {
  const s = marginPage.s;
  if (!s || !s.note.id || marginPage.changing || !['ask', 'along'].includes(mode)) return;
  marginModeMenu(false, true);
  if (mode === marginPage.mode) return;
  marginPage.changing = true;
  marginControl();
  try {
    const done = await api(`/api/notes/${enc(s.note.id)}/margin`, { method: 'POST', body: { mode } });
    if (marginPage.s !== s) return;
    marginPage.mode = done.mode;
    marginPage.baseline = marginKit.blocksOf(s.doc.source());
    clearTimeout(marginPage.timer);
    clearTimeout(marginPage.faceTimer);
    marginSetStatus('');
    marginRedraw();
  } catch (e) {
    if (marginPage.s !== s) return;
    if (e instanceof Locked) throw e;
    marginSetStatus('Could not save this mode. Try again.');
  } finally {
    if (marginPage.s === s) {
      marginPage.changing = false;
      marginControl();
      if (document.activeElement === document.body) $('#margin-mode')?.focus({ preventScroll: true });
    }
  }
}

function marginRedraw() {
  if (!marginPage.layer) return;
  cancelAnimationFrame(marginPage.frame);
  marginPage.frame = requestAnimationFrame(marginDraw);
}

function marginNow(t) {
  const felixSaid = [...(t.messages || [])].reverse().find((m) => m.from === 'felix');
  return felixSaid ? felixSaid.text : '';
}

function inkWords(box, text) {
  box.textContent = '';
  if (stillMotion()) {
    box.textContent = text;
    return;
  }
  const parts = text.split(/(\s+)/);
  const count = parts.filter((part) => part.trim()).length;
  let at = 0;
  for (const part of parts) {
    if (!part.trim()) {
      box.append(part);
      continue;
    }
    const span = document.createElement('span');
    span.className = 'ink-word';
    span.style.animationDelay = `${marginKit.wordDelay(at++, count)}ms`;
    span.textContent = part;
    box.append(span);
  }
}

function marginAnswer(line, text, arriving = false) {
  const md = window.leoMarkdown;
  const html = md.render(text, { interactive: false, dir: marginPage.s?.note.directory || '' });
  const shape = document.createElement('div');
  shape.innerHTML = html;
  const only = shape.firstElementChild;
  if (shape.childElementCount !== 1 || only.tagName !== 'P' || only.childElementCount) {
    line.classList.add('prose');
    line.innerHTML = html;
  } else if (arriving) inkWords(line, text);
  else line.textContent = text;
}

function marginElement(t) {
  const el = document.createElement('article');
  const more = `margin-more-${t.id}`;
  el.className = 'margin-note';
  el.dataset.id = t.id;
  el.setAttribute('aria-label', `Felix, about “${t.quote.slice(0, 80)}”`);
  el.innerHTML = `<button type="button" class="margin-toggle" aria-expanded="false" aria-controls="${esc(more)}"></button><div class="margin-said"></div><div class="margin-more" id="${esc(more)}" aria-hidden="true" inert><div class="margin-more-body">
    <form class="margin-reply"><textarea rows="1" maxlength="1000" aria-label="Reply to Felix" placeholder="Reply"></textarea></form>
    <p class="margin-problem" role="status" hidden></p>
    <div class="margin-acts"><button type="button" class="margin-add" data-margin-act="add">Add to note</button><button type="button" class="msg-act" data-margin-act="chat">Ask in chat</button><button type="button" class="msg-act" data-margin-act="resolve">Resolve</button><button type="button" class="msg-act" data-margin-act="dismiss">Dismiss</button></div></div></div>`;
  marginPage.layer.appendChild(el);
  marginPage.els.set(t.id, el);
  marginPage.observer?.observe(el);
  return el;
}

function marginFill(el, t, arriving) {
  const said = $('.margin-said', el);
  const open = marginPage.open === t.id;
  const key = `${open}:${(t.messages || []).length}`;
  if (el.dataset.drawn !== key) {
    el.dataset.drawn = key;
    said.textContent = '';
    if (!open) {
      const line = document.createElement('div');
      line.className = 'margin-felix';
      marginAnswer(line, marginNow(t), arriving);
      said.append(line);
    } else {
      for (const message of t.messages || []) {
        const line = document.createElement('div');
        line.className = message.from === 'felix' ? 'margin-felix' : 'margin-you';
        if (message.from === 'felix') marginAnswer(line, message.text);
        else line.textContent = message.text;
        said.append(line);
      }
    }
  }
  const draft = t.id === 'draft';
  const more = $('.margin-more', el);
  if (open && !el.hasAttribute('data-open')) more.getBoundingClientRect();
  more.inert = !open;
  more.setAttribute('aria-hidden', String(!open));
  el.toggleAttribute('data-open', open);
  const toggle = $('.margin-toggle', el);
  toggle.setAttribute('aria-expanded', String(open));
  toggle.setAttribute('aria-label', `${open ? 'Close' : 'Open'} Felix’s note on “${t.quote.slice(0, 80)}”`);
  $('.margin-add', el).hidden = draft || !marginNow(t);
  $('[data-margin-act="chat"]', el).hidden = !draft;
  $('[data-margin-act="resolve"]', el).hidden = draft;
  $('[data-margin-act="dismiss"]', el).textContent = draft ? 'Cancel' : 'Dismiss';
  $('textarea', el).placeholder = el.getAttribute('aria-busy') === 'true' ? 'Felix is thinking…' : draft ? 'Ask Felix about this' : 'Reply';
  el.toggleAttribute('data-active', marginLit(t.id));
}

function marginLit(id) {
  return marginPage.open === id || marginPage.hover === id || marginPage.focus === id;
}

function marginPath(id, kind, line = 0) {
  const key = `${id}:${kind}:${line}`;
  let path = marginPage.paths.get(key);
  if (!path) {
    path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('class', kind);
    path.dataset.id = id;
    path.dataset.line = String(line);
    marginPage.svg.appendChild(path);
    marginPage.paths.set(key, path);
  }
  return path;
}

function marginThreadPaths(id) {
  return [...marginPage.paths].filter(([key]) => key.startsWith(`${id}:`));
}

function marginDropThread(id) {
  const el = marginPage.els.get(id);
  if (el) marginPage.observer?.unobserve(el);
  el?.remove();
  marginPage.els.delete(id);
  marginPage.marks.get(id)?.remove();
  marginPage.marks.delete(id);
  marginPage.rects.delete(id);
  for (const [key, path] of marginThreadPaths(id)) {
    path.remove();
    marginPage.paths.delete(key);
  }
  if (marginPage.open === id) marginPage.open = null;
  if (marginPage.hover === id) marginPage.hover = null;
  if (marginPage.focus === id) marginPage.focus = null;
}

function marginMark(t) {
  let mark = marginPage.marks.get(t.id);
  if (!mark) {
    mark = document.createElement('button');
    mark.type = 'button';
    mark.className = 'margin-mark';
    mark.dataset.id = t.id;
    mark.setAttribute('aria-label', `Felix’s note on “${t.quote.slice(0, 80)}”`);
    mark.innerHTML = '<span class="margin-anchor" aria-hidden="true"></span>';
    marginPage.layer.appendChild(mark);
    marginPage.marks.set(t.id, mark);
  }
  return mark;
}

function marginPaint() {
  for (const [key, path] of marginPage.paths) path.classList.toggle('on', marginLit(key.slice(0, key.indexOf(':'))));
  for (const [id, el] of marginPage.els) el.toggleAttribute('data-active', marginLit(id));
  for (const [id, mark] of marginPage.marks) mark.toggleAttribute('data-active', marginLit(id));
}

function marginInk(t, rects) {
  const id = t.id;
  const wash = marginPath(id, 'wash');
  wash.setAttribute('d', rects.map((r) => `M${(r.left - 2).toFixed(1)} ${(r.top + 1).toFixed(1)} H${(r.right + 2).toFixed(1)} V${(r.bottom + 3).toFixed(1)} H${(r.left - 2).toFixed(1)} Z`).join(' '));
  const arriving = marginPage.arriving.has(id) && !stillMotion();
  const total = rects.reduce((sum, r) => sum + Math.max(1, r.right - r.left), 0);
  let before = 0;
  rects.forEach((r, line) => {
    const dots = marginPath(id, 'squiggle', line);
    dots.setAttribute('d', `M${r.left.toFixed(1)} ${(r.bottom + 4).toFixed(1)} H${r.right.toFixed(1)}`);
    const width = Math.max(1, r.right - r.left);
    if (arriving && !dots.classList.contains('arriving')) {
      dots.style.setProperty('--wipe-d', `${Math.round((WIPE_MS * before) / total)}ms`);
      dots.style.setProperty('--wipe-t', `${Math.max(1, Math.round((WIPE_MS * width) / total))}ms`);
      dots.classList.add('arriving');
    }
    before += width;
  });
  for (const [key, path] of marginThreadPaths(id)) {
    if (path.classList.contains('squiggle') && Number(path.dataset.line) >= rects.length) {
      path.remove();
      marginPage.paths.delete(key);
    }
  }
  marginPath(id, 'solid').setAttribute('d', rects.map((r) => `M${r.left.toFixed(1)} ${(r.bottom + 4).toFixed(1)} H${r.right.toFixed(1)}`).join(' '));
}

function marginGutter(columns, count) {
  const had = marginPage.gutter;
  if (had) columns.style.removeProperty('--anchor-gutter');
  let need = 0;
  if (count) {
    const hit = matchMedia('(pointer: coarse)').matches ? 44 : 24;
    const edge = (columns.closest('main') || document.body).getBoundingClientRect().left;
    need = Math.max(0, hit - ($('article.note', columns).getBoundingClientRect().left - edge));
  }
  if (need) columns.style.setProperty('--anchor-gutter', `${need}px`);
  marginPage.gutter = need;
}

function marginDraw() {
  const { s, layer } = marginPage;
  if (!s || !layer) return;
  if (!layer.isConnected) return marginLeave();
  const columns = layer.parentElement;
  const source = s.doc.source();
  const threads = marginPage.draft ? [...marginPage.threads, marginPage.draft] : marginPage.threads;
  const placed = [];
  const lost = [];
  for (const t of threads) {
    const at = marginKit.find(source, t);
    if (at) placed.push({ t, at });
    else if (t.id !== 'draft') lost.push(t);
  }
  chat.setLost(lost);
  const layout = marginLayout(columns);
  if (columns.dataset.margin !== layout) columns.dataset.margin = layout;
  const margins = layout === 'right' || layout === 'both';
  marginGutter(columns, margins ? 0 : 1);
  const keep = new Set(placed.map(({ t }) => t.id));
  for (const id of [...marginPage.els.keys(), ...marginPage.marks.keys()]) if (!keep.has(id)) marginDropThread(id);
  const origin = columns.getBoundingClientRect();
  const page = $('article.note', columns).getBoundingClientRect();
  const boxes = placed.map(({ t, at }) => {
    const rects = s.doc.spanRects(at.from, at.to).map((r) => ({ left: r.left - origin.left, right: r.right - origin.left, top: r.top - origin.top, bottom: r.bottom - origin.top }));
    const el = marginPage.els.get(t.id) || (margins || marginPage.open === t.id ? marginElement(t) : null);
    if (el) marginFill(el, t, marginPage.arriving.has(t.id));
    return { t, at, rects, el };
  }).filter(({ rects }) => rects.length);
  marginPage.roomy?.classList.remove('margin-room');
  marginPage.roomy = null;
  if (margins) marginPlaceInMargin(boxes, layout, page, origin);
  else marginPlaceAsMarks(boxes, page, origin);
  for (const { t, rects } of boxes) {
    marginPage.rects.set(t.id, rects);
    marginInk(t, rects);
  }
  marginPaint();
  if (marginPage.arriving.size && !stillMotion()) {
    const done = [...marginPage.arriving];
    setTimeout(() => {
      for (const id of done) {
        marginPage.arriving.delete(id);
        for (const [, path] of marginThreadPaths(id)) path.classList.remove('arriving');
        marginPage.els.get(id)?.classList.remove('arriving');
      }
    }, MARGIN_SETTLE_MS);
  } else marginPage.arriving.clear();
}

function marginPlaceInMargin(boxes, layout, page, origin) {
  const left = page.left - origin.left;
  const right = page.right - origin.left;
  const items = boxes.map(({ t, rects, el }) => ({ id: t.id, want: rects[0].top - 2, height: el.offsetHeight }));
  const side = layout === 'both' ? marginKit.sides(items) : Object.fromEntries(items.map((item) => [item.id, 'right']));
  const pinned = marginPage.open;
  const tops = { ...marginKit.stack(items.filter((i) => side[i.id] === 'right'), { pinned }), ...marginKit.stack(items.filter((i) => side[i.id] === 'left'), { pinned }) };
  for (const { t, el } of boxes) {
    const on = side[t.id];
    const x = on === 'right' ? right + marginKit.MARGIN_GAP : left - marginKit.MARGIN_GAP - marginKit.MARGIN_WIDTH;
    el.hidden = false;
    el.removeAttribute('data-inline');
    el.style.transform = `translate(${x}px, ${tops[t.id]}px)`;
    el.style.width = '';
    el.classList.toggle('arriving', marginPage.arriving.has(t.id));
    marginPage.marks.get(t.id)?.remove();
    marginPage.marks.delete(t.id);
  }
}

function marginPlaceAsMarks(boxes, page, origin) {
  const marks = boxes.map(({ t, rects }) => {
    const mark = marginMark(t);
    const center = (rects[0].top + rects[rects.length - 1].bottom) / 2;
    return { id: t.id, mark, want: center - mark.offsetHeight / 2, height: mark.offsetHeight };
  });
  const tops = marginKit.stack(marks, { gap: 0 });
  for (const { id, mark } of marks) mark.style.transform = `translate(${page.left - origin.left - mark.offsetWidth}px, ${tops[id]}px)`;
  for (const { t, at, el } of boxes) {
    if (!el) continue;
    const open = marginPage.open === t.id;
    el.hidden = !open;
    if (!open) continue;
    const block = marginPage.s.doc.blockAt(at.to);
    if (!block) continue;
    el.setAttribute('data-inline', '');
    el.style.width = `${Math.min(page.width, 520)}px`;
    const room = el.offsetHeight + 28;
    block.style.setProperty('--margin-room', `${room}px`);
    block.classList.add('margin-room');
    marginPage.roomy = block;
    el.style.transform = `translate(${page.left - origin.left}px, ${block.getBoundingClientRect().bottom - origin.top + 14}px)`;
  }
}

function marginHover(e) {
  if (!marginPage.layer || e.pointerType === 'touch' || marginPage.pointer) return;
  const { clientX, clientY, target } = e;
  marginPage.pointer = requestAnimationFrame(() => {
    marginPage.pointer = 0;
    if (!marginPage.layer) return;
    const origin = marginPage.layer.parentElement.getBoundingClientRect();
    const x = clientX - origin.left;
    const y = clientY - origin.top;
    let id = target?.closest?.('.margin-note, .margin-mark')?.dataset.id || null;
    if (!id) {
      for (const [key, rects] of marginPage.rects) {
        if (rects.some((r) => x >= r.left - 2 && x <= r.right + 2 && y >= r.top && y <= r.bottom + 5)) {
          id = key;
          break;
        }
      }
    }
    if (id !== marginPage.hover) {
      marginPage.hover = id;
      marginPaint();
    }
  });
}

function marginFocus(e) {
  const id = e.type === 'focusin' ? e.target.closest?.('[data-id]')?.dataset.id || null : null;
  if (e.type === 'focusout' && e.relatedTarget && marginPage.layer?.contains(e.relatedTarget)) return;
  if (id === marginPage.focus) return;
  marginPage.focus = id;
  marginPaint();
}

document.addEventListener('pointermove', marginHover, { passive: true });
document.documentElement.addEventListener('pointerleave', () => {
  if (!marginPage.hover) return;
  marginPage.hover = null;
  marginPaint();
});

function marginThread(id) {
  if (marginPage.draft?.id === id) return marginPage.draft;
  return marginPage.threads.find((t) => t.id === id) || null;
}

function marginOpen(id, focus = false) {
  marginPage.open = id;
  marginDraw();
  if (focus) $('textarea', marginPage.els.get(id))?.focus({ preventScroll: true });
}

function marginClose() {
  const id = marginPage.open;
  if (!id) return;
  marginPage.open = null;
  if (id === 'draft') marginPage.draft = null;
  marginDraw();
  (marginPage.marks.get(id) || $('.margin-toggle', marginPage.els.get(id)))?.focus();
}

function marginClick(e) {
  const action = e.target.closest('[data-margin-act]');
  const id = e.target.closest('[data-id]')?.dataset.id;
  if (!id) return;
  if (action) {
    const t = marginThread(id);
    if (!t) return;
    if (action.dataset.marginAct === 'add') marginAdd(t).catch(fail);
    else if (action.dataset.marginAct === 'chat') marginToChat(t);
    else if (action.dataset.marginAct === 'resolve') marginStatus(t, 'resolved').catch(fail);
    else if (t.id === 'draft') marginClose();
    else marginStatus(t, 'dismissed').catch(fail);
    return;
  }
  if (marginPage.open !== id) marginOpen(id, true);
  else if (e.target.closest('.margin-toggle')) marginClose();
}

function marginKey(e) {
  if (e.key === 'Escape' && marginPage.open) {
    e.preventDefault();
    e.stopPropagation();
    marginClose();
  } else if (e.key === 'Enter' && !e.shiftKey && e.target.matches('.margin-reply textarea')) {
    e.preventDefault();
    e.target.form.requestSubmit();
  }
}

document.addEventListener('pointerdown', (e) => {
  if (!marginPage.open || e.target.closest?.('.margin-note, .margin-mark')) return;
  const el = marginPage.els.get(marginPage.open);
  if ($('textarea', el)?.value.trim()) return;
  marginPage.open = null;
  marginPage.draft = null;
  marginRedraw();
});

function marginSaying(el, problem) {
  const box = $('.margin-problem', el);
  box.textContent = problem || '';
  box.hidden = !problem;
}

function marginThinking(el, on) {
  const area = $('textarea', el);
  if (on) el.setAttribute('aria-busy', 'true');
  else el.removeAttribute('aria-busy');
  area.disabled = on;
  area.placeholder = on ? 'Felix is thinking…' : el.dataset.id === 'draft' ? 'Ask Felix about this' : 'Reply';
}

async function marginSubmit(e) {
  e.preventDefault();
  const el = e.target.closest('.margin-note');
  const area = $('textarea', el);
  const text = area.value.trim();
  const t = marginThread(el.dataset.id);
  const s = marginPage.s;
  if (!text || !t || !s) return;
  marginSaying(el, '');
  marginThinking(el, true);
  try {
    let done;
    if (t.id === 'draft') {
      await flush(s);
      done = await api(`/api/notes/${enc(s.note.id)}/margin/threads`, { method: 'POST', body: { version: s.note.version, quote: t.quote, prefix: t.prefix, suffix: t.suffix, question: text } });
    } else done = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}/reply`, { method: 'POST', body: { text } });
    if (marginPage.s !== s) return;
    area.value = '';
    if (t.id === 'draft') {
      marginDropThread('draft');
      marginPage.draft = null;
      marginPage.threads.push(done);
      marginPage.open = done.id;
    } else marginPage.threads = marginPage.threads.map((old) => (old.id === done.id ? done : old));
    marginDraw();
    const fresh = marginPage.els.get(done.id);
    const replies = fresh && [...fresh.querySelectorAll('.margin-felix')];
    if (replies?.length) marginAnswer(replies[replies.length - 1], marginNow(done), true);
    $('textarea', fresh)?.focus();
  } catch (error) {
    if (error instanceof Locked) throw error;
    if (marginPage.s === s) marginSaying(el, error.message);
  } finally {
    if (el.isConnected) marginThinking(el, false);
  }
}

async function marginStatus(t, status) {
  const s = marginPage.s;
  await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'POST', body: { status } });
  if (marginPage.s !== s) return;
  marginPage.threads = marginPage.threads.filter((old) => old.id !== t.id);
  marginDropThread(t.id);
  marginDraw();
  if (status === 'dismissed') {
    toast('Dismissed Felix’s note', {
      action: 'Undo',
      run: async () => {
        await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'POST', body: { status: 'open' } });
        if (marginPage.s === s) await marginLoad(s);
      },
    });
  }
}

function marginToChat(t) {
  const s = marginPage.s;
  const at = marginKit.find(s.doc.source(), t);
  const line = at ? marginKit.lineAfter(s.doc.source(), at.from) : 0;
  const title = cleanTitle(s.title.textContent) || s.note.title || 'Untitled';
  marginPage.open = null;
  marginPage.draft = null;
  chat.attachSelection({ note: s.note.id, title, from: line, to: line, text: t.quote });
  marginRedraw();
}

async function marginInsert(s, t, text, request) {
  await flush(s);
  const source = s.doc.source();
  const at = marginKit.find(source, t);
  const line = at ? marginKit.lineAfter(source, at.to) : null;
  try {
    return { done: await api(`/api/notes/${enc(s.note.id)}/insert`, { method: 'POST', body: { text, line, base: s.note.version, request } }), line };
  } catch (e) {
    if (e.status !== 409) throw e;
  }
  const fresh = await api(`/api/notes/${enc(s.note.id)}`);
  const moved = marginKit.find(fresh.body, t);
  const next = moved ? marginKit.lineAfter(fresh.body, moved.to) : null;
  return { done: await api(`/api/notes/${enc(s.note.id)}/insert`, { method: 'POST', body: { text, line: next, base: fresh.version, request } }), line: next };
}

async function marginAdd(t) {
  if (marginPage.adding.has(t.id)) return;
  marginPage.adding.add(t.id);
  const s = marginPage.s;
  const el = marginPage.els.get(t.id);
  const button = $('.margin-add', el);
  button.disabled = true;
  marginSaying(el, '');
  t.addRequest = t.addRequest || requestId();
  let added;
  try {
    added = await marginInsert(s, t, marginNow(t), t.addRequest);
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (el.isConnected) marginSaying(el, e.status === 409 ? 'The note changed. Try once more.' : e.message);
    return;
  } finally {
    marginPage.adding.delete(t.id);
    button.disabled = false;
  }
  const { done, line } = added;
  const id = s.note.id;
  const threadPath = `/api/notes/${enc(id)}/margin/threads/${enc(t.id)}`;
  const open = () => state.view === 'note' && state.session && state.session.note.id === id;
  toast(line === null ? 'Its passage has moved, so this went at the end of the note' : 'Added to note', {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
        await api(threadPath, { method: 'POST', body: { status: 'open' } }).catch(() => {});
        if (open()) await showNote(id);
        toast('Took it out of the note');
      } catch (e) {
        toast(e.status === 409 ? 'That note has changed since, so leo left it as it is.' : e.message, { bad: true });
      }
    },
  });
  await api(threadPath, { method: 'POST', body: { status: 'resolved' } }).catch((e) => {
    if (e instanceof Locked) throw e;
  });
  if (open()) await showNote(id);
  if (open() && line !== null) {
    state.session.doc.glow(line + 1, line + 3);
    setTimeout(() => { if (open()) state.session.doc.glow(); }, GLOW_MS);
  }
}

function marginAsk(s, picked) {
  const size = [...picked.text.trim()].length;
  if (!marginPage.layer || chat.isOpen() || size < LEAST_ASKED || [...picked.text].length > MOST_ASKED) return false;
  const beside = marginKit.around(s.doc.source(), picked.text, picked.from);
  if (!beside) return false;
  marginDropThread('draft');
  marginPage.draft = { id: 'draft', quote: picked.text, prefix: beside.prefix, suffix: beside.suffix, messages: [] };
  marginOpen('draft', true);
  return Boolean(marginPage.els.get('draft'));
}

function marginEdited(s) {
  if (s !== marginPage.s) return;
  marginRedraw();
  if (marginPage.brief) marginSetStatus('');
  if (s.note.id && !marginPage.loaded) marginLoad(s).catch(() => {});
  clearTimeout(marginPage.timer);
  if (marginPage.mode !== 'along') return;
  const pause = READ_PAUSE[marginPrefs?.pace] || READ_PAUSE.regular;
  marginPage.timer = setTimeout(() => marginRead(s).catch(fail), Math.max(pause, marginPage.nextRead - Date.now()));
}

function marginShowReading(s) {
  marginPage.faceTimer = setTimeout(() => {
    if (marginPage.reading && marginPage.s === s) marginSetStatus('Reading…');
  }, READ_FACE_AFTER_MS);
}

async function marginRead(s) {
  if (marginPage.reading) {
    marginPage.again = true;
    return;
  }
  if (s !== marginPage.s || marginPage.mode !== 'along' || !s.note.id || document.hidden || chat.isBusy()) return;
  if (!marginKit.changedBlocks(marginPage.baseline, s.doc.source()).length) return;
  const reading = {};
  marginPage.reading = reading;
  try {
    await flush(s);
    if (s !== marginPage.s || marginPage.mode !== 'along' || saving.unsaved()) return;
    const source = s.doc.source();
    const blocks = marginKit.changedBlocks(marginPage.baseline, source);
    if (!blocks.length) return;
    marginPage.nextRead = Date.now() + (marginPrefs?.gap ?? READ_GAP_SECONDS) * 1000;
    marginShowReading(s, blocks);
    const got = await api(`/api/notes/${enc(s.note.id)}/margin/read`, { method: 'POST', body: { version: s.note.version, blocks } });
    if (s !== marginPage.s) return;
    const along = marginPage.mode === 'along';
    if (!got?.failed) marginPage.baseline = marginKit.blocksOf(source);
    if (!along) marginSetStatus('');
    else if (got?.failed) marginSetStatus('Could not read this change. Check Writing AI and try again.');
    else if (got?.note) marginSetStatus('');
    else marginSetStatus('No comment this time', true);
    if (got?.note) {
      marginPage.threads.push(got.note);
      marginPage.arriving.add(got.note.id);
      const voice = $('.margin-said-aloud', marginPage.layer);
      if (voice) voice.textContent = `Felix: ${marginNow(got.note)}`;
      marginRedraw();
    }
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (marginPage.s === s) {
      const paused = { daily: 'Paused for today: daily limit reached', soon: 'Paused after a recent comment', busy: 'Already reading', felix: 'Paused while Felix answers', unedited: 'Reads changes after you edit', mode: '' };
      const stale = e.status === 409 && !e.data?.why;
      if (stale) marginPage.nextRead = 0;
      if (marginPage.mode !== 'along') marginSetStatus('');
      else marginSetStatus(paused[e.data?.why] || (stale ? 'This note changed while Felix was reading. He will try again after your next edit.' : 'Could not read this change. Check Writing AI and try again.'));
    }
  } finally {
    if (s === marginPage.s && marginPage.reading === reading) {
      marginPage.reading = false;
      clearTimeout(marginPage.faceTimer);
      const again = marginPage.again;
      marginPage.again = false;
      if (again && marginPage.mode === 'along') {
        marginPage.timer = setTimeout(() => marginRead(s).catch(fail), Math.max(READ_PAUSE[marginPrefs?.pace] || READ_PAUSE.regular, marginPage.nextRead - Date.now()));
      }
    }
  }
}

function marginLostOpen(el) {
  const t = marginPage.threads.find((old) => old.id === el.dataset.id);
  if (t) chat.attachSelection({ note: marginPage.s.note.id, title: marginPage.s.note.title || 'Untitled', from: 0, to: 0, text: t.quote });
}

function marginLostDismiss(el) {
  const t = marginPage.threads.find((old) => old.id === el.dataset.id);
  if (t) return marginStatus(t, 'dismissed');
}

const MARGIN_DIAGRAMS = {
  both: '<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M42 8h36v36H42z"/><path class="d-felix" d="M6 12h28M6 17h22M86 28h28M86 33h20"/></svg>',
  right: '<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M14 8h60v36H14z"/><path class="d-felix" d="M82 14h28M82 19h22M82 32h26M82 37h18"/></svg>',
  marks: '<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M26 8h68v36H26z"/><path class="d-text" d="M33 18h40M33 26h48M33 34h30"/><rect class="d-mark" x="76" y="16" width="5" height="5"/><rect class="d-mark" x="66" y="32" width="5" height="5"/></svg>',
};

function marginWait(seconds) {
  return seconds < 120 ? `${seconds} sec` : `${seconds / 60} min`;
}

function felixRhythm(view) {
  const pause = (READ_PAUSE[view.pace] || READ_PAUSE.regular) / 1000;
  const limit = view.cap ? `up to ${view.cap} times a day` : 'with no daily limit';
  return `After ${pause} sec of quiet, at most once every ${marginWait(view.gap)}, ${limit}. ${view.today} so far today.`;
}

function felixSettingsCards(view, page) {
  if (!view) return '<section class="set-card"><p class="hint">Felix’s settings are not available right now.</p></section>';
  const provider = page.tasks.find((task) => task.task === 'writing')?.provider || '';
  const name = page.tasks.find((task) => task.task === 'writing')?.choices?.find((choice) => choice.id === provider)?.label || 'your writing AI';
  const where = `Reads along sends the note’s title and the text you change to ${esc(name)}${page.reads_stay_here ? ', which runs on this computer' : ''}.`;
  return `<section class="set-card" data-find="felix-new-notes"><header><h3>New notes start</h3></header><p class="hint">${where}</p>${choiceCards({ label: 'New notes start', field: 'data-margin-pref="default_mode"', current: view.default_mode, choices: [['ask', 'When you ask', `Select text, then Ask Felix or ${marginAskKeys()}.`], ['along', 'Reads along', 'Comments after a pause, only when it matters.']] })}</section>
    <section class="set-card" data-find="felix-where"><header><h3>Where notes appear</h3></header>${choiceCards({ label: 'Where notes appear', field: 'data-margin-pref="layout"', current: view.layout, choices: [['both', 'Both margins', 'Your words in the middle.', MARGIN_DIAGRAMS.both], ['right', 'Right margin', 'One column beside you.', MARGIN_DIAGRAMS.right], ['marks', 'Marks only', 'A small mark to open.', MARGIN_DIAGRAMS.marks]] })}</section>
    <section class="set-card felix-rhythm"><header><h3>How often he reads</h3></header><p class="hint" id="felix-rhythm" aria-live="polite">${esc(felixRhythm(view))}</p><div class="set-row" data-find="felix-pace"><span class="set-label">Reading pace</span>${segmented({ label: 'Reading pace', field: 'data-margin-pref="pace"', current: view.pace, choices: [['regular', 'Regular'], ['slow', 'Slow']] })}</div>
    <div class="set-row" data-find="felix-gap"><span class="set-label">Time between reads</span>${segmented({ label: 'Time between reads', field: 'data-margin-pref="gap"', current: String(view.gap), choices: (view.gaps || []).map((gap) => [String(gap), marginWait(gap)]) })}</div>
    <div class="set-row" data-find="felix-cap"><span class="set-label">Reads a day</span>${segmented({ label: 'Reads a day', field: 'data-margin-pref="cap"', current: String(view.cap), choices: (view.caps || []).map((cap) => [String(cap), cap ? String(cap) : 'No limit']) })}</div>
    ${view.problem ? `<p class="set-note warn" role="status">${esc(view.problem)}</p>` : ''}</section>`;
}

document.addEventListener('change', async (e) => {
  const field = e.target.closest?.('[data-margin-pref]');
  if (!field) return;
  const key = field.dataset.marginPref;
  const value = key === 'cap' || key === 'gap' ? Number(field.value) : field.value;
  try {
    const view = await api('/api/margin', { method: 'POST', body: { [key]: value } });
    marginRemember(view);
    if (state.view === 'settings') state.margin = view;
    const rhythm = $('#felix-rhythm');
    if (rhythm) rhythm.textContent = felixRhythm(view);
  } catch (error) {
    fail(error);
  }
});


document.addEventListener('pointerdown', (e) => {
  if (!e.target.closest?.('.margin-mode-wrap')) marginModeMenu(false);
});
