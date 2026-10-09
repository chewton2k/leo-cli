const SIDE_KEY = 'leo-side';
const sideWide = () => Boolean(window.matchMedia && window.matchMedia('(min-width: 1000px)').matches);
const side = { folders: [], loaded: false, mini: false };

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
  if (view === 'folder' || view === 'note' || view === 'new') return `dir:${(state.dir || '').split('/')[0]}`;
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
  const folders = side.folders
    .map((d) => sideRow('open-folder', ICON.folder, d.name, { on: here === `dir:${d.name}`, attrs: ` data-dir="${esc(d.name)}" draggable="true"`, count: d.notes || '' }))
    .join('');
  const dir = state.view === 'folder' || state.view === 'note' ? state.dir || '' : '';
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
      ${sideRow('map', ICON.map, 'Map of ideas', { on: here === 'map' })}
      ${sideRow('record', ICON.mic, 'Record', { on: here === 'record', attrs: ` data-dir="${esc(dir)}"` })}
      ${sideRow('upload', ICON.upload, 'Note from a file')}
      <div class="side-group"><span class="side-label">Folders</span><button class="side-add side-label" data-action="new-folder" aria-label="New folder" title="New folder${dir ? ` in ${esc(folderLabel(dir))}` : ''}">${ICON.plus}</button></div>
      ${folders || '<p class="side-none side-label">Folders you make show up here.</p>'}
    </nav>
    <div class="side-foot">
      ${sideRow('drafts', ICON.pencil, 'Drafts', { on: here === 'drafts' || here === 'draft' })}
      ${sideRow('trash', ICON.trash, 'Trash', { on: here === 'trash' })}
      ${sideRow('storage', ICON.storage, 'Storage', { on: here === 'storage' })}
      ${sideRow('settings', ICON.gear, 'Settings', { on: here === 'settings' })}
    </div>`;
}

async function loadSideFolders() {
  if (!sideWide() || state.view === 'locked') return drawSide();
  try {
    side.folders = await api('/api/dirs?parent=');
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
