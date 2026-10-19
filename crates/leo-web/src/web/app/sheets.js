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

const QUICK_KINDS = [['ask', 'Ask Felix'], ['task', 'Remind me'], ['note', 'Note']];
const QUICK_GO = { ask: 'Ask Felix', task: 'Add reminder', note: 'Save note' };
const quick = { kind: null, due: [] };

function quickLeo() {
  quick.kind = null;
  quick.due = [];
  sheet(`<div class="quick-leo">
    <div class="quick-head">${ICON.bolt}<h3>Quick Leo</h3><kbd>${quickLabel}</kbd></div>
    <input id="quick-input" class="set-input" placeholder="Ask Felix, add a reminder, or jot a note…" autocomplete="off" enterkeyhint="go" maxlength="2000" aria-label="Quick Leo">
    <div class="pick-chips quick-kinds" role="radiogroup" aria-label="What to do">${QUICK_KINDS.map(([id, label]) => `<button type="button" class="pick-chip" role="radio" data-action="quick-kind" data-kind="${id}" aria-checked="false">${label}</button>`).join('')}</div>
    <p class="hint quick-preview" id="quick-preview" aria-live="polite"></p>
    <div class="quick-due" id="quick-due"></div>
    <div class="buttons"><button class="btn plain" data-action="close">Close</button><button class="btn primary" data-action="quick-go" id="quick-go" disabled>Ask Felix</button></div>
  </div>`);
  const input = $('#quick-input');
  input.addEventListener('input', drawQuick);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      quickGo().catch(fail);
    } else if (e.key === 'Tab' && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      const ids = QUICK_KINDS.map(([id]) => id);
      quick.kind = ids[(ids.indexOf(quickPlan().kind) + 1) % ids.length];
      drawQuick();
    }
  });
  drawQuick();
  input.focus();
  loadQuickDue().catch(() => {});
}

function quickPlan() {
  const text = ($('#quick-input')?.value || '').trim();
  const parsed = S.quickParse(text);
  return { text, parsed, kind: quick.kind || parsed.kind };
}

function quickWhen(date, time) {
  if (!date) return 'no date';
  const today = S.dayKey(new Date());
  const day = date === today ? 'Today' : date === S.dayKey(S.addDays(new Date(), 1)) ? 'Tomorrow' : S.fromKey(date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' });
  return time ? `${day}, ${S.timeLabel(time, calLocale())}` : day;
}

function quickTask(plan) {
  return plan.parsed.kind === 'task' ? plan.parsed : S.quickParse(`remind me to ${plan.text}`);
}

function drawQuick() {
  const plan = quickPlan();
  for (const chip of document.querySelectorAll('[data-action="quick-kind"]')) {
    const on = chip.dataset.kind === plan.kind;
    chip.classList.toggle('on', on);
    chip.setAttribute('aria-checked', String(on));
  }
  const preview = $('#quick-preview');
  const button = $('#quick-go');
  if (!preview || !button) return;
  button.textContent = QUICK_GO[plan.kind];
  button.disabled = !plan.text;
  if (!plan.text) {
    preview.textContent = 'Type a question, a reminder like “email Bob tomorrow 3pm”, or a note. Tab switches what happens.';
    return;
  }
  if (plan.kind === 'ask') preview.textContent = 'Felix answers in a new chat.';
  else if (plan.kind === 'task') {
    const t = quickTask(plan);
    preview.textContent = `Reminder: “${t.title || plan.text}” · ${quickWhen(t.date, t.time)}`;
  } else preview.textContent = `New note: “${(plan.parsed.kind === 'note' ? plan.parsed.title : plan.text).split('\n')[0].slice(0, 80)}”`;
}

async function loadQuickDue() {
  const tasks = (await api('/api/tasks')).tasks || [];
  const groups = tasksGroups(tasks);
  quick.due = groups.filter(([name]) => name === 'Overdue' || name === 'Today').flatMap(([name, list]) => list.map((t) => ({ ...t, late: name === 'Overdue' }))).slice(0, 6);
  drawQuickDue();
}

function drawQuickDue() {
  const box = $('#quick-due');
  if (!box) return;
  box.innerHTML = quick.due.length
    ? `<div class="quick-due-head">Due today</div>${quick.due.map((t) => `<label class="quick-task${t.late ? ' late' : ''}"><input type="checkbox" class="sched-check" data-action="quick-done" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''} aria-label="Done: ${esc(t.title)}"><span class="grow">${esc(t.title)}</span><span class="hint">${esc(t.late ? 'Overdue' : t.time ? S.timeLabel(t.time, calLocale()) : 'Today')}</span></label>`).join('')}`
    : '<p class="hint quick-clear">Nothing due today.</p>';
}

async function quickDone(el) {
  quick.due = quick.due.filter((t) => !(t.id === el.dataset.id && (t.on || null) === (el.dataset.on || null)));
  drawQuickDue();
  await setTaskDone(el.dataset.id, true, el.dataset.on || null);
  loadTasksWindow().catch(() => {});
}

async function quickGo() {
  const plan = quickPlan();
  const button = $('#quick-go');
  if (!plan.text || !button || button.disabled) return;
  if (plan.kind === 'ask') {
    closeSheet();
    if (!chat.askNew(plan.text)) toast('Felix is finishing another answer. Try again in a moment.', { bad: true });
    return;
  }
  button.disabled = true;
  try {
    if (plan.kind === 'task') {
      const t = quickTask(plan);
      const made = await api('/api/tasks', { method: 'POST', body: { title: (t.title || plan.text).slice(0, 200), date: t.date, time: t.time } });
      closeSheet();
      loadTasksWindow().catch(() => {});
      toast(`Added “${made.title}” · ${quickWhen(t.date, t.time)}`, {
        action: 'Undo',
        run: async () => {
          await api(`/api/tasks/${enc(made.id)}`, { method: 'DELETE' });
          loadTasksWindow().catch(() => {});
        },
      });
      return;
    }
    const words = plan.parsed.kind === 'note' ? plan.parsed.title : plan.text;
    const [first, ...rest] = words.split('\n');
    const note = await api('/api/notes', { method: 'POST', body: { title: first.trim().slice(0, 120) || 'Quick note', body: rest.join('\n').trim() } });
    closeSheet();
    toast(`Saved “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
    if (state.view === 'folder') showLatest().catch(() => {});
  } finally {
    if (button.isConnected) button.disabled = false;
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
