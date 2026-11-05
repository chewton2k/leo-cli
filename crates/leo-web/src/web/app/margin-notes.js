const MARGIN_KINDS = ['add_margin_note', 'remove_margin_note'];
const READ_PAUSE = { regular: 4000, slow: 8000 };
const READ_RETRY_MS = 90000;
const READ_FACE_AFTER_MS = 200;
const WORD_MS = 62;
const INK_DELAY_MS = 1140;
const GLOW_MS = 2200;
const MOST_ASKED = 600;
const marginKit = window.leoMargin;
const marginPage = { s: null, mode: 'ask', threads: [], layer: null, svg: null, els: new Map(), paths: new Map(), marks: new Map(), open: null, draft: null, arriving: new Set(), timer: 0, reading: false, readFace: null, faceTimer: 0, baseline: [], nextRead: 0, observer: null, frame: 0, roomy: null };
let marginPrefs = null;

async function marginLoadPrefs() {
  if (marginPrefs) return marginPrefs;
  try {
    marginPrefs = await api('/api/margin');
  } catch (e) {
    return { layout: 'right', pace: 'regular' };
  }
  return marginPrefs;
}

function marginLeave() {
  clearTimeout(marginPage.timer);
  clearTimeout(marginPage.faceTimer);
  cancelAnimationFrame(marginPage.frame);
  marginPage.observer?.disconnect();
  marginPage.observer = null;
  marginPage.layer?.remove();
  marginPage.roomy?.classList.remove('margin-room');
  chat.setLost([]);
  Object.assign(marginPage, { s: null, mode: 'ask', threads: [], layer: null, svg: null, open: null, draft: null, reading: false, readFace: null, baseline: [], roomy: null });
  marginPage.els.clear();
  marginPage.paths.clear();
  marginPage.marks.clear();
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
  Object.assign(marginPage, { s, layer, svg: layer.firstChild, baseline: marginKit.blocksOf(s.doc.source()), nextRead: 0 });
  layer.addEventListener('click', marginClick);
  layer.addEventListener('keydown', marginKey);
  layer.addEventListener('submit', marginSubmit);
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

function marginControl() {
  const s = marginPage.s;
  const meta = $('.note-meta');
  if (!s || !meta || !s.note.id) return;
  let button = $('#margin-mode', meta);
  if (!button) {
    button = document.createElement('button');
    button.type = 'button';
    button.id = 'margin-mode';
    button.className = 'chip margin-mode';
    button.addEventListener('click', marginToggleMode);
    meta.appendChild(button);
  }
  const along = marginPage.mode === 'along';
  button.textContent = along ? 'Felix reads along' : 'Felix: when you ask';
  button.setAttribute('aria-pressed', String(along));
  button.title = along ? 'Felix reads what you change after a pause. The text goes to your writing AI.' : 'Felix reads this note only when you ask him.';
}

async function marginToggleMode() {
  const s = marginPage.s;
  if (!s || !s.note.id) return;
  const mode = marginPage.mode === 'along' ? 'ask' : 'along';
  const done = await api(`/api/notes/${enc(s.note.id)}/margin`, { method: 'POST', body: { mode } });
  if (marginPage.s !== s) return;
  marginPage.mode = done.mode;
  marginPage.baseline = marginKit.blocksOf(s.doc.source());
  marginControl();
  marginRedraw();
  if (done.mode === 'along') toast('Felix now reads what you change here, after a pause. The text goes to your writing AI.');
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

function marginFace(thinking = false) {
  return felix.felix(26, `felix-preview margin-face${thinking ? ' think' : ''}`);
}

function inkWords(box, text, delay) {
  box.textContent = '';
  if (stillMotion()) {
    box.textContent = text;
    return;
  }
  text.split(/(\s+)/).forEach((word, i) => {
    if (!word.trim()) {
      box.append(word);
      return;
    }
    const span = document.createElement('span');
    span.className = 'ink-word';
    span.style.animationDelay = `${delay + (i / 2) * WORD_MS}ms`;
    span.textContent = word;
    box.append(span);
  });
}

function marginElement(t) {
  const el = document.createElement('article');
  el.className = 'margin-note';
  el.dataset.id = t.id;
  el.tabIndex = 0;
  el.setAttribute('role', 'note');
  el.setAttribute('aria-label', `Felix, about “${t.quote.slice(0, 80)}”`);
  el.innerHTML = `<span class="margin-surface" aria-hidden="true"></span><div class="margin-head">${marginFace()}</div><div class="margin-said"></div><div class="margin-more" hidden>
    <form class="margin-reply"><textarea rows="1" maxlength="1000" aria-label="Reply to Felix" placeholder="Reply"></textarea></form>
    <p class="margin-problem" role="status" hidden></p>
    <div class="margin-acts"><button type="button" class="margin-add" data-margin-act="add">Add to note</button><button type="button" class="msg-act" data-margin-act="chat">Open in chat</button><button type="button" class="msg-act" data-margin-act="resolve">Resolve</button><button type="button" class="msg-act" data-margin-act="dismiss">Dismiss</button></div></div>`;
  marginPage.layer.appendChild(el);
  marginPage.els.set(t.id, el);
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
      const line = document.createElement('p');
      line.className = 'margin-felix';
      if (arriving) inkWords(line, marginNow(t), INK_DELAY_MS);
      else line.textContent = marginNow(t);
      said.append(line);
    } else {
      for (const message of t.messages || []) {
        const line = document.createElement('p');
        line.className = message.from === 'felix' ? 'margin-felix' : 'margin-you';
        line.textContent = message.text;
        said.append(line);
      }
    }
  }
  const draft = t.id === 'draft';
  $('.margin-more', el).hidden = !open;
  el.toggleAttribute('data-open', open);
  $('.margin-add', el).hidden = draft || !marginNow(t);
  $('[data-margin-act="chat"]', el).textContent = draft ? 'Ask in chat' : 'Open in chat';
  $('[data-margin-act="resolve"]', el).hidden = draft;
  $('[data-margin-act="dismiss"]', el).textContent = draft ? 'Cancel' : 'Dismiss';
  $('textarea', el).placeholder = draft ? 'Ask Felix about this' : 'Reply';
}

