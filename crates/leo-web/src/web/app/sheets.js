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
  closeSheet(true);
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

function closeSheet(now = false) {
  const open = $('.scrim');
  const returnFocus = open && open.returnFocus;
  if (open && now) open.remove();
  else if (open) {
    const ghost = open.cloneNode(true);
    const typed = open.querySelectorAll('input, textarea, select');
    ghost.querySelectorAll('input, textarea, select').forEach((el, i) => {
      if (typed[i]) el.value = typed[i].value;
    });
    ghost.classList.replace('scrim', 'scrim-gone');
    const inner = ghost.querySelector('.sheet');
    if (inner) inner.classList.replace('sheet', 'sheet-gone');
    ghost.setAttribute('aria-hidden', 'true');
    ghost.inert = true;
    for (const el of [ghost, ...ghost.querySelectorAll('*')]) {
      for (const { name } of [...el.attributes]) if (name === 'id' || name.startsWith('data-')) el.removeAttribute(name);
    }
    const scrolled = open.querySelector('.sheet');
    open.replaceWith(ghost);
    if (inner && scrolled) inner.scrollTop = scrolled.scrollTop;
    setTimeout(() => ghost.remove(), LEAVE_MS);
  }
  if (state.keepPending) {
    state.keepPending = null;
    if (state.view === 'storage' && state.storage) drawStorage(state.storage);
  }
  if (returnFocus && returnFocus.isConnected) returnFocus.focus({ preventScroll: true });
}

const QUICK_PLACES = [
  { name: 'Notes', hash: '#/', icon: 'note' },
  { name: 'Calendar', hash: '#/calendar', icon: 'calendar' },
  { name: 'Interview practice', hash: '#/interviews', icon: 'chat' },
  { name: 'Knowledge graph', hash: '#/map', icon: 'map' },
  { name: 'Record', hash: '#/record', icon: 'mic' },
  { name: 'Trash', hash: '#/trash', icon: 'trash' },
  { name: 'Storage', hash: '#/settings/storage', icon: 'storage' },
  { name: 'Settings', hash: '#/settings', icon: 'gear' },
];
const QUICK_SEARCH_MS = 120;
const quick = { due: [], rows: [], at: 0, notes: [], asked: '', timer: 0, seq: 0, busy: false, root: null };
const inQuick = (sel) => (quick.root && !quick.root.classList.contains('scrim-gone') ? quick.root.querySelector(sel) : null);

function quickLeo() {
  Object.assign(quick, { due: [], rows: [], at: 0, notes: [], asked: '' });
  quick.root = sheet(`<div class="quick-leo">
    <label class="quick-bar">${ICON.bolt}<input id="quick-input" placeholder="Ask, remind, find or jot anything…" autocomplete="off" spellcheck="false" enterkeyhint="go" maxlength="2000" aria-label="Quick Leo" role="combobox" aria-controls="quick-results" aria-expanded="true"><kbd>${quickLabel}</kbd></label>
    <div class="quick-results" id="quick-results" role="listbox" aria-label="Results"></div>
  </div>`);
  const input = inQuick('#quick-input');
  input.addEventListener('input', () => {
    quick.at = 0;
    drawQuick();
    clearTimeout(quick.timer);
    quick.timer = setTimeout(() => quickSearch().catch(() => {}), QUICK_SEARCH_MS);
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      quickGo(quick.at).catch(fail);
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (!quick.rows.length) return;
      e.preventDefault();
      quick.at = (quick.at + (e.key === 'ArrowDown' ? 1 : quick.rows.length - 1)) % quick.rows.length;
      drawQuick();
    }
  });
  drawQuick();
  input.focus();
  loadQuickDue().catch(() => {});
}

async function quickSearch() {
  const text = (inQuick('#quick-input')?.value || '').trim();
  const mine = ++quick.seq;
  const found = text.length >= 2 ? await api(`/api/search?q=${enc(text)}&brief=true&limit=4`) : [];
  if (mine !== quick.seq || !inQuick('#quick-input')) return;
  quick.notes = (found || []).map((n) => ({ id: n.id, title: n.title }));
  quick.asked = text;
  drawQuick();
}

