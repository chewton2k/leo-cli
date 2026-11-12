(function (root) {
  'use strict';
  const BARS = 9;
  function display(state) {
    const labels = { starting: 'Starting microphone…', listening: 'Listening', loading: 'Loading model…', transcribing: 'Writing it out…', writing: 'Polishing with AI…', notice: 'Pasted without AI writing', error: 'Dictation stopped', idle: '' };
    const seconds = Math.max(0, Math.floor(Number(state.seconds) || 0));
    const error = state.state === 'error' || state.state === 'notice';
    const live = !error && !state.minimal;
    const committed = live ? String(state.committed || '') : '';
    const tentative = live ? String(state.tentative || '') : '';
    const message = error ? state.message || '' : '';
    const recording = state.state === 'listening';
    return {
      top: Boolean(state.top),
      label: state.state === 'notice' && state.title ? String(state.title) : labels[state.state] || '',
      timer: Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0'),
      message,
      committed: committed ? committed + ' ' : '',
      tentative,
      text: message || (committed ? committed + ' ' : '') + tentative,
      open: Boolean(message || committed || tentative),
      caret: recording && live && Boolean(committed || tentative),
      recording,
    };
  }
  function smooth(previous, levels) {
    return previous.map((value, index) => value * 0.7 + (Number(levels[index]) || 0) * 0.3);
  }
  function height(value) {
    return Math.max(3, Math.min(18, 3 + Math.pow(Math.max(0, value), 0.7) * 15));
  }
  if (typeof module !== 'undefined') { module.exports = { display, smooth, height, BARS }; return; }
  const tauri = root.__TAURI_INTERNALS__;
  const doc = root.document;
  const invoke = (action) => tauri.invoke('dictation_popup', { action });
  const element = (id) => doc.getElementById(id);
  root.addEventListener('contextmenu', (event) => event.preventDefault());
  const wave = element('wave');
  const text = element('text');
  for (let n = 0; n < BARS; n++) wave.appendChild(doc.createElement('i'));
  element('cancel').addEventListener('click', () => invoke('cancel').catch(() => {}));
  let levels = Array(16).fill(0);
  let revision = -1;
  let pinned = true;
  let session = '';
  text.addEventListener('scroll', () => { pinned = text.scrollHeight - text.scrollTop - text.clientHeight <= 16; });
  function render(state) {
    const rev = Number(state.rev) || 0;
    if (rev && rev < revision) return;
    revision = Math.max(revision, rev);
    const view = display(state);
    const html = doc.documentElement;
    if (state.theme === 'light' || state.theme === 'dark') html.setAttribute('data-theme', state.theme);
    else html.removeAttribute('data-theme');
    if (state.state === 'listening' && session !== 'listening') { pinned = true; levels = Array(16).fill(0); }
    session = state.state;
    doc.body.dataset.state = state.state;
    doc.body.dataset.top = String(view.top);
    doc.body.dataset.open = String(view.open);
    element('label').textContent = view.label;
    element('timer').textContent = view.recording ? view.timer : '';
    element('message').textContent = view.message;
    element('committed').textContent = view.committed;
    element('tentative').textContent = view.tentative;
    element('caret').hidden = !view.caret;
    wave.hidden = !view.recording;
    if (view.recording && Array.isArray(state.levels)) levels = smooth(levels, state.levels);
    Array.from(wave.children).forEach((bar, index) => { bar.style.height = height(levels[index]) + 'px'; });
    text.classList.toggle('overflowing', text.scrollHeight > text.clientHeight + 1);
    if (pinned) text.scrollTop = text.scrollHeight;
  }
  let busy = false;
  async function poll() {
    if (busy) return;
    busy = true;
    try {
      const state = await invoke('state');
      if (state.tokens) element('tokens').textContent = state.tokens;
      render(state);
    } catch (e) {}
    finally { busy = false; }
  }
  poll();
  try {
    const handler = tauri.transformCallback((event) => render(event.payload || {}));
    Promise.resolve(tauri.invoke('plugin:event|listen', { event: 'dictation', target: { kind: 'Any' }, handler }))
      .catch(() => root.setInterval(poll, 80));
  } catch (e) {
    root.setInterval(poll, 80);
  }
})(typeof window === 'undefined' ? globalThis : window);
