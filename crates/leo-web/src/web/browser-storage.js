(function (root) {
  'use strict';
  const MOST_KEYS = 1000;
  const NOTE = 'leo-draft-v1:';
  const NAMES = { 'leo-chat-v1': 'Cached Felix conversation', 'leo-felix-access': 'Felix permissions', 'leo-chat-sidebar': 'Chat sidebar', 'leo-chat-width': 'Chat panel width', 'leo-library': 'Notes view, sort and filters', 'leo-calendar-hide-done': 'Calendar completion filter', 'leo-tasks-folded': 'Tasks panel preference', 'leo-source-width': 'Source panel width', 'leo-side': 'Sidebar preference', 'leo-side-open': 'Expanded folders (old)', 'leo-side-folders': 'Expanded folders', 'leo-last-place': 'Where you left off' };
  function size(bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) return 'Unknown';
    if (bytes < 1024) return `${bytes} bytes`;
    const units = ['KB', 'MB', 'GB'];
    let value = bytes / 1024, unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
    return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
  }
  function list(storage) {
    const items = [];
    for (let i = 0; i < Math.min(storage.length, MOST_KEYS); i++) {
      const key = storage.key(i);
      if (!key || !key.startsWith('leo-') || key.startsWith('leo-interview-')) continue;
      const text = storage.getItem(key) || '';
      const note = key.startsWith(NOTE) ? key.slice(NOTE.length) : null;
      items.push({ id: key, label: note ? 'Note draft backup' : NAMES[key] || key, detail: note ? `Unsaved changes to note ${note}` : key === 'leo-chat-v1' ? 'A browser copy; saved conversations stay on the Leo server' : 'A preference or cache in this browser', bytes: (key.length + text.length) * 2, note, locked: false });
    }
    return items.map(item => ({ ...item, size: size(item.bytes) }));
  }
  function remove(storage, ids) {
    if (!Array.isArray(ids) || ids.length > MOST_KEYS + 8) throw new Error('Choose browser data shown in Storage.');
    const known = new Set(list(storage).map(item => item.id));
    if (ids.some(id => !known.has(id))) throw new Error('This browser data changed. Refresh Storage and choose it again.');
    for (const id of new Set(ids)) storage.removeItem(id);
  }
  root.leoBrowserStorage = { list, remove, size };
})(typeof window !== 'undefined' ? window : globalThis);
