(function (root) {
  'use strict';
  function display(state) {
    const labels = { listening: 'Listening', loading: 'Loading model…', transcribing: 'Writing it out…', error: 'Dictation needs a moment', idle: '' };
    const seconds = Math.max(0, Math.floor(Number(state.seconds) || 0));
    return { top: Boolean(state.top), label: labels[state.state] || '', timer: Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0'), text: state.state === 'error' ? state.message || '' : state.minimal ? '' : state.text || '', recording: state.state === 'listening' };
  }
  if (typeof module !== 'undefined') { module.exports = { display }; return; }
  const invoke = (action) => root.__TAURI_INTERNALS__.invoke('dictation_popup', { action });
  const wave = root.document.getElementById('wave');
  for (let n = 0; n < 7; n++) wave.appendChild(root.document.createElement('i'));
  root.document.getElementById('cancel').addEventListener('click', () => invoke('cancel').catch(() => {}));
  let busy = false;
  async function poll() {
    if (busy) return;
    busy = true;
    try {
      const state = await invoke('state');
      const view = display(state);
      root.document.getElementById('tokens').textContent = state.tokens || '';
      const html = root.document.documentElement;
      if (state.theme === 'light' || state.theme === 'dark') html.setAttribute('data-theme', state.theme);
      else html.removeAttribute('data-theme');
      root.document.body.dataset.state = state.state;
      root.document.body.dataset.top = String(view.top);
      root.document.getElementById('label').textContent = view.label;
      root.document.getElementById('timer').textContent = view.recording ? view.timer : '';
      root.document.getElementById('text').textContent = view.text;
      wave.hidden = !view.recording;
      const level = Math.min(1, Math.max(0, Number(state.level) || 0) * 8);
      Array.from(wave.children).forEach((bar, index) => { bar.style.height = (3 + level * (14 - Math.abs(3 - index) * 2)) + 'px'; });
    } catch (e) {}
    finally { busy = false; }
  }
  poll();
  root.setInterval(poll, 80);
})(typeof window === 'undefined' ? globalThis : window);
