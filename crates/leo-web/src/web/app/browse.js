const LIBRARY_KEY = 'leo-library';
const PREVIEWS_KEPT = 200;
const LIBRARY_SORTS = [['updated', 'Last edited'], ['oldest', 'Oldest edit'], ['created', 'Newest made'], ['title', 'Title A–Z'], ['title-desc', 'Title Z–A']];
const LIBRARY_SHOWS = [['all', 'All notes'], ['pinned', 'Pinned'], ['tasks', 'Open tasks'], ['week', 'Edited this week']];
const library = { view: 'grid', sort: 'updated', show: 'all', previews: new Map(), observer: null, arrival: null };

try {
  const raw = localStorage.getItem(LIBRARY_KEY) || '{}';
  const kept = raw.length < 1000 ? JSON.parse(raw) : {};
  if (kept && ['grid', 'list'].includes(kept.view)) library.view = kept.view;
  if (kept && LIBRARY_SORTS.some(([id]) => id === kept.sort)) library.sort = kept.sort;
  if (kept && LIBRARY_SHOWS.some(([id]) => id === kept.show)) library.show = kept.show;
} catch (e) {}

function keepLibrary() {
  try { localStorage.setItem(LIBRARY_KEY, JSON.stringify({ view: library.view, sort: library.sort, show: library.show })); } catch (e) {}
}

function libraryQuery(dir, offset = 0) {
  const params = new URLSearchParams({ dir, limit: String(PAGE), offset: String(offset), brief: 'true', sort: library.sort, show: library.show });
  return `/api/notes?${params}`;
}

function libraryChoice(name, label, choices, value) {
  return `<label class="library-choice"><select data-library="${name}" aria-label="${esc(label)}">${choices.map(([id, text]) => `<option value="${id}"${id === value ? ' selected' : ''}>${esc(text)}</option>`).join('')}</select></label>`;
}

function libraryFiltered() {
  return library.show !== 'all';
}

function libraryNoMatch() {
  const name = (LIBRARY_SHOWS.find(([id]) => id === library.show) || [, ''])[1].toLowerCase();
  return `<div class="library-none"><p>No notes here match “${esc(name)}”.</p><button class="btn sm plain" data-action="library-show-all">Show all notes</button></div>`;
}

async function changeLibrary(name, value) {
  if (state.view !== 'folder') return;
  if (name === 'sort' && LIBRARY_SORTS.some(([id]) => id === value)) library.sort = value;
  else if (name === 'show' && LIBRARY_SHOWS.some(([id]) => id === value)) library.show = value;
  else return;
  keepLibrary();
  const mine = ++seq;
  const dir = state.dir;
  try {
    const page = await api(libraryQuery(dir), { total: true });
    if (mine !== seq || state.view !== 'folder' || state.dir !== dir) return;
    state.listing = { ...state.listing, notes: page.items, total: page.total };
    drawFolder();
    const again = $(`[data-library="${name}"]`);
    if (again) again.focus();
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
}

app.addEventListener('change', (e) => {
  const select = e.target.closest && e.target.closest('select[data-library]');
  if (select) changeLibrary(select.dataset.library, select.value);
});

function libraryTools() {
  return `<div class="library-tools">
    <div class="library-views" role="group" aria-label="Note layout">${['grid', 'list'].map((view) => `<button class="btn sm plain${library.view === view ? ' on' : ''}" data-action="library-view" data-view="${view}" aria-pressed="${library.view === view}">${view === 'grid' ? ICON.grid : ICON.list}<span>${view === 'grid' ? 'Grid' : 'List'}</span></button>`).join('')}</div>
    ${libraryChoice('sort', 'Sort notes', LIBRARY_SORTS, library.sort)}
    ${libraryChoice('show', 'Show', LIBRARY_SHOWS, library.show)}
    <span class="grow"></span>
    ${state.dir ? `<button class="icon-btn" data-action="folder-options" aria-label="Folder options" title="Folder options">${ICON.more}</button>` : ''}
    <button class="btn sm plain" data-action="folder-select">${state.selecting ? 'Done' : 'Select'}</button>
  </div>`;
}

function libraryClasses() {
  document.body.classList.toggle('on-library', state.view === 'folder');
}

function stopLibraryArrival() {
  if (library.arrival) library.arrival.cancel();
  library.arrival = null;
}

function arriveInFolder() {
  stopLibraryArrival();
  if (stillMotion() || typeof app.animate !== 'function') return;
  const run = app.animate([{ opacity: .45, transform: 'translateY(4px)' }, { opacity: 1, transform: 'none' }], { duration: MOTION_MS, easing: EASE_OUT });
  library.arrival = run;
  run.onfinish = () => { if (library.arrival === run) library.arrival = null; };
}

function libraryRows(notes) {
  return notes.map((note) => noteCard(note)).join('');
}

function changeLibraryView(el) {
  if (state.view !== 'folder' || !['grid', 'list'].includes(el.dataset.view)) return;
  stopLibraryArrival();
  library.view = el.dataset.view;
  keepLibrary();
  drawFolder();
  const again = $(`[data-action="library-view"][data-view="${library.view}"]`);
  if (again) again.focus();
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
