function dictationCards(view) {
  if (!view) return '';
  const s = view.settings || {}, keys = view.shortcut || {};
  const select = (name, choices) => `<span class="select-control"><select data-dictation="${name}">${choices.map(([value, label]) => settingsOption(value, label, String(s[name] == null ? '' : s[name]))).join('')}</select>${ICON.chevron}</span>`;
  const row = (label, control) => `<label class="set-row"><span class="set-label">${label}</span><span class="set-value">${control}</span></label>`;
  const toggle = (name, label) => `<label class="set-row"><span class="grow">${label}</span>${settingsSwitch(`data-dictation="${name}"`, s[name])}</label>`;
  const choice = (name, label, choices) => `<div class="set-row"><span class="set-label">${label}</span>${segmented({ label, choices, current: String(s[name] == null ? choices[0][0] : s[name]), field: `data-dictation="${name}"` })}</div>`;
  const microphones = [['', 'System default']].concat((view.microphones || []).map((name) => [name, name]));
  return `<section class="set-card" id="dictation-preferences"><header><h3>Preferences</h3><span class="set-status">Local</span></header>
    ${view.problem ? `<p class="set-note warn">${esc(view.problem)}</p>` : ''}
    <p class="hint">Custom words and filler rules run on this computer, without writing AI.</p>
    <div class="set-row column"><div class="set-line"><span class="set-label">Shortcut</span><span class="set-value set-shortcut"><input class="set-input" data-dictation="shortcut" value="${esc(s.shortcut || '')}" readonly aria-label="Dictation shortcut"><button class="btn plain sm" data-action="dictation-record">Record shortcut</button><button class="btn plain sm" data-action="dictation-use-shortcut" hidden>Use shortcut</button><button class="btn plain sm" data-action="dictation-resume" hidden>Cancel</button></span></div><span id="dictation-shortcut-status" class="hint">${esc(keys.problem || '')}</span></div>
    ${choice('mode', 'Activation', [['both', 'Hold or tap'], ['hold', 'Hold'], ['tap', 'Tap']])}
    ${row('Microphone', select('microphone', microphones))}
    ${row('Lid closed microphone', select('closed_microphone', [['', 'Use regular microphone']].concat(microphones.slice(1))))}
    <label class="set-row column"><span class="set-label">Custom words</span><textarea class="set-input" data-dictation="custom_words" rows="3" placeholder="Names and terms, one per line">${esc((s.custom_words || []).join('\n'))}</textarea><span class="hint">Up to 200 names or terms. Save after editing.</span></label>
    ${toggle('fillers', 'Remove filler words')}${toggle('trailing_space', 'Add trailing space')}${toggle('enter', 'Press Enter after pasting')}${toggle('sounds', 'Start and stop sounds')}
    ${choice('popup', 'Popup', [['live', 'Live'], ['minimal', 'Minimal'], ['off', 'Off']])}
    ${choice('top', 'Popup position', [['false', 'Bottom'], ['true', 'Top']])}
    <div class="set-row"><button class="btn plain sm" data-action="dictation-save">Save dictation settings</button><span class="hint" id="dictation-result"></span></div>
  </section><section class="set-card"><header><h3>Recent dictations</h3><button class="btn plain sm" data-action="dictation-clear">Clear</button></header><p class="hint">Last 20, on this computer only.</p><div id="dictation-recent">${dictationRecent(view.recent || [])}</div></section>`;
}
function dictationRecent(items) {
  return items.length ? items.map((item) => `<div class="set-row"><span class="grow">${esc(item.text)}</span><button class="btn plain sm" data-action="dictation-copy" data-id="${esc(String(item.id))}">Copy</button></div>`).join('') : '<p class="set-note">Your dictations will appear here.</p>';
}
function dictationDraft() {
  const settings = Object.assign({}, state.dictation.settings);
  for (const field of app.querySelectorAll('[data-dictation]')) {
    const name = field.dataset.dictation;
    settings[name] = name === 'custom_words' ? field.value.split('\n').map((word) => word.trim()).filter(Boolean) : field.type === 'checkbox' ? field.checked : name === 'top' ? field.value === 'true' : /microphone/.test(name) ? field.value || null : field.value;
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
  if (result && change.action === 'save') result.textContent = 'Saved';
  pollDictation(mine, generation);
}
function updateDictationStatus(view) {
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
function saveDictation() { return dictationChange({ action: 'save', settings: dictationDraft() }); }
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
