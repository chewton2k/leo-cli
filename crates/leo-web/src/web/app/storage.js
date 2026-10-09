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
}

const keepValue = (days) => (days === null || days === undefined ? 'forever' : String(days));
const keepDays = (value) => (value === 'forever' ? null : Number(value));

function keepCard() {
  const keep = state.keep;
  if (!keep) return '';
  const select = (field, choices) =>
    `<select data-keep="${field}">${choices.map((c) => `<option value="${keepValue(c.days)}"${keepValue(c.days) === keepValue(keep[field]) ? ' selected' : ''}>${esc(c.label === 'forever' ? 'Forever' : c.label)}</option>`).join('')}</select>`;
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
  state.storage = page;
  const open = state.storageOpen || new Set();
  const total = Math.max(1, page.total);
  const shown = page.areas.filter((a) => a.bytes > 0);
  const bar = shown.map((a, i) => `<i style="width:${Math.max(0.6, (a.bytes / total) * 100)}%;background:${STORE_COLORS[page.areas.indexOf(a) % STORE_COLORS.length]}" title="${esc(a.title)}: ${esc(a.size)}"></i>`).join('');
  const legend = page.areas.map((a, i) => `<span class="store-key"><b style="background:${STORE_COLORS[i % STORE_COLORS.length]}"></b>${esc(a.title)} <span class="store-key-size">${esc(a.size)}</span></span>`).join('');
  const areas = page.areas.map((a, i) => {
    const pickable = a.actions.some((x) => x.selected);
    const items = a.items.length
      ? `<div class="store-items">${pickable && a.items.length > 1 ? `<label class="store-item store-all"><input type="checkbox" data-store-all="${esc(a.id)}"><span class="grow">Select all</span></label>` : ''}${a.items
          .map((it) => `<label class="store-item${it.locked ? ' locked' : ''}">${pickable ? `<input type="checkbox" data-store-item="${esc(a.id)}" value="${esc(it.id)}"${it.locked ? ' disabled' : ''}>` : ''}<span class="grow"><span class="store-label">${esc(it.label)}</span><span class="sub">${esc([it.detail, storageWhen(it.when)].filter(Boolean).join(' · '))}</span></span>${it.size ? `<span class="store-size">${esc(it.size)}</span>` : ''}</label>`)
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
      ${items}${actions}
    </details>`;
  }).join('');
  app.innerHTML = `<div class="settings storage">
    <div class="section-title">Storage and data</div>
    <section class="set-card store-total">
      <div class="store-big">${esc(page.total_label)}</div>
      <p class="hint">What leo keeps on this computer. Notes are only deleted from their own page, and backups are left alone.</p>
      <div class="store-bar" aria-hidden="true">${bar}</div>
      <div class="store-legend">${legend}</div>
    </section>
    ${areas}
    ${keepCard()}
    ${sessionsCard()}
    <section class="set-card store-export">
      <header><h3>Export everything</h3></header>
      <p class="hint">A zip of your notes as Markdown, in their folders, to keep or open in another app. Settings and API keys are never included.</p>
      <div class="store-export-parts">
        <label><input type="checkbox" checked disabled> Notes</label>
        <label><input type="checkbox" data-export="uploads" checked> Uploaded files</label>
        <label><input type="checkbox" data-export="chats" checked> Chats with Felix</label>
        <label><input type="checkbox" data-export="trash"> Trash</label>
      </div>
      <a class="btn primary sm" id="export-link" download href="${exportHref()}">Download zip</a>
    </section>
  </div>`;
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
  const area = state.storage && state.storage.areas.find((a) => a.id === el.dataset.area);
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

async function storageGo() {
  const request = state.storagePending;
  closeSheet();
  if (!request) return;
  state.storagePending = null;
  const done = await api('/api/storage', { method: 'POST', body: request });
  if (state.view !== 'storage') return;
  drawStorage(done.storage);
  toast(done.message);
}

app.addEventListener('toggle', (e) => {
  const area = e.target.closest && e.target.closest('details.store-area');
  if (!area || state.view !== 'storage') return;
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
});
