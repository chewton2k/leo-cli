let desktopApp = null;
let appPollGeneration = 0;
let appPollTimer = 0;
let appPollVisible = null;
const APP_STATUS_ACTIVE_MS = 2500;
const APP_STATUS_IDLE_MS = 30000;
function scheduleAppStatus(generation) {
  if (generation !== appPollGeneration) return;
  clearTimeout(appPollTimer);
  appPollTimer = 0;
  if (!appVisible()) return;
  const active = state.view === 'settings' || ['checking', 'downloading', 'installing'].includes(desktopApp?.updates?.state);
  const delay = onboardingHere(0) ? ONBOARDING_POLL_MS : active ? APP_STATUS_ACTIVE_MS : APP_STATUS_IDLE_MS;
  appPollTimer = setTimeout(() => pollAppStatus(generation), delay);
}
function appVisibilityChanged() {
  const visible = appVisible();
  if (visible === appPollVisible) return;
  appPollVisible = visible;
  clearTimeout(appPollTimer);
  appPollTimer = 0;
  const generation = ++appPollGeneration;
  if (typeof checkActivity === 'function') checkActivity();
  if (!visible) return;
  pollAppStatus(generation);
  if (state.view === 'settings') {
    if (typeof startSpeechPolling === 'function') startSpeechPolling(seq);
    if (state.section === 'dictation' && typeof pollDictation === 'function') pollDictation(seq, ++dictationGeneration);
  }
}
function nativeGeneralCard(view) {
  if (!view) return '';
  const p = view.preferences || {};
  return `<section class="set-card"><header><h3>This app</h3></header><p class="set-note warn" id="app-preferences-problem"${view.problem ? '' : ' hidden'}>${esc(view.problem || '')}</p><label class="set-row"><span class="grow">Open at login</span>${settingsSwitch('data-app-pref="open_at_login"', p.open_at_login)}</label><label class="set-row"><span class="grow">Show in ${view.platform === 'macos' ? 'menu bar' : 'system tray'}</span>${settingsSwitch('data-app-pref="show_tray"', p.show_tray)}</label><div class="set-row"><button class="btn plain sm" data-action="app-command" data-app-action="quit">Quit leo</button></div></section>`;
}
function healthCards(view) {
  if (!view) return '<section class="set-card"><p class="hint">Checking Health…</p></section>';
  return `<div class="health-intro"><span class="grow hint">Local checks. Your microphone stays closed and no AI request is sent.</span><button class="btn plain sm" data-action="health-refresh">Check again</button></div>` + (view.sections || []).map((section) => `<section class="set-card"><header><h3>${esc(section.title)}</h3></header>${section.checks.map((check) => `<div class="set-row column"><div class="set-line"><span class="set-label">${esc(check.name)}</span><span class="set-status ${check.state === 'ready' ? 'ok' : 'missing'}">${check.state === 'ready' ? 'Ready' : check.state === 'warn' ? 'Check' : 'Needs setup'}</span></div>${check.detail ? `<span class="hint">${esc(check.detail)}</span>` : ''}${check.state !== 'ready' ? `<span class="set-note health-fix">${esc(check.fix || '')}</span>${check.action ? `<button class="btn plain sm" data-action="app-command" data-app-action="${esc(check.action)}">Allow access</button>` : check.section ? `<a class="btn plain sm" href="#/settings/${esc(check.section)}">Open ${esc(settingsTitle(check.section))}</a>` : ''}` : ''}</div>`).join('')}</section>`).join('');
}
async function loadHealth() {
  const mine = seq;
  const view = await api('/api/health');
  if (mine !== seq || state.view !== 'settings' || state.section !== 'health') return;
  state.health = view;
  const box = $('#app-health');
  refreshPanel(box, healthCards(view));
}
async function appChange(change) {
  const generation = ++appPollGeneration;
  try {
    const view = await api('/api/app', { method: 'POST', body: change });
    if (generation !== appPollGeneration) return;
    desktopApp = view;
    updateAppStatus();
    if (state.view === 'settings' && state.section === 'health') loadHealth().catch(fail);
    return view;
  } finally { scheduleAppStatus(generation); }
}

function updateAppStatus() {
  updateOnboardingPermissions();
  updateUpdateBanner();
  if (state.view === 'settings' && state.section === 'updates') {
    const box = $('#app-updates');
    refreshPanel(box, updatesCard(desktopApp));
  }
  if (state.view === 'settings' && state.section === 'general') {
    const box = $('#app-general');
    if (box && !box.querySelector('[data-app-pref]')) refreshPanel(box, nativeGeneralCard(desktopApp));
    const problem = $('#app-preferences-problem');
    if (problem) { problem.hidden = !desktopApp?.problem; problem.textContent = desktopApp?.problem || ''; }
    for (const field of app.querySelectorAll('[data-app-pref]')) field.checked = Boolean(desktopApp?.preferences?.[field.dataset.appPref]);
  }
}
async function pollAppStatus(generation) {
  if (generation !== appPollGeneration || !appVisible()) return;
  try {
    const view = await api('/api/app');
    if (generation !== appPollGeneration) return;
    desktopApp = view;
    updateAppStatus();
    maybeOnboarding();
  } catch (e) {}
  scheduleAppStatus(generation);
}
async function setupBackup() {
  const field = $('#backup-repository');
  const name = field?.value.trim();
  if (!name) throw new Error('Enter a GitHub repository name, such as leo-notes.');
  const mine = seq;
  const button = $('[data-action="backup-setup"]');
  if (button) { button.disabled = true; button.textContent = 'Connecting…'; }
  try {
    const done = await api('/api/backup/setup', { method: 'POST', body: { name } });
    if (mine !== seq || state.view !== 'settings' || state.section !== 'backup') return;
    drawSettings(done.settings);
    toast(done.message);
  } finally { if (button?.isConnected) { button.disabled = false; button.textContent = 'Set up Backup'; } }
}
app.addEventListener('change', (event) => {
  const field = event.target.closest('[data-app-pref]');
  if (field) appChange({ action: 'preference', field: field.dataset.appPref, value: field.checked }).catch(fail);
});
document.addEventListener('visibilitychange', appVisibilityChanged);
if (typeof window !== 'undefined' && window.addEventListener) {
  window.addEventListener('DOMContentLoaded', appVisibilityChanged, { once: true });
  window.addEventListener('leo-visibility', appVisibilityChanged);
}


