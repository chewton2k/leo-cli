const TASK_TITLE = { writing: 'AI for writing', speech: 'AI for speech' };
const TASK_USE = {
  writing: 'Turns recordings into notes, answers @leo questions, powers Felix and the knowledge graph.',
  speech: 'Turns what was said into text while you record.',
};

async function showSettings(section) {
  section = settingsSection(section);
  // Trash is a page of notes rather than a setting, so a bare #/settings never reopens it.
  if (section === 'trash') return showTrash();
  try { localStorage.setItem('leo-settings-section', section); } catch (e) {}
  if (section === 'storage') return showStorage();
  const mine = ++seq;
  state = { view: 'settings', dir: '', section };
  scheduleAppStatus(appPollGeneration);
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Settings</button>';
  document.title = 'Settings · leo';
  refreshPanel(app, settingsFrame(section, skeleton(3)));
  const [page, calendar, speech, dictation] = await Promise.all([api('/api/settings'), api('/api/calendar').catch(() => null), api('/api/speech').catch(() => null), api('/api/dictation').catch(() => null)]);
  if (mine !== seq) return;
  state.calendar = calendar;
  state.speech = speech;
  state.dictation = dictation;
  drawSettings(page);
  if (section === 'health') loadHealth().catch(fail);
  startSpeechPolling(mine);
  if (section === 'dictation') pollDictation(mine, ++dictationGeneration);
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
  const plan = state.settings?.plans?.find(plan => plan.provider === t.provider);
  const status = plan && plan.installed && plan.signed_in !== true
    ? `<span class="set-status missing">${plan.signed_in === false ? 'Sign in needed' : 'Sign-in unknown'}</span>`
    : t.ready ? '<span class="set-status ok">Ready</span>' : '<span class="set-status missing">Not set up</span>';
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
  const usage = t.usage && !plan ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
  const note = t.note ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
  return `<section class="set-card" data-task-card="${t.task}">
    <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
    <p class="hint">${TASK_USE[t.task]}</p>
    ${t.custom || t.choices.length > 4
      ? `<label class="set-row"><span class="set-label">Provider</span><span class="select-control"><select data-set="provider" data-task="${t.task}">${providers}</select>${ICON.chevron}</span></label>`
      : `<div class="set-row"><span class="set-label">Provider</span>${segmented({ label: 'Provider', choices: t.choices.map((c) => [c.id, c.label]), current: t.provider, field: `data-set="provider" data-task="${t.task}"` })}</div>`}
    ${model}${key}${signin}${usage}${note}
    <div class="set-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test</button><span class="set-result" id="test-${t.task}"></span></div>
  </section>`;
}

const PLAN_STALE_MS = 5 * 60 * 1000;

function planLimits(plan) {
  const limits = plan.limits;
  const now = Date.now();
  const windows = [['five_hour', '5 hour'], ['seven_day', '7 day']].flatMap(([key, name]) => {
    const window = limits?.[key];
    if (!window || !Number.isFinite(window.used)) return [];
    const reset = Number.isFinite(window.resets_at) ? new Date(window.resets_at * 1000) : null;
    const validReset = reset && Number.isFinite(reset.getTime());
    const expired = validReset && reset.getTime() <= now;
    const percent = expired ? 0 : Math.round(Math.max(0, Math.min(1, window.used)) * 100);
    const when = expired ? 'Waiting for the next update' : validReset ? `Resets ${reset.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })}` : 'Reset time unavailable';
    return [`<div class="plan-window"><div class="set-line"><span>${name}</span><strong>${percent}% used</strong></div><div class="plan-meter${percent >= 90 ? ' near-limit' : ''}" role="meter" aria-label="${esc(plan.name)} ${name} usage" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}"><i style="width:${percent}%"></i></div><span class="hint">${esc(when)}</span></div>`];
  });
  const seen = limits && new Date(limits.seen_at).getTime();
  const age = Number.isFinite(seen) ? `<p class="hint">${now - seen >= PLAN_STALE_MS ? 'Saved usage · ' : ''}Updated ${esc(rel(limits.seen_at))}</p>` : '';
  return windows.length ? windows.join('') + age : `<p class="hint">${plan.provider === 'claude_code' ? 'Usage appears after Claude Code reports limits during an answer.' : 'Usage appears when Codex reports account limits. Select Codex and reopen Writing AI to check.'}</p>`;
}