function marginPath(id, kind) {
  const key = `${id}:${kind}`;
  let path = marginPage.paths.get(key);
  if (!path) {
    path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('pathLength', '1');
    path.setAttribute('class', kind);
    marginPage.svg.appendChild(path);
    marginPage.paths.set(key, path);
  }
  return path;
}

function marginDropThread(id) {
  marginPage.els.get(id)?.remove();
  marginPage.els.delete(id);
  marginPage.marks.get(id)?.remove();
  marginPage.marks.delete(id);
  for (const kind of ['squiggle', 'tether']) {
    marginPage.paths.get(`${id}:${kind}`)?.remove();
    marginPage.paths.delete(`${id}:${kind}`);
  }
  if (marginPage.open === id) marginPage.open = null;
}

function marginMark(t) {
  let mark = marginPage.marks.get(t.id);
  if (!mark) {
    mark = document.createElement('button');
    mark.type = 'button';
    mark.className = 'margin-mark';
    mark.dataset.id = t.id;
    mark.setAttribute('aria-label', `Felix’s note on “${t.quote.slice(0, 80)}”`);
    mark.innerHTML = marginFace();
    marginPage.layer.appendChild(mark);
    marginPage.marks.set(t.id, mark);
  }
  return mark;
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
  const layout = marginKit.arrangement({ width: columns.clientWidth, setting: marginPrefs?.layout || 'right', noteWidth: window.leoTheme.noteWidth(), panel: chat.isOpen(), threads: placed.length, along: marginPage.mode === 'along' });
  if (columns.dataset.margin !== layout) columns.dataset.margin = layout;
  const keep = new Set(placed.map(({ t }) => t.id));
  for (const id of [...marginPage.els.keys(), ...marginPage.marks.keys()]) if (!keep.has(id)) marginDropThread(id);
  const origin = columns.getBoundingClientRect();
  const page = $('article.note', columns).getBoundingClientRect();
  const margins = layout === 'right' || layout === 'both';
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
    const squiggle = marginPath(t.id, 'squiggle');
    squiggle.setAttribute('d', rects.map((r) => marginKit.wave(r.left, r.right, r.bottom + 1)).join(' '));
    squiggle.classList.toggle('arriving', marginPage.arriving.has(t.id));
  }
  if (marginPage.arriving.size && !stillMotion()) {
    const done = [...marginPage.arriving];
    setTimeout(() => {
      for (const id of done) {
        marginPage.arriving.delete(id);
        marginPage.paths.get(`${id}:squiggle`)?.classList.remove('arriving');
        marginPage.paths.get(`${id}:tether`)?.classList.remove('arriving');
        marginPage.els.get(id)?.classList.remove('arriving');
      }
    }, INK_DELAY_MS + 900);
  } else marginPage.arriving.clear();
}

