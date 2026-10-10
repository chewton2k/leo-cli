const SIDE_KEY = 'leo-side';
const sideWide = () => Boolean(window.matchMedia && window.matchMedia('(min-width: 1000px)').matches);
const side = { folders: [], loaded: false, mini: false, open: new Set(), shut: new Set(), here: null };
const SIDE_OPEN_KEY = 'leo-side-open';

try {
  side.open = new Set(JSON.parse(localStorage.getItem(SIDE_OPEN_KEY) || '[]'));
} catch (e) {}

try {
  side.mini = localStorage.getItem(SIDE_KEY) === 'mini';
} catch (e) {}
document.body.classList.toggle('side-mini', side.mini);

const shortcutLabel = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? '⌘K' : 'Ctrl K';

function sideRow(action, icon, label, { on = false, attrs = '', count = '' } = {}) {
  const tail = count === '' ? '' : `<span class="side-count side-label">${esc(String(count))}</span>`;
  return `<button class="side-row${on ? ' on' : ''}" data-action="${action}"${attrs} title="${esc(label)}"${on ? ' aria-current="page"' : ''}>${icon}<span class="side-label">${esc(label)}</span>${tail}</button>`;
}

function sidePlace() {
  const view = state.view;
  if (view === 'folder' && !state.dir) return 'home';
  if (view === 'folder' || view === 'note' || view === 'new') return `dir:${state.dir || ''}`;
  if (view === 'storage') return 'storage';
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
    side.shut.clear();
    const parts = dir.split('/').filter(Boolean);
    for (let i = 1; i < parts.length; i++) side.open.add(parts.slice(0, i).join('/'));
    keepOpenFolders();
  }
  const folders = folderTree(dir);
  box.innerHTML = `
    <div class="side-head">
      <button class="brand side-label" data-action="home">leo</button>
      <button class="icon-btn side-fold" data-action="side-fold" aria-label="${side.mini ? 'Widen the sidebar' : 'Narrow the sidebar'}" title="${side.mini ? 'Widen the sidebar' : 'Narrow the sidebar'}">${ICON.sidebar}</button>
    </div>
    <button class="side-new" data-action="new" data-dir="${esc(dir)}" title="New note${dir ? ` in ${esc(folderLabel(dir))}` : ''}">${ICON.plus}<span class="side-label">New note</span></button>
    <button class="side-search" data-action="side-search" title="Search every note (${shortcutLabel})">${ICON.search}<span class="side-label">Search</span><kbd class="side-label">${shortcutLabel}</kbd></button>
    <nav class="side-nav" aria-label="Places">
      ${sideRow('home', ICON.note, 'All notes', { on: here === 'home' })}
      ${sideRow('chat', chat.button(22), 'Ask Felix', { on: chatOpen })}
      ${sideRow('map', ICON.map, 'Knowledge graph', { on: here === 'map' })}
      ${sideRow('record', ICON.mic, 'Record', { on: here === 'record', attrs: ` data-dir="${esc(dir)}"` })}
      ${sideRow('upload', ICON.upload, 'Note from a file')}
      <div class="side-group"><span class="side-label">Folders</span><button class="side-add side-label" data-action="new-folder" data-parent="" aria-label="New folder" title="New folder at the top level">${ICON.plus}</button></div>
      ${folders || '<p class="side-none side-label">Folders you make show up here.</p>'}
    </nav>
    <div class="side-foot">
      ${sideRow('trash', ICON.trash, 'Trash', { on: here === 'trash' })}
      ${sideRow('storage', ICON.storage, 'Storage', { on: here === 'storage' })}
      ${sideRow('settings', ICON.gear, 'Settings', { on: here === 'settings' })}
    </div>`;
}

function sideHere() {
  return state.view === 'folder' || state.view === 'note' ? state.dir || '' : '';
}

function folderShown(path) {
  return !side.shut.has(path) && side.open.has(path);
}

function keepOpenFolders() {
  try {
    localStorage.setItem(SIDE_OPEN_KEY, JSON.stringify([...side.open]));
  } catch (e) {}
}

function folderTree(here) {
  const paths = new Set(side.folders.map((d) => d.name));
  const children = (parent) => side.folders.filter((d) => {
    const cut = d.name.lastIndexOf('/');
    return (cut < 0 ? '' : d.name.slice(0, cut)) === parent;
  });
  const rows = [];
  const walk = (parent, depth) => {
    for (const d of children(parent)) {
      const kids = side.folders.some((k) => k.name.startsWith(`${d.name}/`) && paths.has(k.name));
      const open = kids && folderShown(d.name);
      const name = d.name.split('/').pop();
      const on = here === d.name;
      const toggle = kids
        ? `<button class="side-twist${open ? ' open' : ''}" data-action="side-twist" data-dir="${esc(d.name)}" aria-expanded="${open}" aria-label="${open ? 'Close' : 'Open'} ${esc(name)}">${ICON.chevron}</button>`
        : '<span class="side-twist none"></span>';
      rows.push(`<div class="side-folder${on ? ' on' : ''}" style="--depth:${depth}">${toggle}<button class="side-row" data-action="open-folder" data-dir="${esc(d.name)}" draggable="true" title="${esc(d.name)}"${on ? ' aria-current="page"' : ''}>${ICON.folder}<span class="side-label">${esc(name)}</span>${d.notes ? `<span class="side-count side-label">${d.notes}</span>` : ''}</button><button class="side-add side-sub" data-action="new-folder" data-parent="${esc(d.name)}" aria-label="New folder in ${esc(name)}" title="New folder in ${esc(name)}">${ICON.plus}</button></div>`);
      if (open) walk(d.name, depth + 1);
    }
  };
  walk('', 0);
  return rows.join('');
}

function twistFolder(path) {
  if (folderShown(path)) {
    side.open.delete(path);
    side.shut.add(path);
  } else {
    side.open.add(path);
    side.shut.delete(path);
  }
  keepOpenFolders();
  drawSide();
}

async function loadSideFolders() {
  if (!sideWide() || state.view === 'locked') return drawSide();
  try {
    side.folders = await api('/api/folders');
    side.loaded = true;
  } catch (e) {
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
