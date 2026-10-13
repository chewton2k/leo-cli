const STORE_COLORS = ['#4f46e5', '#0ea5e9', '#16a34a', '#f59e0b', '#db2777', '#8b5cf6', '#14b8a6', '#ef4444', '#64748b', '#a3a29c'];

async function showStorage() {
  const mine = ++seq;
  state = { view: 'storage', dir: '', storageOpen: (state.view === 'storage' && state.storageOpen) || new Set() };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button data-action="settings">Settings</button><span class="sep">/</span><button>Storage</button>';
  document.title = 'Storage · leo';
  app.innerHTML = skeleton(4);
  const [page, keep, sessions] = await Promise.all([api('/api/storage'), api('/api/keep'), api('/api/sessions')]);
  if (mine !== seq) return;
  state.keep = keep;
  state.sessions = sessions.sessions;
  drawStorage(page);
  loadBrowserStorage(mine).catch(() => {});
}

const keepValue = (days) => (days === null || days === undefined ? 'forever' : String(days));
const keepDays = (value) => (value === 'forever' ? null : Number(value));

function keepCard() {
  const keep = state.keep;
  if (!keep) return '';
  const select = (field, choices) =>
    `<span class="select-control"><select data-keep="${field}">${choices.map((c) => `<option value="${keepValue(c.days)}"${keepValue(c.days) === keepValue(keep[field]) ? ' selected' : ''}>${esc(c.label === 'forever' ? 'Forever' : c.label)}</option>`).join('')}</select>${ICON.chevron}</span>`;
  return `<section class="set-card store-keep">
      <header><h3>How long leo keeps things</h3></header>
      <label class="set-row"><span class="set-label">Trash</span>${select('trash_days', keep.trash_choices)}</label>
      <label class="set-row"><span class="set-label">Chats with Felix</span>${select('chat_days', keep.chat_choices)}</label>
      <p class="set-note">Anything older is deleted on its own, along with the documents given to Felix in those chats.</p>
    </section>`;
}

function sessionsCard() {
  const list = state.sessions || [];
  const rows = list
    .map((b) => `<div class="set-row session-row"><span class="grow"><b>${esc(b.device)}</b>${b.current ? ' <span class="chip accent">This browser</span>' : ''}<span class="sub">Signed in ${rel(b.created_at)}${b.place ? ` ${esc(b.place)}` : ''} · last used ${rel(b.last_seen)}</span></span>${b.current ? '' : `<button class="btn sm plain danger-text" data-action="session-end" data-handle="${esc(b.handle)}" data-device="${esc(b.device)}">Sign out</button>`}</div>`)
    .join('');
  const others = list.filter((b) => !b.current).length;
  return `<section class="set-card store-sessions">
      <header><h3>Signed-in browsers</h3></header>
      <p class="hint">Every browser that opened your leo serve link. Signing one out means it needs the link again.</p>
      ${rows || '<p class="set-note">No browsers yet.</p>'}
      <div class="store-actions">${others ? `<button class="btn sm plain danger-text" data-action="session-end-others">Sign out ${others === 1 ? 'the other browser' : `the other ${others} browsers`}</button>` : ''}<button class="btn sm plain" data-action="session-new-link">Make a new link</button></div>
    </section>`;
}

async function refreshSessions() {
  state.sessions = (await api('/api/sessions')).sessions;
  if (state.view === 'storage' && state.storage) drawStorage(state.storage);
}

function sessionAsk(el) {
  state.sessionPending = el.dataset.handle ? { handle: el.dataset.handle } : { others: true };
  const what = el.dataset.handle ? esc(el.dataset.device) : 'every other browser';
  sheet(`<h3>Sign out ${what}?</h3>
    <p>It needs your leo serve link to sign in again.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="session-end-now">Sign out</button></div>`);
}

async function sessionEndNow() {
  const body = state.sessionPending;
  closeSheet();
  if (!body) return;
  state.sessionPending = null;
  const done = await api('/api/sessions/end', { method: 'POST', body });
  toast(done.ended === 1 ? 'Signed out 1 browser' : `Signed out ${done.ended} browsers`);
  await refreshSessions();
}

function newLinkAsk() {
  sheet(`<h3>Make a new link?</h3>
    <p>The old link stops working for browsers that are not signed in yet. Browsers already signed in stay signed in; sign them out above if you want them to need the new link.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="session-new-link-now">Make a new link</button></div>`);
}

