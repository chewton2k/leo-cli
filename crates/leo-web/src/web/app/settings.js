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
  const [page, calendar] = await Promise.all([api('/api/settings'), api('/api/calendar').catch(() => null)]);
  if (mine !== seq) return;
  state.calendar = calendar;
  drawSettings(page);
}

const GOOGLE_CALENDAR_SETTINGS = 'https://calendar.google.com/calendar/r/settings';

function calendarCard(cal, secure) {
  if (!cal) return '';
  const linked = cal.calendars || [];
  const count = linked.length;
  const chip = count ? `<span class="set-status ok">${count === 1 ? 'On' : `${count} calendars`}</span>` : '<span class="set-status missing">Not connected</span>';
  const rows = linked
    .map((c) => `<div class="cal-row"><span class="grow"><b>${esc(c.name)}</b>${c.problem ? `<span class="sub rec-warn">${esc(c.problem)}</span>` : `<span class="sub">Added ${esc(rel(c.added_at))}</span>`}</span><button class="btn plain" data-action="calendar-remove" data-id="${esc(c.id)}" data-name="${esc(c.name)}">Remove</button></div>`)
    .join('');
  const next = (cal.events || [])[0];
  const coming = next ? `<p class="hint">Next: ${esc(next.title)}, ${esc(new Date(next.start).toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' }))}.</p>` : count ? '<p class="hint">Nothing in the next week.</p>' : '';
  const steps = `<ol class="cal-steps">
      <li>Open <a href="${GOOGLE_CALENDAR_SETTINGS}" target="_blank" rel="noopener noreferrer">Google Calendar settings</a> on a computer.</li>
      <li>On the left, under <b>Settings for my calendars</b>, click the calendar you want.</li>
      <li>Scroll to <b>Integrate calendar</b> and copy <b>Secret address in iCal format</b>.</li>
      <li>Paste it below.</li>
    </ol>
    <p class="hint">Outlook: Settings → Calendar → Shared calendars → Publish a calendar, then copy the ICS link. Apple Calendar: share the calendar publicly and copy its link.</p>`;
  const form = secure
    ? `<div class="cal-add"><input id="calendar-link" class="cal-link" type="text" inputmode="url" autocomplete="off" spellcheck="false" placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"><button class="btn primary" data-action="calendar-add">Add calendar</button></div>`
    : '<p class="rec-warn">Calendar links are private, so add one on the computer running leo, or through the https link.</p>';
  return `<section class="set-card cal-card">
    <header><h3>Calendar</h3>${chip}</header>
    <p class="hint">When you record, leo names the recording after the class or meeting you are in, and the AI gets the event’s description and who was invited. leo only reads your calendar; it never changes it.</p>
    ${rows}
    ${coming}
    ${count ? `<div class="buttons"><button class="btn plain" data-action="calendar-refresh"${cal.refreshing ? ' disabled' : ''}>${cal.refreshing ? 'Reading…' : 'Check now'}</button></div>` : ''}
    <details class="cal-how"${count ? '' : ' open'}><summary>${count ? 'Add another calendar' : 'Connect your Google Calendar'}</summary>${steps}${form}</details>
  </section>`;
}

