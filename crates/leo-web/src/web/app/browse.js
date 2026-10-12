const LIBRARY_KEY = 'leo-library';
const OPENED_KEY = 'leo-opened-notes';
const MOST_OPENED = 100;
const PREVIEWS_KEPT = 200;
const QUICK_PREVIEW_CHARS = 12000;
const LIBRARY_SORTS = { updated: 'Recently updated', opened: 'Recently opened', folder: 'Class / folder', title: 'Name' };
const library = { view: 'grid', sort: 'updated', opened: [], previews: new Map(), observer: null };

try {
  const raw = localStorage.getItem(LIBRARY_KEY) || '{}';
  const kept = raw.length < 1000 ? JSON.parse(raw) : {};
  if (kept && ['grid', 'list'].includes(kept.view)) library.view = kept.view;
  if (kept && Object.hasOwn(LIBRARY_SORTS, kept.sort)) library.sort = kept.sort;
  const recent = localStorage.getItem(OPENED_KEY) || '[]';
  const ids = recent.length < 10000 ? JSON.parse(recent) : [];
  if (Array.isArray(ids)) library.opened = [...new Set(ids.filter((id) => typeof id === 'string' && /^[a-zA-Z0-9-]{8,64}$/.test(id)))].slice(0, MOST_OPENED);
} catch (e) {}

function keepLibrary() {
  try { localStorage.setItem(LIBRARY_KEY, JSON.stringify({ view: library.view, sort: library.sort })); } catch (e) {}
}

function openedNote(id) {
  if (!id) return;
  library.opened = [id, ...library.opened.filter((old) => old !== id)].slice(0, MOST_OPENED);
  try { localStorage.setItem(OPENED_KEY, JSON.stringify(library.opened)); } catch (e) {}
}

function libraryQuery(dir, offset = 0) {
  const params = new URLSearchParams({ limit: String(PAGE), offset: String(offset), brief: 'true', sort: library.sort });
  if (dir) params.set('dir', dir);
  if (library.sort === 'opened') params.set('recent', library.opened.join(','));
  return `/api/notes?${params}`;
}

function libraryTools() {
  return `<div class="library-tools">
    <div class="library-views" role="group" aria-label="Note layout">${['grid', 'list'].map((view) => `<button class="btn sm plain${library.view === view ? ' on' : ''}" data-action="library-view" data-view="${view}" aria-pressed="${library.view === view}">${view === 'grid' ? ICON.grid : ICON.list}<span>${view === 'grid' ? 'Grid' : 'List'}</span></button>`).join('')}</div>
    <span class="grow"></span>
    <button class="btn sm plain library-sort" data-action="library-sort">${ICON.sort}<span>${esc(LIBRARY_SORTS[library.sort])}</span></button>
    ${state.dir ? `<button class="icon-btn" data-action="folder-options" aria-label="Folder options" title="Folder options">${ICON.more}</button>` : ''}
    <button class="btn sm plain" data-action="folder-select">${state.selecting ? 'Done' : 'Select'}</button>
  </div>`;
}

function libraryClasses() {
  document.body.classList.toggle('on-library-grid', state.view === 'folder' && library.view === 'grid');
}

function libraryRows(notes, previous = null) {
  const key = (note) => note.pinned ? 'pinned' : `folder:${note.directory}`;
  let before = previous ? key(previous) : null;
  return notes.map((note) => {
    const group = library.sort === 'folder' && !state.dir && key(note) !== before;
    before = key(note);
    const label = note.pinned ? 'Pinned' : note.directory || 'Top level';
    return `${group ? `<div class="note-group">${note.pinned ? ICON.pin : ICON.folder}<span>${esc(label)}</span></div>` : ''}${noteCard(note)}`;
  }).join('');
}

function changeLibraryView(el) {
  if (state.view !== 'folder' || !['grid', 'list'].includes(el.dataset.view)) return;
  library.view = el.dataset.view;
  keepLibrary();
  drawFolder();
  const again = $(`[data-action="library-view"][data-view="${library.view}"]`);
  if (again) again.focus();
}

