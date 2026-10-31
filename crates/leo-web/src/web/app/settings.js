const TASK_TITLE = { writing: 'AI for writing', speech: 'AI for speech' };
const TASK_USE = {
  writing: 'Turns recordings into notes, answers @leo questions, powers Felix and the knowledge graph.',
  speech: 'Turns what was said into text while you record.',
};

async function showSettings() {
  const mine = ++seq;
  state = { view: 'settings', dir: '' };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Settings</button>';
  document.title = 'Settings · leo';
  app.innerHTML = skeleton(3);
  const [page, calendar, speech] = await Promise.all([api('/api/settings'), api('/api/calendar').catch(() => null), api('/api/speech').catch(() => null)]);
  if (mine !== seq) return;
  state.calendar = calendar;
  state.speech = speech;
  drawSettings(page);
  startSpeechPolling(mine);
}

const GOOGLE_CALENDAR_SETTINGS = 'https://calendar.google.com/calendar/r/settings';

function calendarCard(cal) {
  if (!cal) return '';
  const linked = cal.calendars || [];
  const count = linked.length;
  const chip = count ? `<span class="set-status ok">${count === 1 ? 'Connected' : `${count} calendars`}</span>` : '<span class="set-status missing">Not connected</span>';
  const rows = linked
    .map((c) => `<div class="set-row cal-row">
      <span class="cal-icon">${ICON.calendar}</span>
      <span class="grow"><span class="cal-name">${esc(c.name)}</span>${c.problem ? `<span class="set-note warn">${esc(c.problem)}</span>` : `<span class="cal-sub">Added ${esc(rel(c.added_at))}</span>`}</span>
      <button class="btn plain sm" data-action="calendar-remove" data-id="${esc(c.id)}" data-name="${esc(c.name)}">Remove</button>
    </div>`)
    .join('');
  const next = (cal.events || [])[0];
  const when = (e) => new Date(e.start).toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' });
  const upcoming = count
    ? `<div class="set-row"><span class="set-label">Next up</span><span class="grow cal-next">${next ? `${esc(next.title)} <span class="cal-sub">· ${esc(when(next))}</span>` : '<span class="cal-sub">Nothing in the next week</span>'}</span><button class="btn plain sm" data-action="calendar-refresh"${cal.refreshing ? ' disabled' : ''}>${cal.refreshing ? 'Checking…' : 'Check now'}</button></div>`
    : '';
  const open = !count || state.calendarAdding;
  const form = `<ol class="cal-steps">
        <li>Open <a href="${GOOGLE_CALENDAR_SETTINGS}" target="_blank" rel="noopener noreferrer">Google Calendar settings</a>.</li>
        <li>Under <b>Settings for my calendars</b>, pick the calendar.</li>
        <li>In <b>Integrate calendar</b>, copy <b>Secret address in iCal format</b>.</li>
      </ol>
      <div class="cal-add"><input id="calendar-link" class="set-input" type="text" inputmode="url" autocomplete="off" spellcheck="false" placeholder="Paste the address here"><button class="btn primary sm" data-action="calendar-add">Connect</button></div>
      <p class="set-note">Outlook or Apple Calendar work too: paste the calendar’s published ICS link.</p>`;
  const adder = open
    ? `<div class="cal-form">${count ? '<div class="cal-form-head"><span>Add another calendar</span><button class="btn plain sm" data-action="calendar-cancel">Cancel</button></div>' : ''}${form}</div>`
    : `<button class="set-advanced cal-more" data-action="calendar-more">${ICON.plus}<span class="grow">Add another calendar</span>${ICON.chevron}</button>`;
  return `<section class="set-card cal-card">
    <header><h3>Calendar</h3>${chip}</header>
    <p class="hint">Recordings are named after the class or meeting you are in, and the AI gets its description and who was invited. leo only reads your calendar.</p>
    ${rows}
    ${upcoming}
    ${adder}
  </section>`;
}

async function addCalendar() {
  const box = $('#calendar-link');
  if (!box || !box.value.trim()) throw Object.assign(new Error('Paste your calendar’s secret address first.'), { shown: true });
  const button = $('[data-action="calendar-add"]');
  if (button) {
    button.disabled = true;
    button.textContent = 'Checking…';
  }
  const mine = seq;
  try {
    const calendar = await api('/api/calendar', { method: 'POST', body: { link: box.value.trim() } });
    if (mine !== seq || state.view !== 'settings') return;
    state.calendar = calendar;
    state.calendarAdding = false;
    drawSettings(state.settings);
    const added = calendar.calendars[calendar.calendars.length - 1];
    toast(`Connected “${added ? added.name : 'your calendar'}”.`);
  } catch (e) {
    if (button && button.isConnected) {
      button.disabled = false;
      button.textContent = 'Connect';
    }
    throw e;
  }
}