function marginPlaceInMargin(boxes, layout, page, origin) {
  const left = page.left - origin.left;
  const right = page.right - origin.left;
  const items = boxes.map(({ t, rects, el }) => ({ id: t.id, want: rects[0].top - 2, height: el.offsetHeight }));
  const side = layout === 'both' ? marginKit.sides(items) : Object.fromEntries(items.map((item) => [item.id, 'right']));
  const pinned = marginPage.open;
  const tops = { ...marginKit.stack(items.filter((i) => side[i.id] === 'right'), { pinned }), ...marginKit.stack(items.filter((i) => side[i.id] === 'left'), { pinned }) };
  for (const { t, rects, el } of boxes) {
    const on = side[t.id];
    const x = on === 'right' ? right + marginKit.MARGIN_GAP : left - marginKit.MARGIN_GAP - marginKit.MARGIN_WIDTH;
    el.hidden = false;
    el.removeAttribute('data-inline');
    el.style.transform = `translate(${x}px, ${tops[t.id]}px)`;
    el.style.width = '';
    el.classList.toggle('arriving', marginPage.arriving.has(t.id));
    marginPage.marks.get(t.id)?.remove();
    marginPage.marks.delete(t.id);
    const last = rects[0];
    const from = on === 'right' ? { x: Math.min(last.right + 5, right + 4), y: last.bottom - 3 } : { x: last.left - 5, y: last.bottom - 3 };
    const to = on === 'right' ? { x: x - 7, y: tops[t.id] + 12 } : { x: x + marginKit.MARGIN_WIDTH + 7, y: tops[t.id] + 12 };
    const tether = marginPath(t.id, 'tether');
    tether.setAttribute('d', marginKit.tether(from, to));
    tether.classList.toggle('arriving', marginPage.arriving.has(t.id));
  }
}

function marginPlaceAsMarks(boxes, page, origin) {
  for (const { t, at, rects, el } of boxes) {
    const end = rects[rects.length - 1];
    const mark = marginMark(t);
    mark.style.transform = `translate(${Math.min(end.right + 4, page.right - origin.left - 18)}px, ${end.top + 2}px)`;
    marginPage.paths.get(`${t.id}:tether`)?.remove();
    marginPage.paths.delete(`${t.id}:tether`);
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
    const bottom = block.getBoundingClientRect().bottom - origin.top - room;
    el.style.transform = `translate(${page.left - origin.left}px, ${bottom + 14}px)`;
  }
}

function marginThread(id) {
  if (marginPage.draft?.id === id) return marginPage.draft;
  return marginPage.threads.find((t) => t.id === id) || null;
}

function marginOpen(id, focus = false) {
  marginPage.open = id;
  marginDraw();
  if (focus) $('textarea', marginPage.els.get(id))?.focus();
}

function marginClose() {
  const id = marginPage.open;
  if (!id) return;
  marginPage.open = null;
  if (id === 'draft') marginPage.draft = null;
  marginDraw();
  (marginPage.marks.get(id) || marginPage.els.get(id))?.focus();
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
}

