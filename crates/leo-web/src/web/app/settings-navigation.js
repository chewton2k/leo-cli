const SETTINGS_SECTIONS = [
  { group: 'Leo', items: [['general', 'General'], ['dictation', 'Dictation'], ['models', 'Speech models'], ['writing', 'Writing AI']] },
  { group: 'Your notes', items: [['calendar', 'Calendar'], ['backup', 'Backup'], ['storage', 'Storage']] },
  { group: 'This app', items: [['health', 'Health'], ['updates', 'Updates & about']] },
];
function settingsSection(section) {
  if (!section) { try { section = localStorage.getItem('leo-settings-section') || 'general'; } catch (e) { section = 'general'; } }
  return SETTINGS_SECTIONS.some((group) => group.items.some(([id]) => id === section)) ? section : 'general';
}
function settingsTitle(section) {
  return SETTINGS_SECTIONS.flatMap((group) => group.items).find(([id]) => id === section)?.[1] || 'General';
}
function settingsFrame(section, content) {
  return `<div class="settings-layout"><nav class="settings-nav" aria-label="Settings sections">${SETTINGS_SECTIONS.map((group) => `<div class="settings-group"><div class="settings-group-title">${esc(group.group)}</div><div class="library-views">${group.items.map(([id, label]) => `<a class="btn plain${id === section ? ' on' : ''}" href="#/settings/${id}"${id === section ? ' aria-current="page"' : ''}>${esc(label)}</a>`).join('')}</div></div>`).join('')}</nav><div class="settings-section" data-settings-section="${esc(section)}">${content}</div></div>`;
}
function keepSettingsFields() {
  return Array.from(app.querySelectorAll('input:not([type="checkbox"]):not([type="file"]),textarea,[data-dictation]')).map((field) => {
    const selector = field.id ? '#' + CSS.escape(field.id) : field.dataset.dictation ? `[data-dictation="${CSS.escape(field.dataset.dictation)}"]` : field.dataset.keyInput ? `[data-key-input="${CSS.escape(field.dataset.keyInput)}"]` : null;
    return { selector, value: field.value, checked: field.checked, start: field.selectionStart, end: field.selectionEnd, active: field === document.activeElement };
  }).filter((field) => field.selector);
}
function restoreSettingsFields(fields) {
  for (const saved of fields) {
    const field = app.querySelector(saved.selector);
    if (!field) continue;
    field.value = saved.value;
    if (field.type === 'checkbox') field.checked = saved.checked;
    if (saved.active) { field.focus({ preventScroll: true }); if (saved.start !== null && field.setSelectionRange && /^(text|textarea|search|password|url|tel)$/.test(field.type || 'text')) field.setSelectionRange(saved.start, saved.end); }
  }
}
