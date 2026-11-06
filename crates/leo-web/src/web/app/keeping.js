const KEEPING_ROWS = [
  ['trash_days', 'Trash', 'notes in the trash', 'keeping-trash'],
  ['chat_days', 'Chats with Felix', 'chats with Felix', 'keeping-chat'],
  ['upload_days', 'Uploaded files', 'uploaded files', 'keeping-upload'],
  ['transcript_days', 'Recording transcripts', 'recording transcripts', 'keeping-transcript'],
  ['audio_days', 'Unfinished recording audio', 'unfinished recording audio', 'keeping-audio'],
];

async function showKeeping() {
  const mine = ++seq;
  state = { view: 'keeping', dir: '' };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = settingsCrumbs('keeping');
  document.title = 'Keeping · leo';
  refreshPanel(app, settingsFrame('keeping', skeleton(4)));
  const keep = await api('/api/keep');
  if (mine !== seq) return;
  state.keep = keep;
  drawKeeping();
}

const keepValue = (days) => (days === null || days === undefined ? 'forever' : String(days));
const keepDays = (value) => (value === 'forever' ? null : Number(value));

function keepCard() {
  const keep = state.keep;
  if (!keep) return '';
  const select = (field) =>
    `<span class="select-control"><select data-keep="${field}">${keep.choices.map((c) => `<option value="${keepValue(c.days)}"${keepValue(c.days) === keepValue(keep[field]) ? ' selected' : ''}>${esc(c.label === 'forever' ? 'Forever' : c.label === 'a year' ? '1 year' : c.label)}</option>`).join('')}</select>${ICON.chevron}</span>`;
  return `<section class="set-card">
      ${KEEPING_ROWS.map(([field, label, , find]) => `<label class="set-row" data-find="${find}"><span class="set-label">${label}</span>${select(field)}</label>`).join('')}
    </section>`;
}

async function changeKeep(el) {
  const before = state.keep;
  const field = el.dataset.keep;
  const next = Object.fromEntries(KEEPING_ROWS.map(([name]) => [name, before[name]]));
  next[field] = keepDays(el.value);
  const shorter = (a, b) => b !== null && (a === null || b < a);
  if (shorter(before[field], next[field])) {
    const what = KEEPING_ROWS.find(([name]) => name === field)[2];
    const period = el.selectedOptions[0].textContent.toLowerCase();
    sheet(`<h3>Keep ${what} for ${esc(period)}?</h3>
      <p>Any older than that are deleted now, and from then on as they age.</p>
      <div class="buttons"><button class="btn plain" data-action="keep-cancel">Cancel</button><button class="btn danger" data-action="keep-now">Keep for ${esc(period)}</button></div>`);
    state.keepPending = next;
    return;
  }
  await saveKeep(next);
}

async function saveKeep(next) {
  state.keep = await api('/api/keep', { method: 'POST', body: next });
  toast('Saved');
  if (state.view === 'keeping') drawKeeping();
}

function drawKeeping() {
  const exports = [...app.querySelectorAll('[data-export]')].map(el => [el.dataset.export, el.checked]);
  refreshPanel(app, settingsFrame('keeping', `<div class="settings">
    ${keepCard()}
    <section class="set-card store-export" data-find="keeping-export">
      <div class="set-row"><span class="grow">Export notes</span><a class="btn plain sm" id="export-link" download href="${exportHref()}">Download zip</a></div>
      <div class="store-export-parts">
        <label class="set-row"><span class="grow">Uploaded files</span>${settingsSwitch('data-export="uploads"', true)}</label>
        <label class="set-row"><span class="grow">Chats</span>${settingsSwitch('data-export="chats"', true)}</label>
        <label class="set-row"><span class="grow">Trash</span>${settingsSwitch('data-export="trash"', false)}</label>
      </div>
    </section>
  </div>`));
  for (const [part, checked] of exports) { const box = app.querySelector(`[data-export="${CSS.escape(part)}"]`); if (box) box.checked = checked; }
  $('#export-link').href = exportHref();
}

function exportHref() {
  const on = (part, fallback) => {
    const box = app.querySelector(`[data-export="${part}"]`);
    return box ? box.checked : fallback;
  };
  return `/api/export?uploads=${on('uploads', true)}&chats=${on('chats', true)}&trash=${on('trash', false)}`;
}

app.addEventListener('change', (e) => {
  if (state.view !== 'keeping') return;
  if (e.target.closest('[data-export]')) $('#export-link').href = exportHref();
  const keep = e.target.closest('select[data-keep]');
  if (keep) changeKeep(keep).catch(fail);
});