function codingPlans(page) {
  if (!page.plans?.length) return '';
  const selected = page.tasks.find(task => task.task === 'writing')?.provider;
  return `<section class="set-card"><header><h3>Coding accounts</h3></header><p class="hint">Detected from the coding apps on this computer. Sign-in stays with each app; Leo never copies its credentials. Billing follows the account signed into the CLI.</p><div class="coding-plans">${page.plans.map(plan => {
    const connected = plan.installed && plan.signed_in === true;
    const status = !plan.installed ? 'Not available' : connected ? 'Signed in' : plan.signed_in === false ? 'Signed out' : 'Sign-in unknown';
    return `<section class="coding-plan" aria-label="${esc(plan.name)} account"><header><h4>${esc(plan.name)}</h4><span class="set-status${connected ? ' ok' : ' missing'}">${status}</span></header>${planLimits(plan)}${!connected ? `<p class="hint">${plan.installed ? `Check sign-in with ${esc(plan.signin)}.` : 'Install or update this coding app, then reopen Writing AI.'}</p>` : ''}${selected === plan.provider ? '<span class="chip">Selected for writing</span>' : connected ? `<button class="btn sm plain" data-action="use-plan" data-provider="${esc(plan.provider)}">Use ${esc(plan.name)}</button>` : ''}</section>`;
  }).join('')}</div></section>`;
}

const THEME_LABELS = { system: 'Match device', light: 'Light', dark: 'Dark' };
const THEME_ICONS = { system: 'device', light: 'sun', dark: 'moon' };

