const SIDE_KEY = 'leo-side';
const sideWide = () => Boolean(window.matchMedia && window.matchMedia('(min-width: 1000px)').matches);
const side = { folders: [], loaded: false, mini: false, open: new Set(), revealed: new Set(), shut: new Set(), here: null, target: null, twisted: null, notes: new Map(), pending: [], reading: 0, visible: [], automatic: new Set(), generation: 0, sig: [], lastSig: '', pill: '' };
// Folders the user opened with their own hands, kept across restarts. Folders
// unfolded only to show the open note or folder (`side.revealed`) are not
// kept, so the explorer reopens exactly as the user left it, closed by default.
// A new key (2026-11-03): the old one also held every folder browsing had
// unfolded, so starting from it would keep those open.
const SIDE_OPEN_KEY = 'leo-side-folders';
const SIDE_NOTES_PAGE = 100;
const SIDE_NOTES_FOLDERS = 64;
const SIDE_NOTES_AUTOMATIC = 32;
const SIDE_NOTES_READS = 4;
const SIDE_NOTES_TIMEOUT = 10000;

try {
  side.open = new Set(JSON.parse(localStorage.getItem(SIDE_OPEN_KEY) || '[]'));
  localStorage.removeItem('leo-side-open');
} catch (e) {}

try {
  side.mini = localStorage.getItem(SIDE_KEY) === 'mini';
} catch (e) {}
document.body.classList.toggle('side-mini', side.mini);

const SIDE_LEAST_WIDTH = 220;
const SIDE_MOST_WIDTH = 520;
const SIDE_PAGE_ROOM = 380;
const sideGrip = document.createElement('div');
sideGrip.className = 'pane-resize side-resize';
sideGrip.tabIndex = 0;
sideGrip.title = 'Drag to resize folders; double-click to reset';
sideGrip.setAttribute('role', 'separator');
sideGrip.setAttribute('aria-orientation', 'vertical');
sideGrip.setAttribute('aria-label', 'File explorer width');
sideGrip.setAttribute('aria-controls', 'side-explorer');
document.body.appendChild(sideGrip);
const sideResize = window.leoPaneResize.create({
  handle: sideGrip,
  key: 'leo-side-width',
  measure: () => $('#side').getBoundingClientRect().width,
  limits: () => {
    const body = document.body;
    const chat = body.classList.contains('chat-open') && document.querySelector('.chat:not([hidden])');
    const right = Math.max(parseFloat(getComputedStyle(body).paddingRight) || 0, chat ? window.innerWidth - chat.getBoundingClientRect().left : 0);
    return [SIDE_LEAST_WIDTH, Math.max(SIDE_LEAST_WIDTH, Math.min(SIDE_MOST_WIDTH, window.innerWidth - right - SIDE_PAGE_ROOM))];
  },
  apply: (width) => {
    if (width === null) document.body.style.removeProperty('--explorer-w');
    else document.body.style.setProperty('--explorer-w', `${width}px`);
  },
  enabled: () => sideWide() && !side.mini && !document.body.classList.contains('side-folding') && state.view !== 'locked',
  onChange: () => window.dispatchEvent(new Event('resize')),
});

const shortcutLabel = ON_MAC ? '⌘F' : 'Ctrl F';
const quickLabel = shortcutLabel;

function sideRow(action, icon, label, { on = false, attrs = '', count = '', hint = label } = {}) {
  const tail = count === '' ? '' : `<span class="side-count side-label">${esc(String(count))}</span>`;
  return `<button class="side-row${on ? ' on' : ''}" data-action="${action}"${attrs} aria-label="${esc(label)}" title="${esc(hint)}"${on ? ' aria-current="page"' : ''}>${icon}<span class="side-label">${esc(label)}</span>${tail}</button>`;
}

function sidePlace() {
  const view = state.view;
  if (view === 'folder' && !state.dir) return 'home';
  if (view === 'folder' || view === 'note' || view === 'new') return `dir:${state.dir || ''}`;
  // Storage and Trash live inside Settings, so the sidebar shows Settings for them.
  if (view === 'storage' || view === 'trash') return 'settings';
  return view;
}

