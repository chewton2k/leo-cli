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
  swipeToClose(scrim.querySelector('.sheet'));
  return scrim;
}

const SWIPE_CLOSE = 90;
const SWIPE_FLICK = 0.6;

function swipeToClose(panel) {
  let start = null;
  const reset = () => {
    panel.style.transition = '';
    panel.style.transform = '';
    start = null;
  };
  panel.addEventListener('touchstart', (e) => {
    if (e.touches.length !== 1 || window.matchMedia('(min-width: 600px)').matches) return;
    const inside = e.target.closest('textarea, input, select, .viewer-pan, .viewer-frame');
    if (inside || panel.scrollTop > 0) return;
    start = { y: e.touches[0].clientY, at: performance.now(), moved: 0 };
  }, { passive: true });
  panel.addEventListener('touchmove', (e) => {
    if (!start) return;
    const moved = e.touches[0].clientY - start.y;
    if (moved <= 0) {
      panel.style.transform = '';
      start.moved = 0;
      return;
    }
    start.moved = moved;
    panel.style.transition = 'none';
    panel.style.transform = `translateY(${moved}px)`;
    if (e.cancelable) e.preventDefault();
  }, { passive: false });
  const finish = () => {
    if (!start) return;
    const { moved, at } = start;
    const speed = moved / Math.max(1, performance.now() - at);
    if (moved > SWIPE_CLOSE || (moved > 24 && speed > SWIPE_FLICK)) {
      panel.style.transition = 'transform .18s ease-in';
      panel.style.transform = 'translateY(110%)';
      start = null;
      setTimeout(closeSheet, 170);
      return;
    }
    panel.style.transition = 'transform .2s ease-out';
    panel.style.transform = '';
    setTimeout(reset, 210);
  };
  panel.addEventListener('touchend', finish);
  panel.addEventListener('touchcancel', reset);
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
    <button class="list-row" data-action="map">${ICON.map}<span class="grow">Knowledge graph</span></button>
    <button class="list-row" data-action="settings">${ICON.gear}<span class="grow">Settings</span></button>
    <button class="list-row" data-action="trash">${ICON.trash}<span class="grow">Trash</span></button>
    <button class="list-row" data-action="refresh">${ICON.refresh}<span class="grow">Refresh</span></button>`);
}

function newFolder(el) {
  const given = el && el.dataset && el.dataset.parent;
  const parent = given !== undefined ? given : state.view === 'folder' || state.view === 'note' ? state.dir || '' : '';
  const scrim = sheet(`<h3>New folder${parent ? ` in ${esc(folderLabel(parent))}` : ''}</h3>
    <label class="field">${ICON.folder}<input id="folder-name" data-parent="${esc(parent)}" placeholder="${parent ? 'Name, e.g. week 3' : 'Name, e.g. cs130'}" autocomplete="off" enterkeyhint="done"></label>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="create-folder">Create</button></div>`);
  const input = $('#folder-name', scrim);
  input.focus();
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') createFolder();
  });
}

async function createFolder() {
  const input = $('#folder-name');
  const name = input.value.trim().replace(/^\/+|\/+$/g, '');
  if (!name) return;
  const parent = input.dataset.parent || '';
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

async function openTitle(title) {
  const want = title.trim().toLowerCase();
  if (!want) return;
  const hits = await api(`/api/search?q=${enc(title)}&brief=true`);
  const hit = hits.find((n) => n.title.trim().toLowerCase() === want);
  go(hit ? noteHash(hit.id) : `#/search/${enc(title)}`);
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