async function newLinkNow() {
  closeSheet();
  const made = await api('/api/sessions/new-link', { method: 'POST' });
  sheet(`<h3>Your new link</h3>
    <p>Open it on each device you want to use. It is also printed in the terminal running leo serve.</p>
    <input class="new-link" id="new-link" readonly value="${esc(made.link)}">
    <div class="buttons"><button class="btn plain" data-action="close">Done</button><button class="btn primary" data-action="copy-link">Copy</button></div>`);
  const field = $('#new-link');
  if (field) field.select();
}

async function copyLink() {
  const field = $('#new-link');
  if (!field) return;
  try {
    await navigator.clipboard.writeText(field.value);
    toast('Copied the new link');
  } catch (e) {
    field.select();
    toast('Select the link and copy it');
  }
}

async function changeKeep(el) {
  const before = state.keep;
  const next = { trash_days: before.trash_days, chat_days: before.chat_days, [el.dataset.keep]: keepDays(el.value) };
  const shorter = (a, b) => b !== null && (a === null || b < a);
  const what = el.dataset.keep === 'trash_days' ? 'notes in the trash' : 'chats with Felix';
  if (shorter(before[el.dataset.keep], next[el.dataset.keep])) {
    sheet(`<h3>Keep ${what} for ${esc(el.selectedOptions[0].textContent.toLowerCase())}?</h3>
      <p>Any older than that are deleted now, and from then on as they age.</p>
      <div class="buttons"><button class="btn plain" data-action="keep-cancel">Cancel</button><button class="btn danger" data-action="keep-now">Keep for ${esc(el.selectedOptions[0].textContent.toLowerCase())}</button></div>`);
    state.keepPending = next;
    return;
  }
  await saveKeep(next);
}

async function saveKeep(next) {
  state.keep = await api('/api/keep', { method: 'POST', body: next });
  toast('Saved');
  const page = await api('/api/storage');
  if (state.view === 'storage') drawStorage(page);
}

function storageWhen(iso) {
  return iso ? rel(iso) : '';
}

function drawStorage(page) {
  const exports = [...app.querySelectorAll('[data-export]')].map(el => [el.dataset.export, el.checked]);
  const selected = [...app.querySelectorAll('[data-store-item]:checked')].map(el => [el.dataset.storeItem, el.value]);
  const focused = document.activeElement?.matches('[data-store-item]') ? [document.activeElement.dataset.storeItem, document.activeElement.value] : null;
  const pageX = scrollX, pageY = scrollY;
  state.storage = page;
  const open = state.storageOpen || new Set();
  for (const area of app.querySelectorAll('details.store-area')) {
    if (area.open) open.add(area.dataset.area);
    else open.delete(area.dataset.area);
  }
  state.storageOpen = open;
  const total = Math.max(1, page.total);
  const shown = page.areas.filter((a) => a.bytes > 0);
  const bar = shown.map((a) => `<i data-area="${esc(a.id)}" data-name="${esc(a.title)}" data-size="${esc(a.size)}" style="width:${Math.max(0.6, (a.bytes / total) * 100)}%;background:${STORE_COLORS[page.areas.indexOf(a) % STORE_COLORS.length]}"></i>`).join('');
  const legend = page.areas.map((a, i) => `<span class="store-key" data-area="${esc(a.id)}"><b style="background:${STORE_COLORS[i % STORE_COLORS.length]}"></b>${esc(a.title)} <span class="store-key-size">${esc(a.size)}</span></span>`).join('');
  const areas = storageAreas(page.areas, open);
  app.innerHTML = `<div class="settings storage">
    <div class="section-title">Storage and data</div>
    <section class="set-card store-total">
      <div class="store-big">${esc(page.total_label)}</div>
      <p class="hint">What Leo keeps on the computer running this server, including the application, downloads and saved data. Browser copies are shown separately below. Sizes are estimates for very large or inaccessible folders.</p>
      <div class="store-bar" aria-hidden="true">${bar}</div>
      <div class="store-tip" id="store-tip" role="tooltip" hidden></div>
      <div class="store-legend">${legend}</div>
    </section>
    ${areas}
    <div id="browser-storage">${browserStorageCard()}</div>
    ${keepCard()}
    ${sessionsCard()}
    <section class="set-card store-export">
      <header><h3>Export everything</h3></header>
      <p class="hint">A zip of your notes, standalone tasks and the selected data below. Interview practices include frameworks, code, answers, whiteboards, submitted attempts, reviews and schedules. Settings, private calendar links, automatic answer keys and API keys are never included.</p>
      <div class="store-export-parts">
        <label><input type="checkbox" checked disabled> Notes</label>
        <label><input type="checkbox" data-export="uploads" checked> Uploaded files</label>
        <label><input type="checkbox" data-export="chats" checked> Chats and interview practices</label>
        <label><input type="checkbox" data-export="trash"> Trash</label>
      </div>
      <a class="btn primary sm" id="export-link" download href="${exportHref()}">Download zip</a>
    </section>
    <section class="set-card store-restore">
      <header><h3>Restore tasks</h3></header>
      <p class="hint">Bring back tasks from an export: pick the <b>tasks.json</b> inside the zip. Tasks you already have are kept when they are newer.</p>
      <label class="btn plain sm">Choose tasks.json<input type="file" id="tasks-restore" accept=".json,application/json" hidden></label>
    </section>
  </div>`;
  for (const [area, id] of selected) { const box = app.querySelector(`input[data-store-item="${CSS.escape(area)}"][value="${CSS.escape(id)}"]:not(:disabled)`); if (box) box.checked = true; }
  if (focused) app.querySelector(`input[data-store-item="${CSS.escape(focused[0])}"][value="${CSS.escape(focused[1])}"]`)?.focus({ preventScroll: true });
  window.scrollTo({left:pageX, top:pageY, behavior:'instant'});
  for (const [part, checked] of exports) { const box = app.querySelector(`[data-export="${CSS.escape(part)}"]`); if (box) box.checked = checked; }
  $('#export-link').href = exportHref();
  storageCounts();
  storageBarHover();
}

