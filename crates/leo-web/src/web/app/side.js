const SIDE_KEY = 'leo-side';
const sideWide = () => Boolean(window.matchMedia && window.matchMedia('(min-width: 1000px)').matches);
const side = { folders: [], loaded: false, mini: false, open: new Set(), revealed: new Set(), shut: new Set(), here: null, target: null, twisted: null, notes: new Map(), pending: [], reading: 0, visible: [], automatic: new Set(), generation: 0 };
// Folders the user opened with their own hands, kept across restarts. Folders
// unfolded only to show the open note or folder (`side.revealed`) are not
// kept, so the explorer reopens exactly as the user left it, closed by default.
const SIDE_OPEN_KEY = 'leo-side-open';
const SIDE_NOTES_PAGE = 100;
const SIDE_NOTES_FOLDERS = 64;
const SIDE_NOTES_AUTOMATIC = 32;
const SIDE_NOTES_READS = 4;
const SIDE_NOTES_TIMEOUT = 10000;

try {
  side.open = new Set(JSON.parse(localStorage.getItem(SIDE_OPEN_KEY) || '[]'));
} catch (e) {}

try {
  side.mini = localStorage.getItem(SIDE_KEY) === 'mini';
} catch (e) {}
document.body.classList.toggle('side-mini', side.mini);

