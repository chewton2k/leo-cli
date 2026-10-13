(function (root) {
  'use strict';
  const MOST_KEYS = 1000;
  const MOST_JSON = 8 * 1024 * 1024;
  const INTERVIEW = 'leo-interview-drafts';
  const NOTE = 'leo-draft-v1:';
  const NAMES = { 'leo-chat-v1': 'Cached Felix conversation', 'leo-felix-access': 'Felix permissions', 'leo-chat-sidebar': 'Chat sidebar', 'leo-chat-width': 'Chat panel width', 'leo-library': 'Notes view, sort and filters', 'leo-calendar-hide-done': 'Calendar completion filter', 'leo-tasks-folded': 'Tasks panel preference', 'leo-source-width': 'Source panel width', 'leo-side': 'Sidebar preference', 'leo-side-open': 'Expanded folders' };
  function object(text) {
    if (!text || text.length > MOST_JSON) return null;
    try { const value = JSON.parse(text); return value && typeof value === 'object' && !Array.isArray(value) ? value : null; } catch (_) { return null; }
  }
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
      if (!key || !key.startsWith('leo-')) continue;
      const text = storage.getItem(key) || '';
      if (key === INTERVIEW) {
        const drafts = object(text);
        if (drafts) {
          for (const [ref, value] of Object.entries(drafts).slice(0, 8)) {
            const [practice, problem] = ref.split(':');
            const bytes = (ref.length + JSON.stringify(value).length) * 2;
            items.push({ id: `interview@${ref}`, label: 'Interview draft backup', detail: `Practice ${practice} · problem ${problem || 'unknown'} · unsaved code, answers and drawings`, bytes, practice, locked: false });
          }
          continue;
        }
      }
      const note = key.startsWith(NOTE) ? key.slice(NOTE.length) : null;
      items.push({ id: key, label: note ? 'Note draft backup' : key === INTERVIEW ? 'Unreadable interview draft backup' : NAMES[key] || key, detail: note ? `Unsaved changes to note ${note}` : key === 'leo-chat-v1' ? 'A browser copy; saved conversations stay on the Leo server' : 'A preference or cache in this browser', bytes: (key.length + text.length) * 2, note, locked: false });
    }
    return items.map(item => ({ ...item, size: size(item.bytes) }));
  }
  function remove(storage, ids) {
    if (!Array.isArray(ids) || ids.length > MOST_KEYS + 8) throw new Error('Choose browser data shown in Storage.');
    const known = new Set(list(storage).map(item => item.id));
    if (ids.some(id => !known.has(id))) throw new Error('This browser data changed. Refresh Storage and choose it again.');
    const drafts = object(storage.getItem(INTERVIEW));
    let changed = false;
    for (const id of new Set(ids)) {
      if (id.startsWith('interview@')) { if (drafts) { delete drafts[id.slice(10)]; changed = true; } }
      else storage.removeItem(id);
    }
    if (changed) {
      if (Object.keys(drafts).length) storage.setItem(INTERVIEW, JSON.stringify(drafts));
      else storage.removeItem(INTERVIEW);
    }
  }
  function forgetPractices(storage, ids) {
    const drafts = object(storage.getItem(INTERVIEW));
    if (!drafts) return;
    const known = new Set(ids);
    for (const key of Object.keys(drafts).slice(0, 8)) if (known.has(key.split(':')[0])) delete drafts[key];
    if (Object.keys(drafts).length) storage.setItem(INTERVIEW, JSON.stringify(drafts));
    else storage.removeItem(INTERVIEW);
  }
  root.leoBrowserStorage = { list, remove, forgetPractices, size };
})(typeof window !== 'undefined' ? window : globalThis);