const STORE_PAGE_ITEMS = 100;

function storageItemOpen(area, item) {
  const practice = ['interviews', 'interview-checks'].includes(area.id) ? item.id : item.practice;
  const note = ['reading', 'note-task-data'].includes(area.id) ? item.id : item.note;
  if (['chats', 'memory', 'chat-docs'].includes(area.id)) return `<button class="btn sm plain" data-action="storage-open-chat" data-id="${esc(item.id)}">Open chat</button>`;
  if (practice) return `<a class="btn sm plain" href="#/interviews/${enc(practice)}">Open practice</a>`;
  if (note) return `<a class="btn sm plain" href="${noteHash(note)}">Open note</a>`;
  return '';
}

async function storageOpenChat(el) {
  if (chat.isBusy()) return toast('Finish or stop Felix’s reply before opening another conversation.', {bad:true});
  if (!(await chat.openSaved(el.dataset.id))) toast('That conversation could not be opened. Refresh Storage and try again.', {bad:true});
}

function storageAreaOpen(id) {
  const links = { notes: ['#/', 'View notes'], trash: ['#/trash', 'View trash'], 'calendar-cache': ['#/calendar', 'View calendar'], tasks: ['#/calendar', 'View tasks'], settings: ['#/settings', 'Manage settings'], backup: ['#/settings', 'Manage backup'] };
  const link = links[id];
  return link ? `<div class="store-actions"><a class="btn sm plain" href="${link[0]}">${link[1]}</a></div>` : '';
}

function storageAreas(areas, open) {
  return areas.map((a, i) => {
    const pickable = a.actions.some((x) => x.selected);
    const visible = a.items.slice(0, state.storageShown?.[a.id] || STORE_PAGE_ITEMS);
    const items = a.items.length
      ? `<div class="store-items">${pickable && a.items.length > 1 ? `<label class="store-item store-all"><input type="checkbox" data-store-all="${esc(a.id)}"><span class="grow">${visible.length < a.items.length ? 'Select shown' : 'Select all'}</span></label>` : ''}${visible
          .map((it) => `<label class="store-item${it.locked ? ' locked' : ''}">${pickable ? `<input type="checkbox" data-store-item="${esc(a.id)}" value="${esc(it.id)}"${it.locked ? ' disabled' : ''}>` : ''}<span class="grow"><span class="store-label">${esc(it.label)}</span><span class="sub">${esc([it.detail, storageWhen(it.when)].filter(Boolean).join(' · '))}</span>${storageItemOpen(a, it)}</span>${it.size ? `<span class="store-size">${esc(it.size)}</span>` : ''}</label>`)
          .join('')}</div>`
      : '';
    const actions = a.actions.length
      ? `<div class="store-actions">${a.actions
          .map((x) => `<button class="btn sm ${x.selected ? 'plain' : 'plain danger-text'}" data-action="storage-act" data-area="${esc(a.id)}" data-act="${esc(x.id)}"${x.selected ? ' disabled data-needs-items="1"' : ''}>${esc(x.label)}${x.selected ? ' <span class="store-count"></span>' : ''}</button>`)
          .join('')}</div>`
      : '';
    return `<details class="set-card store-area" data-area="${esc(a.id)}"${open.has(a.id) ? ' open' : ''}>
      <summary><b class="store-dot" style="background:${STORE_COLORS[i % STORE_COLORS.length]}"></b><span class="grow"><b>${esc(a.title)}</b>${a.items.length ? `<span class="sub">${a.items.length} item${a.items.length === 1 ? '' : 's'}</span>` : ''}</span><span class="store-size">${esc(a.size)}</span>${ICON.chevron}</summary>
      <p class="hint">${esc(a.about)}</p>
      <code class="set-path">${esc(a.path)}</code>
      ${items}${visible.length < a.items.length ? `<button class="btn sm plain" data-action="storage-more" data-area="${esc(a.id)}">Show more · ${visible.length} of ${a.items.length}</button>` : ''}${actions}${storageAreaOpen(a.id)}
    </details>`;
  }).join('');
}

