// Settings sections, grouped as the sidebar shows them: [id, label, icon, description].
const SETTINGS_SECTIONS = [
  { group: 'Leo', items: [
    ['general', 'General', 'sliders', 'Appearance, and how Leo starts with your computer.'],
    ['dictation', 'Dictation', 'keyboard', 'Speak into any app with a shortcut. Speech stays on this computer.'],
    ['models', 'Speech models', 'wave', 'The local model that turns speech into text.'],
    ['writing', 'Writing AI', 'sparkle', 'The AI that writes your notes and powers Felix.'],
  ] },
  { group: 'Your notes', items: [
    ['calendar', 'Calendar', 'calendar', 'Name recordings after the class or meeting you are in.'],
    ['backup', 'Backup', 'backup', 'Keep a private copy of your notes on GitHub.'],
  ] },
  { group: 'Data', items: [
    ['storage', 'Storage', 'storage', 'What Leo keeps on this computer, and how long.'],
    ['trash', 'Trash', 'trash', 'Deleted notes wait here, so a mistake can be undone.'],
  ] },
  { group: 'This app', items: [
    ['health', 'Health', 'pulse', 'Check that recording, dictation and AI are ready.'],
    ['updates', 'Updates & about', 'update', 'Keep Leo up to date.'],
  ] },
];
const SETTINGS_ICON_PATHS = {
  sliders: '<path d="M4 6h10M4 12h4M12 12h8M4 18h12"/><circle cx="16.5" cy="6" r="2.2"/><circle cx="10" cy="12" r="2.2"/><circle cx="18.5" cy="18" r="2.2"/>',
  keyboard: '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M7.5 14h9"/>',
  wave: '<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3"/>',
  sparkle: '<path d="M11 3.5l1.7 4.8 4.8 1.7-4.8 1.7L11 16.5l-1.7-4.8L4.5 10l4.8-1.7z"/><path d="M18 14.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z"/>',
  backup: '<path d="M7 18.5a4.5 4.5 0 0 1-.5-9 6 6 0 0 1 11.3 1.5 3.8 3.8 0 0 1-.3 7.5"/><path d="M12 12.5v7M9 15.5l3-3 3 3"/>',
  pulse: '<path d="M3 12h4l2.5-6 5 12 2.5-6h4"/>',
  update: '<circle cx="12" cy="12" r="9"/><path d="M12 7.5v8M8.5 12.5l3.5 3.5 3.5-3.5"/>',
};
const settingsItems = () => SETTINGS_SECTIONS.flatMap((group) => group.items);
function settingsIcon(name) {
  return SETTINGS_ICON_PATHS[name] ? svg(SETTINGS_ICON_PATHS[name]) : ICON[name] || '';
}
function settingsSection(section) {
  if (!section) { try { section = localStorage.getItem('leo-settings-section') || 'general'; } catch (e) { section = 'general'; } }
  return settingsItems().some(([id]) => id === section) ? section : 'general';
}
function settingsTitle(section) {
  return settingsItems().find(([id]) => id === section)?.[1] || 'General';
}
function settingsFrame(section, content) {
  const [, title, , about] = settingsItems().find(([id]) => id === section) || settingsItems()[0];
  const nav = SETTINGS_SECTIONS.map((group) => `<div class="settings-group"><div class="settings-group-title">${esc(group.group)}</div>${group.items.map(([id, label, icon]) => `<a class="settings-link${id === section ? ' on' : ''}" href="#/settings/${id}"${id === section ? ' aria-current="page"' : ''}>${settingsIcon(icon)}<span>${esc(label)}</span></a>`).join('')}</div>`).join('');
  // Callers assign the frame to the page right away; once they have, bring the current
  // section into view in the phone strip, where the list scrolls sideways.
  if (typeof queueMicrotask === 'function') queueMicrotask(revealSettingsLink);
  return `<div class="settings-layout"><nav class="settings-nav" aria-label="Settings sections"><div class="settings-nav-title">Settings</div>${nav}</nav><div class="settings-section" data-settings-section="${esc(section)}"><header class="settings-head"><h1>${esc(title)}</h1><p>${esc(about)}</p></header>${content}</div></div>`;
}

function revealSettingsLink() {
  const nav = app.querySelector('.settings-nav'), on = nav?.querySelector('.settings-link.on');
  if (on && nav.scrollWidth > nav.clientWidth) nav.scrollLeft = on.offsetLeft - (nav.clientWidth - on.offsetWidth) / 2;
}

// An iOS-style switch. It stays a real checkbox, so change handlers, forms and tests keep working.
function settingsSwitch(attrs, on, label = '') {
  return `<input type="checkbox" class="switch" role="switch"${label ? ` aria-label="${esc(label)}"` : ''} ${attrs}${on ? ' checked' : ''}>`;
}