async function removeCalendar(el) {
  const mine = seq;
  const calendar = await api(`/api/calendar/${enc(el.dataset.id)}`, { method: 'DELETE' });
  if (mine !== seq || state.view !== 'settings') return;
  state.calendar = calendar;
  drawSettings(state.settings);
  toast(`Removed “${el.dataset.name}”.`);
}

async function refreshCalendar() {
  const mine = seq;
  const calendar = await api('/api/calendar/refresh', { method: 'POST' });
  if (mine !== seq || state.view !== 'settings') return;
  state.calendar = calendar;
  drawSettings(state.settings);
}

function settingsOption(value, label, current) {
  return `<option value="${esc(value)}"${value === current ? ' selected' : ''}>${esc(label)}</option>`;
}

const effortName = (e) => (e === 'xhigh' ? 'Extra high' : e[0].toUpperCase() + e.slice(1));

function modelPicker(t) {
  const known = t.models.some((m) => m.id === t.model);
  const list = (known || !t.model ? [] : [{ id: t.model, price: 'not in the list' }]).concat(t.models);
  const efforts = Array.isArray(t.efforts) ? t.efforts : [];
  const effort = efforts.includes(t.effort) ? t.effort : 'medium';
  const shown = `${esc(t.model || 'Default model')}${efforts.length ? `<span class="picker-effort"> · ${esc(`${effortName(effort).toLowerCase()} effort`)}</span>` : ''}`;
  const rows = list
    .map((m) => {
      const on = m.id === t.model;
      return `<button type="button" class="pick-model${on ? ' on' : ''}" role="menuitemradio" aria-checked="${on}" data-action="set-model" data-task="${t.task}" data-model="${esc(m.id)}"><span class="pick-text"><span class="pick-name">${esc(m.id)}</span><span class="pick-price">${esc(m.price)}</span></span>${on ? ICON.check : ''}</button>`;
    })
    .join('');
  const chips = efforts.length
    ? `<div class="pick-effort"><div class="pick-head">Effort</div><div class="pick-chips">${efforts.map((e) => `<button type="button" class="pick-chip${effort === e ? ' on' : ''}" aria-pressed="${effort === e}" data-action="set-effort" data-effort="${e}">${effortName(e)}</button>`).join('')}</div><p class="pick-hint">More effort means the model thinks longer: better answers, but slower and more of your plan or credit.</p></div>`
    : '';
  return `<div class="set-row"><span class="set-label">Model</span><div class="picker"><button type="button" class="picker-button" data-action="toggle-picker" aria-haspopup="menu" aria-expanded="false" data-task="${t.task}"><span class="picker-text">${shown}</span>${ICON.chevron}</button><div class="picker-menu" role="menu" hidden><div class="pick-head">Model</div><div class="pick-models">${rows}</div>${chips}</div></div></div>`;
}

function closePickers(except) {
  for (const menu of app.querySelectorAll('.picker-menu')) {
    if (menu === except) continue;
    fold(menu);
    const button = menu.parentElement.querySelector('.picker-button');
    if (button) button.setAttribute('aria-expanded', 'false');
  }
}

function togglePicker(el) {
  const menu = el.parentElement.querySelector('.picker-menu');
  closePickers(menu);
  const opening = menu.hidden || Boolean(menu.dataset.folding);
  if (opening) unfold(menu);
  else fold(menu);
  el.setAttribute('aria-expanded', String(opening));
}

document.addEventListener('click', (e) => {
  if (state.view === 'settings' && !e.target.closest('.picker')) closePickers();
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && state.view === 'settings' && app.querySelector('.picker-menu:not([hidden])')) {
    e.stopPropagation();
    closePickers();
  }
}, true);

