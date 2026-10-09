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
  sidebar: svg('<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M9 4v16"/>'),
  chat: svg('<path d="M5 18.5V7a2.5 2.5 0 0 1 2.5-2.5h9A2.5 2.5 0 0 1 19 7v6.5a2.5 2.5 0 0 1-2.5 2.5H9z"/><path d="M9 9.5h6M9 12.5h4"/>'),
  storage: svg('<ellipse cx="12" cy="6" rx="7.5" ry="2.8"/><path d="M4.5 6v6c0 1.5 3.4 2.8 7.5 2.8s7.5-1.3 7.5-2.8V6"/><path d="M4.5 12v6c0 1.5 3.4 2.8 7.5 2.8s7.5-1.3 7.5-2.8v-6"/>'),
  cloud: svg('<path d="M7 18a4.5 4.5 0 0 1-.5-9 6 6 0 0 1 11.3 1.5A3.8 3.8 0 0 1 17.5 18z"/><path d="M4 4l16 16"/>'),
};

const recorder = window.leoRecording.create({ api, esc, toast, go: (hash, opts) => go(hash, opts), noteHash: (id) => noteHash(id), felix, icons: ICON, noteReady: () => showLatest().catch(() => {}) });
const chat = felix.create({
  render: (text) => md.render(text),
  escape: md.escape,
  onOpen: (id) => go(noteHash(id)),
  prepare: async (file) => {
    const ready = await shrink(file);
    return { name: ready.name, type: ready.type, data: await base64(ready.blob) };
  },
  notify: (message) => toast(message, { bad: true }),
  onUndone: (note, removed) => {
    const open = state.view === 'note' && state.session && state.session.note.id === note.id;
    if (open && removed) go(folderHash(state.dir || ''), { replace: true });
    else if (open && !saving.unsaved()) showNote(note.id);
    else showLatest().catch(() => {});
    toast(removed ? `Moved “${note.title}” to the trash` : `Undid the change to “${note.title}”`);
  },
  onChanged: (note) => {
    const open = state.view === 'note' && state.session && state.session.note.id === note.id;
    if (open && !saving.unsaved()) showNote(note.id);
    toast(open && saving.unsaved() ? 'Felix changed this note while you were typing; reopen it to see the change.' : `Changed “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
  },
  onToggle: () => drawSide(),
  onSaved: (note) => {
    showLatest().catch(() => {});
    toast(`Saved as a note${note.directory ? ` in ${note.directory}` : ''}`, { action: 'Open', run: () => go(noteHash(note.id)) });
  },
});
$('#chat-toggle').innerHTML = chat.button(36);
$('#back').innerHTML = ICON.back;
$('#search-toggle').innerHTML = ICON.search;
$('#menu').innerHTML = ICON.more;
$('#search-icon').innerHTML = ICON.search;

class Offline extends Error {}
class Locked extends Error {}

async function api(path, { method = 'GET', body, total = false } = {}) {
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
  const text = await response.text();
  const data = text.trim() ? JSON.parse(text) : null;
  if (total) return { items: data || [], total: Number(response.headers.get('x-total')) || 0 };
  return data;
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

function progress(note) {
  let done;
  let all;
  if (note.tasks) {
    [done, all] = note.tasks;
  } else {
    const boxes = String(note.body || '').match(/^\s*- \[( |x|X)\] /gm) || [];
    all = boxes.length;
    done = boxes.filter((b) => !b.includes('[ ]')).length;
  }
  if (!all) return '';
  return `<span class="chip progress">${ICON.check.replace('<svg', '<svg width="14" height="14"')} ${done}/${all}</span>`;
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

function card(note, { words = [], showFolder = false, pick = null, why = null } = {}) {
  const where = showFolder && note.directory ? `<span class="chip accent">${esc(note.directory)}</span>` : '';
  const text = snippet(note.body, words);
  const open = pick
    ? `class="card pick-card${pick.on ? ' picked' : ''}" role="checkbox" tabindex="0" aria-checked="${pick.on}" data-action="folder-pick" data-key="${esc(pick.key)}"`
    : `class="card" role="link" tabindex="0" draggable="true" data-action="open-note" data-id="${esc(note.id)}" data-title="${esc(note.title)}" data-from="${esc(note.directory || '')}"`;
  const corner = pick ? `<span class="pick-box${pick.on ? ' on' : ''}" aria-hidden="true">${ICON.check}</span>` : pinButton(note);
  return `<div ${open}>
    <div class="card-title"><span>${words.length ? highlight(note.title, words) : esc(note.title)}</span>${corner}</div>
    ${text ? `<div class="card-snippet">${text}</div>` : ''}
    ${why ? `<div class="card-why">${ICON.map}${why.kind === 'idea' ? `Through the idea “${esc(why.name)}” in the knowledge graph` : 'Through its summary in the knowledge graph'}</div>` : ''}
    <div class="card-meta">${where}<span>${rel(note.updated_at)}</span>${progress(note)}</div>
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

const PAGE_NAMES = { '': 'All notes', f: 'All notes', search: 'Search', trash: 'Trash', map: 'Knowledge graph', settings: 'Settings', record: 'Record', new: 'New note' };

function pageName() {
  const [, kind = '', rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [];
  if (kind === 'settings' && rest === 'storage') return 'Storage';
  return PAGE_NAMES[kind] || '';
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
    const name = pageName();
    crumbs.innerHTML = name ? `<span class="page-name">${esc(name)}</span>` : '';
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
  const [dirs, page] = await Promise.all([api(`/api/dirs?parent=${enc(dir)}`), api(`/api/notes?dir=${enc(dir)}&limit=${PAGE}&brief=true`, { total: true })]);
  if (mine !== seq) return;
  state.listing = { dirs, notes: page.items, total: page.total };
  drawFolder();
}

const PAGE = 200;
let moreWatch = null;

function noteCard(n) {
  const key = `n:${n.id}`;
  return card(n, state.selecting ? { pick: { key, on: state.picked.has(key) } } : {});
}

function watchMore() {
  if (moreWatch) moreWatch.disconnect();
  moreWatch = null;
  const sentinel = $('#more-notes');
  if (!sentinel || typeof IntersectionObserver !== 'function') return;
  moreWatch = new IntersectionObserver((seen) => {
    if (seen.some((e) => e.isIntersecting)) loadMoreNotes();
  }, { rootMargin: '600px 0px' });
  moreWatch.observe(sentinel);
}

let loadingMore = null;
function loadMoreNotes() {
  if (loadingMore) return loadingMore;
  const listing = state.listing;
  if (state.view !== 'folder' || !listing || listing.notes.length >= listing.total) return Promise.resolve();
  const dir = state.dir;
  loadingMore = (async () => {
    try {
      const page = await api(`/api/notes?dir=${enc(dir)}&limit=${PAGE}&offset=${listing.notes.length}&brief=true`, { total: true });
      if (state.listing !== listing) return;
      const known = new Set(listing.notes.map((n) => n.id));
      const fresh = page.items.filter((n) => !known.has(n.id));
      listing.notes.push(...fresh);
      listing.total = Math.max(page.total, listing.notes.length);
      if (!fresh.length) listing.total = listing.notes.length;
      const cards = $('.cards');
      if (cards) cards.insertAdjacentHTML('beforeend', fresh.map(noteCard).join(''));
      const sentinel = $('#more-notes');
      if (sentinel && listing.notes.length >= listing.total) sentinel.remove();
    } catch (e) {
      fail(e);
    } finally {
      loadingMore = null;
    }
  })();
  return loadingMore;
}

async function loadAllNotes() {
  while (state.view === 'folder' && state.listing && state.listing.notes.length < state.listing.total) {
    const before = state.listing.notes.length;
    await loadMoreNotes();
    if (!state.listing || state.listing.notes.length === before) break;
  }
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
        return `<button class="folder${sel ? ' picking' : ''}${picked.has(key) ? ' picked' : ''}" data-action="${sel ? 'folder-pick' : 'open-folder'}" data-key="${esc(key)}" data-dir="${esc(full)}"${sel ? '' : ' draggable="true"'} aria-pressed="${sel ? picked.has(key) : ''}">${ICON.folder}<span><span class="name">${esc(d.name)}</span><span class="count">${d.notes} note${d.notes === 1 ? '' : 's'}</span></span>${tick(key)}</button>`;
      })
      .join('')}</div>`;
  }
  if (notes.length) {
    const total = Math.max(state.listing.total || 0, notes.length);
    html += `<div class="section-title">${total > notes.length ? `Notes · ${total}` : 'Notes'}</div><div class="cards${sel ? ' picking' : ''}">${notes.map(noteCard).join('')}</div>`;
    if (total > notes.length) html += '<div class="more-notes" id="more-notes" aria-hidden="true">Loading more notes…</div>';
  }
  app.innerHTML = html;
  watchMore();
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
  const undo = done.ids && (done.ids.length || (done.dirs && done.dirs.length)) ? { action: 'Undo', run: () => undoTrashMove(done) } : { action: 'Open trash', run: () => go('#/trash') };
  toast(`Moved ${parts.join(' and ') || 'nothing'} to the trash`, undo);
  state.selecting = false;
  await showFolder(state.dir);
}

async function undoTrashMove(done) {
  try {
    const back = await api('/api/trash/restore', { method: 'POST', body: { ids: done.ids, dirs: done.dirs || [] } });
    toast(back.restored === done.ids.length ? 'Brought back' : `Brought back ${plural(back.restored, 'note')}`);
    if (state.view === 'folder') await showFolder(state.dir);
  } catch (e) {
    fail(e);
  }
}

floating.addEventListener('change', async (e) => {
  if (state.view !== 'folder' || !e.target.closest('[data-folder-all]')) return;
  const on = e.target.checked;
  if (on) await loadAllNotes();
  if (state.view !== 'folder') return;
  state.picked = on ? new Set(folderKeys()) : new Set();
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
  recovered: (count) => {
    toast(`Saving ${count} edit${count === 1 ? '' : 's'} that had not reached leo yet.`);
    setTimeout(() => saving.retry(), 0);
  },
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
      ${id ? `<button data-action="note-map" data-id="${esc(id)}">${ICON.map}<span>Graph</span></button>` : ''}
      <button data-action="note-picture">${ICON.image}<span>Picture</span></button>
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
  chat.setContext(note.id ? { id: note.id, title: note.title, directory: note.directory } : null);
  const where = `<button class="chip accent" data-action="open-folder" data-dir="${esc(note.directory)}">${ICON.folder.replace('<svg', '<svg width="13" height="13"')} ${esc(folderLabel(note.directory))}</button>`;
  const when = note.id ? `Edited ${rel(note.updated_at)}` : 'New note';
  app.innerHTML = `<article class="note">
    <header class="note-head"><h1 id="title" contenteditable="plaintext-only" spellcheck="true" data-placeholder="Title" enterkeyhint="next">${esc(s.edit.title)}</h1>
    <div class="note-meta">${where}<span id="save-state">${when}</span></div></header>
    <div class="prose doc" id="doc"></div>
  </article>`;
  s.title = $('#title');
  drawOriginals(note);
  s.doc = leoDoc.mount($('#doc'), { source: s.edit.body, onChange: () => changed(s), placeholder: 'Tap here to write', dir: note.directory || '', onPictures: (files) => addPictures(s, files) });
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
    app.innerHTML = empty(ICON.search, 'Search every note', 'Titles, text and the ideas in the knowledge graph. Abbreviations like BFS work too.');
    return;
  }
  const results = await api(`/api/search?q=${enc(query)}&brief=true`);
  if (mine !== seq) return;
  const words = query.split(/\s+/).map((w) => w.replace(/^#/, '')).filter(Boolean);
  const draw = (list) => list.map((n) => card(n, { words, showFolder: true, why: n.why })).join('');
  const count = results.length >= SEARCH_MOST ? `The first ${SEARCH_MOST} notes` : plural(results.length, 'note');
  app.innerHTML = results.length
    ? `<div class="section-title">${count}</div><div class="cards">${draw(results.slice(0, SEARCH_STEP))}</div>${results.length > SEARCH_STEP ? '<div class="more-notes" id="more-notes" aria-hidden="true">Loading more notes…</div>' : ''}`
    : empty(ICON.search, 'Nothing found', `No note mentions “${query}”.`);
  let shown = SEARCH_STEP;
  if (moreWatch) moreWatch.disconnect();
  const sentinel = $('#more-notes');
  if (!sentinel || typeof IntersectionObserver !== 'function') return;
  moreWatch = new IntersectionObserver((seen) => {
    if (mine !== seq || !seen.some((x) => x.isIntersecting)) return;
    $('.cards').insertAdjacentHTML('beforeend', draw(results.slice(shown, shown + SEARCH_STEP)));
    shown += SEARCH_STEP;
    if (shown >= results.length) {
      moreWatch.disconnect();
      sentinel.remove();
    }
  }, { rootMargin: '600px 0px' });
  moreWatch.observe(sentinel);
}

const SEARCH_MOST = 300;
const SEARCH_STEP = 60;

async function showRecord(dir) {
  const mine = ++seq;
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
  if (mine !== seq) return;
  await recorder.show($('#rec-box'), { dir, folders: folders.map((f) => f.name).filter(Boolean) });
}