function updatesCard(view) {
  if (!view) return '<section class="set-card"><p class="hint">Open the Leo app to check for updates.</p></section>';
  const preferences = view.preferences || {}, update = view.updates || {};
  const busy = update.busy;
  const status = { idle: 'Check for a newer version.', checking: 'Checking…', current: 'Leo is up to date.', available: 'A new version is available.', downloading: 'Downloading…', ready: 'The update is ready.', installing: 'Installing…', error: 'Check again when you are connected.' }[update.state] || '';
  const checked = update.last_checked ? new Date(Number(update.last_checked) * 1000).toLocaleString() : 'Not checked yet';
  const progress = update.state === 'downloading' ? `<p class="hint">${speechBytes(update.bytes)}${update.total ? ' of ' + speechBytes(update.total) : ''}</p>` : '';
  const restart = update.state === 'ready' ? `<button class="btn primary sm" data-action="app-command" data-app-action="restart_update"${busy ? ' disabled' : ''}>Restart to update</button>${busy ? '<span class="hint">Finish recording or dictation first.</span>' : ''}` : '';
  const download = update.state === 'available' || update.state === 'error' && update.version ? '<button class="btn plain sm" data-action="app-command" data-app-action="download_update">Download</button>' : '';
  return `<section class="set-card"><header><h3>Updates</h3><span class="chip">leo ${esc(view.version)}</span></header><div class="set-row column"><span>${esc(status)}</span><span class="hint">Last checked: ${esc(checked)}</span><div class="buttons"><button class="btn plain sm" data-action="app-command" data-app-action="check_updates"${['checking','downloading','installing'].includes(update.state) ? ' disabled' : ''}>Check now</button>${download}${restart}</div>${progress}${update.error ? `<p class="set-note warn">${esc(update.error)}</p>` : ''}</div>
    <label class="set-row"><span class="grow">Check for updates automatically</span>${settingsSwitch('data-app-pref="automatic_updates"', preferences.automatic_updates)}</label><label class="set-row"><span class="grow">Install updates when I quit leo</span>${settingsSwitch('data-app-pref="install_on_quit"', preferences.install_on_quit)}</label><p class="set-note">Updates wait for recording and dictation to finish. Leo reopens after every update.</p></section>
    ${update.notes ? `<section class="set-card"><header><h3>What’s new${update.version ? ' in ' + esc(update.version) : ''}</h3></header><div class="prose">${md.render(Array.from(String(update.notes)).slice(0, 8192).join(''), { interactive: false })}</div></section>` : ''}
    <section class="set-card"><header><h3>About Leo</h3></header>${view.platform === 'macos' && view.unsigned ? '<p class="set-note warn">This copy is not signed yet. macOS may ask you to remove and add Leo again in System Settings → Privacy &amp; Security → Accessibility after an update.</p>' : ''}<div class="set-row column"><span class="set-label">Notes</span><code class="set-path">${esc(view.paths?.notes || '')}</code></div><div class="set-row column"><span class="set-label">Speech models</span><code class="set-path">${esc(view.paths?.models || '')}</code></div><div class="set-row column"><span class="set-label">Terminal commands</span><span><code>leo update</code> downloads the latest Leo app.</span><span><code>leo uninstall</code> removes Leo. Your notes stay.</span></div><p class="set-note"><a href="https://github.com/chewton2k/leo-cli/releases" target="_blank" rel="noopener noreferrer">Downloads and release notes</a></p></section>`;
}
function updateUpdateBanner() {
  const banner = $('#app-update-banner');
  if (!banner) return;
  const update = desktopApp?.updates || {};
  const ready = update.state === 'ready';
  banner.hidden = !ready;
  if (!ready) return;
  if (banner.dataset.version !== update.version) {
    banner.dataset.version = update.version;
    banner.innerHTML = `<span class="grow">leo ${esc(update.version)} is ready</span><button class="btn primary sm" data-action="app-command" data-app-action="restart_update">Restart to update</button><span class="hint" data-update-wait></span>`;
  }
  const button = banner.querySelector('button');
  if (button) button.disabled = Boolean(update.busy);
  const wait = banner.querySelector('[data-update-wait]');
  if (wait) wait.textContent = update.busy ? 'After recording or dictation finishes.' : '';
}