const browserAudioStorage = window.leoAudioQueue?.pick();

function browserStorageCard() {
  const data = state.browserStorage;
  if (!data) return '<section class="set-card"><header><h3>This browser</h3></header><p class="hint" role="status">Checking draft backups and recovery audio…</p></section>';
  return `<section class="set-card"><header><h3>This browser · ${esc(window.leoBrowserStorage.size(data.total))} of copies</h3></header><p class="hint">Separate from the server total. Estimated draft and cache sizes; audio sizes count recorded bytes. Browser preferences reset the next time you open Leo. Your browser manages its downloaded website cache separately.</p>${data.error ? `<p class="hint">${esc(data.error)}</p>` : ''}${data.areas.length ? storageAreas(data.areas, state.storageOpen || new Set()) : '<p class="hint">No saved browser copies.</p>'}</section>`;
}

async function loadBrowserStorage(mine = seq) {
  const L = window.leoBrowserStorage;
  if (!L) return;
  const areas = [], errors = [];
  try {
    const items = L.list(window.localStorage);
    if (items.length) areas.push({ id:'browser-local',title:'Draft backups and browser preferences',about:'Unsaved note and interview drafts, a cached Felix conversation, and preferences in this browser. Open a note or practice to recover work before discarding its backup.',path:'This browser only',items,bytes:items.reduce((n,i)=>n+i.bytes,0),actions:[{id:'delete',label:'Discard selected',selected:true,confirm:'The selected browser copies are removed. Unsaved drafts may no longer be recoverable. Saved notes, practices and chats on the Leo server stay.'}] });
  } catch (_) { errors.push('This browser does not allow Leo to inspect its draft backups and preferences.'); }
  try {
    const audio = await browserAudioStorage?.summaries() || [];
    const items = audio.map(i=>({...i,label:'Recording recovery copy',detail:`${i.chunks} chunks · ${i.id}`,size:L.size(i.bytes),locked:recorder.active()}));
    if (items.length) areas.push({id:'browser-recovery',title:'Recovery audio in this browser',about:'Audio waiting for the server to confirm it was saved. These copies belong to this browser; audio on the server is listed separately. Stop a recording before discarding recovery copies.',path:'This browser · recording recovery',items,bytes:items.reduce((n,i)=>n+i.bytes,0),actions:[{id:'delete',label:'Discard selected audio',selected:true,confirm:'Selected recovery audio in this browser is permanently discarded. Already saved server audio and notes stay.'}]});
  } catch (_) { errors.push('Recovery audio could not be inspected. Check this browser’s site storage permissions and refresh Storage.'); }
  if (mine !== seq || state.view !== 'storage') return;
  for (const area of areas) area.size=L.size(area.bytes);
  state.browserStorage={areas,total:areas.reduce((n,a)=>n+a.bytes,0),error:errors.join(' ')};
  drawBrowserStorage();
}

