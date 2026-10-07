(function () {
  'use strict';

  const md = window.leoMarkdown;
  const leoDoc = window.leoDoc;
  const esc = md.escape;
  const enc = encodeURIComponent;
  const $ = (selector, root = document) => root.querySelector(selector);
  const app = $('#app');
  const floating = $('#floating');

  const svg = (paths) =>
    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
  const FOLDER = '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>';
  const ICON = {
    back: svg('<path d="M15 18l-6-6 6-6"/>'),
    search: svg('<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>'),
    close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
    more: svg('<circle cx="5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="19" cy="12" r="1.3"/>'),
    folder: svg(FOLDER),
    folderPlus: svg(FOLDER + '<path d="M12 10.5v6M9 13.5h6"/>'),
    move: svg(FOLDER + '<path d="M9.5 13.5h6M13 11l2.5 2.5L13 16"/>'),
    pin: svg('<path d="M12 16v6"/><path d="M8 3h8l-1.2 6.2L18 12.5V15H6v-2.5l3.2-3.3z"/>'),
    trash: svg('<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/>'),
    share: svg('<path d="M12 3v12M8 7l4-4 4 4"/><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"/>'),
    plus: svg('<path d="M12 5v14M5 12h14"/>'),
    minus: svg('<path d="M5 12h14"/>'),
    fit: svg('<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>'),
    restore: svg('<path d="M4 12a8 8 0 1 0 2.3-5.6L4 8.5"/><path d="M4 4v4.5h4.5"/>'),
    tag: svg('<path d="M3 12V4h8l10 10-8 8z"/><circle cx="7.5" cy="8.5" r="1.3"/>'),
    note: svg('<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>'),
    refresh: svg('<path d="M20 12a8 8 0 1 1-2.3-5.6L20 8.5"/><path d="M20 4v4.5h-4.5"/>'),
    check: svg('<path d="M5 12l4.5 4.5L19 7"/>'),
    lock: svg('<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>'),
    map: svg('<circle cx="6" cy="7" r="2.2"/><circle cx="18" cy="6" r="2.2"/><circle cx="12" cy="17.5" r="2.2"/><path d="M7.4 8.9l3.5 6.7M16.9 7.9l-3.8 7.8M8.2 6.8l7.6-.6"/>'),
    cloud: svg('<path d="M7 18a4.5 4.5 0 0 1-.5-9 6 6 0 0 1 11.3 1.5A3.8 3.8 0 0 1 17.5 18z"/><path d="M4 4l16 16"/>'),
  };

  $('#back').innerHTML = ICON.back;
  $('#search-toggle').innerHTML = ICON.search;
  $('#menu').innerHTML = ICON.more;
  $('#search-icon').innerHTML = ICON.search;

  class Offline extends Error {}
  class Locked extends Error {}

  async function api(path, { method = 'GET', body } = {}) {
    let response;
    try {
      response = await fetch(path, {
        method,
        credentials: 'same-origin',
        headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (e) {
      throw new Offline();
    }
    if (response.status === 401) throw new Locked();
    if (response.status === 204) return null;
    if (!response.ok) {
      const error = new Error(response.status === 404 ? 'That is not there any more.' : response.status === 400 ? 'Use a folder name inside your notes, without . or ..' : `leo answered ${response.status}.`);
      error.status = response.status;
      throw error;
    }
    return response.json();
  }

  function fail(error) {
    if (error instanceof Locked) return showLocked();
    if (error instanceof Offline) return toast("Can't reach leo. Is `leo serve` still running on your computer?", { bad: true });
    toast(error.message || String(error), { bad: true });
  }

  let toastTimer;
  function toast(message, { action, run, bad } = {}) {
    clearTimeout(toastTimer);
    const old = $('.toast');
    if (old) old.remove();
    const el = document.createElement('div');
    el.className = 'toast' + (bad ? ' bad' : '');
    el.setAttribute('role', 'status');
    el.innerHTML = `<span>${esc(message)}</span>${action ? `<button>${esc(action)}</button>` : ''}`;
    if (action) {
      el.querySelector('button').addEventListener('click', () => {
        el.remove();
        run();
      });
    }
    document.body.appendChild(el);
    toastTimer = setTimeout(() => el.remove(), action ? 6000 : 3500);
  }

  function rel(iso) {
    const then = new Date(iso);
    const seconds = (Date.now() - then) / 1000;
    if (seconds < 60) return 'just now';
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    if (seconds < 2 * 86400) return 'yesterday';
    if (seconds < 7 * 86400) return then.toLocaleDateString(undefined, { weekday: 'long' });
    const sameYear = then.getFullYear() === new Date().getFullYear();
    return then.toLocaleDateString(undefined, sameYear ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
  }

  const folderLabel = (dir) => (dir ? dir.split('/').pop() : 'All notes');

  function progress(body) {
    const boxes = body.match(/^\s*- \[( |x|X)\] /gm) || [];
    if (!boxes.length) return '';
    const done = boxes.filter((b) => !b.includes('[ ]')).length;
    return `<span class="chip progress">${ICON.check.replace('<svg', '<svg width="14" height="14"')} ${done}/${boxes.length}</span>`;
  }

  function highlight(text, words) {
    if (!words.length) return esc(text);
    const pattern = new RegExp(`(${words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')})`, 'gi');
    return text
      .split(pattern)
      .map((part, i) => (i % 2 ? `<mark>${esc(part)}</mark>` : esc(part)))
      .join('');
  }

  function snippet(body, words) {
    const text = md.plain(body);
    if (!text) return '';
    if (!words.length) return esc(text.slice(0, 180));
    const lower = text.toLowerCase();
    const at = Math.min(...words.map((w) => lower.indexOf(w.toLowerCase())).filter((i) => i >= 0), Infinity);
    if (at === Infinity) return esc(text.slice(0, 180));
    const start = Math.max(0, at - 50);
    return (start ? '…' : '') + highlight(text.slice(start, start + 180), words);
  }

  function pinButton(note) {
    const label = note.pinned ? 'Unpin' : 'Pin to the top';
    return `<button class="pin-toggle${note.pinned ? ' on' : ''}" data-action="pin-card" data-id="${esc(note.id)}" aria-pressed="${note.pinned}" aria-label="${label}" title="${label}">${ICON.pin}</button>`;
  }

  function card(note, { words = [], showFolder = false } = {}) {
    const tags = note.tags.map((t) => `<span class="chip">#${esc(t)}</span>`).join('');
    const where = showFolder && note.directory ? `<span class="chip accent">${esc(note.directory)}</span>` : '';
    const text = snippet(note.body, words);
    return `<div class="card" role="link" tabindex="0" data-action="open-note" data-id="${esc(note.id)}">
      <div class="card-title"><span>${words.length ? highlight(note.title, words) : esc(note.title)}</span>${pinButton(note)}</div>
      ${text ? `<div class="card-snippet">${text}</div>` : ''}
      <div class="card-meta">${where}<span>${rel(note.updated_at)}</span>${progress(note.body)}${tags}</div>
    </div>`;
  }

  function empty(icon, title, text) {
    return `<div class="empty">${icon}<h3>${esc(title)}</h3><p>${esc(text)}</p></div>`;
  }

  const skeleton = (n) => Array.from({ length: n }, () => '<div class="skeleton"></div>').join('');

  let state = { view: null };
  let seq = 0;

  function go(hash, { replace = false } = {}) {
    if (location.hash === hash) return route();
    if (replace) {
      history.replaceState(null, '', hash);
      route();
    } else {
      location.hash = hash;
    }
  }

  const folderHash = (dir) => (dir ? `#/f/${enc(dir)}` : '#/');
  const noteHash = (id) => `#/n/${enc(id)}`;

  function back() {
    if (state.view === 'map' && state.focus) return go(noteHash(state.focus));
    if (state.view === 'note') return go(folderHash(state.dir || ''));
    if (state.view === 'folder') return go(folderHash((state.dir || '').split('/').slice(0, -1).join('/')));
    go('#/');
  }

  function chrome({ dir = '', showBack = false, fab = null, actions = null }) {
    $('#back').hidden = !showBack;
    const crumbs = $('#crumbs');
    if (dir) {
      const parts = dir.split('/');
      crumbs.innerHTML = parts
        .map((part, i) => {
          const path = parts.slice(0, i + 1).join('/');
          return `<span class="sep">/</span><button data-action="open-folder" data-dir="${esc(path)}">${esc(part)}</button>`;
        })
        .join('');
    } else {
      crumbs.innerHTML = '';
    }
    floating.innerHTML = (fab || '') + (actions || '');
    document.title = dir ? `${folderLabel(dir)} · leo` : 'leo';
  }

  const newButton = (dir) => `<button class="fab" data-action="new" data-dir="${esc(dir)}">${ICON.plus}<span>New note</span></button>`;

  async function showFolder(dir) {
    const mine = ++seq;
    state = { view: 'folder', dir };
    chrome({ dir, showBack: Boolean(dir), fab: newButton(dir) });
    if (!app.innerHTML.trim()) app.innerHTML = skeleton(4);
    const [dirs, notes] = await Promise.all([api(`/api/dirs?parent=${enc(dir)}`), api(`/api/notes?dir=${enc(dir)}&limit=1000`)]);
    if (mine !== seq) return;
    if (!dirs.length && !notes.length) {
      app.innerHTML = dir
        ? empty(ICON.folder, 'This folder is empty', 'Tap “New note” to write the first one here.')
        : empty(ICON.note, 'No notes yet', 'Tap “New note” to write one. Notes you make in leo on your computer show up here too.');
      return;
    }
    let html = '';
    if (dirs.length) {
      html += `<div class="section-title">Folders</div><div class="folders">${dirs
        .map((d) => {
          const full = dir ? `${dir}/${d.name}` : d.name;
          return `<button class="folder" data-action="open-folder" data-dir="${esc(full)}">${ICON.folder}<span><span class="name">${esc(d.name)}</span><span class="count">${d.notes} note${d.notes === 1 ? '' : 's'}</span></span></button>`;
        })
        .join('')}</div>`;
    }
    if (notes.length) {
      html += `<div class="section-title">Notes</div><div class="cards">${notes.map((n) => card(n)).join('')}</div>`;
    }
    app.innerHTML = html;
  }

  const cleanTitle = (text) => text.replace(/\s*\n\s*/g, ' ').trim();

  function mark(s, text) {
    if (state.session !== s) return;
    const el = $('#save-state');
    if (el) el.textContent = text;
  }

  let local;
  try { local = window.localStorage; } catch (_) { local = null; }
  const saving = window.leoSaving.create({
    api, storage: local,
    mark,
    storageError: () => toast('This browser cannot keep a draft. Keep the page open until your note says Saved.', { bad: true }),
    recovered: (count) => toast(`${count} unsaved draft${count === 1 ? '' : 's'} recovered. Open Drafts in the menu.`, { action: 'Drafts', run: () => go('#/drafts') }),
    created: (s, note) => {
      if (state.session === s) history.replaceState(null, '', noteHash(note.id));
    },
    conflict: (s, note) => {
      if (state.session === s) {
        history.replaceState(null, '', noteHash(note.id));
        s.title.textContent = s.edit.title;
      }
      toast('This note changed elsewhere. Your edits were kept in a separate copy.', { action: 'Open copy', run: () => go(noteHash(note.id)) });
    },
    retryable: (error) => error instanceof Offline || error.status >= 500,
  });

  function snapshot(s) {
    const body = s.doc.source();
    const title = cleanTitle(s.title.textContent) || (s.note.id ? s.note.title : md.plain(body).slice(0, 60));
    return { title, body, tags: [...s.tags] };
  }

  function changed(s) { saving.changed(s, snapshot(s)); }
  function flush(s = state.session) { return saving.flush(s); }

  function tagChips(s) {
    return (
      s.tags
        .map(
          (t) =>
            `<span class="chip tag-chip"><button data-action="tag" data-tag="${esc(t)}">#${esc(t)}</button><button class="untag" data-action="untag" data-tag="${esc(t)}" aria-label="Remove #${esc(t)}">${ICON.close}</button></span>`
        )
        .join('') + '<input id="tag-input" class="tag-input" placeholder="+ tag" autocomplete="off" autocapitalize="none" enterkeyhint="done" aria-label="Add a tag">'
    );
  }

  function drawTags(s) {
    const box = $('#tags');
    if (!box) return;
    box.innerHTML = tagChips(s);
    const input = $('#tag-input', box);
    const add = () => {
      const words = input.value
        .split(/[\s,]+/)
        .map((t) => t.replace(/^#/, '').trim())
        .filter(Boolean)
        .filter((t) => !s.tags.includes(t));
      input.value = '';
      if (!words.length) return;
      s.tags.push(...words);
      changed(s);
      drawTags(s);
      $('#tag-input').focus();
    };
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ',') {
        e.preventDefault();
        add();
      } else if (e.key === 'Backspace' && !input.value && s.tags.length) {
        s.tags.pop();
        changed(s);
        drawTags(s);
        $('#tag-input').focus();
      }
    });
    input.addEventListener('blur', add);
  }

  function noteActions(id) {
    return `<nav class="actions" aria-label="Note">
        ${id ? `<button data-action="note-map" data-id="${esc(id)}">${ICON.map}<span>Map</span></button>` : ''}
        <button data-action="move">${ICON.move}<span>Move</span></button>
        <button data-action="share">${ICON.share}<span>PDF</span></button>
        <button data-action="delete" class="danger">${ICON.trash}<span>Delete</span></button>
      </nav>`;
  }

  async function showNote(id, { fresh = null, draft = null } = {}) {
    const mine = ++seq;
    const note = fresh || (await api(`/api/notes/${enc(id)}`));
    if (mine !== seq) return;
    if (state.session && state.session.doc) state.session.doc.destroy();
    const s = draft || saving.open(note);
    s.tags = [...s.edit.tags];
    state = { view: 'note', dir: note.directory, session: s };
    chrome({ dir: note.directory, showBack: true, actions: noteActions(note.id) });
    document.title = `${note.title || 'New note'} · leo`;
    const where = `<button class="chip accent" data-action="open-folder" data-dir="${esc(note.directory)}">${ICON.folder.replace('<svg', '<svg width="13" height="13"')} ${esc(folderLabel(note.directory))}</button>`;
    const when = note.id ? `Edited ${rel(note.updated_at)}` : 'New note';
    app.innerHTML = `<article class="note">
      <header class="note-head"><h1 id="title" contenteditable="plaintext-only" spellcheck="true" data-placeholder="Title" enterkeyhint="next">${esc(s.edit.title)}</h1>
      <div class="note-meta">${where}<span id="save-state">${when}</span></div>
      <div class="tags" id="tags"></div></header>
      <div class="prose doc" id="doc"></div>
    </article>`;
    s.title = $('#title');
    s.doc = leoDoc.mount($('#doc'), { source: s.edit.body, onChange: () => changed(s), placeholder: 'Tap here to write' });
    drawTags(s);
    if (s.dirty) {
      mark(s, 'Recovered draft · saving…');
      flush(s).catch(fail);
    }
    const blank = () => s.title.classList.toggle('blank', !cleanTitle(s.title.textContent));
    blank();
    s.title.addEventListener('input', () => {
      blank();
      document.title = `${cleanTitle(s.title.textContent) || 'Untitled'} · leo`;
      changed(s);
    });
    s.title.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        s.doc.editStart();
      }
    });
    s.title.addEventListener('paste', (e) => {
      e.preventDefault();
      const text = cleanTitle((e.clipboardData || window.clipboardData).getData('text'));
      document.execCommand('insertText', false, text);
    });
    if (!note.id) {
      s.title.focus();
    }
  }

  function showDrafts() {
    ++seq;
    state = { view: 'drafts', dir: '' };
    chrome({ showBack: true });
    const drafts = saving.drafts();
    app.innerHTML = drafts.length
      ? '<div class="section-title">Unsaved drafts on this browser</div><div class="cards">' + drafts.map((d) => `<button class="card" data-action="open-draft" data-key="${esc(d.key)}"><div class="card-title">${esc(d.edit.title || 'Untitled')}</div><div class="card-snippet">${esc(md.plain(d.edit.body).slice(0, 180))}</div></button>`).join('') + '</div>'
      : empty(ICON.check, 'Everything is saved', 'There are no unsaved drafts on this browser.');
  }

  function showDraft(key) {
    const s = saving.get(key);
    if (!s) return go('#/drafts', { replace: true });
    return showNote(s.note.id, { fresh: s.note, draft: s });
  }

  function newNote(dir) {
    return showNote(null, { fresh: { id: null, title: '', body: '', tags: [], directory: dir || '', pinned: false, version: null } });
  }

  async function ready() {
    const s = state.session;
    if (!s) return null;
    s.doc.stop();
    await flush(s);
    return s.note.id ? s : null;
  }

  async function showSearch(query) {
    const mine = ++seq;
    state = { view: 'search', dir: '', query };
    chrome({});
    openSearch(query);
    if (!query.trim()) {
      app.innerHTML = empty(ICON.search, 'Search every note', 'Titles, text and tags. Start a word with # to search tags.');
      return;
    }
    const results = await api(`/api/search?q=${enc(query)}`);
    if (mine !== seq) return;
    const words = query.split(/\s+/).map((w) => w.replace(/^#/, '')).filter(Boolean);
    app.innerHTML = results.length
      ? `<div class="section-title">${results.length} note${results.length === 1 ? '' : 's'}</div><div class="cards">${results
          .map((n) => card(n, { words, showFolder: true }))
          .join('')}</div>`
      : empty(ICON.search, 'Nothing found', `No note mentions “${query}”.`);
  }

  async function showTags() {
    const mine = ++seq;
    state = { view: 'tags', dir: '' };
    chrome({ showBack: true });
    const tags = await api('/api/tags');
    if (mine !== seq) return;
    app.innerHTML = tags.length
      ? `<div class="section-title">Tags</div><div class="tag-cloud">${tags
          .map((t) => `<button class="chip" data-action="tag" data-tag="${esc(t.tag)}">#${esc(t.tag)} <b>${t.count}</b></button>`)
          .join('')}</div>`
      : empty(ICON.tag, 'No tags yet', 'Add tags when you edit a note, or write #tag in leo on your computer.');
  }

  async function showTrash() {
    const mine = ++seq;
    state = { view: 'trash', dir: '' };
    chrome({ showBack: true });
    const items = await api('/api/trash');
    if (mine !== seq) return;
    app.innerHTML = items.length
      ? `<div class="section-title">Trash</div><div class="panel">${items
          .map(
            (t) => `<div class="list-row">${ICON.note}<span class="grow"><div>${esc(t.title)}</div><div class="sub">${esc(
              t.directory ? '/' + t.directory : 'All notes'
            )} · deleted ${rel(t.deleted_at)}</div></span><button class="btn plain" data-action="restore" data-id="${esc(t.id)}">Restore</button></div>`
          )
          .join('')}</div><p class="hint" style="margin:14px 4px">Deleted notes stay here for 30 days. To empty the trash now, run :trash empty in leo on your computer.</p>`
      : empty(ICON.trash, 'The trash is empty', 'Deleted notes wait here for 30 days, so a mistake can be undone.');
  }

  let mapView = null;
  let mapPoll = 0;
  let mapSheet = 'peek';
  const leoGraph = window.leoGraph;
  const plural = (n, word, many = `${word}s`) => `${n} ${n === 1 ? word : many}`;
  const darkScheme = () => Boolean(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
  const className = (n) => (n.top ? n.top : 'Unfiled');
  const mapWide = () => window.innerWidth >= 900;

  function leaveMap() {
    clearTimeout(mapPoll);
    if (mapView) mapView.destroy();
    mapView = null;
    document.body.classList.remove('on-map');
  }

  async function showMap(focus) {
    const mine = ++seq;
    leaveMap();
    state = { view: 'map', dir: '', focus, status: null };
    chrome({ showBack: true });
    $('#crumbs').innerHTML = '<span class="sep">/</span><button data-action="map-clear">Map of ideas</button>';
    document.title = 'Map of ideas · leo';
    app.innerHTML = skeleton(3);
    const data = await api('/api/graph');
    if (mine !== seq) return;
    drawMap(data, focus ? `n:${focus}` : null);
  }

  function drawMap(data, select, keep) {
    state.status = data.status;
    if (!data.graph.nodes.some((n) => n.kind === 'note')) {
      app.innerHTML = empty(ICON.map, 'Nothing to map yet', 'Write a few notes, and the map shows how they connect.');
      return;
    }
    document.body.classList.add('on-map');
    app.innerHTML = `<section class="map" id="map">
      <canvas id="map-canvas" role="img" aria-label="Map of how your notes connect. The panel lists the same connections."></canvas>
      <div class="map-tools">
        <label class="map-search">${ICON.search}<input id="map-find" type="search" placeholder="Find a note or idea" autocomplete="off" enterkeyhint="go"></label>
        <div class="map-found" id="map-found"></div>
        <div class="map-status" id="map-status"></div>
        <div class="map-chips" id="map-chips"></div>
      </div>
      <div class="map-zoom">
        <button data-action="map-zoom-in" aria-label="Zoom in">${ICON.plus}</button>
        <button data-action="map-zoom-out" aria-label="Zoom out">${ICON.minus}</button>
        <button data-action="map-fit" aria-label="Fit the whole map">${ICON.fit}</button>
      </div>
      <aside class="map-panel ${mapSheet}" id="map-panel" aria-live="polite"></aside>
    </section>`;
    $('#map').style.top = `${Math.round($('#bar').getBoundingClientRect().bottom)}px`;
    mapView = leoGraph.create($('#map-canvas'), data.graph, {
      onSelect: mapPanel,
      onOpen: (id) => go(noteHash(id.slice(2))),
      onChange: mapChips,
      insets: mapInsets,
    });
    if (keep) mapView.setOptions(keep);
    mapChips(mapView.options());
    mapStatus(data.status);
    const input = $('#map-find');
    input.addEventListener('input', () => mapFound(input.value));
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        input.value = '';
        mapFound('');
        input.blur();
      }
      if (e.key !== 'Enter') return;
      const first = leoGraph.find(mapView.graph, input.value)[0];
      if (first) mapPick(first.id);
    });
    if (select && mapView.graph.byId.has(select)) mapView.select(select, { center: true });
    else mapPanel(null);
    if (data.status.state === 'building') pollMap();
  }

  function mapInsets() {
    const map = $('#map');
    if (!map) return { top: 0, bottom: 0, right: 0 };
    const box = map.getBoundingClientRect();
    const top = $('.map-tools').getBoundingClientRect().bottom - box.top + 10;
    const panel = $('#map-panel');
    if (mapWide()) return { top, bottom: 0, right: box.right - panel.getBoundingClientRect().left + 12 };
    const tall = panel.classList.contains('open') ? Math.min(panel.scrollHeight, window.innerHeight * 0.5) : 96;
    return { top, bottom: tall + 10, right: 0 };
  }

  function mapChips(opts) {
    const chips = $('#map-chips');
    if (!chips || !mapView) return;
    const g = mapView.graph;
    const dark = darkScheme();
    const toggle = (action, on, label, title) =>
      `<button class="map-chip toggle${on ? ' on' : ''}" data-action="${action}" aria-pressed="${on}" title="${esc(title)}">${label}</button>`;
    const classes =
      g.folders.length > 1
        ? g.folders
            .map((top) => {
              const off = opts.hidden.has(top);
              const color = leoGraph.colorOf(g, top, dark) || 'var(--faint)';
              return `<button class="map-chip${off ? ' off' : ''}" data-action="map-class" data-top="${esc(top)}" aria-pressed="${!off}"><i style="background:${color}"></i>${esc(top || 'Unfiled')}</button>`;
            })
            .join('')
        : '';
    chips.innerHTML =
      toggle('map-across', opts.crossOnly, `Across classes${leoGraph.counts(g).across ? ` · ${leoGraph.counts(g).across}` : ''}`, 'Show only connections between different classes') +
      (g.nodes.some((n) => n.kind === 'concept') ? toggle('map-ideas', opts.ideas, 'Ideas', 'Show the shared ideas as their own dots') : '') +
      (opts.focus ? `<button class="map-chip toggle on" data-action="map-unfocus">Focused · show all</button>` : '') +
      classes;
  }

  function mapStatus(status) {
    state.status = status;
    const box = $('#map-status');
    if (!box || !mapView) return;
    const c = leoGraph.counts(mapView.graph);
    const building = status.state === 'building';
    const summary = c.connections ? `${plural(c.notes, 'note')} · ${plural(c.connections, 'connection')}` : plural(c.notes, 'note');
    let button = '';
    if (building) button = `<button class="btn primary sm" disabled>${status.total ? `Connecting ${status.done}/${status.total}…` : 'Starting…'}</button>`;
    else if (status.read === 0) button = '<button class="btn primary sm" data-action="map-build">Connect notes</button>';
    else if (status.stale > 0 || status.requests > 0) button = `<button class="btn primary sm" data-action="map-build">Update${status.stale ? ` · ${status.stale} changed` : ''}</button>`;
    const bar = building && status.total ? `<span class="map-progress"><i style="width:${Math.round((status.done / status.total) * 100)}%"></i></span>` : '';
    box.innerHTML = `<span class="map-summary">${summary}</span>${button}${bar}`;
  }

  function mapFound(query) {
    const found = leoGraph.find(mapView.graph, query);
    const dark = darkScheme();
    $('#map-found').innerHTML = found
      .map((n) => {
        const dot = n.kind === 'note' ? `<i class="dot" style="background:${leoGraph.colorOf(mapView.graph, n.top, dark) || 'var(--faint)'}"></i>` : '<i class="dot idea"></i>';
        return `<button class="map-row" data-action="map-pick" data-id="${esc(n.id)}">${dot}<span class="grow">${esc(n.label)}<span class="sub">${n.kind === 'note' ? esc(className(n)) : `Idea in ${plural(n.degree, 'note')}`}</span></span></button>`;
      })
      .join('');
  }

  function mapPick(id) {
    $('#map-find').value = '';
    $('#map-found').innerHTML = '';
    $('#map-find').blur();
    mapView.select(id, { center: true });
  }

  function mapSheetTo(next) {
    mapSheet = next;
    const panel = $('#map-panel');
    if (!panel) return;
    panel.classList.toggle('peek', next === 'peek');
    panel.classList.toggle('open', next === 'open');
  }

  const dotFor = (n) => `<i class="dot" style="background:${leoGraph.colorOf(mapView.graph, n.top, darkScheme()) || 'var(--faint)'}"></i>`;

  function connectionRow(c) {
    return `<button class="map-row" data-action="map-select" data-id="${esc(c.node.id)}">${dotFor(c.node)}<span class="grow"><span class="row-head">${esc(c.node.label)}<em class="badge${c.cross ? ' across' : ''}">${esc(c.label)}</em></span>${
      c.edge.why ? `<span class="sub">${esc(c.edge.why)}</span>` : c.edge.kind === 'link' ? '<span class="sub">A link you wrote</span>' : ''
    }<span class="sub class">${esc(className(c.node))}${c.linked && c.edge.kind !== 'link' ? ' · you linked these' : ''}</span></span></button>`;
  }

  function mapPanel(node) {
    const panel = $('#map-panel');
    if (!panel || !mapView) return;
    const g = mapView.graph;
    const status = state.status || {};
    const grip = '<button class="map-grip" data-action="map-sheet" aria-label="Show more or less"><i></i></button>';
    if (!node) {
      mapSheetTo(mapSheet === 'open' && !mapWide() ? 'peek' : mapSheet);
      const c = leoGraph.counts(g);
      if (!c.connections) {
        const first = status.read === 0;
        panel.innerHTML = `${grip}<div class="panel-head"><h3>Connect your notes</h3></div>
          <p class="hint">${first
            ? 'leo can read your notes with your AI and connect the ones worth studying together, especially across classes: the same method in two courses, an idea one class builds on, two approaches that contrast. Each connection says why.'
            : 'No connections yet. Update reads any notes you changed and connects them.'}</p>
          ${status.state === 'building' ? '' : `<div class="map-actions"><button class="btn primary sm" data-action="map-build">${first ? 'Connect notes' : 'Update'}</button></div>`}
          <p class="hint small">Lines you see already are the [[links]] you wrote.</p>`;
        return;
      }
      const top = leoGraph.strongest(g, 12);
      panel.innerHTML = `${grip}<div class="panel-head"><h3>Strongest connections</h3><span class="sub">${plural(c.across, 'connection')} across classes · tap one to explore</span></div>
        <div class="map-list">${top
          .map(
            (x) => `<button class="map-row" data-action="map-select" data-id="${esc(x.a.id)}">${dotFor(x.a)}<span class="grow"><span class="row-head">${esc(x.a.label)} <span class="arrow">↔</span> ${esc(x.b.label)}<em class="badge${x.cross ? ' across' : ''}">${esc(leoGraph.relationFrom(x.edge, x.a.id))}</em></span>${
              x.edge.why ? `<span class="sub">${esc(x.edge.why)}</span>` : ''
            }<span class="sub class">${esc(className(x.a))} · ${esc(className(x.b))}</span></span>${dotFor(x.b)}</button>`
          )
          .join('')}</div>`;
      return;
    }
    mapSheetTo('open');
    const close = `<button class="icon-btn map-close" data-action="map-clear" aria-label="Close">${ICON.close}</button>`;
    if (node.kind === 'note') {
      const id = node.id.slice(2);
      const all = leoGraph.connections(g, node.id);
      const across = all.filter((x) => x.cross);
      const inside = all.filter((x) => !x.cross);
      const focused = mapView.options().focus === node.id;
      const ideas = node.concepts.length
        ? `<h4>Ideas</h4><div class="map-ideas">${node.concepts
            .map((c) => {
              const cid = `c:${c.toLowerCase()}`;
              return g.byId.has(cid)
                ? `<button class="chip accent" data-action="map-select" data-id="${esc(cid)}">${esc(c)}</button>`
                : `<span class="chip">${esc(c)}</span>`;
            })
            .join('')}</div>`
        : '';
      const empty = !all.length
        ? `<p class="hint">${status.read === 0 || status.stale ? 'Not connected yet. Connect notes reads it.' : 'No strong connections to other notes yet.'}</p>`
        : '';
      panel.innerHTML = `${grip}${close}<div class="panel-head"><span class="sub class">${dotFor(node)}${esc(className(node))}</span><h3>${esc(node.label)}</h3>${
        node.summary ? `<p class="summary">${esc(node.summary)}</p>` : ''
      }<div class="map-actions"><button class="btn primary sm" data-action="open-note" data-id="${esc(id)}">Open note</button><button class="btn plain sm" data-action="map-focus" data-id="${esc(node.id)}">${focused ? 'Show all' : 'Focus'}</button></div></div>
        ${across.length ? `<h4>Across classes · ${across.length}</h4><div class="map-list">${across.map(connectionRow).join('')}</div>` : ''}
        ${inside.length ? `<h4>In ${esc(className(node))} · ${inside.length}</h4><div class="map-list">${inside.map(connectionRow).join('')}</div>` : ''}
        ${empty}${ideas}`;
      return;
    }
    const notes = leoGraph.conceptNotes(g, node.id);
    panel.innerHTML = `${grip}${close}<div class="panel-head"><span class="sub class"><i class="dot idea"></i>Idea</span><h3>${esc(node.label)}</h3><span class="sub">In ${plural(notes.length, 'note')} across ${plural(new Set(notes.map((n) => n.top)).size, 'class', 'classes')}</span></div>
      <div class="map-list">${notes
        .map((n) => `<button class="map-row" data-action="map-select" data-id="${esc(n.id)}">${dotFor(n)}<span class="grow">${esc(n.label)}${n.summary ? `<span class="sub">${esc(n.summary)}</span>` : ''}<span class="sub class">${esc(className(n))}</span></span></button>`)
        .join('')}</div>`;
  }

  async function mapBuild(confirmed) {
    const status = state.status || {};
    if (!confirmed && status.read === 0) {
      sheet(`<h3>Connect your notes</h3>
        <p>leo sends your notes to the AI you chose for writing (with :settings in leo on your computer). It reads what each note teaches, then connects the notes worth studying together, especially across classes, and says why. That is about ${plural(status.requests || 1, 'request')}. After that, only notes you change are read again.</p>
        <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="map-build-now">Connect notes</button></div>`);
      return;
    }
    closeSheet();
    mapStatus(await api('/api/graph/build', { method: 'POST' }));
    pollMap();
  }

  function pollMap() {
    clearTimeout(mapPoll);
    mapPoll = setTimeout(async () => {
      if (state.view !== 'map') return;
      const status = await api('/api/graph/status').catch(() => null);
      if (!status || state.view !== 'map') return;
      if (status.state === 'building') {
        mapStatus(status);
        pollMap();
        return;
      }
      if (status.state === 'failed') {
        mapStatus(status);
        mapPanel(mapView && mapView.selected() ? mapView.graph.byId.get(mapView.selected()) : null);
        toast(status.message || 'The map could not be built.', { bad: true });
        return;
      }
      const chosen = mapView ? mapView.selected() : null;
      const keep = mapView ? mapView.options() : null;
      const data = await api('/api/graph');
      if (state.view !== 'map') return;
      leaveMap();
      drawMap(data, chosen, keep);
      const c = mapView ? leoGraph.counts(mapView.graph) : { connections: 0, across: 0 };
      toast(status.message || `Connected: ${plural(c.connections, 'connection')}, ${c.across} across classes.`);
    }, 1200);
  }

  function showLocked() {
    ++seq;
    state = { view: 'locked' };
    chrome({});
    app.innerHTML = empty(
      ICON.lock,
      'This page needs its link',
      'Open the link `leo serve` printed on your computer, or scan its QR code again. If the link was renewed with --new-token, older links stop working.'
    );
  }

  function sheet(html) {
    closeSheet();
    const scrim = document.createElement('div');
    scrim.className = 'scrim';
    scrim.innerHTML = `<div class="sheet" role="dialog" aria-modal="true"><div class="grip"></div>${html}</div>`;
    scrim.addEventListener('click', (e) => {
      if (e.target === scrim) closeSheet();
    });
    document.body.appendChild(scrim);
    return scrim;
  }

  function closeSheet() {
    const open = $('.scrim');
    if (open) open.remove();
  }

  function menu() {
    const here = state.view === 'folder' ? state.dir : '';
    sheet(`
      <button class="list-row" data-action="new" data-dir="${esc(here)}">${ICON.plus}<span class="grow">New note</span></button>
      <button class="list-row" data-action="new-folder">${ICON.folderPlus}<span class="grow">New folder${here ? ` in ${esc(folderLabel(here))}` : ''}</span></button>
      <button class="list-row" data-action="map">${ICON.map}<span class="grow">Map of ideas</span></button>
      <button class="list-row" data-action="tags">${ICON.tag}<span class="grow">Tags</span></button>
      <button class="list-row" data-action="drafts">${ICON.note}<span class="grow">Drafts (${saving.drafts().length})</span></button>
      <button class="list-row" data-action="trash">${ICON.trash}<span class="grow">Trash</span></button>
      <button class="list-row" data-action="refresh">${ICON.refresh}<span class="grow">Refresh</span></button>`);
  }

  function newFolder() {
    const parent = state.view === 'folder' ? state.dir : '';
    const scrim = sheet(`<h3>New folder${parent ? ` in ${esc(folderLabel(parent))}` : ''}</h3>
      <label class="field">${ICON.folder}<input id="folder-name" placeholder="Name, e.g. cs130" autocomplete="off" enterkeyhint="done"></label>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="create-folder">Create</button></div>`);
    const input = $('#folder-name', scrim);
    input.focus();
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') createFolder();
    });
  }

  async function createFolder() {
    const name = $('#folder-name').value.trim().replace(/^\/+|\/+$/g, '');
    if (!name) return;
    const parent = state.view === 'folder' ? state.dir : '';
    const path = parent ? `${parent}/${name}` : name;
    try {
      await api('/api/dirs', { method: 'POST', body: { path } });
      closeSheet();
      go(folderHash(path));
    } catch (e) {
      if (e.status === 409) toast('That folder already exists.', { bad: true });
      else fail(e);
    }
  }

  async function moveSheet() {
    const s = await ready();
    if (!s) return toast('Write something first.');
    const note = s.note;
    const folders = await api('/api/folders');
    const row = (name, label) =>
      `<button class="list-row" data-action="move-to" data-dir="${esc(name)}">${ICON.folder}<span class="grow">${esc(label)}</span>${
        name === note.directory ? ICON.check : ''
      }</button>`;
    sheet(`<h3>Move “${esc(note.title)}”</h3>${row('', 'All notes (top level)')}${folders.map((f) => row(f.name, f.name)).join('')}`);
  }

  async function moveTo(dir) {
    const note = state.session.note;
    closeSheet();
    if (dir === note.directory) return;
    try {
      await api(`/api/notes/${enc(note.id)}/move`, { method: 'POST', body: { directory: dir } });
      toast(`Moved to ${folderLabel(dir)}`);
      showNote(note.id);
    } catch (e) {
      fail(e);
    }
  }

  async function confirmDelete() {
    const s = await ready();
    if (!s) return back();
    sheet(`<h3>Move “${esc(s.note.title)}” to the trash?</h3><p>You can restore it for 30 days, from Trash in the menu.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="delete-now">Move to trash</button></div>`);
  }

  async function deleteNow() {
    const note = state.session.note;
    state.session = null;
    closeSheet();
    try {
      await api(`/api/notes/${enc(note.id)}`, { method: 'DELETE' });
      go(folderHash(note.directory), { replace: true });
      toast('Moved to the trash', { action: 'Undo', run: () => restore(note.id, true) });
    } catch (e) {
      fail(e);
    }
  }

  async function restore(id, open) {
    try {
      const note = await api(`/api/trash/${enc(id)}/restore`, { method: 'POST' });
      if (open) return go(noteHash(note.id));
      toast(`Restored “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
      showTrash();
    } catch (e) {
      fail(e);
    }
  }

  async function pinCard(el) {
    const on = !el.classList.contains('on');
    el.classList.toggle('on', on);
    el.setAttribute('aria-pressed', String(on));
    try {
      await api(`/api/notes/${enc(el.dataset.id)}`, { method: 'PATCH', body: { pinned: on } });
      toast(on ? 'Pinned to the top of its folder' : 'Unpinned');
      await render();
    } catch (e) {
      el.classList.toggle('on', !on);
      el.setAttribute('aria-pressed', String(!on));
      fail(e);
    }
  }

  async function share() {
    const s = await ready();
    if (!s) return toast('Write something first.');
    const before = document.title;
    document.title = s.note.title;
    const restore = () => {
      document.title = before;
      window.removeEventListener('afterprint', restore);
    };
    window.addEventListener('afterprint', restore);
    window.print();
  }

  function openSearch(value) {
    const box = $('#search');
    box.hidden = false;
    const input = $('#search-input');
    if (value !== undefined && document.activeElement !== input) input.value = value;
    $('#search-toggle').innerHTML = ICON.close;
  }

  function closeSearch() {
    $('#search').hidden = true;
    $('#search-input').value = '';
    $('#search-toggle').innerHTML = ICON.search;
  }

  let searchTimer;
  $('#search-input').addEventListener('input', (e) => {
    clearTimeout(searchTimer);
    const query = e.target.value;
    searchTimer = setTimeout(() => {
      if (state.view === 'search') history.replaceState(null, '', `#/search/${enc(query)}`);
      else history.pushState(null, '', `#/search/${enc(query)}`);
      showSearch(query).catch(fail);
    }, 160);
  });

  const actions = {
    back,
    home: () => go('#/'),
    'toggle-search': () => {
      if ($('#search').hidden) {
        openSearch('');
        $('#search-input').focus();
      } else {
        closeSearch();
        if (state.view === 'search') back();
      }
    },
    menu,
    close: closeSheet,
    'open-folder': (el) => {
      closeSheet();
      go(folderHash(el.dataset.dir));
    },
    'open-note': (el) => go(noteHash(el.dataset.id)),
    'pin-card': pinCard,
    new: (el) => {
      closeSheet();
      go(el.dataset.dir ? `#/new/${enc(el.dataset.dir)}` : '#/new');
    },
    'new-folder': newFolder,
    'create-folder': createFolder,
    tags: () => {
      closeSheet();
      go('#/tags');
    },
    map: () => {
      closeSheet();
      go('#/map');
    },
    'note-map': (el) => go(`#/map/${enc(el.dataset.id)}`),
    'map-build': () => mapBuild(false),
    'map-build-now': () => mapBuild(true),
    'map-select': (el) => mapView && mapView.select(el.dataset.id, { center: true }),
    'map-pick': (el) => mapView && mapPick(el.dataset.id),
    'map-clear': () => mapView && mapView.select(null),
    'map-fit': () => mapView && mapView.fit(),
    'map-zoom-in': () => mapView && mapView.zoomBy(1.35),
    'map-zoom-out': () => mapView && mapView.zoomBy(1 / 1.35),
    'map-across': () => mapView && mapView.setOptions({ crossOnly: !mapView.options().crossOnly }),
    'map-ideas': () => mapView && mapView.setOptions({ ideas: !mapView.options().ideas }),
    'map-unfocus': () => {
      if (!mapView) return;
      mapView.setOptions({ focus: null });
      const chosen = mapView.selected();
      if (chosen) mapPanel(mapView.graph.byId.get(chosen));
    },
    'map-focus': (el) => {
      if (!mapView) return;
      const on = mapView.options().focus === el.dataset.id;
      mapView.setOptions({ focus: on ? null : el.dataset.id, depth: 2 });
      mapView.select(el.dataset.id);
    },
    'map-class': (el) => {
      if (!mapView) return;
      const hidden = mapView.options().hidden;
      if (hidden.has(el.dataset.top)) hidden.delete(el.dataset.top);
      else hidden.add(el.dataset.top);
      mapView.setOptions({ hidden });
    },
    'map-sheet': () => mapSheetTo(mapSheet === 'open' ? 'peek' : 'open'),
    drafts: () => { closeSheet(); go('#/drafts'); },
    'open-draft': (el) => go(`#/draft/${enc(el.dataset.key)}`),
    trash: () => {
      closeSheet();
      go('#/trash');
    },
    refresh: () => {
      closeSheet();
      saving.retry();
      route();
    },
    tag: (el) => go(`#/search/${enc('#' + el.dataset.tag)}`),
    untag: (el) => {
      const s = state.session;
      s.tags = s.tags.filter((t) => t !== el.dataset.tag);
      changed(s);
      drawTags(s);
    },
    move: () => moveSheet(),
    'move-to': (el) => moveTo(el.dataset.dir),
    share,
    delete: confirmDelete,
    'delete-now': deleteNow,
    restore: (el) => restore(el.dataset.id, false),
  };

  document.addEventListener('click', (e) => {
    const el = e.target.closest('[data-action]');
    if (!el) return;
    const act = actions[el.dataset.action];
    if (!act) return;
    e.preventDefault();
    Promise.resolve(act(el)).catch(fail);
  });

  document.addEventListener('keydown', (e) => {
    const typing = e.target.closest('input, textarea, select, [contenteditable]');
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && state.view === 'note') {
      e.preventDefault();
      flush().catch(fail);
      return;
    }
    if (e.key === 'Escape') {
      if ($('.scrim')) return closeSheet();
      if (typing) return e.target.blur();
      if (state.view === 'map' && mapView && mapView.selected()) return mapView.select(null);
      if (state.view !== 'folder' || state.dir) return back();
    }
    if (e.key === 'Enter' && e.target.matches('.card')) {
      e.preventDefault();
      go(noteHash(e.target.dataset.id));
      return;
    }
    if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === '/') {
      e.preventDefault();
      openSearch('');
      $('#search-input').focus();
    } else if (e.key === 'n' && state.view === 'folder') {
      go(state.dir ? `#/new/${enc(state.dir)}` : '#/new');
    }
  });

  const unsaved = () => saving.unsaved();

  window.addEventListener('beforeunload', (e) => {
    if (!unsaved()) return;
    flush().catch(() => {});
    e.preventDefault();
  });

  window.addEventListener('hashchange', () => {
    const leaving = state.session;
    if (leaving) {
      leaving.doc.stop();
      flush(leaving).catch(fail);
      leaving.doc.destroy();
    }
    route();
  });

  const busy = () => {
    const active = document.activeElement;
    return unsaved() || Boolean(active && active !== document.body && app.contains(active));
  };

  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') {
      if (state.session) flush().catch(() => {});
      return;
    }
    if (!['folder', 'note', 'search', 'trash', 'tags'].includes(state.view)) return;
    if (state.view === 'note' && (busy() || !state.session.note.id)) return;
    render().catch(fail);
  });

  window.addEventListener(
    'scroll',
    () => $('#bar').classList.toggle('scrolled', window.scrollY > 4),
    { passive: true }
  );

  async function route() {
    closeSheet();
    window.scrollTo(0, 0);
    await render();
  }

  async function render() {
    const hash = decodeURI(location.hash || '#/');
    const [, kind, rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [null, '', ''];
    const arg = decodeURIComponent(rest);
    if (kind !== 'search') closeSearch();
    if (kind !== 'map') leaveMap();
    try {
      if (kind === 'f') await showFolder(arg);
      else if (kind === 'n') await showNote(arg.replace(/\/edit$/, ''));
      else if (kind === 'new') await newNote(arg);
      else if (kind === 'search') await showSearch(arg);
      else if (kind === 'tags') await showTags();
      else if (kind === 'trash') await showTrash();
      else if (kind === 'map') await showMap(arg);
      else if (kind === 'drafts') showDrafts();
      else if (kind === 'draft') await showDraft(arg);
      else await showFolder('');
    } catch (e) {
      if (e.status === 404 && hash.startsWith('#/n/')) {
        toast('That note is not there any more.', { bad: true });
        go('#/', { replace: true });
      } else if (e instanceof Offline) {
        app.innerHTML = empty(ICON.cloud, "Can't reach leo", 'Check that `leo serve` is still running on your computer, then pull down or tap Refresh in the menu.');
      } else {
        fail(e);
      }
    }
  }

  window.addEventListener('online', () => saving.retry());
  route();
})();