function drawSide() {
  const box = $('#side');
  if (!box) return;
  box.hidden = state.view === 'locked';
  sideGrip.hidden = box.hidden;
  sideResize.fit();
  if (box.hidden) return;
  const here = sidePlace();
  const chatOpen = document.body.classList.contains('chat-open');
  const dir = sideHere();
  // Unfold to what is open: again whenever the folder or the open note changes,
  // so a note opened after Collapse all still shows where it is.
  const showing = `${dir}\n${state.view === 'note' ? state.session?.note?.id || '' : ''}`;
  if (showing !== side.showing) {
    side.showing = showing;
    side.revealed.clear();
    side.shut.clear();
    const parts = dir.split('/').filter(Boolean);
    for (let i = 1; i <= parts.length; i++) side.revealed.add(parts.slice(0, i).join('/'));
  }
  if (dir !== side.here) {
    side.here = dir;
    side.target = null;
  }
  const actionDir = side.target ?? dir;
  let folders = folderTree(dir);
  if (sideWide() && !side.mini) {
    side.automatic = new Set(side.visible.slice(0, SIDE_NOTES_AUTOMATIC));
    let queued = false;
    for (const path of side.automatic) {
      const entry = side.notes.get(path);
      if (!entry || entry.seq !== seq) { loadSideNotes(path, entry?.offset || 0); queued = true; }
    }
    if (queued) folders = folderTree(dir);
  }
  const scrolled = box.querySelector('.side-tree');
  const kept = scrolled ? scrolled.scrollTop : 0;
  const railScroll = box.querySelector('.side-rail-main')?.scrollTop || 0;
  const active = box.contains(document.activeElement) ? document.activeElement.closest('button[data-action]') : null;
  const focused = active ? Object.fromEntries(['action', 'dir', 'parent', 'id', 'offset'].filter((key) => key in active.dataset).map((key) => [key, active.dataset[key]])) : null;
  const sig = side.sig.join('\n');
  const listChanged = sig !== side.lastSig;
  side.lastSig = sig;
  const pillBefore = box.querySelector('.side-tree .side-pill');
  const pillFrom = pillBefore ? pillBefore.getBoundingClientRect() : null;
  const paint = () => refreshPanel(box, `
    <div class="side-head" data-tauri-drag-region="deep">
      <button class="icon-btn side-fold" data-action="side-fold" aria-label="${side.mini ? 'Show folders' : 'Hide folders'}" title="${side.mini ? 'Show folders' : 'Hide folders'}" aria-expanded="${!side.mini}" aria-controls="side-explorer">${ICON.sidebar}</button>
      <button class="brand side-label" data-action="home">leo</button>
    </div>
    <div class="side-rail">
      <nav class="side-rail-main" aria-label="Leo tools">
        ${sideRow('new', ICON.plus, 'New note', { attrs: ` data-dir="${esc(actionDir)}"`, hint: actionDir ? `New note in ${folderLabel(actionDir)}` : 'New note' })}
        ${sideRow('quick-leo', ICON.search, 'Search', { hint: `Search, ask, remind or jot anything (${shortcutLabel})` })}
        ${sideRow('chat', chat.button(22), 'Ask Felix', { on: chatOpen })}
        ${sideRow('calendar', ICON.calendar, 'Calendar', { on: here === 'calendar' })}
        ${sideRow('boards', ICON.board, 'Boards', { on: here === 'boards', hint: 'Boards · track tasks from To do to Done' })}
        ${sideRow('record', ICON.mic, 'Record', { on: here === 'record', attrs: ` data-dir="${esc(actionDir)}"` })}
        ${sideRow('upload', ICON.upload, 'Note from a file', { attrs: ` data-dir="${esc(actionDir)}"` })}
      </nav>
      <nav class="side-foot" aria-label="This app">
        ${sideRow('usage', svg('<path d="M4 19V9m8 10V5m8 14v-7"/>'), 'Usage', { hint: 'Coding accounts and remaining usage' })}
        ${sideRow('settings', ICON.gear, 'Settings', { on: here === 'settings' })}
      </nav>
    </div>
    <div class="side-explorer" id="side-explorer">
      <div class="side-group" data-drop-dir="" title="Drop a folder or note here to move it to the top level"><span>Folders</span><span class="side-group-tools"><button class="side-add side-collapse" data-action="side-collapse" aria-label="Collapse all folders" title="Collapse all folders">${svg('<path d="m7 20 5-5 5 5"/><path d="m7 4 5 5 5-5"/>')}</button><button class="side-add" data-action="new-folder" data-parent="" aria-label="New folder" title="New folder at the top level">${ICON.plus}</button></span></div>
      <nav class="side-nav side-tree" aria-label="Folders" data-drop-dir="">
        ${folders || '<p class="side-none">Folders you make show up here.</p>'}
      </nav>
    </div>`);
  if (listChanged && !side.twisted && sideWide()) flipList(box, '.side-tree .side-folder[data-refresh-key], .side-tree .side-note[data-id]', (el) => (el.dataset.id ? `n:${el.dataset.id}` : `d:${el.dataset.refreshKey}`), paint);
  else paint();
  const pill = box.querySelector('.side-tree .side-pill');
  const owner = pillKey(pill);
  if (pill && owner !== side.pill && pillFrom && sideWide()) pillGlide(pillFrom, pill);
  side.pill = owner;
  const tree = box.querySelector('.side-tree');
  if (tree) tree.scrollTop = kept;
  revealNoteRow(tree);
  box.querySelector('.side-rail-main').scrollTop = railScroll;
  if (focused) {
    const buttons = [...box.querySelectorAll('button[data-action]')];
    const button = buttons.find((button) => Object.entries(focused).every(([key, value]) => button.dataset[key] === value)) || (focused.action === 'side-notes' && buttons.find((button) => ['side-notes', 'open-folder'].includes(button.dataset.action) && button.dataset.dir === focused.dir));
    if (button) button.focus({ preventScroll: true });
  }
}