// A segmented control whose highlight slides to the picked option. Two kinds:
// - `field`: a form value. A hidden input carries `field` attributes and the value; picking one
//   updates it and fires `change` on it, like the <select> it replaces.
// - `action`: each option is a data-action button (the caller's handler stores the choice).
function segmented({ label, choices, current, field = '', action = '', key = '' }) {
  const options = choices.map(([value, text, icon = '']) => {
    const on = String(value) === String(current);
    const what = action ? ` data-action="${action}" data-${key}="${esc(value)}" aria-pressed="${on}"` : ` data-action="segment-pick" data-value="${esc(value)}" role="radio" aria-checked="${on}" tabindex="${on ? 0 : -1}"`;
    return `<button type="button" class="segment${on ? ' on' : ''}"${what} title="${esc(text)}">${icon}<span>${esc(text)}</span></button>`;
  }).join('');
  const index = Math.max(0, choices.findIndex(([value]) => String(value) === String(current)));
  const hidden = field ? `<input type="hidden" ${field} value="${esc(current ?? '')}">` : '';
  return `<div class="segmented" role="${action ? 'group' : 'radiogroup'}" aria-label="${esc(label)}" style="--count:${choices.length};--at:${index}">${hidden}<span class="segment-pill" aria-hidden="true"></span>${options}</div>`;
}
// Move the highlight to whichever option is on. `from` lets a redraw slide from where it was.
function syncSegmented(box, from = null) {
  const options = [...box.querySelectorAll('.segment')];
  const field = box.querySelector('input[type="hidden"]');
  if (field) for (const option of options) {
    const on = option.dataset.value === field.value;
    option.classList.toggle('on', on);
    option.setAttribute('aria-checked', String(on));
    option.tabIndex = on ? 0 : -1;
  }
  const at = Math.max(0, options.findIndex((option) => option.classList.contains('on')));
  const pill = box.querySelector('.segment-pill');
  if (from !== null && from !== at && pill) {
    pill.style.transition = 'none';
    box.style.setProperty('--at', from);
    void pill.offsetWidth;
    pill.style.transition = '';
  }
  box.style.setProperty('--at', at);
}
function pickSegment(el) {
  const box = el.closest('.segmented');
  const field = box?.querySelector('input[type="hidden"]');
  if (!field || field.value === el.dataset.value) return;
  field.value = el.dataset.value;
  syncSegmented(box);
  field.dispatchEvent(new Event('change', { bubbles: true }));
}
// Arrow keys move between options, as in a native radio group.
document.addEventListener('keydown', (e) => {
  const option = e.target.closest?.('.segment[role="radio"]');
  if (!option || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
  const options = [...option.parentElement.querySelectorAll('.segment')];
  const at = options.indexOf(option);
  const next = e.key === 'Home' ? 0 : e.key === 'End' ? options.length - 1 : (at + (/Left|Up/.test(e.key) ? -1 : 1) + options.length) % options.length;
  e.preventDefault();
  options[next].focus();
  pickSegment(options[next]);
});

function keepSettingsFields() {
  return Array.from(app.querySelectorAll('input:not([type="checkbox"]):not([type="file"]),textarea,[data-dictation]')).map((field) => {
    const selector = field.id ? '#' + CSS.escape(field.id) : field.dataset.dictation ? `[data-dictation="${CSS.escape(field.dataset.dictation)}"]` : field.dataset.keyInput ? `[data-key-input="${CSS.escape(field.dataset.keyInput)}"]` : null;
    return { selector, value: field.value, checked: field.checked, start: field.selectionStart, end: field.selectionEnd, active: field === document.activeElement };
  }).filter((field) => field.selector);
}
function keepSegmented() {
  return new Map(Array.from(app.querySelectorAll('.segmented[aria-label]')).map((box) => [box.getAttribute('aria-label'), Number(box.style.getPropertyValue('--at')) || 0]));
}
function restoreSettingsFields(fields, segments = new Map()) {
  for (const saved of fields) {
    const field = app.querySelector(saved.selector);
    if (!field) continue;
    field.value = saved.value;
    if (field.type === 'checkbox') field.checked = saved.checked;
    if (saved.active) { field.focus({ preventScroll: true }); if (saved.start !== null && field.setSelectionRange && /^(text|textarea|search|password|url|tel)$/.test(field.type || 'text')) field.setSelectionRange(saved.start, saved.end); }
  }
  for (const box of app.querySelectorAll('.segmented')) syncSegmented(box, segments.get(box.getAttribute('aria-label')) ?? null);
}
