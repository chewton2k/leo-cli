(function () {
  'use strict';

  const md = window.leoMarkdown;
  const leoDoc = window.leoDoc;
  const felix = window.leoChat;
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
    upload: svg('<path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3"/>'),
    image: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/>'),
    paperclip: svg('<path d="M21 11l-8.5 8.5a5 5 0 0 1-7-7L14 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 7"/>'),
    gear: svg('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"/>'),
    fit: svg('<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>'),
    restore: svg('<path d="M4 12a8 8 0 1 0 2.3-5.6L4 8.5"/><path d="M4 4v4.5h4.5"/>'),
    note: svg('<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>'),
    refresh: svg('<path d="M20 12a8 8 0 1 1-2.3-5.6L20 8.5"/><path d="M20 4v4.5h-4.5"/>'),
    check: svg('<path d="M5 12l4.5 4.5L19 7"/>'),
    chevron: svg('<path d="M9 6l6 6-6 6"/>'),
    lock: svg('<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>'),
    map: svg('<circle cx="6" cy="7" r="2.2"/><circle cx="18" cy="6" r="2.2"/><circle cx="12" cy="17.5" r="2.2"/><path d="M7.4 8.9l3.5 6.7M16.9 7.9l-3.8 7.8M8.2 6.8l7.6-.6"/>'),
    mic: svg('<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>'),
    screen: svg('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>'),
    cloud: svg('<path d="M7 18a4.5 4.5 0 0 1-.5-9 6 6 0 0 1 11.3 1.5A3.8 3.8 0 0 1 17.5 18z"/><path d="M4 4l16 16"/>'),
  };

  const recorder = window.leoRecording.create({ api, esc, toast, go: (hash, opts) => go(hash, opts), noteHash: (id) => noteHash(id), felix, icons: ICON });
  const chat = felix.create({
    render: (text) => md.render(text),
    escape: md.escape,
    onOpen: (id) => go(noteHash(id)),
    prepare: async (file) => {
      const ready = await shrink(file);
      return { name: ready.name, type: ready.type, data: await base64(ready.blob) };
    },
    notify: (message) => toast(message, { bad: true }),
  });
  $('#chat-toggle').innerHTML = chat.button(36);
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
      let said = '';
      try {
        said = (await response.json()).error || '';
      } catch (e) {
        said = '';
      }
      const error = new Error(said || (response.status === 404 ? 'That is not there any more.' : response.status === 400 ? 'Use a folder name inside your notes, without . or ..' : `leo answered ${response.status}.`));
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

  function card(note, { words = [], showFolder = false, pick = null } = {}) {
    const where = showFolder && note.directory ? `<span class="chip accent">${esc(note.directory)}</span>` : '';
    const text = snippet(note.body, words);
    const open = pick
      ? `class="card pick-card${pick.on ? ' picked' : ''}" role="checkbox" tabindex="0" aria-checked="${pick.on}" data-action="folder-pick" data-key="${esc(pick.key)}"`
      : `class="card" role="link" tabindex="0" data-action="open-note" data-id="${esc(note.id)}"`;
    const corner = pick ? `<span class="pick-box${pick.on ? ' on' : ''}" aria-hidden="true">${ICON.check}</span>` : pinButton(note);
    return `<div ${open}>
      <div class="card-title"><span>${words.length ? highlight(note.title, words) : esc(note.title)}</span>${corner}</div>
      ${text ? `<div class="card-snippet">${text}</div>` : ''}
      <div class="card-meta">${where}<span>${rel(note.updated_at)}</span>${progress(note.body)}</div>
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
    if (state.view === 'storage') return go('#/settings');
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

  const newButton = (dir) => `<div class="fabs"><button class="fab ghost" data-action="record" data-dir="${esc(dir)}" aria-label="Record" title="Record a lecture or meeting">${ICON.mic}<span>Record</span></button><button class="fab ghost" data-action="upload" aria-label="Upload a file" title="Make a note from a file or photo">${ICON.upload}<span>Upload</span></button><button class="fab" data-action="new" data-dir="${esc(dir)}">${ICON.plus}<span>New note</span></button></div>`;

  async function showFolder(dir) {
    const mine = ++seq;
    const before = state.view === 'folder' && state.dir === dir ? state : null;
    state = { view: 'folder', dir, selecting: before ? before.selecting : false, picked: new Set() };
    chrome({ dir, showBack: Boolean(dir), fab: newButton(dir) });
    if (!app.innerHTML.trim()) app.innerHTML = skeleton(4);
    const [dirs, notes] = await Promise.all([api(`/api/dirs?parent=${enc(dir)}`), api(`/api/notes?dir=${enc(dir)}&limit=1000`)]);
    if (mine !== seq) return;
    state.listing = { dirs, notes };
    drawFolder();
  }

  function folderKeys() {
    const { dirs, notes } = state.listing;
    const dir = state.dir;
    return [...dirs.map((d) => `d:${dir ? `${dir}/${d.name}` : d.name}`), ...notes.map((n) => `n:${n.id}`)];
  }

  function drawFolder() {
    const { dirs, notes } = state.listing;
    const dir = state.dir;
    const sel = state.selecting;
    const picked = state.picked;
    if (!dirs.length && !notes.length) {
      state.selecting = false;
      chrome({ dir, showBack: Boolean(dir), fab: newButton(dir) });
      app.innerHTML = dir
        ? empty(ICON.folder, 'This folder is empty', 'Tap “New note” to write the first one here.')
        : empty(ICON.note, 'No notes yet', 'Tap “New note” to write one. Notes you make in leo on your computer show up here too.');
      return;
    }
    const tick = (key) => (sel ? `<span class="pick-box${picked.has(key) ? ' on' : ''}" aria-hidden="true">${ICON.check}</span>` : '');
    let html = `<div class="folder-tools">${sel ? '<button class="btn sm plain" data-action="folder-select">Done</button>' : '<button class="btn sm plain" data-action="folder-select">Select</button>'}</div>`;
    if (dirs.length) {
      html += `<div class="section-title">Folders</div><div class="folders">${dirs
        .map((d) => {
          const full = dir ? `${dir}/${d.name}` : d.name;
          const key = `d:${full}`;
          return `<button class="folder${sel ? ' picking' : ''}${picked.has(key) ? ' picked' : ''}" data-action="${sel ? 'folder-pick' : 'open-folder'}" data-key="${esc(key)}" data-dir="${esc(full)}" aria-pressed="${sel ? picked.has(key) : ''}">${ICON.folder}<span><span class="name">${esc(d.name)}</span><span class="count">${d.notes} note${d.notes === 1 ? '' : 's'}</span></span>${tick(key)}</button>`;
        })
        .join('')}</div>`;
    }
    if (notes.length) {
      html += `<div class="section-title">Notes</div><div class="cards${sel ? ' picking' : ''}">${notes
        .map((n) => {
          const key = `n:${n.id}`;
          return card(n, sel ? { pick: { key, on: picked.has(key) } } : {});
        })
        .join('')}</div>`;
    }
    app.innerHTML = html;
    if (sel) {
      const all = picked.size === folderKeys().length;
      floating.innerHTML = `<div class="select-bar" role="toolbar" aria-label="Selected notes and folders">
          <label class="select-all"><input type="checkbox" data-folder-all${all ? ' checked' : ''}><span>All</span></label>
          <span class="select-count">${picked.size} selected</span>
          <button class="btn sm danger" data-action="folder-trash"${picked.size ? '' : ' disabled'}>Move to trash</button>
        </div>`;
    } else {
      chrome({ dir, showBack: Boolean(dir), fab: newButton(dir) });
    }
  }

  function folderTrashAsk() {
    const keys = [...state.picked];
    if (!keys.length) return;
    const dirs = keys.filter((k) => k.startsWith('d:')).map((k) => k.slice(2));
    const notes = keys.filter((k) => k.startsWith('n:')).map((k) => k.slice(2));
    const inside = state.listing.dirs
      .filter((d) => dirs.includes(state.dir ? `${state.dir}/${d.name}` : d.name))
      .reduce((sum, d) => sum + d.notes, 0);
    const parts = [];
    if (notes.length) parts.push(plural(notes.length, 'note'));
    if (dirs.length) parts.push(`${plural(dirs.length, 'folder')}${inside ? ` (with ${plural(inside, 'note')} inside)` : ''}`);
    state.trashMove = { notes, dirs };
    sheet(`<h3>Move ${esc(parts.join(' and '))} to the trash?</h3>
      <p>Notes stay in the trash for 30 days, so you can restore them from there.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="folder-trash-now">Move to trash</button></div>`);
  }

  async function folderTrashNow() {
    const move = state.trashMove;
    closeSheet();
    if (!move) return;
    state.trashMove = null;
    const done = await api('/api/trash/move', { method: 'POST', body: move });
    const parts = [];
    if (done.notes) parts.push(plural(done.notes, 'note'));
    if (done.folders) parts.push(plural(done.folders, 'folder'));
    toast(`Moved ${parts.join(' and ') || 'nothing'} to the trash`, { action: 'Open trash', run: () => go('#/trash') });
    state.selecting = false;
    await showFolder(state.dir);
  }

  floating.addEventListener('change', (e) => {
    if (state.view !== 'folder' || !e.target.closest('[data-folder-all]')) return;
    state.picked = e.target.checked ? new Set(folderKeys()) : new Set();
    drawFolder();
  });

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
    chat.setContext(note.id ? { id: note.id, title: note.title } : null);
    const where = `<button class="chip accent" data-action="open-folder" data-dir="${esc(note.directory)}">${ICON.folder.replace('<svg', '<svg width="13" height="13"')} ${esc(folderLabel(note.directory))}</button>`;
    const when = note.id ? `Edited ${rel(note.updated_at)}` : 'New note';
    app.innerHTML = `<article class="note">
      <header class="note-head"><h1 id="title" contenteditable="plaintext-only" spellcheck="true" data-placeholder="Title" enterkeyhint="next">${esc(s.edit.title)}</h1>
      <div class="note-meta">${where}<span id="save-state">${when}</span></div></header>
      <div class="prose doc" id="doc"></div>
    </article>`;
    s.title = $('#title');
    drawOriginals(note);
    s.doc = leoDoc.mount($('#doc'), { source: s.edit.body, onChange: () => changed(s), placeholder: 'Tap here to write' });
    if (s.dirty) {
      mark(s, 'Recovered draft · saving…');
      flush(s).catch(fail);
    }
    const blank = () => {
      const empty = !cleanTitle(s.title.textContent);
      if (empty && s.title.innerHTML !== '') s.title.textContent = '';
      s.title.classList.toggle('blank', empty);
    };
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
      app.innerHTML = empty(ICON.search, 'Search every note', 'Titles and text of every note.');
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

  async function showTrash() {
    const mine = ++seq;
    const before = state.view === 'trash' ? state : null;
    state = { view: 'trash', dir: '', selecting: before ? before.selecting : false, picked: new Set() };
    chrome({ showBack: true });
    $('#crumbs').innerHTML = '<span class="sep">/</span><button>Trash</button>';
    document.title = 'Trash · leo';
    const [items, keep] = await Promise.all([api('/api/trash'), api('/api/keep').catch(() => null)]);
    if (mine !== seq) return;
    state.items = items;
    state.trashDays = keep ? keep.trash_days : 30;
    if (!items.length) state.selecting = false;
    drawTrash();
  }

  function drawTrash() {
    const items = state.items || [];
    if (!items.length) {
      app.innerHTML = empty(ICON.trash, 'The trash is empty', 'Deleted notes wait here for 30 days, so a mistake can be undone.');
      floating.innerHTML = '';
      return;
    }
    const sel = state.selecting;
    const picked = state.picked;
    const rows = items
      .map((t) => {
        const box = sel ? `<input type="checkbox" class="trash-box" data-trash-pick value="${esc(t.id)}"${picked.has(t.id) ? ' checked' : ''} aria-label="Select ${esc(t.title)}">` : ICON.note;
        const actions = sel ? '' : `<button class="btn sm plain" data-action="restore" data-id="${esc(t.id)}">Restore</button><button class="icon-btn trash-forget" data-action="trash-forget" data-id="${esc(t.id)}" aria-label="Delete ${esc(t.title)} for good" title="Delete for good">${ICON.trash}</button>`;
        const row = sel ? 'label' : 'div';
        return `<${row} class="list-row trash-row${picked.has(t.id) ? ' picked' : ''}">${box}<span class="grow"><div>${esc(t.title)}</div><div class="sub">${esc(t.directory ? '/' + t.directory : 'All notes')} · deleted ${rel(t.deleted_at)}</div></span>${actions}</${row}>`;
      })
      .join('');
    const all = picked.size === items.length;
    app.innerHTML = `<div class="trash-head"><div class="section-title">Trash · ${items.length} note${items.length === 1 ? '' : 's'}</div>
        <div class="trash-tools">${sel ? '<button class="btn sm plain" data-action="trash-select">Done</button>' : `<button class="btn sm plain" data-action="trash-select">Select</button><button class="btn sm plain danger-text" data-action="trash-empty">Empty trash</button>`}</div></div>
      <div class="panel">${rows}</div>
      <p class="hint" style="margin:14px 4px">${state.trashDays === null ? 'Deleted notes stay here until you empty the trash.' : `Deleted notes stay here for ${state.trashDays === 365 ? 'a year' : plural(state.trashDays, 'day')}, then leave on their own.`} <a href="#/settings/storage">Change</a></p>`;
    floating.innerHTML = sel
      ? `<div class="select-bar" role="toolbar" aria-label="Selected notes">
          <label class="select-all"><input type="checkbox" data-trash-all${all ? ' checked' : ''}><span>All</span></label>
          <span class="select-count">${picked.size} selected</span>
          <button class="btn sm plain" data-action="trash-restore-picked"${picked.size ? '' : ' disabled'}>Restore</button>
          <button class="btn sm danger" data-action="trash-delete-picked"${picked.size ? '' : ' disabled'}>Delete</button>
        </div>`
      : '';
  }

  function trashAsk(ids, all) {
    const items = state.items || [];
    const names = all ? items : items.filter((t) => ids.includes(t.id));
    const what = all ? `all ${plural(items.length, 'note')} in the trash` : names.length === 1 ? `“${names[0].title}”` : plural(names.length, 'note');
    state.trashPending = { ids, all };
    sheet(`<h3>Delete ${esc(what)} for good?</h3>
      <p>${all || names.length > 1 ? 'They' : 'It'} cannot be restored afterwards.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="trash-delete-now">Delete for good</button></div>`);
  }

  async function trashDeleteNow() {
    const pending = state.trashPending;
    closeSheet();
    if (!pending) return;
    state.trashPending = null;
    const done = await api('/api/trash/delete', { method: 'POST', body: pending });
    toast(`Deleted ${plural(done.deleted, 'note')} for good`);
    state.picked = new Set();
    await showTrash();
  }

  async function trashRestorePicked() {
    const ids = [...state.picked];
    if (!ids.length) return;
    const done = await api('/api/trash/restore', { method: 'POST', body: { ids } });
    toast(`Restored ${plural(done.restored, 'note')}`);
    state.picked = new Set();
    state.selecting = false;
    await showTrash();
  }

  app.addEventListener('change', (e) => {
    if (state.view !== 'trash') return;
    const box = e.target.closest('[data-trash-pick]');
    if (!box) return;
    if (box.checked) state.picked.add(box.value);
    else state.picked.delete(box.value);
    drawTrash();
  });

  floating.addEventListener('change', (e) => {
    if (state.view !== 'trash' || !e.target.closest('[data-trash-all]')) return;
    state.picked = e.target.checked ? new Set((state.items || []).map((t) => t.id)) : new Set();
    drawTrash();
  });

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
    const again = !building && status.read > 0 ? '<button class="btn plain sm map-rebuild" data-action="map-rebuild" title="Read and connect every note again">Rebuild</button>' : '';
    box.innerHTML = `<span class="map-summary">${summary}</span>${button}${again}${bar}`;
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
    chat.setContext(node.kind === 'note' ? { id: node.id.slice(2), title: node.label } : null);
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

  function mapRebuildAsk() {
    const status = state.status || {};
    sheet(`<h3>Rebuild the map from scratch?</h3>
      <p>leo forgets what it found and reads and connects every note again with the AI you chose for writing. That is about ${plural(status.rebuild_requests || 1, 'request')}. Usually Update is enough: it only reads notes that are new or changed.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="map-rebuild-now">Rebuild</button></div>`);
  }

  async function mapRebuild() {
    closeSheet();
    mapStatus(await api('/api/graph/build?fresh=1', { method: 'POST' }));
    checkActivity();
    pollMap();
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
    checkActivity();
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

  const TASK_TITLE = { writing: 'AI for writing', speech: 'AI for speech' };
  const TASK_USE = {
    writing: 'Turns recordings into notes, answers @leo questions, powers Felix and the map.',
    speech: 'Turns what was said into text while you record.',
  };

  async function showRecord(dir) {
    state = { view: 'record', dir: '' };
    chrome({ showBack: true });
    $('#crumbs').innerHTML = '<span class="sep">/</span><button>Record</button>';
    document.title = 'Record · leo';
    app.innerHTML = '<div id="rec-box"></div>';
    let folders = [];
    try {
      folders = await api('/api/folders');
    } catch (e) {
      if (e instanceof Locked) throw e;
    }
    await recorder.show($('#rec-box'), { dir, folders: folders.map((f) => f.name).filter(Boolean) });
  }

  async function showSettings() {
    const mine = ++seq;
    state = { view: 'settings', dir: '' };
    chrome({ showBack: true });
    $('#crumbs').innerHTML = '<span class="sep">/</span><button>Settings</button>';
    document.title = 'Settings · leo';
    app.innerHTML = skeleton(3);
    const page = await api('/api/settings');
    if (mine !== seq) return;
    drawSettings(page);
  }

  function settingsOption(value, label, current) {
    return `<option value="${esc(value)}"${value === current ? ' selected' : ''}>${esc(label)}</option>`;
  }

  function taskCard(t, secure) {
    const status = t.ready ? '<span class="set-status ok">Ready</span>' : '<span class="set-status missing">Not set up</span>';
    const providers = t.choices.map((c) => settingsOption(c.id, c.label, t.provider)).join('') + (t.custom ? settingsOption(t.provider, `${t.provider} (from config.toml)`, t.provider) : '');
    let model = '';
    if (t.fixed_model) {
      model = `<div class="set-row"><span class="set-label">Model</span><span class="set-value">${esc(t.model || 'Built in')} <span class="hint">free, runs on your computer</span></span></div>`;
    } else if (t.models.length) {
      const known = t.models.some((m) => m.id === t.model);
      const options = (known || !t.model ? '' : settingsOption(t.model, `${t.model} (not in the list)`, t.model)) + t.models.map((m) => settingsOption(m.id, `${m.id} — ${m.price}`, t.model)).join('');
      model = `<label class="set-row"><span class="set-label">Model</span><select data-set="model" data-task="${t.task}">${options}</select></label>`;
    }
    let key = '';
    if (t.key) {
      const has = t.key.stored;
      const form = secure
        ? `<div class="set-key-form"><input type="password" autocomplete="off" spellcheck="false" placeholder="${has ? 'Paste a new key to replace it' : `Paste your ${esc(t.key.name)} API key`}" data-key-input="${esc(t.key.account)}"><button class="btn primary sm" data-action="set-key" data-account="${esc(t.key.account)}">${has ? 'Replace' : 'Save key'}</button></div>`
        : '<p class="hint">To add a key, open the https link leo serve printed (not the Wi-Fi one), or use this page on your computer. Keys never cross Wi-Fi unencrypted.</p>';
      key = `<div class="set-row column"><div class="set-line"><span class="set-label">${esc(t.key.name)} key</span><span class="set-value">${has ? '<span class="set-status ok">Stored on your computer</span>' : '<span class="set-status missing">Not added</span>'}${has ? `<button class="btn plain sm" data-action="remove-key" data-account="${esc(t.key.account)}" data-name="${esc(t.key.name)}">Remove</button>` : ''}</span></div>${form}${t.key.ignored ? `<p class="hint">leo does not read $${esc(t.key.ignored)}; add the key here instead.</p>` : ''}</div>`;
    }
    const signin = t.signin ? `<p class="set-note${t.signin.installed ? '' : ' warn'}">${esc(t.signin.text)}</p>` : '';
    const usage = t.usage ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
    const note = t.note ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
    return `<section class="set-card" data-task-card="${t.task}">
      <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
      <p class="hint">${TASK_USE[t.task]}</p>
      <label class="set-row"><span class="set-label">Provider</span><select data-set="provider" data-task="${t.task}">${providers}</select></label>
      ${model}${key}${signin}${usage}${note}
      <div class="set-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test</button><span class="set-result" id="test-${t.task}"></span></div>
    </section>`;
  }

  function drawSettings(page) {
    state.settings = page;
    const backup = page.backup;
    app.innerHTML = `<div class="settings">
      <div class="section-title">Settings</div>
      ${page.tasks.map((t) => taskCard(t, page.secure)).join('')}
      <section class="set-card">
        <header><h3>Backup</h3>${backup.remote ? '<span class="set-status ok">On</span>' : '<span class="set-status missing">Not set up</span>'}</header>
        <p class="hint">${backup.remote ? `Your notes are copied to ${esc(backup.remote)}.` : 'Backup is not set up yet. Run :backup in leo on your computer to keep a copy on GitHub.'}</p>
        <label class="set-row"><span class="set-label">Push changes</span><select data-set="auto_push">${backup.options.map((o) => settingsOption(o.id, o.label, backup.auto_push)).join('')}</select></label>
      </section>
      <section class="set-card">
        <header><h3>Where things are</h3></header>
        <div class="set-row column"><span class="set-label">Notes</span><code class="set-path">${esc(page.paths.notes || '')}</code></div>
        <div class="set-row column"><span class="set-label">Settings file</span><code class="set-path">${esc(page.paths.config || '')}</code></div>
        <p class="hint">Changes here save straight away and are the same settings as :settings in leo. Keys are kept in a file only your account can read, and are never shown again.</p>
      </section>
      <section class="set-card">
        <header><h3>Advanced</h3></header>
        <button class="list-row set-advanced" data-action="storage">${ICON.folder}<span class="grow">Storage and data<span class="sub">See what leo keeps on this computer, how much space it takes, and delete what you no longer need</span></span>${ICON.chevron}</button>
      </section>
    </div>`;
  }

  const STORE_COLORS = ['#4f46e5', '#0ea5e9', '#16a34a', '#f59e0b', '#db2777', '#8b5cf6', '#14b8a6', '#ef4444', '#64748b', '#a3a29c'];

  async function showStorage() {
    const mine = ++seq;
    state = { view: 'storage', dir: '', storageOpen: (state.view === 'storage' && state.storageOpen) || new Set() };
    chrome({ showBack: true });
    $('#crumbs').innerHTML = '<span class="sep">/</span><button data-action="settings">Settings</button><span class="sep">/</span><button>Storage</button>';
    document.title = 'Storage · leo';
    app.innerHTML = skeleton(4);
    const [page, keep, sessions] = await Promise.all([api('/api/storage'), api('/api/keep'), api('/api/sessions')]);
    if (mine !== seq) return;
    state.keep = keep;
    state.sessions = sessions.sessions;
    drawStorage(page);
  }

  const keepValue = (days) => (days === null || days === undefined ? 'forever' : String(days));
  const keepDays = (value) => (value === 'forever' ? null : Number(value));

  function keepCard() {
    const keep = state.keep;
    if (!keep) return '';
    const select = (field, choices) =>
      `<select data-keep="${field}">${choices.map((c) => `<option value="${keepValue(c.days)}"${keepValue(c.days) === keepValue(keep[field]) ? ' selected' : ''}>${esc(c.label === 'forever' ? 'Forever' : c.label)}</option>`).join('')}</select>`;
    return `<section class="set-card store-keep">
        <header><h3>How long leo keeps things</h3></header>
        <label class="set-row"><span class="set-label">Trash</span>${select('trash_days', keep.trash_choices)}</label>
        <label class="set-row"><span class="set-label">Chats with Felix</span>${select('chat_days', keep.chat_choices)}</label>
        <p class="set-note">Anything older is deleted on its own, along with the documents given to Felix in those chats.</p>
      </section>`;
  }

  function sessionsCard() {
    const list = state.sessions || [];
    const rows = list
      .map((b) => `<div class="set-row session-row"><span class="grow"><b>${esc(b.device)}</b>${b.current ? ' <span class="chip accent">This browser</span>' : ''}<span class="sub">Signed in ${rel(b.created_at)} · last used ${rel(b.last_seen)}</span></span>${b.current ? '' : `<button class="btn sm plain danger-text" data-action="session-end" data-handle="${esc(b.handle)}" data-device="${esc(b.device)}">Sign out</button>`}</div>`)
      .join('');
    const others = list.filter((b) => !b.current).length;
    return `<section class="set-card store-sessions">
        <header><h3>Signed-in browsers</h3></header>
        <p class="hint">Every browser that opened your leo serve link. Signing one out means it needs the link again.</p>
        ${rows || '<p class="set-note">No browsers yet.</p>'}
        <div class="store-actions">${others ? `<button class="btn sm plain danger-text" data-action="session-end-others">Sign out ${others === 1 ? 'the other browser' : `the other ${others} browsers`}</button>` : ''}<button class="btn sm plain" data-action="session-new-link">Make a new link</button></div>
      </section>`;
  }

  async function refreshSessions() {
    state.sessions = (await api('/api/sessions')).sessions;
    if (state.view === 'storage' && state.storage) drawStorage(state.storage);
  }

  function sessionAsk(el) {
    state.sessionPending = el.dataset.handle ? { handle: el.dataset.handle } : { others: true };
    const what = el.dataset.handle ? esc(el.dataset.device) : 'every other browser';
    sheet(`<h3>Sign out ${what}?</h3>
      <p>It needs your leo serve link to sign in again.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="session-end-now">Sign out</button></div>`);
  }

  async function sessionEndNow() {
    const body = state.sessionPending;
    closeSheet();
    if (!body) return;
    state.sessionPending = null;
    const done = await api('/api/sessions/end', { method: 'POST', body });
    toast(done.ended === 1 ? 'Signed out 1 browser' : `Signed out ${done.ended} browsers`);
    await refreshSessions();
  }

  function newLinkAsk() {
    sheet(`<h3>Make a new link?</h3>
      <p>The old link stops working for browsers that are not signed in yet. Browsers already signed in stay signed in; sign them out above if you want them to need the new link.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="session-new-link-now">Make a new link</button></div>`);
  }

  async function newLinkNow() {
    closeSheet();
    const made = await api('/api/sessions/new-link', { method: 'POST' });
    sheet(`<h3>Your new link</h3>
      <p>Open it on each device you want to use. It is also printed in the terminal running leo serve.</p>
      <input class="new-link" id="new-link" readonly value="${esc(made.link)}">
      <div class="buttons"><button class="btn plain" data-action="close">Done</button><button class="btn primary" data-action="copy-link">Copy</button></div>`);
    const field = $('#new-link');
    if (field) field.select();
  }

  async function copyLink() {
    const field = $('#new-link');
    if (!field) return;
    try {
      await navigator.clipboard.writeText(field.value);
      toast('Copied the new link');
    } catch (e) {
      field.select();
      toast('Select the link and copy it');
    }
  }

  async function changeKeep(el) {
    const before = state.keep;
    const next = { trash_days: before.trash_days, chat_days: before.chat_days, [el.dataset.keep]: keepDays(el.value) };
    const shorter = (a, b) => b !== null && (a === null || b < a);
    const what = el.dataset.keep === 'trash_days' ? 'notes in the trash' : 'chats with Felix';
    if (shorter(before[el.dataset.keep], next[el.dataset.keep])) {
      sheet(`<h3>Keep ${what} for ${esc(el.selectedOptions[0].textContent.toLowerCase())}?</h3>
        <p>Any older than that are deleted now, and from then on as they age.</p>
        <div class="buttons"><button class="btn plain" data-action="keep-cancel">Cancel</button><button class="btn danger" data-action="keep-now">Keep for ${esc(el.selectedOptions[0].textContent.toLowerCase())}</button></div>`);
      state.keepPending = next;
      return;
    }
    await saveKeep(next);
  }

  async function saveKeep(next) {
    state.keep = await api('/api/keep', { method: 'POST', body: next });
    toast('Saved');
    const page = await api('/api/storage');
    if (state.view === 'storage') drawStorage(page);
  }

  function storageWhen(iso) {
    return iso ? rel(iso) : '';
  }

  function drawStorage(page) {
    state.storage = page;
    const open = state.storageOpen || new Set();
    const total = Math.max(1, page.total);
    const shown = page.areas.filter((a) => a.bytes > 0);
    const bar = shown.map((a, i) => `<i style="width:${Math.max(0.6, (a.bytes / total) * 100)}%;background:${STORE_COLORS[page.areas.indexOf(a) % STORE_COLORS.length]}" title="${esc(a.title)}: ${esc(a.size)}"></i>`).join('');
    const legend = page.areas.map((a, i) => `<span class="store-key"><b style="background:${STORE_COLORS[i % STORE_COLORS.length]}"></b>${esc(a.title)} <span class="store-key-size">${esc(a.size)}</span></span>`).join('');
    const drafts = saving.drafts().length;
    const areas = page.areas.map((a, i) => {
      const pickable = a.actions.some((x) => x.selected);
      const items = a.items.length
        ? `<div class="store-items">${pickable && a.items.length > 1 ? `<label class="store-item store-all"><input type="checkbox" data-store-all="${esc(a.id)}"><span class="grow">Select all</span></label>` : ''}${a.items
            .map((it) => `<label class="store-item${it.locked ? ' locked' : ''}">${pickable ? `<input type="checkbox" data-store-item="${esc(a.id)}" value="${esc(it.id)}"${it.locked ? ' disabled' : ''}>` : ''}<span class="grow"><span class="store-label">${esc(it.label)}</span><span class="sub">${esc([it.detail, storageWhen(it.when)].filter(Boolean).join(' · '))}</span></span>${it.size ? `<span class="store-size">${esc(it.size)}</span>` : ''}</label>`)
            .join('')}</div>`
        : '';
      const actions = a.actions.length
        ? `<div class="store-actions">${a.actions
            .map((x) => `<button class="btn sm ${x.selected ? 'plain' : 'plain danger-text'}" data-action="storage-act" data-area="${esc(a.id)}" data-act="${esc(x.id)}"${x.selected ? ' disabled data-needs-items="1"' : ''}>${esc(x.label)}${x.selected ? ' <span class="store-count"></span>' : ''}</button>`)
            .join('')}</div>`
        : '';
      return `<details class="set-card store-area" data-area="${esc(a.id)}"${open.has(a.id) ? ' open' : ''}>
        <summary><b class="store-dot" style="background:${STORE_COLORS[i % STORE_COLORS.length]}"></b><span class="grow"><b>${esc(a.title)}</b>${a.items.length ? `<span class="sub">${a.items.length} item${a.items.length === 1 ? '' : 's'}</span>` : ''}</span><span class="store-size">${esc(a.size)}</span>${ICON.chevron}</summary>
        <p class="hint">${esc(a.about)}</p>
        <code class="set-path">${esc(a.path)}</code>
        ${items}${actions}
      </details>`;
    }).join('');
    app.innerHTML = `<div class="settings storage">
      <div class="section-title">Storage and data</div>
      <section class="set-card store-total">
        <div class="store-big">${esc(page.total_label)}</div>
        <p class="hint">What leo keeps on this computer. Notes are only deleted from their own page, and backups are left alone.</p>
        <div class="store-bar" aria-hidden="true">${bar}</div>
        <div class="store-legend">${legend}</div>
      </section>
      ${areas}
      ${keepCard()}
      ${sessionsCard()}
      <section class="set-card store-export">
        <header><h3>Export everything</h3></header>
        <p class="hint">A zip of your notes as Markdown, in their folders, to keep or open in another app. Settings and API keys are never included.</p>
        <div class="store-export-parts">
          <label><input type="checkbox" checked disabled> Notes</label>
          <label><input type="checkbox" data-export="uploads" checked> Uploaded files</label>
          <label><input type="checkbox" data-export="chats" checked> Chats with Felix</label>
          <label><input type="checkbox" data-export="trash"> Trash</label>
        </div>
        <a class="btn primary sm" id="export-link" download href="${exportHref()}">Download zip</a>
      </section>
      <section class="set-card store-browser">
        <header><h3>This browser</h3></header>
        <p class="hint">${drafts ? `${plural(drafts, 'unsaved draft')} kept here until they reach leo. Clearing them throws those edits away; the notes keep their last saved version.` : 'No unsaved drafts are kept here.'}</p>
        ${drafts ? '<div class="store-actions"><button class="btn sm plain" data-action="drafts">Open drafts</button><button class="btn sm plain danger-text" data-action="drafts-clear">Clear drafts</button></div>' : ''}
      </section>
    </div>`;
  }

  function exportHref() {
    const on = (part, fallback) => {
      const box = app.querySelector(`[data-export="${part}"]`);
      return box ? box.checked : fallback;
    };
    return `/api/export?uploads=${on('uploads', true)}&chats=${on('chats', true)}&trash=${on('trash', false)}`;
  }

  function storagePicked(area) {
    return [...app.querySelectorAll(`input[data-store-item="${CSS.escape(area)}"]:checked`)].map((i) => i.value);
  }

  function storageCounts() {
    for (const button of app.querySelectorAll('[data-needs-items]')) {
      const n = storagePicked(button.dataset.area).length;
      button.disabled = n === 0;
      const count = button.querySelector('.store-count');
      if (count) count.textContent = n ? `(${n})` : '';
    }
  }

  function storageAsk(el) {
    const area = state.storage && state.storage.areas.find((a) => a.id === el.dataset.area);
    const action = area && area.actions.find((x) => x.id === el.dataset.act);
    if (!action) return;
    const items = action.selected ? storagePicked(area.id) : [];
    if (action.selected && !items.length) return;
    state.storagePending = { area: area.id, action: action.id, items };
    const what = action.selected ? `${items.length} item${items.length === 1 ? '' : 's'} from ${area.title}` : area.title;
    sheet(`<h3>${esc(action.label)}?</h3>
      <p><b>${esc(what)}</b></p>
      <p>${esc(action.confirm || 'This cannot be undone.')}</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="storage-go">${esc(action.selected ? 'Delete' : action.label)}</button></div>`);
  }

  async function storageGo() {
    const request = state.storagePending;
    closeSheet();
    if (!request) return;
    state.storagePending = null;
    const done = await api('/api/storage', { method: 'POST', body: request });
    if (state.view !== 'storage') return;
    drawStorage(done.storage);
    toast(done.message);
  }

  app.addEventListener('toggle', (e) => {
    const area = e.target.closest && e.target.closest('details.store-area');
    if (!area || state.view !== 'storage') return;
    if (area.open) state.storageOpen.add(area.dataset.area);
    else state.storageOpen.delete(area.dataset.area);
  }, true);

  app.addEventListener('change', (e) => {
    if (state.view !== 'storage') return;
    const all = e.target.closest('[data-store-all]');
    if (all) {
      for (const box of app.querySelectorAll(`input[data-store-item="${CSS.escape(all.dataset.storeAll)}"]:not(:disabled)`)) box.checked = all.checked;
    }
    if (all || e.target.closest('[data-store-item]')) storageCounts();
    if (e.target.closest('[data-export]')) $('#export-link').href = exportHref();
    const keep = e.target.closest('select[data-keep]');
    if (keep) changeKeep(keep).catch(fail);
  });

  async function changeSetting(change) {
    try {
      const done = await api('/api/settings', { method: 'POST', body: change });
      drawSettings(done.settings);
      toast(done.message);
    } catch (e) {
      if (state.settings) drawSettings(state.settings);
      throw e;
    }
  }

  app.addEventListener('change', (e) => {
    const el = e.target.closest('select[data-set]');
    if (!el || state.view !== 'settings') return;
    const change = { set: el.dataset.set, value: el.value };
    if (el.dataset.task) change.task = el.dataset.task;
    changeSetting(change).catch(fail);
  });

  app.addEventListener('keydown', (e) => {
    const input = e.target.closest('[data-key-input]');
    if (input && e.key === 'Enter') {
      e.preventDefault();
      saveKey(input.dataset.keyInput).catch(fail);
    }
  });

  async function saveKey(account) {
    const input = app.querySelector(`[data-key-input="${CSS.escape(account)}"]`);
    const value = input ? input.value.trim() : '';
    if (!value) return toast('Paste the key first.', { bad: true });
    input.value = '';
    await changeSetting({ set: 'key', account, value });
  }

  function confirmRemoveKey(el) {
    sheet(`<h3>Remove the ${esc(el.dataset.name)} key?</h3>
      <p>leo stops using ${esc(el.dataset.name)} until you add a key again.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="remove-key-now" data-account="${esc(el.dataset.account)}">Remove</button></div>`);
  }

  async function testAi(el) {
    const out = $(`#test-${el.dataset.task}`);
    el.disabled = true;
    out.className = 'set-result';
    out.textContent = 'Asking…';
    try {
      const done = await api('/api/settings/test', { method: 'POST', body: { task: el.dataset.task } });
      out.className = 'set-result ok';
      out.textContent = done.message;
    } catch (e) {
      out.className = 'set-result bad';
      out.textContent = e.message;
    } finally {
      el.disabled = false;
    }
  }

  const UPLOAD_ACCEPT = '.pdf,.docx,.pptx,.txt,.md,image/*';
  const UPLOAD_MOST = 90 * 1024 * 1024;
  let picked = [];
  let uploadPoll = 0;

  const size = (n) => (n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);

  async function shrink(file) {
    if (!/^image\//.test(file.type) || file.type === 'image/gif') return { name: file.name, type: file.type, blob: file };
    try {
      const bitmap = await createImageBitmap(file);
      const scale = Math.min(1, 2000 / Math.max(bitmap.width, bitmap.height));
      const canvas = document.createElement('canvas');
      canvas.width = Math.round(bitmap.width * scale);
      canvas.height = Math.round(bitmap.height * scale);
      canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.86));
      if (!blob) throw new Error('no blob');
      return { name: file.name.replace(/\.[^.]+$/, '') + '.jpg', type: 'image/jpeg', blob };
    } catch (e) {
      return { name: file.name, type: file.type, blob: file };
    }
  }

  function base64(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(',')[1] || '');
      reader.onerror = () => reject(new Error(`${blob.name || 'A file'} could not be read.`));
      reader.readAsDataURL(blob);
    });
  }

  function drawPicked() {
    const list = $('#upload-list');
    if (!list) return;
    list.innerHTML = picked
      .map((f, i) => `<div class="upload-file">${/^image\//.test(f.type) ? ICON.image : ICON.note}<span class="grow">${esc(f.name)}<span class="sub">${size(f.size)}</span></span><button class="icon-btn" data-action="upload-drop" data-i="${i}" aria-label="Remove ${esc(f.name)}">${ICON.close}</button></div>`)
      .join('');
    $('#upload-go').disabled = !picked.length;
    $('#upload-pick-label').textContent = picked.length ? 'Add more files' : 'Choose files, or take photos';
  }

  async function uploadSheet(files) {
    const here = state.view === 'folder' ? state.dir : state.view === 'note' ? state.dir || '' : '';
    picked = files ? [...files] : [];
    let folders = [];
    try {
      folders = await api('/api/folders');
    } catch (e) {
      folders = [];
    }
    const options = [{ name: '' }, ...folders].map((d) => `<option value="${esc(d.name)}"${d.name === here ? ' selected' : ''}>${esc(d.name ? folderLabel(d.name) + (d.name.includes('/') ? ` (${d.name})` : '') : 'All notes (top level)')}</option>`).join('');
    sheet(`<h3>Make a note from a file</h3>
      <p>Slides, handouts, papers or worksheets as PDF, Word, PowerPoint or text, or photos and scans of pages. Your AI reads them and writes study notes.</p>
      <label class="upload-drop" id="upload-zone">${ICON.upload}<span id="upload-pick-label">Choose files, or take photos</span><input type="file" id="upload-input" multiple accept="${UPLOAD_ACCEPT}"></label>
      <div class="upload-list" id="upload-list"></div>
      <label class="field">${ICON.folder}<select id="upload-dir">${options}</select></label>
      <label class="field">${ICON.note}<input id="upload-title" placeholder="Title (optional; the AI names it otherwise)" autocomplete="off"></label>
      <p class="hint upload-hint">Photos and scans need an AI that can see images: OpenAI, Anthropic, Gemini, xAI, Claude Code or Codex.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" id="upload-go" data-action="upload-go" disabled>Make the note</button></div>`);
    $('#upload-input').addEventListener('change', (e) => {
      picked.push(...e.target.files);
      e.target.value = '';
      drawPicked();
    });
    const zone = $('#upload-zone');
    zone.addEventListener('dragover', (e) => {
      e.preventDefault();
      zone.classList.add('over');
    });
    zone.addEventListener('dragleave', () => zone.classList.remove('over'));
    zone.addEventListener('drop', (e) => {
      e.preventDefault();
      zone.classList.remove('over');
      picked.push(...e.dataTransfer.files);
      drawPicked();
    });
    drawPicked();
  }

  function uploadProgress(text, done, total) {
    const bar = total ? Math.round((done / total) * 100) : 8;
    const box = $('#upload-progress');
    if (!box) return;
    box.innerHTML = `<div class="upload-step">${esc(text)}</div><div class="upload-bar"><i style="width:${Math.max(6, bar)}%"></i></div>`;
  }

  async function uploadGo() {
    if (!picked.length) return;
    const dir = $('#upload-dir').value;
    const title = $('#upload-title').value.trim();
    const names = picked.map((f) => f.name);
    sheet(`<div class="upload-working">${felix.felix(72, 'idle think')}<h3>Making your note</h3><p>${esc(names.length === 1 ? names[0] : `${names.length} files`)}</p><div id="upload-progress"></div><p class="hint">This can take a minute for long files. You can close this; the note appears in the folder when it is ready.</p></div>`);
    uploadProgress('Preparing the files', 0, 0);
    const files = [];
    let total = 0;
    for (const file of picked) {
      const ready = await shrink(file);
      total += ready.blob.size;
      if (total > UPLOAD_MOST) {
        closeSheet();
        return toast('That is too much to upload at once; send fewer files.', { bad: true });
      }
      files.push({ name: ready.name, type: ready.type, data: await base64(ready.blob) });
    }
    uploadProgress('Uploading', 0, 0);
    let started;
    try {
      started = await api('/api/import', { method: 'POST', body: { directory: dir, title: title || null, files } });
    } catch (e) {
      closeSheet();
      throw e;
    }
    picked = [];
    watchUpload(started.id, dir);
    checkActivity();
  }

  function watchUpload(id, dir) {
    clearTimeout(uploadPoll);
    uploadPoll = setTimeout(async () => {
      let job;
      try {
        job = await api(`/api/import/${enc(id)}`);
      } catch (e) {
        return watchUpload(id, dir);
      }
      if (job.state === 'working') {
        uploadProgress(job.step, job.done, job.total);
        return watchUpload(id, dir);
      }
      const open = $('.upload-working');
      if (job.state === 'failed') {
        if (open) {
          open.innerHTML = `${felix.felix(72, 'droop')}<h3>The note could not be made</h3><p class="upload-error">${esc(job.error || 'Something went wrong.')}</p><div class="buttons"><button class="btn plain" data-action="close">Close</button><button class="btn primary" data-action="upload">Try again</button></div>`;
        } else {
          toast(job.error || 'The upload failed.', { bad: true });
        }
        return;
      }
      if (open) {
        closeSheet();
        go(noteHash(job.note));
      } else {
        await showLatest().catch(() => {});
        toast('Your note from the upload is ready.', { action: 'Open', run: () => go(noteHash(job.note)) });
      }
    }, 900);
  }

  async function showLatest() {
    if ($('.scrim')) return;
    if (state.view === 'folder' && !state.selecting) await showFolder(state.dir);
    else if (state.view === 'search') await render();
  }

  async function drawOriginals(note) {
    if (!note.id) return;
    let files = [];
    try {
      files = await api(`/api/notes/${enc(note.id)}/originals`);
    } catch (e) {
      return;
    }
    const meta = $('.note-meta');
    if (!files.length || !meta || state.view !== 'note' || state.session.note.id !== note.id) return;
    const all = files.length > 1 ? `<a class="chip original all" href="/api/notes/${enc(note.id)}/originals.zip" download>${ICON.paperclip}Download all ${files.length}</a>` : '';
    meta.insertAdjacentHTML('beforeend', files.map((f) => `<a class="chip original" href="/api/notes/${enc(note.id)}/originals/${enc(f.name)}" download="${esc(f.name)}">${ICON.paperclip}${esc(f.name)}</a>`).join('') + all);
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
    if (state.keepPending) {
      state.keepPending = null;
      if (state.view === 'storage' && state.storage) drawStorage(state.storage);
    }
  }

  function menu() {
    const here = state.view === 'folder' ? state.dir : '';
    sheet(`
      <button class="list-row" data-action="new" data-dir="${esc(here)}">${ICON.plus}<span class="grow">New note</span></button>
      <button class="list-row" data-action="record" data-dir="${esc(here)}">${ICON.mic}<span class="grow">Record a lecture or meeting</span></button>
      <button class="list-row" data-action="upload">${ICON.upload}<span class="grow">Make a note from a file</span></button>
      <button class="list-row" data-action="new-folder">${ICON.folderPlus}<span class="grow">New folder${here ? ` in ${esc(folderLabel(here))}` : ''}</span></button>
      <button class="list-row" data-action="map">${ICON.map}<span class="grow">Map of ideas</span></button>
      <button class="list-row" data-action="settings">${ICON.gear}<span class="grow">Settings</span></button>
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
    map: () => {
      closeSheet();
      go('#/map');
    },
    settings: () => {
      closeSheet();
      go('#/settings');
    },
    storage: () => go('#/settings/storage'),
    'storage-act': storageAsk,
    'storage-go': () => storageGo().catch(fail),
    'session-end': sessionAsk,
    'session-end-others': sessionAsk,
    'session-end-now': () => sessionEndNow().catch(fail),
    'session-new-link': newLinkAsk,
    'session-new-link-now': () => newLinkNow().catch(fail),
    'copy-link': () => copyLink(),
    'keep-now': () => {
      const next = state.keepPending;
      state.keepPending = null;
      closeSheet();
      if (next) saveKeep(next).catch(fail);
    },
    'keep-cancel': () => closeSheet(),
    'drafts-clear': () => {
      const n = saving.drafts().length;
      sheet(`<h3>Clear ${plural(n, 'unsaved draft')}?</h3>
        <p>The edits in ${n === 1 ? 'it are' : 'them are'} thrown away. Each note keeps its last saved version.</p>
        <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="drafts-clear-now">Clear drafts</button></div>`);
    },
    'drafts-clear-now': () => {
      closeSheet();
      const n = saving.discardAll();
      toast(`Cleared ${plural(n, 'draft')}`);
      if (state.view === 'storage' && state.storage) drawStorage(state.storage);
    },
    'set-key': (el) => saveKey(el.dataset.account),
    'remove-key': confirmRemoveKey,
    'remove-key-now': (el) => {
      closeSheet();
      return changeSetting({ set: 'key', account: el.dataset.account, value: null });
    },
    'test-ai': testAi,
    'note-map': (el) => go(`#/map/${enc(el.dataset.id)}`),
    record: (el) => {
      closeSheet();
      go(el.dataset.dir ? `#/record/${enc(el.dataset.dir)}` : '#/record');
    },
    'rec-start': () => recorder.begin().catch(fail),
    'rec-pause': () => recorder.pause().catch(fail),
    'rec-stop': () => recorder.stop().then(checkActivity).catch(fail),
    'activity-open': (el) => go(el.dataset.href),
    'activity-fold': () => {
      activity.folded = !activity.folded;
      drawActivity();
    },
    'rec-rejoin': () => recorder.rejoin().catch(fail),
    'rec-point': () => recorder.point(),
    'rec-again': () => recorder.again(),
    upload: () => uploadSheet().catch(fail),
    'upload-go': () => uploadGo().catch(fail),
    'upload-drop': (el) => {
      picked.splice(Number(el.dataset.i), 1);
      drawPicked();
    },
    chat: () => chat.toggle(),
    'map-build': () => mapBuild(false),
    'map-build-now': () => mapBuild(true),
    'map-rebuild': mapRebuildAsk,
    'map-rebuild-now': () => mapRebuild().catch(fail),
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
    move: () => moveSheet(),
    'move-to': (el) => moveTo(el.dataset.dir),
    share,
    delete: confirmDelete,
    'delete-now': deleteNow,
    restore: (el) => restore(el.dataset.id, false),
    'folder-select': () => {
      state.selecting = !state.selecting;
      state.picked = new Set();
      drawFolder();
    },
    'folder-pick': (el) => {
      const key = el.dataset.key;
      const focused = document.activeElement === el;
      if (state.picked.has(key)) state.picked.delete(key);
      else state.picked.add(key);
      drawFolder();
      if (focused) {
        const again = app.querySelector(`[data-key="${CSS.escape(key)}"]`);
        if (again) again.focus();
      }
    },
    'folder-trash': folderTrashAsk,
    'folder-trash-now': () => folderTrashNow().catch(fail),
    'trash-select': () => {
      state.selecting = !state.selecting;
      state.picked = new Set();
      drawTrash();
    },
    'trash-empty': () => trashAsk([], true),
    'trash-forget': (el) => trashAsk([el.dataset.id], false),
    'trash-delete-picked': () => state.picked.size && trashAsk([...state.picked], false),
    'trash-delete-now': () => trashDeleteNow().catch(fail),
    'trash-restore-picked': () => trashRestorePicked().catch(fail),
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
      if ((state.view === 'folder' || state.view === 'trash') && state.selecting) {
        state.selecting = false;
        state.picked = new Set();
        return state.view === 'folder' ? drawFolder() : drawTrash();
      }
      if (state.view === 'map' && mapView && mapView.selected()) return mapView.select(null);
      if (state.view !== 'folder' || state.dir) return back();
    }
    if ((e.key === 'Enter' || e.key === ' ') && e.target.matches('.pick-card')) {
      e.preventDefault();
      e.target.click();
      return;
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
    if (!['folder', 'note', 'search', 'trash'].includes(state.view)) return;
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

  const activity = { tasks: [], timer: 0, folded: false, route: '' };

  function hiddenHere(task) {
    if (task.kind === 'recording') return activity.route === 'record';
    if (task.kind === 'map') return activity.route === 'map';
    return Boolean($('.upload-working'));
  }

  function drawActivity() {
    let box = $('#activity');
    if (!box) {
      box = document.createElement('aside');
      box.id = 'activity';
      box.className = 'activity';
      box.setAttribute('aria-label', 'Working in the background');
      box.setAttribute('aria-live', 'polite');
      document.body.appendChild(box);
    }
    const shown = activity.tasks.filter((t) => !hiddenHere(t));
    box.hidden = !shown.length;
    if (!shown.length) return;
    box.classList.toggle('folded', activity.folded);
    const rows = shown
      .map((t) => {
        const share = t.total ? Math.max(4, Math.round((t.done / t.total) * 100)) : 0;
        const count = t.total ? ` · ${t.done}/${t.total}` : '';
        return `<button class="activity-row" data-action="activity-open" data-href="${esc(t.href)}">
          <span class="activity-label">${esc(t.label)}</span>
          <span class="activity-step">${esc(t.step)}${count}</span>
          <span class="activity-bar${t.total ? '' : ' busy'}"><i style="width:${share}%"></i></span>
        </button>`;
      })
      .join('');
    box.innerHTML = `<button class="activity-head" data-action="activity-fold" aria-expanded="${!activity.folded}"><span class="activity-spin"></span><span class="grow">${shown.length === 1 ? 'Working in the background' : `${shown.length} things in the background`}</span><span class="activity-fold">${activity.folded ? 'Show' : 'Hide'}</span></button>${activity.folded ? '' : rows}`;
  }

  async function checkActivity() {
    clearTimeout(activity.timer);
    try {
      activity.tasks = (await api('/api/activity')).tasks;
    } catch (e) {
      if (e instanceof Locked) return;
    }
    drawActivity();
    if (activity.tasks.length) activity.timer = setTimeout(checkActivity, 1500);
  }

  async function render() {
    const hash = decodeURI(location.hash || '#/');
    const [, kind, rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [null, '', ''];
    const arg = decodeURIComponent(rest);
    if (kind !== 'search') closeSearch();
    if (kind !== 'map') leaveMap();
    if (kind !== 'n') chat.setContext(null);
    if (kind !== 'record') recorder.leave();
    activity.route = kind;
    drawActivity();
    try {
      if (kind === 'f') await showFolder(arg);
      else if (kind === 'n') await showNote(arg.replace(/\/edit$/, ''));
      else if (kind === 'new') await newNote(arg);
      else if (kind === 'search') await showSearch(arg);
      else if (kind === 'trash') await showTrash();
      else if (kind === 'map') await showMap(arg);
      else if (kind === 'settings') await (arg === 'storage' ? showStorage() : showSettings());
      else if (kind === 'record') await showRecord(arg);
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

  window.addEventListener('dragover', (e) => {
    if (state.view === 'folder' && !$('.scrim') && e.dataTransfer && [...e.dataTransfer.types].includes('Files')) e.preventDefault();
  });
  window.addEventListener('drop', (e) => {
    if (state.view !== 'folder' || $('.scrim') || !e.dataTransfer || !e.dataTransfer.files.length) return;
    e.preventDefault();
    uploadSheet(e.dataTransfer.files).catch(fail);
  });

  window.addEventListener('online', () => saving.retry());
  route();
  checkActivity();
})();
