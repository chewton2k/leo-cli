function dictationCards(view) {
  if (!view) return '';
  const s = view.settings || {}, keys = view.shortcut || {};
  const select = (name, choices) => `<span class="select-control"><select data-dictation="${name}">${choices.map(([value, label]) => settingsOption(value, label, String(s[name] == null ? '' : s[name]))).join('')}</select>${ICON.chevron}</span>`;
  const row = (label, control) => `<label class="set-row"><span class="set-label">${label}</span><span class="set-value">${control}</span></label>`;
  const toggle = (name, label) => `<label class="set-row"><span class="grow">${label}</span>${settingsSwitch(`data-dictation="${name}"`, s[name])}</label>`;
  const choice = (name, label, choices) => `<div class="set-row"><span class="set-label">${label}</span>${segmented({ label, choices, current: String(s[name] == null ? choices[0][0] : s[name]), field: `data-dictation="${name}"` })}</div>`;
  const microphones = [['', 'System default']].concat((view.microphones || []).map((name) => [name, name]));
  return `<section class="set-card" id="dictation-preferences"><header><h3>Preferences</h3><span class="set-value"><span class="hint" id="dictation-result" role="status"></span><span class="set-status" id="dictation-badge">${dictationBadge(s)}</span></span></header>
    ${view.problem ? `<p class="set-note warn">${esc(view.problem)}</p>` : ''}
    <p class="hint">Custom words and filler rules run on this computer, without writing AI unless you turn on AI writing below.</p>
    <div class="set-row column"><div class="set-line"><span class="set-label">Shortcut</span><span class="set-shortcut"><input class="set-input" data-dictation="shortcut" value="${esc(s.shortcut || '')}" readonly aria-label="Dictation shortcut"><button class="btn plain sm" data-action="dictation-record">Record shortcut</button><button class="btn plain sm" data-action="dictation-use-shortcut" hidden>Use shortcut</button><button class="btn plain sm" data-action="dictation-resume" hidden>Cancel</button></span></div><span id="dictation-shortcut-status" class="hint">${esc(keys.problem || '')}</span></div>
    ${choice('mode', 'Activation', [['both', 'Hold or tap'], ['hold', 'Hold'], ['tap', 'Tap']])}
    ${row('Microphone', select('microphone', microphones))}
    ${row('Lid closed microphone', select('closed_microphone', [['', 'Use regular microphone']].concat(microphones.slice(1))))}
    <label class="set-row column"><span class="set-label">Custom words</span><textarea class="set-input" data-dictation="custom_words" rows="3" placeholder="Names and terms, one per line">${esc((s.custom_words || []).join('\n'))}</textarea><span class="hint">Names and terms Leo should spell your way, up to 200.</span></label>
    ${toggle('fillers', 'Remove filler words')}${toggle('trailing_space', 'Add trailing space')}${toggle('enter', 'Press Enter after pasting')}${toggle('sounds', 'Start and stop sounds')}
    <div class="set-row column"><label class="set-line"><span class="grow">AI writing</span>${settingsSwitch(`data-dictation="writing" data-on="${s.writing && s.writing !== 'off' ? esc(s.writing) : 'quick'}"`, s.writing && s.writing !== 'off')}</label><span class="hint">Fixes punctuation and grammar with your <a href="#/settings/writing">Writing AI</a>. Adds a few seconds and sends the text to it; Escape cancels.</span>${view.writing_ready === false ? '<span class="set-note warn">No writing AI is set up yet, so dictations are pasted as spoken. Choose one in <a href="#/settings/writing">Writing AI</a>.</span>' : ''}</div>
    ${choice('popup', 'Popup', [['live', 'Live'], ['minimal', 'Minimal'], ['off', 'Off']])}
    ${choice('top', 'Popup position', [['false', 'Bottom'], ['true', 'Top']])}
  </section><section class="set-card"><header><h3>Recent dictations</h3><button class="btn plain sm" data-action="dictation-clear">Clear</button></header><p class="hint">Last 20, on this computer only.</p><div id="dictation-recent">${dictationRecent(view.recent || [])}</div></section>`;
}
function dictationRecent(items) {
  return items.length ? items.map((item) => `<div class="set-row"><span class="grow">${esc(item.text)}</span><button class="btn plain sm" data-action="dictation-copy" data-id="${esc(String(item.id))}">Copy</button></div>`).join('') : '<p class="set-note">Your dictations will appear here.</p>';
}
function dictationDraft() {
  const settings = Object.assign({}, state.dictation.settings);
  for (const field of app.querySelectorAll('[data-dictation]')) {
    const name = field.dataset.dictation;
    settings[name] = name === 'custom_words' ? field.value.split('\n').map((word) => word.trim()).filter(Boolean) : name === 'writing' ? (field.checked ? field.dataset.on : 'off') : field.type === 'checkbox' ? field.checked : name === 'top' ? field.value === 'true' : /microphone/.test(name) ? field.value || null : field.value;
  }
  return settings;
}
let dictationGeneration = 0;
async function dictationChange(change) {
  const mine = seq, generation = ++dictationGeneration;
  const view = await api('/api/dictation', { method: 'POST', body: change });
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
  state.dictation = view;
  updateDictationStatus(view);
  const recent = $('#dictation-recent');
  if (recent && !recent.contains(document.activeElement)) recent.innerHTML = dictationRecent(view.recent || []);
  const result = $('#dictation-result');
  if (result && change.action === 'save') {
    result.textContent = 'Saved';
    clearTimeout(dictationSavedTimer);
    dictationSavedTimer = setTimeout(() => { if (result.isConnected) result.textContent = ''; }, 2000);
  }
  pollDictation(mine, generation);
}
function dictationBadge(settings) {
  return settings && settings.writing && settings.writing !== 'off' ? 'Uses writing AI' : 'Local';
}
function updateDictationStatus(view) {
  const badge = $('#dictation-badge');
  if (badge && view.settings) badge.textContent = dictationBadge(view.settings);
  const shortcut = view.shortcut || {};
  const status = $('#dictation-shortcut-status');
  if (status) status.textContent = shortcut.recording ? shortcut.candidate || 'Press your shortcut. Modifier-only and Fn work too.' : shortcut.problem || '';
  const use = $('[data-action="dictation-use-shortcut"]'), cancel = $('[data-action="dictation-resume"]');
  if (use) { use.hidden = !shortcut.recording; use.disabled = !shortcut.candidate; }
  if (cancel) cancel.hidden = !shortcut.recording;
}
async function pollDictation(mine, generation) {
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings' || !state.dictation) return;
  await new Promise((resolve) => setTimeout(resolve, 600));
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
  try {
    const view = await api('/api/dictation');
    if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
    state.dictation = view;
    updateDictationStatus(view);
    const recent = $('#dictation-recent');
    if (recent && !recent.contains(document.activeElement)) recent.innerHTML = dictationRecent(view.recent || []);
  } catch (e) {}
  pollDictation(mine, generation);
}
let dictationSaving = Promise.resolve(), dictationTyping = null, dictationSavedTimer = null;
function saveDictation() {
  clearTimeout(dictationTyping);
  dictationSaving = dictationSaving.catch(() => {}).then(() => dictationChange({ action: 'save', settings: dictationDraft() }));
  return dictationSaving;
}
app.addEventListener('change', (e) => {
  if (e.target.closest?.('[data-dictation]') && e.target.dataset.dictation !== 'shortcut') saveDictation().catch(fail);
});
app.addEventListener('input', (e) => {
  if (e.target.dataset?.dictation !== 'custom_words') return;
  clearTimeout(dictationTyping);
  dictationTyping = setTimeout(() => saveDictation().catch(fail), 700);
});
function useDictationShortcut() {
  const candidate = state.dictation && state.dictation.shortcut && state.dictation.shortcut.candidate;
  if (!candidate) return;
  const field = $('[data-dictation="shortcut"]');
  if (field) field.value = candidate;
  return saveDictation();
}
if (typeof window !== 'undefined' && window.leoTheme) {
  const syncDictationTheme = () => api('/api/dictation', { method: 'POST', body: { action: 'theme', theme: window.leoTheme.choice() } }).catch(() => {});
  window.leoTheme.onChange(syncDictationTheme);
  syncDictationTheme();
}