function quickWhen(date, time) {
  if (!date) return 'no date';
  const today = S.dayKey(new Date());
  const day = date === today ? 'Today' : date === S.dayKey(S.addDays(new Date(), 1)) ? 'Tomorrow' : S.fromKey(date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' });
  return time ? `${day}, ${S.timeLabel(time, calLocale())}` : day;
}

function quickLook(row) {
  if (row.kind === 'ask') return { icon: chat.button(20), label: row.text, hint: 'Ask Felix' };
  if (row.kind === 'task') return { icon: ICON.check, label: row.title, hint: row.date ? `Reminder · ${quickWhen(row.date, row.time)}` : 'Add to tasks' };
  if (row.kind === 'open') return { icon: ICON.note, label: row.title, hint: 'Open note' };
  if (row.kind === 'place') return { icon: ICON[QUICK_PLACES.find((p) => p.hash === row.hash)?.icon] || ICON.folder, label: row.name, hint: 'Go to' };
  return { icon: ICON.plus, label: row.title, hint: 'New note' };
}

function drawQuick() {
  const box = inQuick('#quick-results');
  if (!box) return;
  const text = (inQuick('#quick-input')?.value || '').trim();
  const notes = quick.asked && text.toLowerCase().startsWith(quick.asked.toLowerCase()) ? quick.notes : [];
  quick.rows = S.quickRank(text, { notes, places: QUICK_PLACES });
  quick.at = Math.min(quick.at, Math.max(0, quick.rows.length - 1));
  if (!quick.rows.length) {
    box.innerHTML = '';
    inQuick('#quick-input')?.removeAttribute('aria-activedescendant');
    drawQuickDue();
    return;
  }
  box.innerHTML = quick.rows.map((row, i) => {
    const look = quickLook(row);
    const on = i === quick.at;
    return `<button type="button" class="quick-row${on ? ' on' : ''}" id="quick-row-${i}" role="option" aria-selected="${on}" data-action="quick-run" data-at="${i}" tabindex="-1"><span class="quick-icon">${look.icon}</span><span class="quick-label">${esc(look.label)}</span><span class="quick-hint">${esc(look.hint)}</span>${on ? '<kbd>↵</kbd>' : ''}</button>`;
  }).join('');
  inQuick('#quick-input')?.setAttribute('aria-activedescendant', `quick-row-${quick.at}`);
}

async function loadQuickDue() {
  const tasks = (await api('/api/tasks')).tasks || [];
  const groups = tasksGroups(tasks);
  quick.due = groups.filter(([name]) => name === 'Overdue' || name === 'Today').flatMap(([name, list]) => list.map((t) => ({ ...t, late: name === 'Overdue' }))).slice(0, 6);
  drawQuickDue();
}

function drawQuickDue() {
  const box = inQuick('#quick-results');
  if (!box || quick.rows.length) return;
  box.innerHTML = quick.due.length
    ? `<div class="quick-due-head">Due today</div>${quick.due.map((t) => `<label class="quick-task${t.late ? ' late' : ''}"><input type="checkbox" class="sched-check" data-action="quick-done" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''} aria-label="Done: ${esc(t.title)}"><span class="grow">${esc(t.title)}</span><span class="hint">${esc(t.late ? 'Overdue' : t.time ? S.timeLabel(t.time, calLocale()) : 'Today')}</span></label>`).join('')}`
    : '';
}

async function quickDone(el) {
  quick.due = quick.due.filter((t) => !(t.id === el.dataset.id && (t.on || null) === (el.dataset.on || null)));
  drawQuickDue();
  await setTaskDone(el.dataset.id, true, el.dataset.on || null);
  loadTasksWindow().catch(() => {});
}

async function quickGo(at) {
  const row = quick.rows[at];
  if (!row || quick.busy) return;
  if (row.kind === 'ask') {
    closeSheet();
    if (!chat.askNew(row.text)) toast('Felix is finishing another answer. Try again in a moment.', { bad: true });
    return;
  }
  if (row.kind === 'open' || row.kind === 'place') {
    closeSheet();
    go(row.kind === 'open' ? noteHash(row.id) : row.hash);
    return;
  }
  quick.busy = true;
  try {
    if (row.kind === 'task') {
      const made = await api('/api/tasks', { method: 'POST', body: { title: row.title.slice(0, 200), date: row.date, time: row.time } });
      closeSheet();
      loadTasksWindow().catch(() => {});
      toast(`Added “${made.title}” · ${quickWhen(row.date, row.time)}`, {
        action: 'Undo',
        run: async () => {
          await api(`/api/tasks/${enc(made.id)}`, { method: 'DELETE' });
          loadTasksWindow().catch(() => {});
        },
      });
      return;
    }
    const [first, ...rest] = row.title.split('\n');
    const note = await api('/api/notes', { method: 'POST', body: { title: first.trim().slice(0, 120) || 'Quick note', body: rest.join('\n').trim() } });
    closeSheet();
    toast(`Saved “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
    if (state.view === 'folder') showLatest().catch(() => {});
  } finally {
    quick.busy = false;
  }
}

function menu() {
  const here = state.view === 'folder' ? state.dir : '';
  sheet(`
    <button class="list-row" data-action="quick-leo">${ICON.bolt}<span class="grow">Quick Leo</span></button>
    <button class="list-row" data-action="new" data-dir="${esc(here)}">${ICON.plus}<span class="grow">New note</span></button>
    <button class="list-row" data-action="record" data-dir="${esc(here)}">${ICON.mic}<span class="grow">Record a lecture or meeting</span></button>
    <button class="list-row" data-action="upload">${ICON.upload}<span class="grow">Make a note from a file</span></button>
    <button class="list-row" data-action="new-folder">${ICON.folderPlus}<span class="grow">New folder${here ? ` in ${esc(folderLabel(here))}` : ''}</span></button>
    <button class="list-row" data-action="interviews">${ICON.chat}<span class="grow">Interview practice</span></button>
    <button class="list-row" data-action="calendar">${ICON.calendar}<span class="grow">Calendar</span>${tasksDueBadge()}</button>
    <button class="list-row" data-action="map">${ICON.map}<span class="grow">Knowledge graph</span></button>
    <button class="list-row" data-action="settings">${ICON.gear}<span class="grow">Settings</span></button>
    <button class="list-row" data-action="trash">${ICON.trash}<span class="grow">Trash</span></button>
    <button class="list-row" data-action="storage">${ICON.storage}<span class="grow">Storage</span></button>
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

function folderOptions() {
  if (state.view !== 'folder' || !state.dir) return;
  sheet(`<h3>${esc(folderLabel(state.dir))}</h3>
    <button class="list-row" data-action="folder-rename">${ICON.note}<span class="grow">Rename folder</span></button>
    ${parentOf(state.dir) ? `<button class="list-row" data-action="folder-root">${ICON.move}<span class="grow">Move to top level</span></button>` : ''}
    <button class="list-row danger" data-action="folder-delete">${ICON.trash}<span class="grow">Delete folder</span></button>`);
}

function renameFolderSheet() {
  if (state.view !== 'folder' || !state.dir) return;
  const from = state.dir;
  const box = sheet(`<h3>Rename folder</h3>
    <label class="field">${ICON.folder}<input id="folder-rename" data-from="${esc(from)}" value="${esc(folderLabel(from))}" maxlength="120" autocomplete="off" aria-label="Folder name" enterkeyhint="done"></label>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="folder-rename-save">Rename</button></div>`);
  const input = $('#folder-rename', box);
  input.focus();
  input.select();
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); renameFolder().catch(fail); }
  });
}