function drawBrowserStorage() {
  const box = $('#browser-storage');
  if (!box || state.view !== 'storage') return;
  const selected = [...box.querySelectorAll('[data-store-item]:checked')].map(el => [el.dataset.storeItem, el.value]);
  const active = document.activeElement;
  const focused = box.contains(active) && active.matches('[data-store-item]') ? [active.dataset.storeItem, active.value] : null;
  const pageX = scrollX, pageY = scrollY;
  for (const area of box.querySelectorAll('details.store-area')) {
    if (area.open) state.storageOpen.add(area.dataset.area);
    else state.storageOpen.delete(area.dataset.area);
  }
  box.innerHTML = browserStorageCard();
  for (const [area, id] of selected) { const selectedBox = box.querySelector(`input[data-store-item="${CSS.escape(area)}"][value="${CSS.escape(id)}"]:not(:disabled)`); if (selectedBox) selectedBox.checked = true; }
  if (focused) box.querySelector(`input[data-store-item="${CSS.escape(focused[0])}"][value="${CSS.escape(focused[1])}"]`)?.focus({preventScroll:true});
  window.scrollTo({left:pageX, top:pageY, behavior:'instant'});
  storageCounts();
}

function storageMore(el) {
  state.storageShown ||= {};
  state.storageShown[el.dataset.area] = (state.storageShown[el.dataset.area] || STORE_PAGE_ITEMS) + STORE_PAGE_ITEMS;
  drawStorage(state.storage);
  app.querySelector(`[data-action="storage-more"][data-area="${CSS.escape(el.dataset.area)}"]`)?.focus({ preventScroll: true });
}

const TIP_FOR = 2500;

function storageBarHover() {
  const bar = $('.store-bar');
  const tip = $('#store-tip');
  if (!bar || !tip) return;
  const card = bar.closest('.store-total');
  const show = (seg, x, y) => {
    for (const el of card.querySelectorAll('.store-bar i.on, .store-key.on')) el.classList.remove('on');
    if (!seg) {
      bar.classList.remove('hovering');
      tip.hidden = true;
      return;
    }
    bar.classList.add('hovering');
    seg.classList.add('on');
    const key = card.querySelector(`.store-key[data-area="${CSS.escape(seg.dataset.area)}"]`);
    if (key) key.classList.add('on');
    tip.innerHTML = `<b>${esc(seg.dataset.name)}</b><span>${esc(seg.dataset.size)}</span>`;
    tip.hidden = false;
    const box = card.getBoundingClientRect();
    const width = tip.offsetWidth;
    const left = Math.min(Math.max(x - box.left + 14, 8), box.width - width - 8);
    tip.style.left = `${left}px`;
    tip.style.top = `${y - box.top + 16}px`;
  };
  bar.addEventListener('pointermove', (e) => show(e.target.closest('.store-bar i'), e.clientX, e.clientY));
  let fade = 0;
  bar.addEventListener('pointerdown', (e) => {
    show(e.target.closest('.store-bar i'), e.clientX, e.clientY);
    clearTimeout(fade);
    if (e.pointerType !== 'mouse') fade = setTimeout(() => show(null), TIP_FOR);
  });
  bar.addEventListener('pointerleave', (e) => {
    if (e.pointerType === 'mouse') show(null);
  });
  bar.addEventListener('click', (e) => {
    const seg = e.target.closest('.store-bar i');
    const area = seg && document.querySelector(`.store-area[data-area="${CSS.escape(seg.dataset.area)}"]`);
    if (!area || e.pointerType === 'touch') return;
    area.open = true;
    area.scrollIntoView({ block: 'center', behavior: 'smooth' });
  });
}

function exportHref() {
  const on = (part, fallback) => {
    const box = app.querySelector(`[data-export="${part}"]`);
    return box ? box.checked : fallback;
  };
  return `/api/export?uploads=${on('uploads', true)}&chats=${on('chats', true)}&trash=${on('trash', false)}`;
}

function storagePicked(area) {
  return [...app.querySelectorAll(`input[data-store-item="${CSS.escape(area)}"]:checked`)].map((i) => i.value);
}

function storageCounts() {
  for (const button of app.querySelectorAll('[data-needs-items]')) {
    const n = storagePicked(button.dataset.area).length;
    button.disabled = n === 0;
    const count = button.querySelector('.store-count');
    if (count) count.textContent = n ? `(${n})` : '';
  }
}

function storageAsk(el) {
  const area = [...(state.storage?.areas || []), ...(state.browserStorage?.areas || [])].find((a) => a.id === el.dataset.area);
  const action = area && area.actions.find((x) => x.id === el.dataset.act);
  if (!action) return;
  const items = action.selected ? storagePicked(area.id) : [];
  if (action.selected && !items.length) return;
  state.storagePending = { area: area.id, action: action.id, items };
  const what = action.selected ? `${items.length} item${items.length === 1 ? '' : 's'} from ${area.title}` : area.title;
  sheet(`<h3>${esc(action.label)}?</h3>
    <p><b>${esc(what)}</b></p>
    <p>${esc(action.confirm || 'This cannot be undone.')}</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="storage-go">${esc(action.selected ? 'Delete' : action.label)}</button></div>`);
}