function sideHere() {
  return state.view === 'folder' || state.view === 'note' ? state.dir || '' : '';
}

function folderShown(path) {
  return !side.shut.has(path) && (side.open.has(path) || side.revealed.has(path));
}

// Like VS Code's auto reveal: the open note's row is scrolled into the
// explorer's view once, when it first appears for that note; later redraws keep
// whatever scroll the user chose. Only the explorer scrolls, never the page.
function revealNoteRow(tree) {
  const id = state.view === 'note' ? state.session?.note?.id : null;
  if (!id) side.shownNote = null;
  // A hidden explorer has nothing to scroll; try again when it shows.
  if (!tree || !id || side.shownNote === id || !tree.clientHeight) return;
  const row = [...tree.querySelectorAll('.side-note[data-id]')].find((el) => el.dataset.id === id);
  if (!row) return;
  side.shownNote = id;
  const at = row.getBoundingClientRect();
  const view = tree.getBoundingClientRect();
  if (at.top < view.top) tree.scrollTop -= view.top - at.top;
  else if (at.bottom > view.bottom) tree.scrollTop += at.bottom - view.bottom;
}

// Collapse all, as in VS Code and Obsidian: every folder closes and stays closed.
function collapseFolders() {
  side.open.clear();
  side.revealed.clear();
  side.shut.clear();
  keepOpenFolders();
  drawSide();
}

function keepOpenFolders() {
  try {
    localStorage.setItem(SIDE_OPEN_KEY, JSON.stringify([...side.open]));
  } catch (e) {}
}

function folderTree(here) {
  side.visible = [''];
  side.sig = [];
  const children = (parent) => side.folders.filter((d) => {
    const cut = d.name.lastIndexOf('/');
    return (cut < 0 ? '' : d.name.slice(0, cut)) === parent;
  });
  const rows = [];
  const twisted = side.twisted;
  const walk = (parent, depth) => {
    for (const d of children(parent)) {
      const open = folderShown(d.name);
      const name = d.name.split('/').pop();
      const on = state.view === 'folder' && here === d.name;
      const shown = twisted && twisted.path === d.name ? twisted.was : open;
      const toggle = `<button class="side-twist${shown ? ' open' : ''}" data-action="side-twist" data-dir="${esc(d.name)}" aria-expanded="${open}" aria-label="${open ? 'Close' : 'Open'} ${esc(name)}">${ICON.chevron}</button>`;
      side.sig.push(`d:${d.name}`);
      rows.push(`<div data-refresh-key="${esc(d.name)}" class="side-folder${on ? ' on' : ''}" data-drop-dir="${esc(d.name)}" style="--depth:${depth}">${on ? '<span class="side-pill" aria-hidden="true"></span>' : ''}${toggle}<button class="side-row" data-action="open-folder" data-dir="${esc(d.name)}" draggable="true" title="${esc(d.name)}" aria-expanded="${open}"${on ? ' aria-current="page"' : ''}>${ICON.folder}<span class="side-label">${esc(name)}</span>${d.notes ? `<span class="side-count side-label">${esc(String(d.notes))}</span>` : ''}</button><button class="side-add side-sub" data-action="new-folder" data-parent="${esc(d.name)}" aria-label="New folder in ${esc(name)}" title="New folder in ${esc(name)}">${ICON.plus}</button></div>`);
      if (open) {
        side.visible.push(d.name);
        rows.push(`<div class="side-kids" data-kids="${esc(d.name)}" data-drop-dir="${esc(d.name)}" style="--depth:${depth}">`);
        walk(d.name, depth + 1);
        rows.push('</div>');
      }
    }
    rows.push(sideNoteRows(parent, depth));
  };
  walk('', 0);
  return rows.join('');
}

