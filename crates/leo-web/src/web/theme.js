(function (root) {
  'use strict';

  const KEY = 'leo-theme';
  const FELIX_KEY = 'leo-felix';
  const WIDTH_KEY = 'leo-note-width';
  const WIDTHS = ['comfortable', 'wide', 'full'];
  const CHOICES = ['system', 'light', 'dark'];
  const COLORS = { light: '#f7f7f5', dark: '#141414' };
  const media = root.matchMedia ? root.matchMedia('(prefers-color-scheme: dark)') : null;
  const listeners = new Set();
  let current = 'system';

  function stored() {
    try {
      const value = root.localStorage.getItem(KEY);
      return CHOICES.includes(value) ? value : 'system';
    } catch (e) {
      return 'system';
    }
  }

  function dark() {
    if (current === 'system') return Boolean(media && media.matches);
    return current === 'dark';
  }

  function tell() {
    const now = dark();
    for (const fn of listeners) {
      try {
        fn(now);
      } catch (e) {}
    }
  }

  function apply(choice) {
    current = CHOICES.includes(choice) ? choice : 'system';
    const html = root.document.documentElement;
    if (current === 'system') html.removeAttribute('data-theme');
    else html.setAttribute('data-theme', current);
    for (const meta of root.document.querySelectorAll('meta[name="theme-color"]')) {
      const own = /dark/.test(meta.getAttribute('media') || '') ? COLORS.dark : COLORS.light;
      meta.setAttribute('content', current === 'system' ? own : COLORS[current]);
    }
  }

  function set(choice) {
    apply(choice);
    try {
      if (current === 'system') root.localStorage.removeItem(KEY);
      else root.localStorage.setItem(KEY, current);
    } catch (e) {}
    tell();
  }

  function felix() {
    try {
      return root.localStorage.getItem(FELIX_KEY) === 'demon' ? 'demon' : 'cute';
    } catch (e) {
      return 'cute';
    }
  }

  function showFelix(kind) {
    const html = root.document.documentElement;
    if (kind === 'demon') html.setAttribute('data-felix', 'demon');
    else html.removeAttribute('data-felix');
  }

  function setFelix(kind) {
    const demon = kind === 'demon';
    showFelix(demon ? 'demon' : 'cute');
    try {
      if (demon) root.localStorage.setItem(FELIX_KEY, 'demon');
      else root.localStorage.removeItem(FELIX_KEY);
    } catch (e) {}
  }

  function noteWidth() {
    try {
      const value = root.localStorage.getItem(WIDTH_KEY);
      return WIDTHS.includes(value) ? value : 'wide';
    } catch (e) {
      return 'wide';
    }
  }

  function showWidth(width) {
    root.document.documentElement.setAttribute('data-note-width', WIDTHS.includes(width) ? width : 'wide');
  }

  function setNoteWidth(width) {
    const chosen = WIDTHS.includes(width) ? width : 'wide';
    showWidth(chosen);
    try {
      if (chosen === 'wide') root.localStorage.removeItem(WIDTH_KEY);
      else root.localStorage.setItem(WIDTH_KEY, chosen);
    } catch (e) {}
  }

  function onChange(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  }

  apply(stored());
  showFelix(felix());
  showWidth(noteWidth());
  if (media && media.addEventListener) {
    media.addEventListener('change', () => {
      if (current === 'system') tell();
    });
  }
  root.addEventListener('storage', (e) => {
    if (e.key === FELIX_KEY || e.key === null) showFelix(felix());
    if (e.key === WIDTH_KEY || e.key === null) showWidth(noteWidth());
    if (e.key !== KEY && e.key !== null) return;
    const was = dark();
    apply(stored());
    if (dark() !== was) tell();
  });
  if (root.document.readyState === 'loading') {
    root.document.addEventListener('DOMContentLoaded', () => apply(current), { once: true });
  }

  root.leoTheme = { KEY, CHOICES, choice: () => current, dark, set, onChange, felix, setFelix, WIDTHS, noteWidth, setNoteWidth };
})(window);