function marginKey(e) {
  if (e.key === 'Escape' && marginPage.open) {
    e.preventDefault();
    e.stopPropagation();
    marginClose();
  } else if (e.key === 'Enter' && e.target.classList.contains('margin-note') && marginPage.open !== e.target.dataset.id) {
    e.preventDefault();
    marginOpen(e.target.dataset.id, true);
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
  const face = $('.margin-head .felix', el);
  face.classList.toggle('think', on);
  $('textarea', el).disabled = on;
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
    if (replies?.length) inkWords(replies[replies.length - 1], marginNow(done), 0);
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

async function marginInsert(s, text, line, request) {
  await flush(s);
  try {
    return await api(`/api/notes/${enc(s.note.id)}/insert`, { method: 'POST', body: { text, line, base: s.note.version, request } });
  } catch (e) {
    if (e.status !== 409) throw e;
  }
  const fresh = await api(`/api/notes/${enc(s.note.id)}`);
  return api(`/api/notes/${enc(s.note.id)}/insert`, { method: 'POST', body: { text, line: null, base: fresh.version, request } });
}

async function marginAdd(t) {
  const s = marginPage.s;
  const el = marginPage.els.get(t.id);
  const text = marginNow(t);
  const at = marginKit.find(s.doc.source(), t);
  const line = at ? marginKit.lineAfter(s.doc.source(), at.to) : null;
  marginSaying(el, '');
  let done;
  try {
    done = await marginInsert(s, text, line, requestId());
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (el.isConnected) marginSaying(el, e.status === 409 ? 'The note changed. Try once more.' : e.message);
    return;
  }
  const id = s.note.id;
  await api(`/api/notes/${enc(id)}/margin/threads/${enc(t.id)}`, { method: 'POST', body: { status: 'resolved' } });
  const open = () => state.view === 'note' && state.session && state.session.note.id === id;
  if (open()) await showNote(id);
  if (open() && line !== null) {
    state.session.doc.glow(line + 1, line + 3);
    setTimeout(() => { if (open()) state.session.doc.glow(); }, GLOW_MS);
  }
  toast('Added to note', {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
        await api(`/api/notes/${enc(id)}/margin/threads/${enc(t.id)}`, { method: 'POST', body: { status: 'open' } });
        if (open()) await showNote(id);
        toast('Took it out of the note');
      } catch (e) {
        toast(e.status === 409 ? 'That note has changed since, so leo left it as it is.' : e.message, { bad: true });
      }
    },
  });
}

function marginAsk(s, picked) {
  if (!marginPage.layer || chat.isOpen() || !picked.text.trim() || [...picked.text].length > MOST_ASKED) return false;
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
  if (s.note.id && !$('#margin-mode')) marginLoad(s).catch(() => {});
  clearTimeout(marginPage.timer);
  if (marginPage.mode !== 'along') return;
  const pause = READ_PAUSE[marginPrefs?.pace] || READ_PAUSE.regular;
  marginPage.timer = setTimeout(() => marginRead(s), Math.max(pause, marginPage.nextRead - Date.now()));
}

function marginShowReading(s, blocks) {
  marginPage.faceTimer = setTimeout(() => {
    if (!marginPage.reading || marginPage.s !== s) return;
    const source = s.doc.source();
    const last = blocks[blocks.length - 1];
    const block = s.doc.blockAt(Math.max(0, source.indexOf(last)));
    const columns = marginPage.layer.parentElement;
    const origin = columns.getBoundingClientRect();
    const page = $('article.note', columns).getBoundingClientRect();
    const face = document.createElement('div');
    face.className = 'margin-reading';
    face.innerHTML = marginFace(true);
    const top = (block ? block.getBoundingClientRect().top : page.top) - origin.top;
    const inMargin = ['right', 'both'].includes(columns.dataset.margin);
    face.style.transform = `translate(${page.right - origin.left + (inMargin ? marginKit.MARGIN_GAP : -28)}px, ${top}px)`;
    marginPage.layer.appendChild(face);
    marginPage.readFace = face;
  }, READ_FACE_AFTER_MS);
}