function taskCard(t) {
  const status = t.ready ? '<span class="set-status ok">Ready</span>' : '<span class="set-status missing">Not set up</span>';
  const providers = t.choices.map((c) => settingsOption(c.id, c.label, t.provider)).join('') + (t.custom ? settingsOption(t.provider, `${t.provider} (from config.toml)`, t.provider) : '');
  let model = '';
  if (t.fixed_model) {
    model = `<div class="set-row"><span class="set-label">Model</span><span class="set-value">${esc(t.model || 'Built in')} <span class="hint">free, runs on your computer</span></span></div>`;
  } else if (t.models.length) {
    model = modelPicker(t);
  }
  let key = '';
  if (t.key) {
    const has = t.key.stored;
    const form = `<div class="set-key-form"><input type="password" autocomplete="off" spellcheck="false"${has ? ` class="has-key" placeholder="${esc(t.key.hint || '••••••••')}" aria-label="${esc(t.key.name)} key is stored; type a new one to replace it"` : ` placeholder="Paste your ${esc(t.key.name)} API key"`} data-key-input="${esc(t.key.account)}"><button class="btn primary sm" data-action="set-key" data-account="${esc(t.key.account)}">${has ? 'Save' : 'Save key'}</button></div>`;
    key = `<div class="set-row column"><div class="set-line"><span class="set-label">${esc(t.key.name)} key</span><span class="set-value">${has ? '' : '<span class="set-status missing">Not added</span>'}${has ? `<button class="btn plain sm" data-action="remove-key" data-account="${esc(t.key.account)}" data-name="${esc(t.key.name)}">Remove</button>` : ''}</span></div>${form}${t.key.ignored ? `<p class="hint">leo does not read $${esc(t.key.ignored)}; add the key here instead.</p>` : ''}</div>`;
  }
  const signin = t.signin ? `<p class="set-note${t.signin.installed ? '' : ' warn'}">${esc(t.signin.text)}</p>` : '';
  const usage = t.usage ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
  const note = t.note ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
  return `<section class="set-card" data-task-card="${t.task}">
    <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
    <p class="hint">${TASK_USE[t.task]}</p>
    <label class="set-row"><span class="set-label">Provider</span><span class="select-control"><select data-set="provider" data-task="${t.task}">${providers}</select>${ICON.chevron}</span></label>
    ${model}${key}${signin}${usage}${note}
    <div class="set-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test</button><span class="set-result" id="test-${t.task}"></span></div>
  </section>`;
}

const THEME_LABELS = { system: 'Match this device', light: 'Light', dark: 'Dark' };
const THEME_ICONS = { system: 'device', light: 'sun', dark: 'moon' };

function themeCard() {
  const now = window.leoTheme.choice();
  const felixNow = window.leoTheme.felix();
  return `<section class="set-card">
      <header><h3>Appearance</h3></header>
      <div class="set-row column"><span class="set-label">Theme</span><div class="choice-cards" role="group" aria-label="Theme">${window.leoTheme.CHOICES.map((choice) => `<button class="choice-card${now === choice ? ' on' : ''}" data-action="theme-pick" data-theme="${choice}" aria-pressed="${now === choice}">${ICON[THEME_ICONS[choice]]}<span>${THEME_LABELS[choice]}</span></button>`).join('')}</div></div>
      <div class="set-row column"><span class="set-label">Felix</span><div class="choice-cards" role="group" aria-label="Felix">${[['cute', 'Cute'], ['demon', 'Demon']].map(([kind, label]) => `<button class="choice-card felix-choice${felixNow === kind ? ' on' : ''}" data-action="felix-pick" data-felix="${kind}" aria-pressed="${felixNow === kind}">${felix.felix(64, `felix-preview ${kind}-look`)}<span>${label}</span></button>`).join('')}</div></div>
      <div class="set-row column"><span class="set-label">Note width</span><div class="choice-cards" role="group" aria-label="Note width">${window.leoTheme.WIDTHS.map((width) => `<button class="choice-card${window.leoTheme.noteWidth() === width ? ' on' : ''}" data-action="width-pick" data-width="${width}" aria-pressed="${window.leoTheme.noteWidth() === width}">${svg(WIDTH_ICONS[width])}<span>${WIDTH_LABELS[width]}</span></button>`).join('')}</div></div>
      <p class="hint">Kept on this computer. Demon Felix has scary moves of his own, and tickling either one too much has a surprise.</p>
    </section>`;
}