function librarySortSheet() {
  const box = sheet(`<h3>Sort notes</h3>${Object.entries(LIBRARY_SORTS).map(([key, title]) => `<button class="list-row" data-action="library-order" data-sort="${key}" aria-pressed="${key === library.sort}"><span class="grow">${esc(title)}</span>${key === library.sort ? ICON.check : ''}</button>`).join('')}`);
  box.querySelector('[aria-pressed="true"]').focus();
}

async function changeLibrarySort(el) {
  if (state.view !== 'folder' || !Object.hasOwn(LIBRARY_SORTS, el.dataset.sort)) return;
  library.sort = el.dataset.sort;
  keepLibrary();
  closeSheet();
  const dir = state.dir;
  await showFolder(dir);
  if (state.view === 'folder' && state.dir === dir) {
    const again = $('[data-action="library-sort"]');
    if (again) again.focus();
  }
}

function watchPreviews() {
  if (library.observer) library.observer.disconnect();
  library.observer = null;
  if (state.view !== 'folder' || library.view !== 'grid' || !state.listing) return;
  const notes = new Map(state.listing.notes.map((note) => [note.id, note]));
  const draw = (el) => {
    if (!el.isConnected) return;
    const note = notes.get(el.dataset.id);
    if (!note) return;
    const key = `${note.id}:${note.version}:${note.directory}`;
    let html = library.previews.get(key);
    if (html === undefined) {
      html = md.render(note.preview || note.body || '', { dir: note.directory || '', interactive: false });
      library.previews.set(key, html);
      if (library.previews.size > PREVIEWS_KEPT) library.previews.delete(library.previews.keys().next().value);
    }
    el.innerHTML = `<div class="prose note-mini">${html || '<p class="hint">An empty page, ready for your notes.</p>'}</div>`;
    el.removeAttribute('data-pending');
  };
  const pending = [...app.querySelectorAll('.note-thumb[data-pending]')];
  if (typeof IntersectionObserver !== 'function') {
    pending.slice(0, PAGE).forEach(draw);
    return;
  }
  library.observer = new IntersectionObserver((entries) => {
    for (const entry of entries) if (entry.isIntersecting) {
      library.observer.unobserve(entry.target);
      draw(entry.target);
    }
  }, { rootMargin: '240px 0px' });
  pending.forEach((el) => library.observer.observe(el));
}

async function quickPreview(el) {
  const mine = seq;
  const id = el.dataset.id;
  const box = sheet(`<div class="viewer-head"><b id="preview-title">Note preview</b><button class="icon-btn" data-action="close" aria-label="Close preview">${ICON.close}</button></div><div class="prose preview-body">Loading…</div>`);
  const panel = box.querySelector('.sheet');
  box.returnFocus = el;
  panel.classList.add('note-preview');
  panel.setAttribute('aria-labelledby', 'preview-title');
  panel.querySelector('[data-action="close"]').focus();
  panel.addEventListener('keydown', (event) => {
    if (event.key !== 'Tab') return;
    const buttons = [...panel.querySelectorAll('button, a[href]')];
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if ((event.shiftKey && document.activeElement === first) || (!event.shiftKey && document.activeElement === last)) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    }
  });
  try {
    const note = await api(`/api/notes/${enc(id)}`);
    if (mine !== seq || !box.isConnected) return;
    $('#preview-title', box).textContent = note.title;
    let source = '';
    let count = 0;
    for (const char of note.body) {
      if (count++ >= QUICK_PREVIEW_CHARS) break;
      source += char;
    }
    $('.preview-body', box).innerHTML = md.render(source, { dir: note.directory || '', interactive: false });
    $('.preview-body', box).querySelectorAll('input').forEach((input) => { input.disabled = true; });
    const foot = document.createElement('div');
    foot.className = 'preview-foot';
    foot.innerHTML = `${source.length < note.body.length ? '<span class="hint">The beginning of this note</span>' : ''}<button class="btn primary sm" data-action="preview-open" data-id="${esc(id)}">Open note</button>`;
    panel.appendChild(foot);
  } catch (e) {
    if (mine !== seq || !box.isConnected) return;
    if (e instanceof Locked) return fail(e);
    $('.preview-body', box).textContent = e instanceof Offline ? 'Cannot reach leo. Try again when your computer reconnects.' : 'This note is no longer available. Close the preview and refresh your notes.';
  }
}
