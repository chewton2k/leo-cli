async function showTrash() {
  const mine = ++seq;
  const before = state.view === 'trash' ? state : null;
  state = { view: 'trash', dir: '', selecting: before ? before.selecting : false, picked: new Set() };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Trash</button>';
  document.title = 'Trash · leo';
  const [items, keep] = await Promise.all([api('/api/trash'), api('/api/keep').catch(() => null)]);
  if (mine !== seq) return;
  state.items = items;
  state.trashDays = keep ? keep.trash_days : 30;
  if (!items.length) state.selecting = false;
  drawTrash();
}

function drawTrash() {
  const items = state.items || [];
  if (!items.length) {
    app.innerHTML = empty(ICON.trash, 'The trash is empty', 'Deleted notes wait here for 30 days, so a mistake can be undone.');
    floating.innerHTML = '';
    return;
  }
  const sel = state.selecting;
  const picked = state.picked;
  const rows = items
    .map((t) => {
      const box = sel ? `<input type="checkbox" class="trash-box" data-trash-pick value="${esc(t.id)}"${picked.has(t.id) ? ' checked' : ''} aria-label="Select ${esc(t.title)}">` : ICON.note;
      const actions = sel ? '' : `<button class="btn sm plain" data-action="restore" data-id="${esc(t.id)}">Restore</button><button class="icon-btn trash-forget" data-action="trash-forget" data-id="${esc(t.id)}" aria-label="Delete ${esc(t.title)} for good" title="Delete for good">${ICON.trash}</button>`;
      const row = sel ? 'label' : 'div';
      return `<${row} class="list-row trash-row${picked.has(t.id) ? ' picked' : ''}">${box}<span class="grow"><div>${esc(t.title)}</div><div class="sub">${esc(t.directory ? '/' + t.directory : 'All notes')} · deleted ${rel(t.deleted_at)}</div></span>${actions}</${row}>`;
    })
    .join('');
  const all = picked.size === items.length;
  app.innerHTML = `<div class="trash-head"><div class="section-title">Trash · ${items.length} note${items.length === 1 ? '' : 's'}</div>
      <div class="trash-tools">${sel ? '<button class="btn sm plain" data-action="trash-select">Done</button>' : `<button class="btn sm plain" data-action="trash-select">Select</button><button class="btn sm plain danger-text" data-action="trash-empty">Empty trash</button>`}</div></div>
    <div class="panel">${rows}</div>
    <p class="hint" style="margin:14px 4px">${state.trashDays === null ? 'Deleted notes stay here until you empty the trash.' : `Deleted notes stay here for ${state.trashDays === 365 ? 'a year' : plural(state.trashDays, 'day')}, then leave on their own.`} <a href="#/settings/storage">Change</a></p>`;
  floating.innerHTML = sel
    ? `<div class="select-bar" role="toolbar" aria-label="Selected notes">
        <label class="select-all"><input type="checkbox" data-trash-all${all ? ' checked' : ''}><span>All</span></label>
        <span class="select-count">${picked.size} selected</span>
        <button class="btn sm plain" data-action="trash-restore-picked"${picked.size ? '' : ' disabled'}>Restore</button>
        <button class="btn sm danger" data-action="trash-delete-picked"${picked.size ? '' : ' disabled'}>Delete</button>
      </div>`
    : '';
}

function trashAsk(ids, all) {
  const items = state.items || [];
  const names = all ? items : items.filter((t) => ids.includes(t.id));
  const what = all ? `all ${plural(items.length, 'note')} in the trash` : names.length === 1 ? `“${names[0].title}”` : plural(names.length, 'note');
  state.trashPending = { ids, all };
  sheet(`<h3>Delete ${esc(what)} for good?</h3>
    <p>${all || names.length > 1 ? 'They' : 'It'} cannot be restored afterwards.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="trash-delete-now">Delete for good</button></div>`);
}

async function trashDeleteNow() {
  const pending = state.trashPending;
  closeSheet();
  if (!pending) return;
  state.trashPending = null;
  const done = await api('/api/trash/delete', { method: 'POST', body: pending });
  toast(`Deleted ${plural(done.deleted, 'note')} for good`);
  state.picked = new Set();
  await showTrash();
}

async function trashRestorePicked() {
  const ids = [...state.picked];
  if (!ids.length) return;
  const done = await api('/api/trash/restore', { method: 'POST', body: { ids } });
  toast(`Restored ${plural(done.restored, 'note')}`);
  state.picked = new Set();
  state.selecting = false;
  await showTrash();
}

app.addEventListener('change', (e) => {
  if (state.view !== 'trash') return;
  const box = e.target.closest('[data-trash-pick]');
  if (!box) return;
  if (box.checked) state.picked.add(box.value);
  else state.picked.delete(box.value);
  drawTrash();
});

floating.addEventListener('change', (e) => {
  if (state.view !== 'trash' || !e.target.closest('[data-trash-all]')) return;
  state.picked = e.target.checked ? new Set((state.items || []).map((t) => t.id)) : new Set();
  drawTrash();
});