async function addCalendar() {
  const box = $('#calendar-link');
  if (!box || !box.value.trim()) throw Object.assign(new Error('Paste your calendar’s secret address first.'), { shown: true });
  const button = $('[data-action="calendar-add"]');
  if (button) {
    button.disabled = true;
    button.textContent = 'Checking the link…';
  }
  const mine = seq;
  try {
    const calendar = await api('/api/calendar', { method: 'POST', body: { link: box.value.trim() } });
    if (mine !== seq || state.view !== 'settings') return;
    state.calendar = calendar;
    drawSettings(state.settings);
    const added = calendar.calendars[calendar.calendars.length - 1];
    toast(`Connected “${added ? added.name : 'your calendar'}”.`);
  } catch (e) {
    if (button && button.isConnected) {
      button.disabled = false;
      button.textContent = 'Add calendar';
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
    menu.hidden = true;
    const button = menu.parentElement.querySelector('.picker-button');
    if (button) button.setAttribute('aria-expanded', 'false');
  }
}

function togglePicker(el) {
  const menu = el.parentElement.querySelector('.picker-menu');
  closePickers(menu);
  menu.hidden = !menu.hidden;
  el.setAttribute('aria-expanded', String(!menu.hidden));
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

function taskCard(t, secure) {
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
    const form = secure
      ? `<div class="set-key-form"><input type="password" autocomplete="off" spellcheck="false" placeholder="${has ? 'Paste a new key to replace it' : `Paste your ${esc(t.key.name)} API key`}" data-key-input="${esc(t.key.account)}"><button class="btn primary sm" data-action="set-key" data-account="${esc(t.key.account)}">${has ? 'Replace' : 'Save key'}</button></div>`
      : '<p class="hint">To add a key, open the https link leo serve printed (not the Wi-Fi one), or use this page on your computer. Keys never cross Wi-Fi unencrypted.</p>';
    key = `<div class="set-row column"><div class="set-line"><span class="set-label">${esc(t.key.name)} key</span><span class="set-value">${has ? '<span class="set-status ok">Stored on your computer</span>' : '<span class="set-status missing">Not added</span>'}${has ? `<button class="btn plain sm" data-action="remove-key" data-account="${esc(t.key.account)}" data-name="${esc(t.key.name)}">Remove</button>` : ''}</span></div>${form}${t.key.ignored ? `<p class="hint">leo does not read $${esc(t.key.ignored)}; add the key here instead.</p>` : ''}</div>`;
  }
  const signin = t.signin ? `<p class="set-note${t.signin.installed ? '' : ' warn'}">${esc(t.signin.text)}</p>` : '';
  const usage = t.usage ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
  const note = t.note ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
  return `<section class="set-card" data-task-card="${t.task}">
    <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
    <p class="hint">${TASK_USE[t.task]}</p>
    <label class="set-row"><span class="set-label">Provider</span><select data-set="provider" data-task="${t.task}">${providers}</select></label>
    ${model}${key}${signin}${usage}${note}
    <div class="set-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test</button><span class="set-result" id="test-${t.task}"></span></div>
  </section>`;
}

function drawSettings(page) {
  state.settings = page;
  const backup = page.backup;
  app.innerHTML = `<div class="settings">
    <div class="section-title">Settings</div>
    ${page.tasks.map((t) => taskCard(t, page.secure)).join('')}
    ${calendarCard(state.calendar, page.secure)}
    <section class="set-card">
      <header><h3>Backup</h3>${backup.remote ? '<span class="set-status ok">On</span>' : '<span class="set-status missing">Not set up</span>'}</header>
      <p class="hint">${backup.remote ? `Your notes are copied to ${esc(backup.remote)}.` : 'Backup is not set up yet. Run :backup in leo on your computer to keep a copy on GitHub.'}</p>
      <label class="set-row"><span class="set-label">Push changes</span><select data-set="auto_push">${backup.options.map((o) => settingsOption(o.id, o.label, backup.auto_push)).join('')}</select></label>
    </section>
    <section class="set-card">
      <header><h3>Where things are</h3></header>
      <div class="set-row column"><span class="set-label">Notes</span><code class="set-path">${esc(page.paths.notes || '')}</code></div>
      <div class="set-row column"><span class="set-label">Settings file</span><code class="set-path">${esc(page.paths.config || '')}</code></div>
      <p class="hint">Changes here save straight away and are the same settings as :settings in leo. Keys are kept in a file only your account can read, and are never shown again.</p>
    </section>
    <section class="set-card">
      <header><h3>Advanced</h3></header>
      <button class="list-row set-advanced" data-action="storage">${ICON.folder}<span class="grow">Storage and data<span class="sub">See what leo keeps on this computer, how much space it takes, and delete what you no longer need</span></span>${ICON.chevron}</button>
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