async function renameFolder() {
  const input = $('#folder-rename');
  const button = $('[data-action="folder-rename-save"]');
  if (!input || !button || button.disabled) return;
  const from = input.dataset.from;
  const name = input.value.trim();
  if (!name || name === folderLabel(from)) return;
  if (new TextEncoder().encode(name).length > 240) return toast('That folder name is too long. Use a shorter name.', { bad: true });
  if (name.includes('/') || name.includes('\\') || name === '.' || name === '..') return toast('Use a single folder name, without / or ..', { bad: true });
  const box = input.closest('.scrim');
  button.disabled = true;
  try {
    const moved = await api('/api/dirs/rename', { method: 'POST', body: { from, name } });
    if (box.isConnected) closeSheet();
    await afterMove(from, moved.path);
    toast(`Renamed folder to “${name}”`, {
      action: 'Undo',
      run: () => api('/api/dirs/rename', { method: 'POST', body: { from: moved.path, name: folderLabel(from) } }).then(() => afterMove(moved.path, from)).catch(fail),
    });
  } catch (error) {
    if (error.status === 409) toast('That folder name is already in use. Choose another name.', { bad: true });
    else fail(error);
  } finally { if (button.isConnected) button.disabled = false; }
}

function deleteFolderAsk() {
  if (state.view !== 'folder' || !state.dir) return;
  const from = state.dir;
  sheet(`<h3>Delete “${esc(folderLabel(from))}”?</h3>
    <p>This folder, its subfolders and all their notes move to the trash. Undo brings them back.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="folder-delete-now" data-from="${esc(from)}">Move to trash</button></div>`);
}

async function deleteFolderNow(el) {
  if (el.disabled) return;
  const from = el.dataset.from;
  if (!from) return;
  const box = el.closest('.scrim');
  el.disabled = true;
  try {
    const done = await api('/api/trash/move', { method: 'POST', body: { notes: [], dirs: [from] } });
    if (box.isConnected) closeSheet();
    if (state.view === 'folder' && (state.dir === from || state.dir.startsWith(`${from}/`))) go(folderHash(parentOf(from)), { replace: true });
    else await showLatest();
    loadSideFolders();
    toast(`Moved “${folderLabel(from)}” to the trash`, { action: 'Undo', run: () => undoTrashMove(done) });
  } catch (error) { fail(error); }
  finally { if (el.isConnected) el.disabled = false; }
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
  sheet(`<h3>Move “${esc(note.title)}”</h3>${row('', 'Top level')}${folders.map((f) => row(f.name, f.name)).join('')}`);
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

function discardRecordingAsk(el) {
  const box = sheet(`<h3>Delete this recording?</h3><p class="hint">Its saved audio will be permanently deleted from this device and your computer. Notes already written stay.</p><div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="rec-discard-now" data-id="${esc(el.dataset.id)}">Delete recording</button></div>`);
  box.returnFocus = el;
  box.querySelector('[data-action="close"]').focus();
}

async function discardRecordingNow(el) {
  if (el.disabled) return;
  el.disabled = true;
  const box = el.closest('.scrim');
  try {
    await recorder.discard(el.dataset.id);
    if (box && box.isConnected) closeSheet();
  } finally {
    if (el.isConnected) el.disabled = false;
  }
}