function pickTheme(el) {
  window.leoTheme.set(el.dataset.theme);
  for (const button of document.querySelectorAll('[data-action="theme-pick"]')) {
    const on = button.dataset.theme === window.leoTheme.choice();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
}

const WIDTH_LABELS = { comfortable: 'Comfortable', wide: 'Wide', full: 'Full width' };
const WIDTH_ICONS = {
  comfortable: '<rect x="7" y="4" width="10" height="16" rx="1.5"/><path d="M9.5 8.5h5M9.5 12h5M9.5 15.5h3"/>',
  wide: '<rect x="4" y="4" width="16" height="16" rx="1.5"/><path d="M7 8.5h10M7 12h10M7 15.5h6"/>',
  full: '<rect x="2" y="4" width="20" height="16" rx="1.5"/><path d="M5 8.5h14M5 12h14M5 15.5h9"/>',
};

function pickWidth(el) {
  window.leoTheme.setNoteWidth(el.dataset.width);
  for (const button of document.querySelectorAll('[data-action="width-pick"]')) {
    const on = button.dataset.width === window.leoTheme.noteWidth();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
}

function pickFelix(el) {
  window.leoTheme.setFelix(el.dataset.felix);
  for (const button of document.querySelectorAll('[data-action="felix-pick"]')) {
    const on = button.dataset.felix === window.leoTheme.felix();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
}

function felixUsageCard(page) {
  const rows = window.leoChat.usageRows(page.felix_usage);
  const tools = page.felix_tools || {};
  const count = (value) => Number.isSafeInteger(value) && value > 0 ? value.toLocaleString() : '0';
  return `<section class="set-card">
    <header><h3>Felix activity</h3></header>
    <p class="hint">Since this Leo server started. Cached input is part of total input; these counts are not your subscription balance. Provider retries may have unreported usage. Timing shows the latest 64 requests of each kind.</p>
    <div class="set-row column"><span class="set-label">Tools</span><span class="set-note">${count(tools.calls)} calls · ${count(tools.reused)} unchanged reads reused · ${count(tools.failures)} failed calls</span></div>
    ${rows.length ? `<details class="source-more"><summary>${ICON.chevron}Request breakdown</summary>${rows.map((row) => `<div class="set-row column"><span class="set-label">${esc(row.label)}</span><span class="set-note">${esc(row.detail)}</span></div>`).join('')}</details>` : '<p class="set-note">No AI requests recorded yet.</p>'}
  </section>`;
}

function drawSettings(page) {
  state.settings = page;
  const backup = page.backup;
  app.innerHTML = `<div class="settings">
    <div class="section-title">Settings</div>
    ${themeCard()}
    ${page.tasks.filter((t) => t.task === 'writing').map((t) => taskCard(t)).join('')}
    <div id="speech-library">${speechCards(state.speech)}</div>
    ${calendarCard(state.calendar)}
    ${felixUsageCard(page)}
    <section class="set-card">
      <header><h3>Backup</h3>${backup.remote ? '<span class="set-status ok">On</span>' : '<span class="set-status missing">Not set up</span>'}</header>
      <p class="hint">${backup.remote ? `Your notes are copied to ${esc(backup.remote)}.` : 'Backup is not set up yet.'}</p>
      <p class="hint">On quit also backs up notes and tasks when you close the leo window or quit leo.</p>
      <label class="set-row"><span class="set-label">Push changes</span><span class="select-control"><select data-set="auto_push">${backup.options.map((o) => settingsOption(o.id, o.label, backup.auto_push)).join('')}</select>${ICON.chevron}</span></label>
    </section>
    <section class="set-card">
      <header><h3>Where things are</h3></header>
      <div class="set-row column"><span class="set-label">Notes</span><code class="set-path">${esc(page.paths.notes || '')}</code></div>
      <div class="set-row column"><span class="set-label">Settings file</span><code class="set-path">${esc(page.paths.config || '')}</code></div>
      <p class="hint">Changes here save straight away. Keys are kept in a file only your account can read, and are never shown again.</p>
    </section>
  </div>`;
}

async function changeSetting(change) {
  try {
    const done = await api('/api/settings', { method: 'POST', body: change });
    drawSettings(done.settings);
    toast(done.message);
  } catch (e) {
    if (state.settings) drawSettings(state.settings);
    throw e;
  }
}

app.addEventListener('change', (e) => {
  const el = e.target.closest('select[data-set]');
  if (!el || state.view !== 'settings') return;
  const change = { set: el.dataset.set, value: el.value };
  if (el.dataset.task) change.task = el.dataset.task;
  changeSetting(change).catch(fail);
});

app.addEventListener('keydown', (e) => {
  const input = e.target.closest('[data-key-input]');
  if (input && e.key === 'Enter') {
    e.preventDefault();
    saveKey(input.dataset.keyInput).catch(fail);
  }
});

async function saveKey(account) {
  const input = app.querySelector(`[data-key-input="${CSS.escape(account)}"]`);
  const value = input ? input.value.trim() : '';
  if (!value) return toast('Paste the key first.', { bad: true });
  input.value = '';
  await changeSetting({ set: 'key', account, value });
}

function confirmRemoveKey(el) {
  sheet(`<h3>Remove the ${esc(el.dataset.name)} key?</h3>
    <p>leo stops using ${esc(el.dataset.name)} until you add a key again.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="remove-key-now" data-account="${esc(el.dataset.account)}">Remove</button></div>`);
}

async function testAi(el) {
  const out = $(`#test-${el.dataset.task}`);
  el.disabled = true;
  out.className = 'set-result';
  out.textContent = 'Asking…';
  try {
    const done = await api('/api/settings/test', { method: 'POST', body: { task: el.dataset.task } });
    out.className = 'set-result ok';
    out.textContent = done.message;
  } catch (e) {
    out.className = 'set-result bad';
    out.textContent = e.message;
  } finally {
    el.disabled = false;
  }
}

function speechCards(view) {
  if (!view) return '';
  const selected = view.selected || {};
  const rows = view.models.map((model) => {
    const progress = model.progress;
    const downloading = progress && progress.state === 'downloading';
    const percent = downloading ? Math.min(100, Math.floor(progress.bytes * 100 / progress.total)) : 0;
    const action = (verb, label) => `<button class="btn plain sm" data-action="speech-model" data-model="${esc(model.id)}" data-verb="${verb}">${label}</button>`;
    const controls = downloading ? `<span class="hint">${percent}%</span>${action('cancel', 'Cancel')}` : model.verified
      ? `${model.in_use ? '<span class="chip">In use</span>' : action('use', 'Use')}${action('delete', 'Delete')}`
      : `${action('download', model.downloaded ? 'Download again' : 'Download')}${model.downloaded ? action('delete', 'Delete') : ''}`;
    return `<div class="set-row column"><div class="set-line"><span class="set-label">${esc(model.name)}</span><span class="set-value">${controls}</span></div>
      <span class="hint">${esc(model.languages.length === 1 ? model.languages[0] : `${model.languages.length} languages`)} · ${Math.ceil(model.size / 1048576)} MB · ${model.live ? 'Live lecture transcripts' : 'Transcript after recording'}</span>
      ${progress && progress.error && progress.state !== 'cancelled' ? `<span class="set-note warn">${esc(progress.error)}</span>` : ''}</div>`;
  }).join('');
  const current = view.models.find((model) => model.id === selected.id);
  const languages = current && current.languages.length > 1 ? `<label class="set-row"><span class="set-label">Language</span><span class="select-control"><select id="speech-language" data-speech-language="${esc(current.id)}">${settingsOption('auto', 'Detect language', selected.language)}${current.languages.map((language) => settingsOption(language, language, selected.language)).join('')}</select>${ICON.chevron}</span></label>` : '';
  return `<section class="set-card"><header><h3>Speech models</h3><span class="set-status ${view.ready ? 'ok' : 'missing'}">${view.ready ? 'Ready' : 'Choose a model'}</span></header>
    <p class="hint">Dictation and recording use one local model. Audio stays on this computer.</p>${languages}${rows}</section>`;
}

let speechPollGeneration = 0;
function startSpeechPolling(mine) { pollSpeech(mine, ++speechPollGeneration); }

async function pollSpeech(mine, generation) {
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
  if (!(state.speech && state.speech.models.some((m) => m.progress && m.progress.state === 'downloading'))) return;
  await new Promise((resolve) => setTimeout(resolve, 1000));
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
  try {
    const view = await api('/api/speech');
    if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
    state.speech = view;
    const box = $('#speech-library');
    if (box && !box.contains(document.activeElement)) box.innerHTML = speechCards(view);
  } catch (e) { if (mine !== seq) return; }
  pollSpeech(mine, generation);
}

async function speechChange(change) {
  const mine = seq;
  const generation = ++speechPollGeneration;
  const view = await api('/api/speech', { method: 'POST', body: change });
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
  state.speech = view;
  const box = $('#speech-library');
  if (box) box.innerHTML = speechCards(view);
  pollSpeech(mine, generation);
}

function changeSpeechModel(el) {
  const model = state.speech.models.find((m) => m.id === el.dataset.model);
  if (!model) return;
  const change = { action: el.dataset.verb, id: model.id, language: model.languages.length === 1 ? model.languages[0] : 'auto' };
  if (change.action === 'delete') {
    sheet(`<h3>Delete ${esc(model.name)}?</h3><p>The download is removed from this computer. Your notes stay.</p><div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="speech-delete" data-model="${esc(model.id)}">Delete</button></div>`);
    return;
  }
  return speechChange(change);
}

app.addEventListener('change', (event) => {
  const language = event.target.closest('[data-speech-language]');
  if (language) speechChange({ action: 'use', id: language.dataset.speechLanguage, language: language.value }).catch(fail);
});