async function tasksRestoreAsk(file) {
  let tasks;
  try {
    if (file.size > 8 * 1024 * 1024) throw new Error('too big');
    const data = JSON.parse(await file.text());
    tasks = Array.isArray(data) ? data : data && data.tasks;
    if (!Array.isArray(tasks)) throw new Error('no tasks');
  } catch (e) {
    return toast('Choose tasks.json from a backup downloaded through Storage → Export.', { bad: true });
  }
  if (!tasks.length) return toast('That file has no tasks in it.');
  state.tasksRestore = tasks;
  sheet(`<h3>Restore ${esc(plural(tasks.length, 'task'))}?</h3>
    <p><b>${esc(file.name)}</b></p>
    <p>Tasks that are not here come back. A task you already have stays as it is unless the file's copy is newer.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="tasks-restore-go">Restore</button></div>`);
}

async function tasksRestoreGo() {
  const tasks = state.tasksRestore;
  closeSheet();
  state.tasksRestore = null;
  if (!tasks) return;
  const done = await api('/api/tasks/import', { method: 'POST', body: { tasks } });
  toast(done.added ? `Restored ${plural(done.added, 'task')}${done.kept ? ` · ${done.kept} already here` : ''}` : 'Every task in that file is already here');
  loadTasksWindow().catch(() => {});
  if (state.view === 'storage') drawStorage(await api('/api/storage'));
}

async function storageGo() {
  const request = state.storagePending;
  closeSheet();
  if (!request) return;
  state.storagePending = null;
  const mine = seq;
  if (request.area === 'browser-local') {
    if (request.items.includes('leo-chat-v1') && chat.isBusy()) return toast('Finish or stop Felix’s reply before discarding its browser copy.', {bad:true});
    window.leoBrowserStorage.remove(window.localStorage, request.items);
    if (request.items.includes('leo-chat-v1')) chat.forgetBrowserCache();
    await loadBrowserStorage(mine);
    return toast('Discarded selected browser copies. Saved server data stays.');
  }
  if (request.area === 'browser-recovery') {
    if (recorder.active()) return toast('Stop the recording before discarding recovery audio.', {bad:true});
    for (const id of request.items) await browserAudioStorage.clear(id);
    await loadBrowserStorage(mine);
    return toast('Discarded selected recovery audio from this browser.');
  }
  if (request.area === 'interviews' && chat.isBusy()) return toast('Finish or stop Felix’s reply before deleting interview practices.', {bad:true});
  const practiceIds = request.area === 'interviews' ? request.action === 'all' ? state.storage.areas.find(a=>a.id==='interviews').items.map(i=>i.id) : request.items : [];
  const done = await api('/api/storage', { method: 'POST', body: request });
  let browserWarning = '';
  if (practiceIds.length) {
    try { window.leoBrowserStorage.forgetPractices(window.localStorage, practiceIds); }
    catch (_) { browserWarning = ' A draft backup could not be removed from this browser. Discard it under This browser.'; }
  }
  if (mine !== seq || state.view !== 'storage') return;
  drawStorage(done.storage);
  loadBrowserStorage(mine).catch(() => {});
  loadTasksWindow().catch(() => {});
  toast(done.message + browserWarning);
}

app.addEventListener('toggle', (e) => {
  const area = e.target.closest && e.target.closest('details.store-area');
  if (!area || !app.contains(area) || state.view !== 'storage') return;
  if (area.open) state.storageOpen.add(area.dataset.area);
  else state.storageOpen.delete(area.dataset.area);
}, true);

app.addEventListener('change', (e) => {
  if (state.view !== 'storage') return;
  const all = e.target.closest('[data-store-all]');
  if (all) {
    for (const box of app.querySelectorAll(`input[data-store-item="${CSS.escape(all.dataset.storeAll)}"]:not(:disabled)`)) box.checked = all.checked;
  }
  if (all || e.target.closest('[data-store-item]')) storageCounts();
  if (e.target.closest('[data-export]')) $('#export-link').href = exportHref();
  const keep = e.target.closest('select[data-keep]');
  if (keep) changeKeep(keep).catch(fail);
  const restore = e.target.closest('#tasks-restore');
  if (restore && restore.files && restore.files[0]) {
    tasksRestoreAsk(restore.files[0]).catch(fail);
    restore.value = '';
  }
});
