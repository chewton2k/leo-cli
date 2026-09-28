(function () {
  'use strict';

  const md = window.leoMarkdown;
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
    edit: svg('<path d="M4 20h4L19.5 8.5l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/>'),
    trash: svg('<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/>'),
    share: svg('<path d="M12 3v12M8 7l4-4 4 4"/><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"/>'),
    copy: svg('<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>'),
    plus: svg('<path d="M12 5v14M5 12h14"/>'),
    restore: svg('<path d="M4 12a8 8 0 1 0 2.3-5.6L4 8.5"/><path d="M4 4v4.5h4.5"/>'),
    tag: svg('<path d="M3 12V4h8l10 10-8 8z"/><circle cx="7.5" cy="8.5" r="1.3"/>'),
    note: svg('<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>'),
    refresh: svg('<path d="M20 12a8 8 0 1 1-2.3-5.6L20 8.5"/><path d="M20 4v4.5h-4.5"/>'),
    check: svg('<path d="M5 12l4.5 4.5L19 7"/>'),
    lock: svg('<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>'),
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
      const error = new Error(response.status === 404 ? 'That is not there any more.' : `leo answered ${response.status}.`);
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

  function card(note, { words = [], showFolder = false } = {}) {
    const tags = note.tags.map((t) => `<span class="chip">#${esc(t)}</span>`).join('');
    const where = showFolder && note.directory ? `<span class="chip accent">${esc(note.directory)}</span>` : '';
    const text = snippet(note.body, words);
    return `<button class="card" data-action="open-note" data-id="${esc(note.id)}">
      <div class="card-title">${note.pinned ? ICON.pin.replace('<svg', '<svg class="pin"') : ''}<span>${words.length ? highlight(note.title, words) : esc(note.title)}</span></div>
      ${text ? `<div class="card-snippet">${text}</div>` : ''}
      <div class="card-meta">${where}<span>${rel(note.updated_at)}</span>${progress(note.body)}${tags}</div>
    </button>`;
  }

  function empty(icon, title, text) {
    return `<div class="empty">${icon}<h3>${esc(title)}</h3><p>${esc(text)}</p></div>`;
  }

  const skeleton = (n) => Array.from({ length: n }, () => '<div class="skeleton"></div>').join('');

  let state = { view: null };
  let dirty = false;
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
    if (state.view === 'edit') return go(state.id ? noteHash(state.id) : folderHash(state.dir || ''), { replace: true });
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

  async function showNote(id) {
    const mine = ++seq;
    const note = await api(`/api/notes/${enc(id)}`);
    if (mine !== seq) return;
    state = { view: 'note', dir: note.directory, note };
    const canShare = typeof navigator.share === 'function';
    chrome({
      dir: note.directory,
      showBack: true,
      actions: `<nav class="actions" aria-label="Note">
        <button data-action="edit">${ICON.edit}<span>Edit</span></button>
        <button data-action="pin" class="${note.pinned ? 'on' : ''}">${ICON.pin}<span>${note.pinned ? 'Pinned' : 'Pin'}</span></button>
        <button data-action="move">${ICON.move}<span>Move</span></button>
        <button data-action="share">${canShare ? ICON.share : ICON.copy}<span>${canShare ? 'Share' : 'Copy'}</span></button>
        <button data-action="delete" class="danger">${ICON.trash}<span>Delete</span></button>
      </nav>`,
    });
    document.title = `${note.title} · leo`;
    const tags = note.tags.map((t) => `<button class="chip" data-action="tag" data-tag="${esc(t)}">#${esc(t)}</button>`).join('');
    const where = `<button class="chip accent" data-action="open-folder" data-dir="${esc(note.directory)}">${ICON.folder.replace('<svg', '<svg width="13" height="13"')} ${esc(folderLabel(note.directory))}</button>`;
    app.innerHTML = `<article>
      <header class="note-head"><h1>${esc(note.title)}</h1>
      <div class="note-meta">${where}<span>Edited ${rel(note.updated_at)}</span>${tags}</div></header>
      <div class="prose" id="prose">${note.body.trim() ? md.render(note.body) : '<p class="hint">This note is empty. Tap Edit to write in it.</p>'}</div>
    </article>`;
  }

  async function showEditor(id, dir) {
    const mine = ++seq;
    const [note, folders] = await Promise.all([
      id ? api(`/api/notes/${enc(id)}`) : Promise.resolve({ title: '', body: '', tags: [], directory: dir || '' }),
      id ? Promise.resolve([]) : api('/api/folders'),
    ]);
    if (mine !== seq) return;
    state = { view: 'edit', dir: note.directory, id };
    chrome({ dir: note.directory, showBack: true });
    const folderField = id
      ? ''
      : `<label class="field">${ICON.folder}<select id="ed-dir" aria-label="Folder"><option value="">All notes (top level)</option>${folders
          .map((f) => `<option value="${esc(f.name)}"${f.name === note.directory ? ' selected' : ''}>${esc(f.name)}</option>`)
          .join('')}</select></label>`;
    app.innerHTML = `<div class="editor">
      <input class="title-input" id="ed-title" placeholder="Title" value="${esc(note.title)}" autocomplete="off">
      <div class="row">${folderField}<label class="field">${ICON.tag}<input id="ed-tags" placeholder="Tags, separated by commas" value="${esc(note.tags.join(', '))}" autocomplete="off"></label></div>
      <div class="save-row">
        <div class="tabs" role="tablist"><button data-action="tab" data-tab="write" class="on">Write</button><button data-action="tab" data-tab="preview">Preview</button></div>
        <div class="toolbar" id="toolbar">
          <button data-action="fmt" data-fmt="heading" title="Heading">H</button>
          <button data-action="fmt" data-fmt="bold" title="Bold"><b>B</b></button>
          <button data-action="fmt" data-fmt="italic" title="Italic"><i>I</i></button>
          <button data-action="fmt" data-fmt="list" title="List">• List</button>
          <button data-action="fmt" data-fmt="task" title="Checkbox">☐ Task</button>
          <button data-action="fmt" data-fmt="code" title="Code">&lt;/&gt;</button>
          <button data-action="fmt" data-fmt="link" title="Link">Link</button>
        </div>
      </div>
      <textarea id="ed-body" placeholder="Write in Markdown. - [ ] makes a checkbox." spellcheck="true">${esc(note.body)}</textarea>
      <div class="preview prose" id="ed-preview" hidden></div>
      <div class="save-row"><span class="hint">⌘S or Ctrl-S saves.</span>
      <div class="row"><button class="btn plain" data-action="cancel">Cancel</button><button class="btn primary" data-action="save">Save</button></div></div>
    </div>`;
    dirty = false;
    const body = $('#ed-body');
    grow(body);
    for (const field of ['#ed-title', '#ed-tags', '#ed-body', '#ed-dir']) {
      const el = $(field);
      if (el) el.addEventListener('input', () => (dirty = true));
    }
    body.addEventListener('input', () => grow(body));
    if (!id) $('#ed-title').focus();
  }

  function grow(textarea) {
    textarea.style.height = 'auto';
    textarea.style.height = `${Math.max(textarea.scrollHeight + 2, window.innerHeight * 0.5)}px`;
  }

  async function save() {
    const button = $('[data-action="save"]');
    const body = $('#ed-body').value;
    const title = $('#ed-title').value.trim() || md.plain(body).slice(0, 60) || 'Untitled';
    const tags = $('#ed-tags')
      .value.split(',')
      .map((t) => t.trim().replace(/^#/, ''))
      .filter(Boolean);
    button.disabled = true;
    try {
      let note;
      if (state.id) {
        note = await api(`/api/notes/${enc(state.id)}`, { method: 'PATCH', body: { title, body, tags } });
      } else {
        const dir = $('#ed-dir') ? $('#ed-dir').value : state.dir;
        note = await api('/api/notes', { method: 'POST', body: { title, body, tags, directory: dir } });
      }
      dirty = false;
      toast('Saved');
      go(noteHash(note.id), { replace: true });
    } catch (e) {
      button.disabled = false;
      fail(e);
    }
  }

  function format(kind) {
    const area = $('#ed-body');
    const { selectionStart: start, selectionEnd: end, value } = area;
    const picked = value.slice(start, end);
    const lineStart = value.lastIndexOf('\n', start - 1) + 1;
    const wrap = (before, after, fallback) => {
      const text = picked || fallback;
      area.setRangeText(before + text + after, start, end, 'end');
      if (!picked) area.setSelectionRange(start + before.length, start + before.length + text.length);
    };
    const prefix = (mark) => {
      area.setRangeText(mark, lineStart, lineStart, 'end');
      area.setSelectionRange(end + mark.length, end + mark.length);
    };
    if (kind === 'bold') wrap('**', '**', 'bold');
    if (kind === 'italic') wrap('*', '*', 'italic');
    if (kind === 'code') picked.includes('\n') ? wrap('```\n', '\n```', '') : wrap('`', '`', 'code');
    if (kind === 'link') wrap('[', '](https://)', 'link');
    if (kind === 'heading') prefix('## ');
    if (kind === 'list') prefix('- ');
    if (kind === 'task') prefix('- [ ] ');
    area.focus();
    area.dispatchEvent(new Event('input'));
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
          .join('')}</div><p class="hint" style="margin:14px 4px">Deleted notes stay here for 30 days. To empty the trash now, run /trash empty in leo on your computer.</p>`
      : empty(ICON.trash, 'The trash is empty', 'Deleted notes wait here for 30 days, so a mistake can be undone.');
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
      <button class="list-row" data-action="tags">${ICON.tag}<span class="grow">Tags</span></button>
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
    const note = state.note;
    const folders = await api('/api/folders');
    const row = (name, label) =>
      `<button class="list-row" data-action="move-to" data-dir="${esc(name)}">${ICON.folder}<span class="grow">${esc(label)}</span>${
        name === note.directory ? ICON.check : ''
      }</button>`;
    sheet(`<h3>Move “${esc(note.title)}”</h3>${row('', 'All notes (top level)')}${folders.map((f) => row(f.name, f.name)).join('')}`);
  }

  async function moveTo(dir) {
    const note = state.note;
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

  function confirmDelete() {
    const note = state.note;
    sheet(`<h3>Move “${esc(note.title)}” to the trash?</h3><p>You can restore it for 30 days, from Trash in the menu.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="delete-now">Move to trash</button></div>`);
  }

  async function deleteNow() {
    const note = state.note;
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

  async function togglePin() {
    const note = state.note;
    try {
      await api(`/api/notes/${enc(note.id)}`, { method: 'PATCH', body: { pinned: !note.pinned } });
      toast(note.pinned ? 'Unpinned' : 'Pinned to the top of its folder');
      showNote(note.id);
    } catch (e) {
      fail(e);
    }
  }

  async function share() {
    const note = state.note;
    const text = `${note.title}\n\n${note.body}`;
    if (typeof navigator.share === 'function') {
      try {
        await navigator.share({ title: note.title, text });
      } catch (e) {
        return;
      }
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      toast('Copied the note');
    } catch (e) {
      toast("Couldn't copy here.", { bad: true });
    }
  }

  async function tick(input) {
    const label = input.closest('.task');
    const note = state.note;
    label.classList.toggle('done', input.checked);
    label.classList.add('busy');
    try {
      const updated = await api(`/api/notes/${enc(note.id)}/toggle?checkbox=${input.dataset.box}`, { method: 'POST' });
      state.note = updated;
      $('#prose').innerHTML = md.render(updated.body);
    } catch (e) {
      input.checked = !input.checked;
      label.classList.toggle('done', input.checked);
      label.classList.remove('busy');
      fail(e);
    }
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
    trash: () => {
      closeSheet();
      go('#/trash');
    },
    refresh: () => {
      closeSheet();
      route();
    },
    tag: (el) => go(`#/search/${enc('#' + el.dataset.tag)}`),
    edit: () => go(`${noteHash(state.note.id)}/edit`),
    pin: togglePin,
    move: () => moveSheet().catch(fail),
    'move-to': (el) => moveTo(el.dataset.dir),
    share,
    delete: confirmDelete,
    'delete-now': deleteNow,
    restore: (el) => restore(el.dataset.id, false),
    save,
    cancel: () => {
      if (dirty && !window.confirm('Leave without saving your changes?')) return;
      dirty = false;
      back();
    },
    tab: (el) => {
      const preview = el.dataset.tab === 'preview';
      for (const b of document.querySelectorAll('.tabs button')) b.classList.toggle('on', b === el);
      $('#ed-body').hidden = preview;
      $('#toolbar').hidden = preview;
      const pane = $('#ed-preview');
      pane.hidden = !preview;
      if (preview) {
        pane.innerHTML = md.render($('#ed-body').value) || '<p class="hint">Nothing to preview yet.</p>';
        for (const box of pane.querySelectorAll('input')) box.disabled = true;
      }
    },
    fmt: (el) => format(el.dataset.fmt),
  };

  document.addEventListener('click', (e) => {
    const el = e.target.closest('[data-action]');
    if (!el) return;
    const act = actions[el.dataset.action];
    if (!act) return;
    e.preventDefault();
    Promise.resolve(act(el)).catch(fail);
  });

  app.addEventListener('change', (e) => {
    if (e.target.matches('#prose input[data-box]') && state.view === 'note') tick(e.target);
  });

  document.addEventListener('keydown', (e) => {
    const typing = e.target.closest('input, textarea, select');
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && state.view === 'edit') {
      e.preventDefault();
      save();
      return;
    }
    if (e.key === 'Escape') {
      if ($('.scrim')) return closeSheet();
      if (typing) return e.target.blur();
      if (state.view !== 'folder' || state.dir) return actions.cancel();
    }
    if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === '/') {
      e.preventDefault();
      openSearch('');
      $('#search-input').focus();
    } else if (e.key === 'n' && state.view === 'folder') {
      go(state.dir ? `#/new/${enc(state.dir)}` : '#/new');
    } else if (e.key === 'e' && state.view === 'note') {
      actions.edit();
    }
  });

  window.addEventListener('beforeunload', (e) => {
    if (dirty) e.preventDefault();
  });

  let shown = location.hash;
  window.addEventListener('hashchange', () => {
    if (dirty && state.view === 'edit' && !window.confirm('Leave without saving your changes?')) {
      history.replaceState(null, '', shown);
      return;
    }
    dirty = false;
    route();
  });

  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible' && ['folder', 'note', 'search', 'trash', 'tags'].includes(state.view)) route();
  });

  window.addEventListener(
    'scroll',
    () => $('#bar').classList.toggle('scrolled', window.scrollY > 4),
    { passive: true }
  );

  async function route() {
    shown = location.hash;
    closeSheet();
    const hash = decodeURI(location.hash || '#/');
    const [, kind, rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [null, '', ''];
    const arg = decodeURIComponent(rest);
    if (kind !== 'search') closeSearch();
    window.scrollTo(0, 0);
    try {
      if (kind === 'f') await showFolder(arg);
      else if (kind === 'n' && arg.endsWith('/edit')) await showEditor(arg.slice(0, -5));
      else if (kind === 'n') await showNote(arg);
      else if (kind === 'new') await showEditor(null, arg);
      else if (kind === 'search') await showSearch(arg);
      else if (kind === 'tags') await showTags();
      else if (kind === 'trash') await showTrash();
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

  route();
})();
