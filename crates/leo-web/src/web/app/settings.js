const TASK_TITLE = { writing: 'AI for writing', speech: 'AI for speech' };
const TASK_USE = {
  writing: 'Turns recordings into notes, powers Felix and note connections.',
  speech: 'Turns what was said into text while you record.',
};

async function showSettings(section) {
  section = settingsSection(section);
  // Trash is a page of notes rather than a setting, so a bare #/settings never reopens it.
  if (section === 'trash') return showTrash();
  try { localStorage.setItem('leo-settings-section', section); } catch (e) {}
  if (section === 'keeping') return showKeeping();
  const mine = ++seq;
  state = { view: 'settings', dir: '', section };
  scheduleAppStatus(appPollGeneration);
  chrome({ showBack: true });
  $('#crumbs').innerHTML = settingsCrumbs(section);
  document.title = 'Settings · leo';
  refreshPanel(app, settingsFrame(section, skeleton(3)));
  const [page, calendar, speech, dictation, margin] = await Promise.all([api('/api/settings'), api('/api/calendar').catch(() => null), api('/api/speech').catch(() => null), api('/api/dictation').catch(() => null), section === 'felix' ? api('/api/margin').catch(() => null) : null]);
  if (mine !== seq) return;
  state.margin = margin;
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
    ? `<div class="set-row" data-find="calendar-next"><span class="set-label">Next up</span><span class="grow cal-next">${next ? `${esc(next.title)} <span class="cal-sub">· ${esc(when(next))}</span>` : '<span class="cal-sub">Nothing in the next week</span>'}</span><button class="btn plain sm" data-action="calendar-refresh"${cal.refreshing ? ' disabled' : ''}>${cal.refreshing ? 'Checking…' : 'Check now'}</button></div>`
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
  return `<section class="set-card cal-card" data-find="calendar-connect">
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
  return `<div class="set-row" data-find="writing-model"><span class="set-label">Model</span><div class="picker"><button type="button" class="picker-button" data-action="toggle-picker" aria-haspopup="menu" aria-expanded="false" data-task="${t.task}"><span class="picker-text">${shown}</span>${ICON.chevron}</button><div class="picker-menu" role="menu" hidden><div class="pick-head">Model</div><div class="pick-models">${rows}</div>${chips}</div></div></div>`;
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
  if (opening) unfold(menu, el);
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
    model = `<div class="set-row" data-find="writing-model"><span class="set-label">Model</span><span class="set-value">${esc(t.model || 'Built in')} <span class="hint">free, runs on your computer</span></span></div>`;
  } else if (t.models.length) {
    model = modelPicker(t);
  }
  let key = '';
  if (t.key) {
    const has = t.key.stored;
    const form = `<div class="set-key-form"><input type="password" autocomplete="off" spellcheck="false"${has ? ` class="has-key" placeholder="${esc(t.key.hint || '••••••••')}" aria-label="${esc(t.key.name)} key is stored; type a new one to replace it"` : ` placeholder="Paste your ${esc(t.key.name)} API key"`} data-key-input="${esc(t.key.account)}"><button class="btn primary sm" data-action="set-key" data-account="${esc(t.key.account)}">${has ? 'Save' : 'Save key'}</button></div>`;
    key = `<div class="set-row column" data-find="writing-key"><div class="set-line"><span class="set-label">${esc(t.key.name)} key</span><span class="set-value">${has ? '' : '<span class="set-status missing">Not added</span>'}${has ? `<button class="btn plain sm" data-action="remove-key" data-account="${esc(t.key.account)}" data-name="${esc(t.key.name)}">Remove</button>` : ''}</span></div>${form}${t.key.ignored ? `<p class="hint">leo does not read $${esc(t.key.ignored)}; add the key here instead.</p>` : ''}</div>`;
  }
  const signin = t.signin ? `<p class="set-note${t.signin.installed ? '' : ' warn'}">${esc(t.signin.text)}</p>` : '';
  const usage = t.usage && !plan ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
  const note = t.note ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
  return `<section class="set-card" data-task-card="${t.task}" data-find="writing-provider">
    <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
    <p class="hint">${TASK_USE[t.task]}</p>
    ${t.custom || t.choices.length > 4
      ? `<label class="set-row"><span class="set-label">Provider</span><span class="select-control"><select data-set="provider" data-task="${t.task}">${providers}</select>${ICON.chevron}</span></label>`
      : `<div class="set-row"><span class="set-label">Provider</span>${segmented({ label: 'Provider', choices: t.choices.map((c) => [c.id, c.label]), current: t.provider, field: `data-set="provider" data-task="${t.task}"` })}</div>`}
    ${model}${key}${signin}${usage}${note}
    <div class="set-test" data-find="writing-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test</button><span class="set-result" id="test-${t.task}"></span></div>
  </section>`;
}

const PLAN_STALE_MS = 5 * 60 * 1000;

function planLimits(plan) {
  const limits = plan.limits;
  const now = Date.now();
  return [['five_hour', 'Session', '5 hour'], ['seven_day', 'Weekly', '7 day']].map(([key, name, period]) => {
    const window = limits?.[key];
    const used = window?.used;
    const valid = Number.isFinite(used);
    const reset = Number.isFinite(window?.resets_at) ? new Date(window.resets_at * 1000) : null;
    const validReset = reset && Number.isFinite(reset.getTime());
    const expired = validReset && reset.getTime() <= now;
    const available = valid && !expired;
    const percent = available ? Math.max(0, Math.min(100, 100 - Math.round(used * 100))) : null;
    const missing = expired ? 'Reset' : 'Not read yet';
    const when = !available ? 'Refresh to check' : validReset ? `Resets in ${duration(reset.getTime() - now)}` : 'Reset time unavailable';
    return `<div class="plan-window"><div class="plan-metric"><span>${name}</span><strong${available ? '' : ' class="plan-none"'}>${available ? `${percent}% <small>left</small>` : missing}</strong></div><div class="plan-track"><div class="plan-meter${available ? '' : ' unavailable'}" role="meter" aria-label="${esc(plan.name)} ${period} remaining" aria-valuemin="0" aria-valuemax="100"${available ? ` aria-valuenow="${percent}"` : ` aria-valuetext="${missing}"`}>${available ? `<i style="width:${percent}%"></i>` : ''}</div><span class="hint">${esc(when)}</span></div></div>`;
  }).join('');
}

function planUpdated(plan) {
  const limits = plan.limits;
  const now = Date.now();
  const seenAt = typeof limits?.seen_at === 'string' && limits.seen_at ? limits.seen_at : null;
  const seen = seenAt ? new Date(seenAt).getTime() : NaN;
  const stale = Number.isFinite(seen) && now - seen >= PLAN_STALE_MS;
  return Number.isFinite(seen) ? `${stale ? 'Saved · ' : ''}Updated ${rel(seenAt)}` : '';
}

function duration(ms) {
  const minutes = Math.max(1, Math.floor(ms / 60000));
  const days = Math.floor(minutes / 1440), hours = Math.floor((minutes % 1440) / 60), rest = minutes % 60;
  return days ? `${days}d ${hours}h` : hours ? `${hours}h ${rest}m` : `${rest}m`;
}

function codingPlans(page) {
  if (!page.plans?.length) return '';
  return `<div class="coding-plans">${page.plans.map(plan => {
    const connected = plan.installed && plan.signed_in === true;
    const status = !plan.installed ? 'Not available' : connected ? 'Signed in' : plan.signed_in === false ? 'Signed out' : 'Sign-in unknown';
    const updated = planUpdated(plan);
    return `<section class="coding-plan" data-provider="${esc(plan.provider)}" aria-label="${esc(plan.name)} account"><header><div class="plan-title"><h4>${esc(plan.name)}</h4>${updated ? `<span class="hint plan-updated">${esc(updated)}</span>` : ''}</div><span class="set-status${connected ? ' ok' : ' missing'}">${status}</span></header>${planLimits(plan)}${!connected ? `<p class="hint">${plan.installed ? `Check sign-in with ${esc(plan.signin)}.` : 'Install or update this coding app, then refresh Usage.'}</p>` : ''}</section>`;
  }).join('')}</div>`;
}

const APPEARANCE_DEFAULTS = { theme: 'system', felix: 'cute', width: 'wide', size: 'medium', letters: 'own' };
const THEME_LABELS = { system: 'Match device', light: 'Light', dark: 'Dark' };

const THEME_SURFACES = {
  light: { ground: '#f2f1ec', page: '#f9f8f5', head: '#1c1c1a', line: '#d9d8d3', side: '#e3e2dc' },
  dark: { ground: '#141413', page: '#1f1f1e', head: '#ecebe7', line: '#3a3a38', side: '#2a2a28' },
};
function themeScene(look) {
  const c = THEME_SURFACES[look];
  return `<rect width="120" height="52" fill="${c.ground}"/><rect x="7" y="10" width="15" height="3" rx="1.5" fill="${c.side}"/><rect x="7" y="17" width="11" height="3" rx="1.5" fill="${c.side}"/><rect x="7" y="24" width="13" height="3" rx="1.5" fill="${c.side}"/><rect x="29" y="5" width="96" height="52" rx="5" fill="${c.page}"/><rect x="40" y="14" width="30" height="4" rx="2" fill="${c.head}"/><rect x="40" y="24" width="64" height="3" rx="1.5" fill="${c.line}"/><rect x="40" y="31" width="56" height="3" rx="1.5" fill="${c.line}"/><rect x="40" y="38" width="42" height="3" rx="1.5" fill="${c.line}"/>`;
}
const THEME_DIAGRAMS = {
  system: `<svg viewBox="0 0 120 52">${themeScene('light')}<svg x="60" width="60" height="52" viewBox="60 0 60 52">${themeScene('dark')}</svg></svg>`,
  light: `<svg viewBox="0 0 120 52">${themeScene('light')}</svg>`,
  dark: `<svg viewBox="0 0 120 52">${themeScene('dark')}</svg>`,
};
const TEXT_SIZE_LABELS = { small: 'Small', medium: 'Medium', large: 'Large' };
const LETTERING_LABELS = { own: "Felix's own", notes: 'Like your notes' };
const LETTERING_SAMPLE = 'Found it, in your Tuesday note.';
const WIDTH_COLUMNS = { comfortable: 44, wide: 70, full: 108 };
function widthDiagram(width) {
  const w = WIDTH_COLUMNS[width], x = (120 - w) / 2, line = (share) => Math.round((w - 12) * share);
  return `<svg viewBox="0 0 120 52"><path class="d-page" d="M${x} 6h${w}v46H${x}z"/><path class="d-head" d="M${x + 6} 15h${line(0.45)}"/><path class="d-text" d="M${x + 6} 24h${line(1)}M${x + 6} 31h${line(0.9)}M${x + 6} 38h${line(0.65)}"/></svg>`;
}

function themeCard() {
  const now = window.leoTheme.choice();
  const felixNow = window.leoTheme.felix();
  const widthNow = window.leoTheme.noteWidth();
  const sizeNow = window.leoTheme.textSize();
  const lettersNow = window.leoTheme.felixLetters();
  return `<section class="set-card appearance">
      <header><h3>Appearance</h3></header>
      <div class="set-row column" data-find="theme"><span class="set-label">Theme${settingReset('Theme', 'theme', now !== APPEARANCE_DEFAULTS.theme)}</span>${choiceCards({ label: 'Theme', action: 'theme-pick', key: 'theme', current: now, choices: window.leoTheme.CHOICES.map((choice) => [choice, THEME_LABELS[choice], '', THEME_DIAGRAMS[choice]]) })}</div>
      <div class="set-row column" data-find="felix-look"><span class="set-label">Felix${settingReset('Felix', 'felix', felixNow !== APPEARANCE_DEFAULTS.felix)}</span>${choiceCards({ label: 'Felix', action: 'felix-pick', key: 'felix', current: felixNow, columns: 3, choices: [['cute', 'Cute'], ['demon', 'Demon']].map(([kind, label]) => [kind, label, '', `<svg viewBox="0 0 120 52">${felix.felix(40, `felix-preview ${kind}-look`).replace('<svg ', '<svg x="40" y="10" ')}</svg>`]) })}</div>
      <div class="set-row column" data-find="note-width"><span class="set-label">Note width${settingReset('Note width', 'width', widthNow !== APPEARANCE_DEFAULTS.width)}</span>${choiceCards({ label: 'Note width', action: 'width-pick', key: 'width', current: widthNow, choices: window.leoTheme.WIDTHS.map((width) => [width, WIDTH_LABELS[width], '', widthDiagram(width)]) })}</div>
      <div class="set-row column" data-find="text-size"><span class="set-label">Text size${settingReset('Text size', 'size', sizeNow !== APPEARANCE_DEFAULTS.size)}</span>${choiceCards({ label: 'Text size', action: 'text-size-pick', key: 'size', current: sizeNow, choices: window.leoTheme.TEXT_SIZES.map((size) => [size, TEXT_SIZE_LABELS[size], '', `<span class="read-sample size" data-text-size="${size}">Notes</span>`]) })}</div>
      <div class="set-row column" data-find="felix-lettering"><span class="set-label">Felix's lettering${settingReset("Felix's lettering", 'letters', lettersNow !== APPEARANCE_DEFAULTS.letters)}</span>${choiceCards({ label: "Felix's lettering", action: 'letters-pick', key: 'letters', current: lettersNow, choices: window.leoTheme.LETTERINGS.map((letters) => [letters, LETTERING_LABELS[letters], '', `<span class="read-sample letters" data-letters="${letters}">${LETTERING_SAMPLE}</span>`]) })}</div>
    </section>`;
}

function syncAppearanceResets() {
  const now = { theme: window.leoTheme.choice(), felix: window.leoTheme.felix(), width: window.leoTheme.noteWidth(), size: window.leoTheme.textSize(), letters: window.leoTheme.felixLetters() };
  for (const button of document.querySelectorAll('[data-action="setting-reset"]')) button.toggleAttribute('data-on', now[button.dataset.reset] !== APPEARANCE_DEFAULTS[button.dataset.reset]);
}
function resetSetting(el) {
  const key = el.dataset.reset;
  if (key === 'theme') pickTheme({ dataset: { theme: APPEARANCE_DEFAULTS.theme } });
  else if (key === 'felix') pickFelix({ dataset: { felix: APPEARANCE_DEFAULTS.felix } });
  else if (key === 'width') pickWidth({ dataset: { width: APPEARANCE_DEFAULTS.width } });
  else if (key === 'size') pickTextSize({ dataset: { size: APPEARANCE_DEFAULTS.size } });
  else if (key === 'letters') pickLetters({ dataset: { letters: APPEARANCE_DEFAULTS.letters } });
}

function pickTheme(el) {
  window.leoTheme.set(el.dataset.theme);
  for (const button of document.querySelectorAll('[data-action="theme-pick"]')) {
    const on = button.dataset.theme === window.leoTheme.choice();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="theme-pick"]')?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}

const WIDTH_LABELS = { comfortable: 'Comfortable', wide: 'Wide', full: 'Full width' };

function pickWidth(el) {
  window.leoTheme.setNoteWidth(el.dataset.width);
  for (const button of document.querySelectorAll('[data-action="width-pick"]')) {
    const on = button.dataset.width === window.leoTheme.noteWidth();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="width-pick"]')?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}

function pickFelix(el) {
  window.leoTheme.setFelix(el.dataset.felix);
  for (const button of document.querySelectorAll('[data-action="felix-pick"]')) {
    const on = button.dataset.felix === window.leoTheme.felix();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="felix-pick"]')?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}

function pickReading(action, field, now) {
  for (const button of document.querySelectorAll(`[data-action="${action}"]`)) {
    const on = button.dataset[field] === now;
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  const box = document.querySelector(`[data-action="${action}"]`)?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}
function pickTextSize(el) {
  window.leoTheme.setTextSize(el.dataset.size);
  pickReading('text-size-pick', 'size', window.leoTheme.textSize());
}
function pickLetters(el) {
  window.leoTheme.setFelixLetters(el.dataset.letters);
  pickReading('letters-pick', 'letters', window.leoTheme.felixLetters());
}

function felixUsageCard(page) {
  const rows = window.leoChat.usageRows(page.felix_usage);
  const [week, ...models] = window.leoChat.usageWeek(page.felix_week);
  const tools = page.felix_tools || {};
  const count = (value) => Number.isSafeInteger(value) && value > 0 ? value.toLocaleString() : '0';
  return `<section class="set-card" data-find="writing-activity">
    <header><h3>Felix activity</h3></header>
    ${week ? `<div class="set-row column"><span class="set-label">${esc(week.label)}</span><span class="set-note">${esc(week.detail)}</span></div>` : ''}
    ${models.length ? `<details class="source-more"><summary>${ICON.chevron}By model</summary>${models.map((row) => `<div class="set-row column"><span class="set-label">${esc(row.label)}</span><span class="set-note">${esc(row.detail)}</span></div>`).join('')}</details>` : ''}
    <p class="hint">The rest is since this Leo server started. Cached input is part of total input; these counts are not your subscription balance. Provider retries may have unreported usage. Timing shows the latest 64 requests of each kind.</p>
    <div class="set-row column"><span class="set-label">Tools</span><span class="set-note">${count(tools.calls)} calls · ${count(tools.reused)} unchanged reads reused · ${count(tools.failures)} failed calls</span></div>
    ${rows.length ? `<details class="source-more"><summary>${ICON.chevron}Request breakdown</summary>${rows.map((row) => `<div class="set-row column"><span class="set-label">${esc(row.label)}</span><span class="set-note">${esc(row.detail)}</span></div>`).join('')}</details>` : '<p class="set-note">No AI requests recorded yet.</p>'}
  </section>`;
}

function drawSettings(page) {
  const fields = keepSettingsFields(), segments = keepSegmented();
  state.settings = page;
  const section = state.section || 'general';
  let content = '';
  if (section === 'general') content = themeCard() + `<div id="app-general">${nativeGeneralCard(desktopApp)}</div>`;
  else if (section === 'dictation') content = `<div id="dictation-library">${dictationCards(state.dictation) || '<section class="set-card">Open the Leo app to set up dictation.</section>'}</div>`;
  else if (section === 'models') content = `<div id="speech-library">${speechCards(state.speech)}</div>`;
  else if (section === 'writing') content = page.tasks.filter((task) => task.task === 'writing').map(taskCard).join('') + felixUsageCard(page);
  else if (section === 'felix') content = felixSettingsCards(state.margin, page);
  else if (section === 'calendar') content = calendarCard(state.calendar);
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
  const { name, account } = el.dataset;
  popupMenu(el, {
    label: `Remove the ${name} key`,
    rows: [
      { note: `leo stops using ${name} until you add a key again.` },
      { text: 'Remove key', icon: ICON.trash, danger: true, run: () => changeSetting({ set: 'key', account, value: null }) },
    ],
  });
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
  const language = current && view.ready && current.languages.length > 1 ? `<label class="set-row" data-find="speech-language"><span class="set-label">Language</span><span class="select-control"><select id="speech-language" data-speech-language="${esc(current.id)}">${settingsOption('auto', 'Detect language', selected.language)}${current.languages.map((code) => settingsOption(code, speechLanguage(code), selected.language)).join('')}</select>${ICON.chevron}</span></label>` : '';
  const downloaded = view.models.filter((model) => model.downloaded);
  const extras = view.extra_files || [];
  const filter = state.speechFilter || 'recommended';
  const filters = [['recommended', 'Recommended'], ['english', 'English'], ['many', 'Many languages'], ['downloaded', 'Downloaded'], ['all', `All ${view.models.length}`]];
  const rows = speechFiltered(view.models, filter).map(speechModelRow).join('');
  return `<section class="set-card" data-find="speech-in-use"><header><h3>In use</h3><span class="set-status ${view.ready ? 'ok' : 'missing'}">${view.ready ? 'Ready' : 'Choose a model'}</span></header><p class="hint">${view.ready && current ? esc(current.name) : 'Download a model below, then choose Use.'}</p><p class="set-note">Dictation and recording share this model. Audio stays on this computer.</p>${language}</section>
    <section class="set-card" data-models-downloaded data-find="speech-downloaded"><header><h3>Downloaded</h3><span class="hint">${downloaded.length + extras.length} models · ${speechBytes(view.disk_bytes)} on this computer</span></header>
      ${downloaded.map(speechModelRow).join('')}${extras.map((file) => `<div class="set-row"><span class="grow"><span>${esc(file.name)}</span><span class="set-note">${speechBytes(file.bytes)} · Not in the current catalog</span></span><button class="btn plain sm" data-action="speech-model" data-model="${esc(file.name)}" data-verb="delete_file">Delete</button></div>`).join('')}${!downloaded.length && !extras.length ? '<p class="set-note">No models downloaded yet.</p>' : ''}</section>
    <section class="set-card" data-models-all data-find="speech-all"><header><h3>All models</h3></header><p class="hint">Choose a local model. Larger models use more memory.</p><div class="library-views model-filters" role="group" aria-label="Filter models">${filters.map(([id, label]) => `<button class="btn plain sm${filter === id ? ' on' : ''}" data-action="speech-filter" data-filter="${id}" aria-pressed="${filter === id}">${label}</button>`).join('')}</div>${rows || '<p class="set-note">No models match this filter.</p>'}<p class="set-note">Accuracy uses the chosen language, or combines measured languages with Detect language. Benchmark speed may differ on your computer.</p></section>`;
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

function speechDeleteMenu(el, name, note, change) {
  popupMenu(el, { label: `Delete ${name}`, rows: [{ note }, { text: 'Delete', icon: ICON.trash, danger: true, run: () => speechChange(change) }] });
}

function changeSpeechModel(el) {
  if (el.dataset.verb === 'delete_file') {
    const file = (state.speech.extra_files || []).find((file) => file.name === el.dataset.model);
    if (file) speechDeleteMenu(el, file.name, 'This removes the model file from this computer.', { action: 'delete_file', id: file.name });
    return;
  }
  const model = state.speech.models.find((m) => m.id === el.dataset.model);
  if (!model) return;
  const change = { action: el.dataset.verb, id: model.id, language: model.languages.length === 1 ? model.languages[0] : 'auto' };
  if (change.action === 'delete') return speechDeleteMenu(el, model.name, 'The download is removed from this computer. Your notes stay.', { action: 'delete', id: model.id });
  return speechChange(change);
}

app.addEventListener('change', (event) => {
  const language = event.target.closest('[data-speech-language]');
  if (language) speechChange({ action: 'use', id: language.dataset.speechLanguage, language: language.value }).catch(fail);
});

async function showUsage(trigger = null) {
  const opener = trigger?.closest?.('.scrim') ? $('#menu') : trigger || document.activeElement;
  const plan = '<section class="coding-plan"><div class="skeleton plan-sk-head"></div><div class="skeleton plan-sk-row"></div><div class="skeleton plan-sk-row"></div></section>';
  const box = sheet(`<header class="usage-head"><div><h3>Usage</h3><p class="usage-sub">Plan limits</p></div><div class="usage-tools"><button type="button" class="icon-btn" data-action="usage-refresh" aria-label="Refresh" disabled>${ICON.refresh}</button><button type="button" class="icon-btn" data-action="close" aria-label="Close">${ICON.close}</button></div></header><div class="usage-content" aria-busy="true"><div class="coding-plans" aria-hidden="true">${plan}${plan}</div></div>`);
  box.returnFocus = opener;
  box.querySelector('.sheet').classList.add('usage-sheet');
  box.querySelector('[data-action="close"]').focus();
  const mine = seq;
  let saved = false, refreshing = true, drawn = false;
  const live = () => mine === seq && box.isConnected && !box.classList.contains('scrim-gone');
  const content = box.querySelector('.usage-content');
  const button = box.querySelector('[data-action="usage-refresh"]');
  const busy = (on) => {
    refreshing = on;
    button.disabled = on;
    button.innerHTML = on ? '<span class="activity-spin"></span>' : ICON.refresh;
  };
  const draw = (page) => {
    content.removeAttribute('aria-busy');
    content.innerHTML = (codingPlans(page) || '<p class="hint" role="status">No coding accounts detected.</p>') + (page.note ? `<p class="hint usage-note">${esc(page.note)}</p>` : '');
    if (!drawn) cascade(content.querySelector('.coding-plans'));
    drawn = true;
  };
  const refresh = async (body = {}) => {
    if (refreshing || !live()) return;
    const focused = document.activeElement === button;
    busy(true);
    try {
      const fresh = await api('/api/settings/usage', { method: 'POST', body });
      if (!live()) return;
      draw(fresh);
      saved = true;
    } catch (e) {
      if (!live()) return;
      if (!saved) { content.removeAttribute('aria-busy'); content.innerHTML = '<p class="hint" role="status">Usage could not be loaded.</p>'; }
      else {
        content.querySelector('.usage-error')?.remove();
        content.insertAdjacentHTML('beforeend', '<p class="hint usage-error" role="status">Refresh failed · showing saved usage</p>');
      }
    } finally {
      if (live()) {
        busy(false);
        if (focused && document.activeElement === document.body) button.focus();
      } else refreshing = false;
    }
  };
  box.addEventListener('click', event => {
    if (event.target.closest('[data-action="usage-refresh"]') && !refreshing) { button.focus(); refresh({ claude: true }); }
  });
  try {
    const page = await api('/api/settings');
    if (!live()) return;
    draw(page);
    saved = Boolean(page.plans?.length);
  } catch (e) {
    if (!live()) return;
    content.removeAttribute('aria-busy');
    content.innerHTML = '<p class="hint" role="status">Usage could not be loaded.</p>';
  }
  busy(false);
  refresh();
}