const shortcutLabel = ON_MAC ? '⌘K' : 'Ctrl K';
const quickLabel = ON_MAC ? '⌘J' : 'Ctrl J';

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
  if (box.hidden) return;
  const here = sidePlace();
  const chatOpen = document.body.classList.contains('chat-open');
  const dir = sideHere();
  if (dir !== side.here) {
    side.here = dir;
    side.target = null;
    side.shut.clear();
    side.revealed.clear();
    const parts = dir.split('/').filter(Boolean);
    for (let i = 1; i <= parts.length; i++) side.revealed.add(parts.slice(0, i).join('/'));
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
  box.innerHTML = `
    <div class="side-head" data-tauri-drag-region="deep">
      <button class="brand side-label" data-action="home">leo</button>
      <button class="icon-btn side-fold" data-action="side-fold" aria-label="${side.mini ? 'Show folders' : 'Hide folders'}" title="${side.mini ? 'Show folders' : 'Hide folders'}" aria-expanded="${!side.mini}" aria-controls="side-explorer">${ICON.sidebar}</button>
    </div>
    <div class="side-rail">
      <nav class="side-rail-main" aria-label="Leo tools">
        ${sideRow('new', ICON.plus, 'New note', { attrs: ` data-dir="${esc(actionDir)}"`, hint: actionDir ? `New note in ${folderLabel(actionDir)}` : 'New note' })}
        ${sideRow('side-search', ICON.search, 'Search', { hint: `Search every note (${shortcutLabel})` })}
        ${sideRow('quick-leo', ICON.bolt, 'Quick Leo', { hint: `Quick Leo · ask, remind, find or jot anything (${quickLabel})` })}
        ${sideRow('chat', chat.button(22), 'Ask Felix', { on: chatOpen })}
        ${sideRow('interviews', ICON.chat, 'Interview practice', { on: here === 'interviews' })}
        ${sideRow('calendar', ICON.calendar, 'Calendar', { on: here === 'calendar' })}
        ${sideRow('boards', ICON.board, 'Boards', { on: here === 'boards', hint: 'Boards · track tasks from To do to Done' })}
        ${sideRow('map', ICON.map, 'Knowledge graph', { on: here === 'map' })}
        ${sideRow('record', ICON.mic, 'Record', { on: here === 'record', attrs: ` data-dir="${esc(actionDir)}"` })}
        ${sideRow('upload', ICON.upload, 'Note from a file', { attrs: ` data-dir="${esc(actionDir)}"` })}
      </nav>
      <nav class="side-foot" aria-label="This app">
        ${sideRow('settings', ICON.gear, 'Settings', { on: here === 'settings' })}
      </nav>
    </div>
    <div class="side-explorer" id="side-explorer">
      <div class="side-group" data-drop-dir="" title="Drop a folder or note here to move it to the top level"><span>Folders</span><span class="side-group-tools"><button class="side-add side-collapse" data-action="side-collapse" aria-label="Collapse all folders" title="Collapse all folders">${svg('<path d="m7 20 5-5 5 5"/><path d="m7 4 5 5 5-5"/>')}</button><button class="side-add" data-action="new-folder" data-parent="" aria-label="New folder" title="New folder at the top level">${ICON.plus}</button></span></div>
      <nav class="side-nav side-tree" aria-label="Folders" data-drop-dir="">
        ${folders || '<p class="side-none">Folders you make show up here.</p>'}
      </nav>
    </div>`;
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
  if (!tree || !id || side.shownNote === id) return;
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
      rows.push(`<div class="side-folder${on ? ' on' : ''}" style="--depth:${depth}">${toggle}<button class="side-row" data-action="open-folder" data-dir="${esc(d.name)}" draggable="true" title="${esc(d.name)}" aria-expanded="${open}"${on ? ' aria-current="page"' : ''}>${ICON.folder}<span class="side-label">${esc(name)}</span>${d.notes ? `<span class="side-count side-label">${esc(String(d.notes))}</span>` : ''}</button><button class="side-add side-sub" data-action="new-folder" data-parent="${esc(d.name)}" aria-label="New folder in ${esc(name)}" title="New folder in ${esc(name)}">${ICON.plus}</button></div>`);
      if (open) {
        side.visible.push(d.name);
        rows.push(`<div class="side-kids" data-kids="${esc(d.name)}" style="--depth:${depth}">`);
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

function twistFolder(path, control = null) {
  if (control) control.focus({ preventScroll: true });
  const was = folderShown(path);
  if (was) {
    side.open.delete(path);
    side.shut.add(path);
  } else {
    side.open.add(path);
    side.shut.delete(path);
  }
  keepOpenFolders();
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
  document.body.classList.toggle('side-mini', side.mini);
  try {
    localStorage.setItem(SIDE_KEY, side.mini ? 'mini' : 'wide');
  } catch (e) {}
  drawSide();
  window.dispatchEvent(new Event('resize'));
}

function searchEverywhere() {
  openSearch('');
  $('#search-input').focus();
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
  if (!entry) return `<div class="side-note-status" style="--depth:${depth}">${action('Show notes')}</div>`;
  const selected = state.view === 'note' ? state.session?.note?.id : null;
  const rows = entry.items.map((note) => `<button class="side-row side-note${selected === note.id ? ' on' : ''}" data-action="open-note" data-id="${esc(note.id)}" data-from="${esc(path)}" data-title="${esc(note.title)}" draggable="true" style="--depth:${depth}" title="${esc(note.title)}"${selected === note.id ? ' aria-current="page"' : ''}>${ICON.note}<span class="side-label">${esc(note.title || 'Untitled')}</span></button>`).join('');
  const status = entry.loading ? '<span>Opening notes…</span>' : entry.error ? `Notes could not be opened. ${action('Try again', entry.offset)}` : !entry.items.length && !side.folders.some((folder) => folder.name.startsWith(path ? `${path}/` : '')) ? '<span>Empty folder</span>' : '';
  const pages = !entry.error ? `${entry.offset ? action('Previous notes', Math.max(0, entry.offset - SIDE_NOTES_PAGE)) : ''}${entry.offset + entry.items.length < entry.total ? action('Next notes', entry.offset + SIDE_NOTES_PAGE) : ''}` : '';
  return rows + (status || pages ? `<div class="side-note-status" style="--depth:${depth}">${status}${pages}</div>` : '');
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
  const entry = { items: previous?.items || [], total: previous?.total || 0, offset, loading: true, error: false, seq };
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
