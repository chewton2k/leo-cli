function dictationCards(view) {
  if (!view) return '';
  const s = view.settings || {}, keys = view.shortcut || {};
  const select = (name, choices) => `<span class="select-control"><select data-dictation="${name}">${choices.map(([value, label]) => settingsOption(value, label, String(s[name] == null ? '' : s[name]))).join('')}</select>${ICON.chevron}</span>`;
  const row = (label, control, find) => `<label class="set-row" data-find="${find}"><span class="set-label">${label}</span><span class="set-value">${control}</span></label>`;
  const toggle = (name, label, find) => `<label class="set-row" data-find="${find}"><span class="grow">${label}</span>${settingsSwitch(`data-dictation="${name}"`, s[name])}</label>`;
  const choice = (name, label, choices, find) => `<div class="set-row" data-find="${find}"><span class="set-label">${label}</span>${segmented({ label, choices, current: String(s[name] == null ? choices[0][0] : s[name]), field: `data-dictation="${name}"` })}</div>`;
  const microphones = [['', 'System default']].concat((view.microphones || []).map((name) => [name, name]));
  return `<section class="set-card" id="dictation-preferences"><header><h3>Shortcut and microphone</h3><span class="set-value"><span class="hint" id="dictation-result" role="status"></span></span></header>
    ${view.problem ? `<p class="set-note warn">${esc(view.problem)}</p>` : ''}
    <p class="hint">Custom words and filler rules run on this computer, without writing AI unless you turn on AI writing below.</p>
    <div class="set-row column" data-find="dictation-shortcut"><div class="set-line"><span class="set-label">Shortcut</span><span class="set-shortcut"><input class="set-input" data-dictation="shortcut" value="${esc(s.shortcut || '')}" readonly aria-label="Dictation shortcut"><button class="btn plain sm" data-action="dictation-record">Change shortcut</button><button class="btn plain sm" data-action="dictation-use-shortcut" hidden>Use shortcut</button><button class="btn plain sm" data-action="dictation-resume" hidden>Cancel</button></span></div><span id="dictation-shortcut-status" class="hint">${esc(keys.problem || '')}</span></div>
    ${choice('mode', 'Activation', [['both', 'Hold or tap'], ['hold', 'Hold'], ['tap', 'Tap']], 'dictation-activation')}
    ${row('Microphone', select('microphone', microphones), 'dictation-microphone')}
    ${toggle('sounds', 'Start and stop sounds', 'dictation-sounds')}
  </section><section class="set-card"><header><h3>Text</h3></header>
    <label class="set-row column" data-find="dictation-words"><span class="set-label">Custom words</span><textarea class="set-input" data-dictation="custom_words" rows="3" placeholder="Names and terms, one per line">${esc((s.custom_words || []).join('\n'))}</textarea><span class="hint">Names and terms leo should spell your way, up to 200.</span></label>
    ${toggle('fillers', 'Remove filler words', 'dictation-fillers')}
    <div class="set-row column" data-find="dictation-ai"><label class="set-line"><span class="grow">AI writing</span>${settingsSwitch(`data-dictation="writing" data-on="${s.writing && s.writing !== 'off' ? esc(s.writing) : 'quick'}"`, s.writing && s.writing !== 'off')}</label><span class="hint">Fixes grammar, writes times like 2:30 and formats lists with your <a href="#/settings/writing">Writing AI</a>. Adds a few seconds; Escape cancels.</span>${view.writing_ready === false ? '<span class="set-note warn">No writing AI is set up yet, so dictations are pasted as spoken. Choose one in <a href="#/settings/writing">Writing AI</a>.</span>' : ''}</div>
  </section><section class="set-card"><header><h3>Popup</h3></header>
    ${choice('popup', 'Popup', [['live', 'Live'], ['minimal', 'Minimal'], ['off', 'Off']], 'dictation-popup')}
    <div class="set-row" id="dictation-position-row"${s.popup === 'off' ? ' hidden' : ''}><span class="set-label">Position</span>${segmented({ label: 'Popup position', choices: [['false', 'Bottom'], ['true', 'Top']], current: String(s.top == null ? 'false' : s.top), field: 'data-dictation="top"' })}</div>
  </section><section class="set-card" data-find="dictation-recent"><header><h3>Recent dictations</h3><button class="btn plain sm" data-action="dictation-clear">Clear history</button></header><p class="hint">Last 20, on this computer only.</p><div id="dictation-recent">${dictationRecent(view.recent || [])}</div></section>`;
}
let dictationEdit = null;
const dictationOffers = {}, DICTATION_MOST_WORDS = 200;
function dictationRecent(items) {
  return items.length ? items.map((item) => {
    const id = esc(String(item.id));
    const text = dictationEdit && dictationEdit.id === item.id
      ? `<input class="set-input grow" data-edit-text value="${esc(dictationEdit.text)}" aria-label="Edit dictation">`
      : `<span class="grow" role="button" tabindex="0" title="Edit" data-action="dictation-edit" data-id="${id}">${esc(item.text)}</span>`;
    const offers = (dictationOffers[item.id] || []).map((word) => `<button class="chip accent" data-action="dictation-add-word" data-id="${id}" data-word="${esc(word)}">${ICON.plus}Add \u201c${esc(word)}\u201d to custom words</button>`).join('');
    return `<div class="set-row" data-refresh-key="${id}">${text}${item.edited ? '<span class="hint">Edited</span>' : ''}<button class="btn plain sm" data-action="dictation-copy" data-id="${id}">Copy</button></div>${offers ? `<div class="card-meta" data-refresh-key="offers-${id}">${offers}</div>` : ''}`;
  }).join('') : '<p class="set-note">Your dictations will appear here.</p>';
}
function showDictationRecent() {
  refreshPanel($('#dictation-recent'), dictationRecent((state.dictation && state.dictation.recent) || []));
}
function dictationWordOffers(before, after, known) {
  const seen = new Set(before.split(/\s+/)), runs = [];
  let run = [];
  for (const word of after.split(/\s+/).filter(Boolean)) {
    if (seen.has(word)) { if (run.length) runs.push(run); run = []; } else run.push(word);
  }
  if (run.length) runs.push(run);
  const lower = new Set(known.map((word) => word.toLowerCase()));
  const phrases = runs.map((r) => r.join(' ').replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '')).filter((phrase) => {
    const key = phrase.toLowerCase();
    if (phrase.length < 2 || before.includes(phrase) || lower.has(key)) return false;
    lower.add(key);
    return true;
  });
  return phrases.slice(0, 3);
}
function startDictationEdit(el) {
  const id = Number(el.dataset.id), item = ((state.dictation && state.dictation.recent) || []).find((i) => i.id === id);
  if (!item) return;
  dictationEdit = { id, text: item.text };
  showDictationRecent();
  const input = $('[data-edit-text]');
  if (input) { input.focus(); input.select(); }
}
function cancelDictationEdit() {
  dictationEdit = null;
  showDictationRecent();
}
async function saveDictationEdit() {
  const edit = dictationEdit, text = edit.text.trim();
  const before = (state.dictation.recent.find((i) => i.id === edit.id) || {}).text || '';
  dictationEdit = null;
  try {
    if (text && text !== before) {
      await dictationChange({ action: 'edit', id: edit.id, text });
      dictationOffers[edit.id] = dictationWordOffers(before, text, state.dictation.settings.custom_words || []);
    }
  } finally {
    showDictationRecent();
  }
}
function addDictationWord(el) {
  const id = Number(el.dataset.id), word = el.dataset.word, words = dictationDraft().custom_words;
  if (!words.some((w) => w.toLowerCase() === word.toLowerCase())) {
    if (words.length >= DICTATION_MOST_WORDS) {
      const result = $('#dictation-result');
      if (result) result.textContent = `Custom words is full (${DICTATION_MOST_WORDS}). Remove one to add more.`;
      return;
    }
    $('[data-dictation="custom_words"]').value = words.concat(word).join('\n');
  }
  dictationOffers[id] = (dictationOffers[id] || []).filter((w) => w !== word);
  showDictationRecent();
  return saveDictation();
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
async function dictationChange(change, mine = seq) {
  const generation = mine === seq ? ++dictationGeneration : dictationGeneration;
  const view = await api('/api/dictation', { method: 'POST', body: change });
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
  state.dictation = view;
  updateDictationStatus(view);
  const recent = $('#dictation-recent');
  refreshPanel(recent, dictationRecent(view.recent || []));
  const result = $('#dictation-result');
  if (result && change.action === 'save') {
    result.textContent = 'Saved';
    result.classList.remove('flash');
    void result.offsetWidth;
    result.classList.add('flash');
    pulse(result);
    clearTimeout(dictationSavedTimer);
    dictationSavedTimer = setTimeout(() => { if (result.isConnected) result.textContent = ''; }, 2000);
  }
  pollDictation(mine, generation);
}
function clearDictationsAsk() {
  return confirmSheet({ title: 'Clear recent dictations?', text: 'The last 20 dictations are removed from this computer. This cannot be undone.', yes: 'Clear history', action: 'dictation-clear-now' });
}
function clearDictationsNow() {
  closeSheet();
  return dictationChange({ action: 'clear' });
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
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings' || !state.dictation || !appVisible()) return;
  await new Promise((resolve) => setTimeout(resolve, 600));
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings' || !appVisible()) return;
  try {
    const view = await api('/api/dictation');
    if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
    state.dictation = view;
    updateDictationStatus(view);
    const recent = $('#dictation-recent');
    refreshPanel(recent, dictationRecent(view.recent || []));
  } catch (e) {}
  pollDictation(mine, generation);
}
let dictationSaving = Promise.resolve(), dictationTyping = null, dictationSavedTimer = null;
function saveDictation(settings = state.dictation && dictationDraft(), mine = seq) {
  if (!settings) return dictationSaving;
  clearTimeout(dictationTyping);
  dictationSaving = dictationSaving.catch(() => {}).then(() => dictationChange({ action: 'save', settings }, mine));
  return dictationSaving;
}
app.addEventListener('change', (e) => {
  if (e.target.closest?.('[data-dictation]') && e.target.dataset.dictation !== 'shortcut') {
    if (e.target.dataset.dictation === 'popup') $('#dictation-position-row').hidden = e.target.value === 'off';
    saveDictation().catch(fail);
  }
});
app.addEventListener('keydown', (e) => {
  const el = e.target;
  if (el.matches?.('[data-edit-text]')) {
    if (e.isComposing || (e.key !== 'Enter' && e.key !== 'Escape')) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') cancelDictationEdit();
    else saveDictationEdit().catch(fail);
  } else if ((e.key === 'Enter' || e.key === ' ') && el.dataset?.action === 'dictation-edit') {
    e.preventDefault();
    startDictationEdit(el);
  }
});
app.addEventListener('input', (e) => {
  if (e.target.matches?.('[data-edit-text]') && dictationEdit) dictationEdit.text = e.target.value;
  if (e.target.dataset?.dictation !== 'custom_words') return;
  clearTimeout(dictationTyping);
  const settings = dictationDraft(), mine = seq;
  dictationTyping = setTimeout(() => saveDictation(settings, mine).catch(fail), 700);
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