function themeCard() {
  const now = window.leoTheme.choice();
  const felixNow = window.leoTheme.felix();
  return `<section class="set-card">
      <header><h3>Appearance</h3></header>
      <div class="set-row"><span class="set-label">Theme</span>${segmented({ label: 'Theme', action: 'theme-pick', key: 'theme', current: now, choices: window.leoTheme.CHOICES.map((choice) => [choice, THEME_LABELS[choice], ICON[THEME_ICONS[choice]]]) })}</div>
      <div class="set-row"><span class="set-label">Felix</span>${segmented({ label: 'Felix', action: 'felix-pick', key: 'felix', current: felixNow, choices: [['cute', 'Cute'], ['demon', 'Demon']].map(([kind, label]) => [kind, label, felix.felix(22, `felix-preview ${kind}-look`)]) })}</div>
      <div class="set-row"><span class="set-label">Note width</span>${segmented({ label: 'Note width', action: 'width-pick', key: 'width', current: window.leoTheme.noteWidth(), choices: window.leoTheme.WIDTHS.map((width) => [width, WIDTH_LABELS[width], svg(WIDTH_ICONS[width])]) })}</div>
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
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="theme-pick"]')?.closest('.segmented');
  if (box) syncSegmented(box);
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
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="width-pick"]')?.closest('.segmented');
  if (box) syncSegmented(box);
}

function pickFelix(el) {
  window.leoTheme.setFelix(el.dataset.felix);
  for (const button of document.querySelectorAll('[data-action="felix-pick"]')) {
    const on = button.dataset.felix === window.leoTheme.felix();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="felix-pick"]')?.closest('.segmented');
  if (box) syncSegmented(box);
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

function backupCard(backup) {
  return `    <section class="set-card">
      <header><h3>GitHub</h3>${backup.remote ? '<span class="set-status ok">On</span>' : '<span class="set-status missing">Not set up</span>'}</header>
      <p class="hint">${backup.remote ? `Your notes are copied to ${esc(backup.remote)}.` : 'Backup is not set up yet.'}</p>
      ${!backup.remote ? '<div class="set-row"><input class="set-input grow" id="backup-repository" value="leo-notes" aria-label="GitHub repository name"><button class="btn primary sm" data-action="backup-setup">Set up Backup</button></div><p class="set-note">Uses GitHub CLI. If needed, install gh from <a href="https://cli.github.com" target="_blank" rel="noopener noreferrer">cli.github.com</a> and sign in once with gh auth login.</p>' : ''}
      <p class="hint">On quit also backs up notes and tasks when you close the leo window or quit leo.</p>
      <label class="set-row"><span class="set-label">Push changes</span><span class="select-control"><select data-set="auto_push">${backup.options.map((o) => settingsOption(o.id, o.label, backup.auto_push)).join('')}</select>${ICON.chevron}</span></label>
    </section>
`;
}
function drawSettings(page) {
  const fields = keepSettingsFields(), segments = keepSegmented();
  state.settings = page;
  const section = state.section || 'general';
  let content = '';
  if (section === 'general') content = themeCard() + `<div id="app-general">${nativeGeneralCard(desktopApp)}</div>`;
  else if (section === 'dictation') content = `<div id="dictation-library">${dictationCards(state.dictation) || '<section class="set-card">Open the Leo app to set up dictation.</section>'}</div>`;
  else if (section === 'models') content = `<div id="speech-library">${speechCards(state.speech)}</div>`;
  else if (section === 'writing') content = codingPlans(page) + page.tasks.filter((task) => task.task === 'writing').map(taskCard).join('') + felixUsageCard(page);
  else if (section === 'calendar') content = calendarCard(state.calendar);
  else if (section === 'backup') content = backupCard(page.backup);
  else if (section === 'health') content = `<div id="app-health">${healthCards(state.health)}</div>`;
  else if (section === 'updates') content = `<div id="app-updates">${updatesCard(desktopApp)}</div>`;
  else content = '';
  refreshPanel(app, settingsFrame(section, `<div class="settings">${content}</div>`));
  restoreSettingsFields(fields, segments);
}

async function changeSetting(change) {
  const mine = seq;
  try {
    const done = await api('/api/settings', { method: 'POST', body: change });
    if (mine !== seq || state.view !== 'settings') return;
    drawSettings(done.settings);
    toast(done.message);
  } catch (e) {
    if (mine === seq && state.view === 'settings' && state.settings) drawSettings(state.settings);
    throw e;
  }
}

app.addEventListener('change', (e) => {
  const el = e.target.closest('select[data-set],input[data-set]');
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
  if (!view) return '<section class="set-card"><p class="hint">Speech models could not be opened. Reopen Leo and try again.</p></section>';
  const selected = view.selected || {};
  const current = view.models.find((model) => model.id === selected.id);
  const language = current && view.ready && current.languages.length > 1 ? `<label class="set-row"><span class="set-label">Language</span><span class="select-control"><select id="speech-language" data-speech-language="${esc(current.id)}">${settingsOption('auto', 'Detect language', selected.language)}${current.languages.map((code) => settingsOption(code, speechLanguage(code), selected.language)).join('')}</select>${ICON.chevron}</span></label>` : '';
  const downloaded = view.models.filter((model) => model.downloaded);
  const extras = view.extra_files || [];
  const filter = state.speechFilter || 'recommended';
  const filters = [['recommended', 'Recommended'], ['english', 'English'], ['many', 'Many languages'], ['downloaded', 'Downloaded'], ['all', `All ${view.models.length}`]];
  const rows = speechFiltered(view.models, filter).map(speechModelRow).join('');
  return `<section class="set-card"><header><h3>In use</h3><span class="set-status ${view.ready ? 'ok' : 'missing'}">${view.ready ? 'Ready' : 'Choose a model'}</span></header><p class="hint">${view.ready && current ? esc(current.name) : 'Download a model below, then choose Use.'}</p><p class="set-note">Dictation and recording share this model. Audio stays on this computer.</p>${language}</section>
    <section class="set-card" data-models-downloaded><header><h3>Downloaded</h3><span class="hint">${downloaded.length + extras.length} models · ${speechBytes(view.disk_bytes)} on this computer</span></header>
      ${downloaded.map(speechModelRow).join('')}${extras.map((file) => `<div class="set-row"><span class="grow"><span>${esc(file.name)}</span><span class="set-note">${speechBytes(file.bytes)} · Not in the current catalog</span></span><button class="btn plain sm" data-action="speech-model" data-model="${esc(file.name)}" data-verb="delete_file">Delete</button></div>`).join('')}${!downloaded.length && !extras.length ? '<p class="set-note">No models downloaded yet.</p>' : ''}</section>
    <section class="set-card" data-models-all><header><h3>All models</h3></header><p class="hint">Choose a local model. Larger models use more memory.</p><div class="library-views model-filters" role="group" aria-label="Filter models">${filters.map(([id, label]) => `<button class="btn plain sm${filter === id ? ' on' : ''}" data-action="speech-filter" data-filter="${id}" aria-pressed="${filter === id}">${label}</button>`).join('')}</div>${rows || '<p class="set-note">No models match this filter.</p>'}<p class="set-note">Accuracy uses the chosen language, or combines measured languages with Detect language. Benchmark speed may differ on your computer.</p></section>`;
}
function speechBytes(bytes) {
  bytes = Math.max(0, Number(bytes) || 0);
  return bytes >= 1073741824 ? (bytes / 1073741824).toFixed(1) + ' GB' : bytes >= 1048576 ? Math.round(bytes / 1048576) + ' MB' : bytes + ' B';
}
function speechLanguage(code) {
  try { return new Intl.DisplayNames(['en'], { type: 'language' }).of(code) || code; } catch (e) { return code; }
}
function speechFiltered(models, filter) {
  return models.filter((model) => filter === 'all' || filter === 'recommended' && model.recommended || filter === 'english' && model.languages.includes('en') || filter === 'many' && model.languages.length > 1 || filter === 'downloaded' && model.downloaded);
}
function speechRatings(model) {
  const speed = Math.max(0, Number(model.speed) || 0);
  const accuracy = Math.max(0, Math.min(100, Number(model.accuracy) || 0));
  const measured = model.accuracy_measured ?? (accuracy > 0);
  const language = model.benchmark_language && model.benchmark_language !== 'auto' ? speechLanguage(model.benchmark_language) : 'Measured languages';
  const wait = speed > 0 ? 30 / speed : null;
  const level = wait === null ? 0 : [2, 3.5, 7.5, 20].filter(cutoff => wait < cutoff).length;
  const speedLabel = speed > 0 ? speed.toFixed(1) + '× real time' : 'Not measured';
  const accuracyLabel = measured ? accuracy.toFixed(1) + '%' : 'Not measured';
  return `<div class="model-ratings"><span class="model-rating" title="${esc(language)} benchmark"><span>Accuracy</span>${measured ? `<meter class="model-meter" min="0" max="100" value="${accuracy}" aria-label="Accuracy ${esc(accuracyLabel)}"></meter>` : ''}<span class="hint">${esc(accuracyLabel)}</span></span><span class="model-rating" title="${wait === null ? 'No speed benchmark' : 'Estimated wait after 30 seconds of speech: ' + wait.toFixed(1) + ' seconds'}"><span>Speed</span>${speed > 0 ? `<meter class="model-meter" min="0" max="4" value="${level}" aria-label="Speed ${esc(speedLabel)}"></meter>` : ''}<span class="hint">${esc(speedLabel)}</span></span></div>`;
}
function speechModelRow(model) {
  const progress = model.progress;
  const downloading = progress && progress.state === 'downloading';
  const percent = downloading ? Math.min(100, Math.floor(progress.bytes * 100 / Math.max(1, progress.total))) : 0;
  const action = (verb, label) => `<button class="btn plain sm" data-action="speech-model" data-model="${esc(model.id)}" data-verb="${verb}">${label}</button>`;
  const controls = downloading ? `<span class="hint">${percent}%</span><progress value="${percent}" max="100" aria-label="Downloading ${esc(model.name)}"></progress>${action('cancel', 'Cancel')}` : model.verified ? `${model.in_use ? '<span class="chip">In use</span>' : action('use', 'Use')}${action('delete', 'Delete')}` : `${model.downloaded ? '<span class="chip">Check failed</span>' : ''}${action('download', model.downloaded ? 'Download again' : 'Download')}${model.downloaded ? action('delete', 'Delete') : ''}`;
  const names = model.languages.map(speechLanguage);
  return `<div class="set-row column" data-model-row="${esc(model.id)}"><div class="set-line"><span class="set-label">${esc(model.name)}${model.recommended ? ' <span class="chip">Recommended</span>' : ''}</span><span class="set-value">${controls}</span></div>${model.description ? `<p class="hint">${esc(model.description)}</p>` : ''}<span class="hint" title="${esc(names.join(', '))}">${esc(names.length === 1 ? names[0] + ' only' : names.length + ' languages')} · ${speechBytes(model.size)} · ${model.streaming ? 'Streaming' : 'Text after you speak'} · ${model.live ? 'Live lecture transcripts' : 'Lecture transcript after recording'}</span>${speechRatings(model)}${progress && progress.error && progress.state !== 'cancelled' ? `<span class="set-note warn">${esc(progress.error)}</span>` : ''}</div>`;
}
function filterSpeech(el) {
  state.speechFilter = el.dataset.filter;
  const box = $('#speech-library');
  if (!box) return;
  const section = box.closest('.settings-section') || box.parentElement;
  const before = box.querySelector('.model-filters')?.getBoundingClientRect().top;
  const top = scrollY;
  refreshPanel(box, speechCards(state.speech));
  if (section) {
    const short = Math.ceil(top + innerHeight - document.documentElement.scrollHeight);
    if (short > 0) section.style.minHeight = `${section.offsetHeight + short}px`;
  }
  const after = box.querySelector('.model-filters')?.getBoundingClientRect().top;
  if (before != null && after != null) scrollBy(0, after - before);
  box.querySelector(`[data-filter="${CSS.escape(state.speechFilter)}"]`)?.focus({ preventScroll: true });
}

let speechPollGeneration = 0;
function startSpeechPolling(mine) { pollSpeech(mine, ++speechPollGeneration); }

async function pollSpeech(mine, generation) {
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings' || !appVisible()) return;
  if (!(state.speech && state.speech.models.some((m) => m.progress && m.progress.state === 'downloading'))) return;
  await new Promise((resolve) => setTimeout(resolve, 1000));
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings' || !appVisible()) return;
  try {
    const view = await api('/api/speech');
    if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
    state.speech = view;
    const box = $('#speech-library');
    refreshPanel(box, speechCards(view));
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
  refreshPanel(box, speechCards(view));
  pollSpeech(mine, generation);
}

function changeSpeechModel(el) {
  if (el.dataset.verb === 'delete_file') {
    const file = (state.speech.extra_files || []).find((file) => file.name === el.dataset.model);
    if (!file) return;
    sheet(`<h3>Delete ${esc(file.name)}?</h3><p>This removes the model file from this computer.</p><div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="speech-delete" data-model="${esc(file.name)}" data-verb="delete_file">Delete</button></div>`);
    return;
  }
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