function sideKids(path) {
  return [...document.querySelectorAll('#side .side-kids')].find((k) => k.dataset.kids === path) || null;
}

function sideArrow(path) {
  return [...document.querySelectorAll('#side .side-twist')].find((b) => b.dataset.dir === path) || null;
}

function twistFolder(path, control = null, keep = true) {
  if (control) control.focus({ preventScroll: true });
  const was = folderShown(path);
  if (was) {
    side.open.delete(path);
    side.shut.add(path);
  } else {
    (keep ? side.open : side.revealed).add(path);
    side.shut.delete(path);
  }
  if (keep) keepOpenFolders();
  const kids = sideKids(path);
  if (was && kids) {
    const arrow = sideArrow(path);
    if (arrow) arrow.classList.remove('open');
    slideHeight(kids, kids.getBoundingClientRect().height, 0, drawSide);
    return;
  }
  side.twisted = { path, was };
  drawSide();
  side.twisted = null;
  const grown = sideKids(path);
  if (grown) slideHeight(grown, 0, grown.getBoundingClientRect().height);
  requestAnimationFrame(() => {
    const arrow = sideArrow(path);
    if (arrow) arrow.classList.toggle('open', !was);
  });
}

async function loadSideFolders() {
  if (!sideWide() || state.view === 'locked') return drawSide();
  const mine = seq, generation = ++side.generation;
  try {
    const folders = await api('/api/folders');
    if (mine !== seq || generation !== side.generation) return;
    side.folders = folders;
    if (side.target && !folders.some((folder) => folder.name === side.target)) side.target = null;
    side.loaded = true;
    for (const [path, entry] of side.notes) {
      if (path && !folders.some((folder) => folder.name === path)) side.notes.delete(path);
      else entry.seq = -1;
    }
  } catch (e) {
    if (mine !== seq || generation !== side.generation) return;
    if (!side.loaded) side.folders = [];
  }
  drawSide();
}

function foldSide() {
  side.mini = !side.mini;
  const generation = side.folding = (side.folding || 0) + 1;
  const body = document.body;
  body.classList.toggle('side-folding', sideWide() && !stillMotion());
  body.classList.toggle('side-mini', side.mini);
  sideResize.fit();
  try {
    localStorage.setItem(SIDE_KEY, side.mini ? 'mini' : 'wide');
  } catch (e) {}
  drawSide();
  window.dispatchEvent(new Event('resize'));
  if (body.classList.contains('side-folding')) {
    const transitions = body.getAnimations().filter((run) => run.transitionProperty === '--side');
    Promise.allSettled(transitions.map((run) => run.finished)).then(() => {
      if (side.folding !== generation) return;
      body.classList.remove('side-folding');
      window.dispatchEvent(new Event('resize'));
    });
  }
}

function moveOpenFolders(from, to) {
  const moved = (path) => (path === from || path.startsWith(`${from}/`) ? to + path.slice(from.length) : path);
  for (const set of [side.open, side.revealed, side.shut]) {
    const paths = [...set];
    set.clear();
    for (const path of paths) set.add(moved(path));
  }
  keepOpenFolders();
}

function openFolder(el) {
  const path = el.dataset.dir;
  if (el.closest('#side .side-tree')) {
    side.target = path;
    twistFolder(path, el);
    return;
  }
  closeSheet();
  go(folderHash(path));
}