async function marginRead(s) {
  if (s !== marginPage.s || marginPage.reading || marginPage.mode !== 'along' || !s.note.id || document.hidden || chat.isBusy()) return;
  if (!marginKit.changedBlocks(marginPage.baseline, s.doc.source()).length) return;
  marginPage.reading = true;
  try {
    await flush(s);
    if (s !== marginPage.s || saving.unsaved()) return;
    const source = s.doc.source();
    const blocks = marginKit.changedBlocks(marginPage.baseline, source);
    if (!blocks.length) return;
    marginPage.nextRead = Date.now() + READ_RETRY_MS;
    marginShowReading(s, blocks);
    const got = await api(`/api/notes/${enc(s.note.id)}/margin/read`, { method: 'POST', body: { version: s.note.version, blocks } });
    if (s !== marginPage.s) return;
    marginPage.baseline = marginKit.blocksOf(source);
    if (got?.note) {
      marginPage.threads.push(got.note);
      marginPage.arriving.add(got.note.id);
      const voice = $('.margin-said-aloud', marginPage.layer);
      if (voice) voice.textContent = `Felix: ${marginNow(got.note)}`;
      marginRedraw();
    }
  } catch (e) {
    if (e instanceof Locked) throw e;
  } finally {
    marginPage.reading = false;
    clearTimeout(marginPage.faceTimer);
    marginPage.readFace?.remove();
    marginPage.readFace = null;
    if (s === marginPage.s && marginPage.mode === 'along' && marginKit.changedBlocks(marginPage.baseline, s.doc.source()).length) {
      marginPage.timer = setTimeout(() => marginRead(s), Math.max(READ_PAUSE[marginPrefs?.pace] || READ_PAUSE.regular, marginPage.nextRead - Date.now()));
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

function felixSettingsCards(view, page) {
  if (!view) return '<section class="set-card"><p class="hint">Felix’s settings are not available right now.</p></section>';
  const provider = page.tasks.find((task) => task.task === 'writing')?.provider || '';
  const name = page.tasks.find((task) => task.task === 'writing')?.choices?.find((choice) => choice.id === provider)?.label || 'your writing AI';
  const where = provider === 'ollama'
    ? 'Reads along sends the text you change to Ollama after each pause. Ollama runs on this computer, so it stays here.'
    : `Reads along sends the text you change to ${esc(name)} after each pause, without a press each time. When you ask sends nothing until you ask.`;
  return `<section class="set-card" data-find="felix-new-notes"><header><h3>New notes start</h3></header><p class="hint">${where}</p>${choiceCards({ label: 'New notes start', field: 'data-margin-pref="default_mode"', current: view.default_mode, choices: [['ask', 'When you ask', 'Felix stays quiet until you ask him.'], ['along', 'Reads along', 'He reads what changed after a pause and speaks only when it matters.']] })}<p class="hint">Applies to notes made from now on. Each note has its own switch under its title.</p></section>
    <section class="set-card" data-find="felix-where"><header><h3>Where notes appear</h3></header>${choiceCards({ label: 'Where notes appear', field: 'data-margin-pref="layout"', current: view.layout, choices: [['both', 'Both margins', 'Your words in the middle.', MARGIN_DIAGRAMS.both], ['right', 'Right margin', 'One column beside you.', MARGIN_DIAGRAMS.right], ['marks', 'Marks only', 'A small mark to open.', MARGIN_DIAGRAMS.marks]] })}</section>
    <section class="set-card"><div class="set-row" data-find="felix-pace"><span class="set-label">Reading pace</span>${segmented({ label: 'Reading pace', field: 'data-margin-pref="pace"', current: view.pace, choices: [['regular', 'Regular'], ['slow', 'Slow']] })}</div>
    <div class="set-row column" data-find="felix-cap"><div class="set-line"><span class="set-label">Reads a day</span>${segmented({ label: 'Reads a day', field: 'data-margin-pref="cap"', current: view.cap, choices: (view.caps || []).map((cap) => [String(cap), String(cap)]) })}</div><span class="hint" id="margin-today">${view.today} of ${view.cap} used today. Each read starts your writing AI once.</span>${view.problem ? `<span class="set-note warn">${esc(view.problem)}</span>` : ''}</div></section>`;
}

document.addEventListener('change', async (e) => {
  const field = e.target.closest?.('[data-margin-pref]');
  if (!field) return;
  const key = field.dataset.marginPref;
  const value = key === 'cap' ? Number(field.value) : field.value;
  try {
    const view = await api('/api/margin', { method: 'POST', body: { [key]: value } });
    marginPrefs = view;
    if (state.view === 'settings') state.margin = view;
    const today = $('#margin-today');
    if (today) today.textContent = `${view.today} of ${view.cap} used today. Each read starts your writing AI once.`;
  } catch (error) {
    fail(error);
  }
});