function sideNoteRows(path, depth) {
  const entry = side.notes.get(path);
  const action = (label, offset = 0) => `<button class="side-note-page" data-action="side-notes" data-dir="${esc(path)}" data-offset="${offset}">${label}</button>`;
  if (!entry) return `<div class="side-note-status" data-drop-dir="${esc(path)}" style="--depth:${depth}">${action('Show notes')}</div>`;
  const selected = state.view === 'note' ? state.session?.note?.id : null;
  for (const note of entry.items) side.sig.push(`n:${note.id}`);
  const rows = entry.items.map((note) => `<button class="side-row side-note${selected === note.id ? ' on' : ''}" data-action="open-note" data-id="${esc(note.id)}" data-from="${esc(path)}" data-drop-dir="${esc(path)}" data-title="${esc(note.title)}" draggable="true" style="--depth:${depth}" title="${esc(note.title)}"${selected === note.id ? ' aria-current="page"' : ''}>${selected === note.id ? '<span class="side-pill" aria-hidden="true"></span>' : ''}${ICON.note}<span class="side-label">${esc(note.title || 'Untitled')}</span></button>`).join('');
  const status = entry.loading && !entry.refreshing ? '<span>Opening notes…</span>' : entry.error ? `Notes could not be opened. ${action('Try again', entry.offset)}` : !entry.items.length && !side.folders.some((folder) => folder.name.startsWith(path ? `${path}/` : '')) ? '<span>Empty folder</span>' : '';
  const pages = !entry.error ? `${entry.offset ? action('Previous notes', Math.max(0, entry.offset - SIDE_NOTES_PAGE)) : ''}${entry.offset + entry.items.length < entry.total ? action('Next notes', entry.offset + SIDE_NOTES_PAGE) : ''}` : '';
  return rows + (status || pages ? `<div class="side-note-status" data-drop-dir="${esc(path)}" style="--depth:${depth}">${status}${pages}</div>` : '');
}

function loadSideNotes(path, offset = 0, force = false) {
  if (path !== '' && !side.folders.some((folder) => folder.name === path)) return;
  if (!Number.isSafeInteger(offset) || offset < 0) return;
  const previous = side.notes.get(path);
  if (previous?.seq === seq && (previous.loading || !force)) return;
  if (!previous && side.notes.size >= SIDE_NOTES_FOLDERS) {
    const oldest = [...side.notes.keys()].find((key) => !side.automatic.has(key));
    side.notes.delete(oldest);
  }
  const entry = { items: previous?.items || [], total: previous?.total || 0, offset, loading: true, refreshing: Boolean(previous?.items.length && previous.offset === offset), error: false, seq };
  side.notes.set(path, entry);
  side.pending = side.pending.filter((read) => side.notes.get(read.path) === read.entry);
  side.pending.push({ path, entry });
  pumpSideNotes();
  if (force) drawSide();
}

function pumpSideNotes() {
  while (side.reading < SIDE_NOTES_READS && side.pending.length) {
    const read = side.pending.shift();
    if (side.notes.get(read.path) !== read.entry || read.entry.seq !== seq) continue;
    side.reading++;
    readSideNotes(read.path, read.entry).finally(() => { side.reading--; pumpSideNotes(); });
  }
}

async function readSideNotes(path, entry) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), SIDE_NOTES_TIMEOUT);
  try {
    const query = new URLSearchParams({ dir: path, offset: String(entry.offset), limit: String(SIDE_NOTES_PAGE), brief: 'true', sort: 'title', show: 'all' });
    const page = await api(`/api/notes?${query}`, { total: true, signal: controller.signal });
    if (side.notes.get(path) !== entry || entry.seq !== seq) return;
    if (!Array.isArray(page.items)) throw new Error('Notes could not be opened.');
    entry.items = page.items.slice(0, SIDE_NOTES_PAGE).filter((note) => typeof note.id === 'string' && typeof note.title === 'string');
    entry.total = Number.isSafeInteger(page.total) && page.total >= 0 ? page.total : entry.items.length;
  } catch (e) {
    if (side.notes.get(path) === entry && entry.seq === seq) entry.error = true;
  } finally {
    clearTimeout(timeout);
    entry.loading = false;
    if (side.notes.get(path) === entry && entry.seq === seq) drawSide();
  }
}
