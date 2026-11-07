const plural = (n, word, many = `${word}s`) => `${n} ${n === 1 ? word : many}`;
const md = window.leoMarkdown;
const leoDoc = window.leoDoc;
const felix = window.leoChat;
const esc = md.escape;
const enc = encodeURIComponent;
const $ = (selector, root = document) => root.querySelector(selector);
const app = $('#app');
const floating = $('#floating');

const svg = (paths) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const ON_MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const FOLDER = '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>';
const ICON = {
  back: svg('<path d="M15 18l-6-6 6-6"/>'),
  pointer: svg('<path d="M6 3l12 9-5.5 1.2L15 20l-2.4 1-2.6-6.6L6 18z"/>'),
  pen: svg('<path d="M4 20l1.2-4.6L16.6 4a2 2 0 012.8 2.8L8 18.4z"/><path d="M14.5 6.1l3.4 3.4"/>'),
  box: svg('<rect x="4" y="6" width="16" height="12" rx="2"/>'),
  circle: svg('<circle cx="12" cy="12" r="7.5"/>'),
  arrow: svg('<path d="M5 19L19 5"/><path d="M10 5h9v9"/>'),
  line: svg('<path d="M5 19L19 5"/>'),
  text: svg('<path d="M5 7V5h14v2"/><path d="M12 5v14"/><path d="M9 19h6"/>'),
  undo: svg('<path d="M9 14L4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 010 11H11"/>'),
  redo: svg('<path d="M15 14l5-5-5-5"/><path d="M20 9H9.5a5.5 5.5 0 000 11H13"/>'),
  search: svg('<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>'),
  grid: svg('<rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><rect x="14" y="14" width="6" height="6" rx="1"/>'),
  list: svg('<path d="M9 6h11M9 12h11M9 18h11M4 6h.1M4 12h.1M4 18h.1"/>'),
  sort: svg('<path d="M4 6h16M4 12h11M4 18h6"/>'),
  bolt: svg('<path d="M13.5 2.5L5 13.5h6l-1.5 8 8.5-11h-6z"/>'),
  filter: svg('<path d="M4 5h16l-6.2 7.6V18l-3.6 2v-7.4z"/>'),
  close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
  more: svg('<circle cx="5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="19" cy="12" r="1.3"/>'),
  folder: svg(FOLDER),
  folderPlus: svg(FOLDER + '<path d="M12 10.5v6M9 13.5h6"/>'),
  move: svg(FOLDER + '<path d="M9.5 13.5h6M13 11l2.5 2.5L13 16"/>'),
  pin: svg('<path d="M12 16v6"/><path d="M8 3h8l-1.2 6.2L18 12.5V15H6v-2.5l3.2-3.3z"/>'),
  trash: svg('<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/>'),
  share: svg('<path d="M12 3v12M8 7l4-4 4 4"/><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"/>'),
  plus: svg('<path d="M12 5v14M5 12h14"/>'),
  minus: svg('<path d="M5 12h14"/>'),
  download: svg('<path d="M12 4v12M7 11l5 5 5-5M4 18v3h16v-3"/>'),
  upload: svg('<path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3"/>'),
  image: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/>'),
  paperclip: svg('<path d="M21 11l-8.5 8.5a5 5 0 0 1-7-7L14 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 7"/>'),
  gear: svg('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"/>'),
  board: svg('<rect x="3.5" y="4" width="4.6" height="15" rx="1.3"/><rect x="9.7" y="4" width="4.6" height="9.5" rx="1.3"/><rect x="15.9" y="4" width="4.6" height="12" rx="1.3"/>'),
  calendar: svg('<rect x="4" y="5" width="16" height="15" rx="2"/><path d="M4 10h16M9 3v4M15 3v4"/>'),
  call: svg('<path d="M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2"/>'),
  tidy: svg('<circle cx="6" cy="6" r="2"/><circle cx="18" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M8 6h8M6 8v8M18 8v8M8 18h8"/>'),
  fit: svg('<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>'),
  restore: svg('<path d="M4 12a8 8 0 1 0 2.3-5.6L4 8.5"/><path d="M4 4v4.5h4.5"/>'),
  filePlus: svg('<path d="M6 3h9l4 4v14H6z"/><path d="M12 10v7M8.5 13.5h7"/>'),
  note: svg('<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>'),
  refresh: svg('<path d="M20 12a8 8 0 1 1-2.3-5.6L20 8.5"/><path d="M20 4v4.5h-4.5"/>'),
  check: svg('<path d="M5 12l4.5 4.5L19 7"/>'),
  sun: svg('<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8"/>'),
  moon: svg('<path d="M20 14.5A8 8 0 019.5 4a8 8 0 1010.5 10.5z"/>'),
  device: svg('<rect x="3" y="4" width="13" height="10" rx="1.5"/><path d="M1.5 18h16"/><rect x="17.5" y="9" width="5" height="11" rx="1.2"/>'),
  chevron: svg('<path d="M9 6l6 6-6 6"/>'),
  lock: svg('<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>'),
  map: svg('<circle cx="6" cy="7" r="2.2"/><circle cx="18" cy="6" r="2.2"/><circle cx="12" cy="17.5" r="2.2"/><path d="M7.4 8.9l3.5 6.7M16.9 7.9l-3.8 7.8M8.2 6.8l7.6-.6"/>'),
  access: svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="7.2" r="1.3"/><path d="M7.5 10.2l4.5 1 4.5-1M12 11.2v3.6l-2.2 3.7M12 14.8l2.2 3.7"/>'),
  mic: svg('<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>'),
  screen: svg('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>'),
  sidebar: svg('<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M9 4v16"/>'),
  chat: svg('<path d="M5 18.5V7a2.5 2.5 0 0 1 2.5-2.5h9A2.5 2.5 0 0 1 19 7v6.5a2.5 2.5 0 0 1-2.5 2.5H9z"/><path d="M9 9.5h6M9 12.5h4"/>'),
  storage: svg('<ellipse cx="12" cy="6" rx="7.5" ry="2.8"/><path d="M4.5 6v6c0 1.5 3.4 2.8 7.5 2.8s7.5-1.3 7.5-2.8V6"/><path d="M4.5 12v6c0 1.5 3.4 2.8 7.5 2.8s7.5-1.3 7.5-2.8v-6"/>'),
  cloud: svg('<path d="M7 18a4.5 4.5 0 0 1-.5-9 6 6 0 0 1 11.3 1.5A3.8 3.8 0 0 1 17.5 18z"/><path d="M4 4l16 16"/>'),
};

const recorder = window.leoRecording.create({ encodePicture: async (file) => base64((await pictureFor(file)).blob), api, esc, toast, go: (hash, opts) => go(hash, opts), noteHash: (id) => noteHash(id), felix, icons: ICON, noteFields: (options) => noteFields(options), noteReady: () => showLatest().catch(() => {}) });
const chat = felix.create({
  render: (text) => md.render(text),
  escape: md.escape,
  icons: ICON,
  onOpen: (id, lines) => {
    citedLines = lines ? { id, ...lines } : null;
    if (state.view === 'note' && state.session && state.session.note.id === id && lines) showCitedLines();
    else go(noteHash(id));
  },
  onCite: (id, lines) => {
    const open = state.view === 'note' ? state.session : null;
    if (open && open.doc) open.doc.glow(...(id && lines && open.note.id === id ? [lines.from, lines.to] : []));
  },
  onOriginal: (note, name, at) => showPassage(note, name, at).catch(fail),
  prepare: async (file) => {
    const ready = await shrink(file);
    return { name: ready.name, type: ready.type, data: await base64(ready.blob) };
  },
  notify: (message) => toast(message, { bad: true }),
  onUndone: (note, removed) => {
    const open = state.view === 'note' && state.session && state.session.note.id === note.id;
    if (open && removed) go(folderHash(state.dir || ''), { replace: true });
    else if (open && !saving.unsaved()) showNote(note.id);
    else showLatest().catch(() => {});
    toast(removed ? `Moved “${note.title}” to the trash` : `Undid the change to “${note.title}”`);
  },
  onChanged: (note) => {
    const open = state.view === 'note' && state.session && state.session.note.id === note.id;
    if (open && !saving.unsaved()) showNote(note.id);
    toast(open && saving.unsaved() ? 'Felix changed this note while you were typing; reopen it to see the change.' : `Changed “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
  },
  onToggle: () => { drawSide(); pageBarLater(); marginRedraw(); },
  onOrganized: (proposal) => {
    if (MARGIN_KINDS.includes(proposal.kind)) {
      if (state.view === 'note' && state.session && state.session.note.id === proposal.note) marginLoad(state.session).catch(fail);
      return;
    }
    if (BOARD_KINDS.includes(proposal.kind)) {
      if (state.view === 'boards') showBoards((kanban.board && kanban.board.id) || '').catch(fail);
      return;
    }
    loadSideFolders().catch(() => {});
    if (state.view === 'calendar') loadSchedule(seq);
    loadTasksWindow().catch(() => {});
    if (state.view === 'note' && state.session && state.session.note.id && !saving.unsaved()) showNote(state.session.note.id).catch(() => go(folderHash(state.dir || ''), { replace: true }));
    else if (state.view !== 'calendar') showLatest().catch(() => {});
  },
  onAdd: (picked) => {
    if (!window.matchMedia('(min-width: 900px)').matches) chat.toggle(false);
    addSheet(picked);
  },
  onSaved: (note) => {
    showLatest().catch(() => {});
    toast(`Saved as a note${note.directory ? ` in ${note.directory}` : ''}`, { action: 'Open', run: () => go(noteHash(note.id)) });
  },
});
$('#chat-toggle').innerHTML = chat.button(36);
$('#back').innerHTML = ICON.back;
$('#search-toggle').innerHTML = ICON.search;
$('#menu').innerHTML = ICON.more;
$('#search-icon').innerHTML = ICON.search;

const pageBar = document.createElement('div');
pageBar.className = 'page-bar';
pageBar.hidden = true;
pageBar.dataset.keepSelection = '';
pageBar.setAttribute('aria-hidden', 'true');
pageBar.innerHTML = '<div class="page-bar-thumb"></div>';
document.body.appendChild(pageBar);
const pageThumb = pageBar.firstChild;
let pageBarFrame = 0;
let pageBarSize = { track: 0, thumb: 0, top: 0, room: 0 };

function pageBarRight() {
  const panels = [document.querySelector('aside.chat'), document.querySelector('.source-pane')]
    .filter(panel => panel && !panel.hidden && panel.getClientRects().length);
  if (!panels.length) return 0;
  const left = Math.min(...panels.map(panel => panel.offsetLeft));
  return left < 240 ? 0 : Math.max(0, document.documentElement.clientWidth - left);
}

function drawPageBar() {
  pageBarFrame = 0;
  const right = pageBarRight();
  const root = document.documentElement;
  root.classList.toggle('page-bar-on', right > 0);
  const room = root.scrollHeight - innerHeight;
  if (!right || room < 1) {
    pageBar.hidden = true;
    return;
  }
  pageBar.style.right = `${right}px`;
  pageBar.hidden = false;
  const track = pageBar.clientHeight;
  const thumb = Math.min(track, Math.max(36, track * innerHeight / root.scrollHeight));
  const top = (track - thumb) * Math.min(1, Math.max(0, scrollY / room));
  pageThumb.style.height = `${thumb}px`;
  pageThumb.style.transform = `translateY(${top}px)`;
  pageBarSize = { track, thumb, top, room };
}

function pageBarLater() {
  if (!pageBarFrame) pageBarFrame = requestAnimationFrame(drawPageBar);
}

pageThumb.addEventListener('pointerdown', (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  pageThumb.setPointerCapture(e.pointerId);
  pageBar.classList.add('dragging');
  const startY = e.clientY;
  const startScroll = scrollY;
  const { track, thumb, room } = pageBarSize;
  const move = (m) => {
    if (!(m.buttons & 1)) return stop();
    scrollTo(0, startScroll + (m.clientY - startY) * room / Math.max(1, track - thumb));
  };
  const stop = () => {
    pageBar.classList.remove('dragging');
    pageThumb.removeEventListener('pointermove', move);
    pageThumb.removeEventListener('pointerup', stop);
    pageThumb.removeEventListener('pointercancel', stop);
    pageThumb.removeEventListener('lostpointercapture', stop);
  };
  pageThumb.addEventListener('pointermove', move);
  pageThumb.addEventListener('pointerup', stop);
  pageThumb.addEventListener('pointercancel', stop);
  pageThumb.addEventListener('lostpointercapture', stop);
});
pageBar.addEventListener('pointerdown', (e) => {
  if (e.target !== pageBar || e.button !== 0) return;
  const y = e.clientY - pageBar.getBoundingClientRect().top;
  scrollBy(0, (y < pageBarSize.top ? -1 : 1) * innerHeight * 0.9);
});
let pageBarIdle = 0;
addEventListener('scroll', () => {
  pageBarLater();
  pageBar.classList.add('active');
  clearTimeout(pageBarIdle);
  pageBarIdle = setTimeout(() => pageBar.classList.remove('active'), 900);
}, { passive: true });
addEventListener('resize', pageBarLater);
if (window.ResizeObserver) new ResizeObserver(pageBarLater).observe(document.body);
new MutationObserver(pageBarLater).observe(document.body, { attributes: true, attributeFilter: ['class'] });
new MutationObserver(pageBarLater).observe(document.documentElement, { attributes: true, attributeFilter: ['style'] });
document.addEventListener('animationend', pageBarLater, true);

class Offline extends Error {}
class Locked extends Error {}

async function api(path, { method = 'GET', body, total = false, signal } = {}) {
  let response;
  try {
    response = await fetch(path, {
      method,
      credentials: 'same-origin',
      signal,
      headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (e) {
    throw new Offline();
  }
  if (response.status === 401) throw new Locked();
  if (response.status === 204) return null;
  if (!response.ok) {
    let said = '';
    let data = null;
    try {
      data = await response.json();
      said = (data && data.error) || '';
    } catch (e) {
      said = '';
    }
    const error = new Error(said || (response.status === 404 ? 'That is not there any more.' : response.status === 400 ? 'Use a folder name inside your notes, without . or ..' : `leo answered ${response.status}.`));
    error.status = response.status;
    error.data = data;
    throw error;
  }
  const text = await response.text();
  const data = text.trim() ? JSON.parse(text) : null;
  if (total) return { items: data || [], total: Number(response.headers.get('x-total')) || 0 };
  return data;
}

function fail(error) {
  if (error instanceof Locked) return showLocked();
  if (error instanceof Offline) return toast("Can't reach leo. Quit leo and open it again.", { bad: true });
  toast(error.message || String(error), { bad: true });
}

let toastTimer;
function toast(message, { action, run, bad } = {}) {
  clearTimeout(toastTimer);
  const old = $('.toast');
  if (old) old.remove();
  const el = document.createElement('div');
  el.className = 'toast' + (bad ? ' bad' : '');
  el.setAttribute('role', 'status');
  const lasts = action ? 10000 : 3500;
  el.innerHTML = `<span>${esc(message)}</span>${action ? `<button>${esc(action)}</button><i class="toast-time" aria-hidden="true" style="animation-duration:${lasts}ms"></i>` : ''}`;
  if (action) {
    el.querySelector('button').addEventListener('click', () => {
      el.remove();
      run();
    });
  }
  document.body.appendChild(el);
  toastTimer = setTimeout(() => {
    el.classList.replace('toast', 'toast-gone');
    el.setAttribute('aria-hidden', 'true');
    el.inert = true;
    setTimeout(() => el.remove(), LEAVE_MS);
  }, lasts);
}

const LEAVE_MS = 160;
const MOTION_MS = 180;
const OPEN_MS = 200;
const DRAWER_MS = 240;
const PULSE_MS = 320;
const ICON_MS = 140;
const EXIT_SHARE = .8;
const POP_FROM = .95;
const POP_OPEN = 'inset(-24px)';
const CASCADE_MS = 30 * 5 + MOTION_MS + 30;
const EASE_OUT = 'cubic-bezier(.2, .8, .2, 1)';
const EASE_MOVE = 'cubic-bezier(.45, 0, .15, 1)';
const EASE_EXIT = 'cubic-bezier(.4, 0, 1, 1)';

function stillMotion() {
  return matchMedia('(prefers-reduced-motion: reduce)').matches;
}

const moving = new WeakMap();
const popEdges = new WeakMap();

function stopSlide(el) {
  delete el.dataset.folding;
  const run = moving.get(el);
  if (!run) return;
  moving.delete(el);
  run.onfinish = null;
  run.oncancel = null;
  run.cancel();
}

function pulse(el, { lift = 1.04, ring = false } = {}) {
  if (!el || stillMotion() || typeof el.animate !== 'function') return null;
  const frames = ring
    ? { boxShadow: ['0 0 0 0 transparent', '0 0 0 3px color-mix(in srgb, var(--accent) 45%, transparent)', '0 0 0 5px transparent'] }
    : { scale: [1, lift, 1] };
  return el.animate(frames, { duration: PULSE_MS, easing: EASE_MOVE });
}

function cascade(el, kind = 'rows-in') {
  if (!el || stillMotion()) return;
  el.classList.add(kind);
  setTimeout(() => el.classList.remove(kind), CASCADE_MS);
}

function rubber(over, reach = 24) {
  return Math.sign(over) * reach * (1 - 1 / (1 + Math.abs(over) / reach));
}

const pastEdge = (at, low, high) => (stillMotion() ? at : at < low ? low + rubber(at - low) : at > high ? high + rubber(at - high) : at);

function iconSwap(host, change, part = 'svg') {
  const old = host?.querySelector(part);
  if (!old || stillMotion() || typeof old.animate !== 'function') return change();
  const box = old.getBoundingClientRect();
  const ghost = old.cloneNode(true);
  ghost.setAttribute('aria-hidden', 'true');
  Object.assign(ghost.style, { position: 'fixed', zIndex: 1000, left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px`, margin: 0, pointerEvents: 'none', color: getComputedStyle(old).color });
  document.body.appendChild(ghost);
  const result = change();
  const out = { opacity: [1, 0], filter: ['blur(0)', 'blur(2px)'], scale: [1, .85] };
  ghost.animate(out, { duration: ICON_MS * EXIT_SHARE, easing: EASE_EXIT, fill: 'forwards' }).onfinish = () => ghost.remove();
  host.querySelector(part)?.animate({ opacity: [0, 1], filter: ['blur(2px)', 'blur(0)'], scale: [.85, 1] }, { duration: ICON_MS, easing: EASE_OUT });
  return result;
}

function popEdge(menu, from) {
  const box = menu.getBoundingClientRect();
  const at = from.getBoundingClientRect ? from.getBoundingClientRect() : { left: from.x, right: from.x, top: from.y, bottom: from.y };
  const x = Math.max(0, Math.min(box.width, (at.left + at.right) / 2 - box.left));
  const y = Math.max(0, Math.min(box.height, (at.top + at.bottom) / 2 - box.top));
  return { origin: `${x}px ${y}px`, up: y > box.height / 2 };
}

const popShut = (edge) => (edge.up ? 'inset(100% 0 0 0)' : 'inset(0 0 100% 0)');

function unfold(menu, from = null) {
  stopSlide(menu);
  menu.hidden = false;
  if (stillMotion()) return;
  const edge = from ? popEdge(menu, from) : { origin: '50% 0', up: false };
  popEdges.set(menu, edge);
  menu.style.transformOrigin = edge.origin;
  const run = menu.animate([{ clipPath: popShut(edge), transform: `scale(${POP_FROM})` }, { clipPath: POP_OPEN, transform: 'none' }], { duration: MOTION_MS, easing: EASE_OUT });
  moving.set(menu, run);
  run.onfinish = () => moving.delete(menu);
}

function fold(menu, then = null) {
  if (menu.hidden || menu.dataset.folding) {
    if (then && menu.hidden) then();
    return;
  }
  stopSlide(menu);
  if (stillMotion()) {
    menu.hidden = true;
    if (then) then();
    return;
  }
  menu.dataset.folding = '1';
  const edge = popEdges.get(menu) || { up: false };
  const run = menu.animate([{ clipPath: POP_OPEN, opacity: 1, transform: 'none' }, { clipPath: popShut(edge), opacity: 0, transform: `scale(${POP_FROM})` }], { duration: MOTION_MS * EXIT_SHARE, easing: EASE_EXIT });
  moving.set(menu, run);
  run.onfinish = () => {
    moving.delete(menu);
    delete menu.dataset.folding;
    menu.hidden = true;
    if (then) then();
  };
  run.oncancel = () => delete menu.dataset.folding;
}

function slideHeight(el, from, to, then) {
  stopSlide(el);
  if (stillMotion() || Math.abs(from - to) < 1) {
    if (then) then();
    return;
  }
  const leaving = to < from;
  const run = el.animate([{ height: `${from}px`, overflow: 'hidden' }, { height: `${to}px`, overflow: 'hidden' }], { duration: leaving ? MOTION_MS * EXIT_SHARE : MOTION_MS, easing: leaving ? EASE_EXIT : EASE_OUT });
  moving.set(el, run);
  run.onfinish = () => {
    moving.delete(el);
    if (then) then();
  };
}

function rel(iso) {
  const then = new Date(iso);
  const seconds = (Date.now() - then) / 1000;
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  if (seconds < 2 * 86400) return 'yesterday';
  if (seconds < 7 * 86400) return then.toLocaleDateString(undefined, { weekday: 'long' });
  const sameYear = then.getFullYear() === new Date().getFullYear();
  return then.toLocaleDateString(undefined, sameYear ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
}

const folderLabel = (dir) => (dir ? dir.split('/').pop() : 'Top level');

function progress(note) {
  let done;
  let all;
  if (note.tasks) {
    [done, all] = note.tasks;
  } else {
    const boxes = String(note.body || '').match(/^\s*- \[( |x|X)\] /gm) || [];
    all = boxes.length;
    done = boxes.filter((b) => !b.includes('[ ]')).length;
  }
  if (!all) return '';
  return `<span class="chip progress">${ICON.check.replace('<svg', '<svg width="14" height="14"')} ${done}/${all}</span>`;
}

function highlight(text, words) {
  if (!words.length) return esc(text);
  const pattern = new RegExp(`(${words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')})`, 'gi');
  return text
    .split(pattern)
    .map((part, i) => (i % 2 ? `<mark>${esc(part)}</mark>` : esc(part)))
    .join('');
}

function snippet(body, words) {
  const text = md.plain(body);
  if (!text) return '';
  if (!words.length) return esc(text.slice(0, 180));
  const lower = text.toLowerCase();
  const at = Math.min(...words.map((w) => lower.indexOf(w.toLowerCase())).filter((i) => i >= 0), Infinity);
  if (at === Infinity) return esc(text.slice(0, 180));
  const start = Math.max(0, at - 50);
  return (start ? '…' : '') + highlight(text.slice(start, start + 180), words);
}

function pinButton(note) {
  const label = note.pinned ? 'Unpin' : 'Pin to the top';
  return `<button class="pin-toggle${note.pinned ? ' on' : ''}" data-action="pin-card" data-id="${esc(note.id)}" aria-pressed="${note.pinned}" aria-label="${label}" title="${label}">${ICON.pin}</button>`;
}

function card(note, { words = [], showFolder = false, why = null } = {}) {
  const where = showFolder && note.directory ? `<span class="chip accent">${esc(note.directory)}</span>` : '';
  const text = snippet(note.body, words);
  const open = `class="card" role="link" tabindex="0" draggable="true" data-action="open-note" data-id="${esc(note.id)}" data-title="${esc(note.title)}" data-from="${esc(note.directory || '')}"`;
  return `<div ${open}>
    <div class="card-title"><span>${words.length ? highlight(note.title, words) : esc(note.title)}</span>${pinButton(note)}</div>
    ${text ? `<div class="card-snippet">${text}</div>` : ''}
    ${why ? `<div class="card-why">${ICON.map}${why.kind === 'idea' ? `Through the idea “${esc(why.name)}”` : why.kind === 'meaning' ? 'Close in meaning, though the words differ' : 'Through its summary'}</div>` : ''}
    <div class="card-meta">${where}<span title="${esc(`Edited ${rel(note.updated_at)}`)}">${rel(note.updated_at)}</span>${progress(note)}</div>
  </div>`;
}

function empty(icon, title, text) {
  return `<div class="empty">${icon}<h3>${esc(title)}</h3><p>${esc(text)}</p></div>`;
}

const skeleton = (n) => Array.from({ length: n }, () => '<div class="skeleton"></div>').join('');

let state = { view: null };
let seq = 0;

const SEARCH_HIGHLIGHT_MS = 5000;
const SEARCH_HIGHLIGHT_MOST = 300;
const SEARCH_HIGHLIGHT_NODES = 20000;
let searchHighlightTimer;
let searchPassage = null;
let openingNote = null;
let navigationRequest = 0;

function clearSearchHighlight() {
  clearTimeout(searchHighlightTimer);
  CSS.highlights?.delete('leo-search');
}

function highlightSearch(root, query) {
  clearSearchHighlight();
  if (!CSS.highlights || typeof Highlight === 'undefined') return false;
  const words = Array.from(String(query || '')).slice(0, 2000).join('').split(/\s+/).map(word => word.replace(/^#/, '')).filter(Boolean).slice(0, 20);
  if (!words.length) return false;
  const pattern = new RegExp(words.map(word => word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'), 'gi');
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const ranges = [];
  let node;
  let seen = 0;
  while (ranges.length < SEARCH_HIGHLIGHT_MOST && seen++ < SEARCH_HIGHLIGHT_NODES && (node = walk.nextNode())) {
    if (node.parentElement.closest('textarea, script, style, .katex-mathml')) continue;
    pattern.lastIndex = 0;
    let match;
    while (ranges.length < SEARCH_HIGHLIGHT_MOST && (match = pattern.exec(node.textContent))) {
      const range = document.createRange();
      range.setStart(node, match.index);
      range.setEnd(node, match.index + match[0].length);
      ranges.push(range);
    }
  }
  if (!ranges.length) return false;
  CSS.highlights.set('leo-search', new Highlight(...ranges));
  const first = ranges[0];
  const rect = first.getBoundingClientRect();
  if (rect.top < 80 || rect.bottom > window.innerHeight) window.scrollBy({ top: (rect.top + rect.bottom - window.innerHeight) / 2, behavior: 'instant' });
  searchHighlightTimer = setTimeout(clearSearchHighlight, SEARCH_HIGHLIGHT_MS);
  return true;
}

window.addEventListener('hashchange', () => {
  clearSearchHighlight();
  if (searchPassage && searchPassage.hash !== location.hash) searchPassage = null;
});

function showSaveFailure(s, error, retry, current) {
  history.replaceState(history.state, '', s.note.id ? noteHash(s.note.id) : s.note.directory ? `#/new/${enc(s.note.directory)}` : '#/new');
  if (!s.persisted || error instanceof Locked) return fail(error);
  toast(error instanceof Offline ? "Can't reach leo. Your draft is kept in this browser." : error.message || 'Leo could not save this note. Your draft is kept in this browser.', {
    bad: true,
    action: 'Leave, draft kept',
    run: () => { if (current() && s.persisted) return retry(s); },
  });
}

async function go(hash, { replace = false, search = null, note = null, isCurrent = null, kept = null } = {}) {
  const request = ++navigationRequest;
  const leaving = state.session;
  const mine = seq;
  const current = () => request === navigationRequest && mine === seq && state.session === leaving && (!isCurrent || isCurrent());
  if (leaving) {
    leaving.doc.stop();
    if (!(kept === leaving && leaving.persisted)) {
      try { await flush(leaving); }
      catch (error) {
        if (current()) showSaveFailure(leaving, error, saved => go(hash, { replace, search, note, isCurrent, kept: saved }), current);
        return;
      }
    }
    if (!current()) return;
  }
  if (!current()) return;
  if (leaving) {
    leaving.doc.destroy();
    state.session = null;
  }
  openingNote = note;
  clearSearchHighlight();
  const query = search ?? (state.view === 'search' ? state.query : null);
  searchPassage = query && hash.startsWith('#/n/') ? { hash, query } : null;
  if (location.hash === hash) {
    marginLeave();
    return route();
  }
  if (replace) {
    history.replaceState(history.state, '', hash);
    marginLeave();
    route();
  } else {
    location.hash = hash;
  }
}

const folderHash = (dir) => (dir ? `#/f/${enc(dir)}` : '#/');
const noteHash = (id) => `#/n/${enc(id)}`;

const asideHash = (hash) => /^#\/(settings|trash)(\/|$)/.test(hash || '');
const entryAt = () => (Number.isInteger(history.state?.leoAt) ? history.state.leoAt : null);
const places = { before: '', from: '', now: location.hash, at: entryAt() ?? 0, fromAt: -1 };
addEventListener('hashchange', (e) => {
  try {
    places.now = new URL(e.oldURL).hash;
  } catch (err) {}
  if (asideHash(location.hash) && !asideHash(places.now)) places.before = places.now || '#/';
  places.from = places.now;
  places.now = location.hash;
  let at = entryAt();
  if (at === null) {
    at = places.at + 1;
    try { history.replaceState({ ...(history.state || {}), leoAt: at }, '', location.hash); } catch (err) {}
  }
  places.fromAt = places.at;
  places.at = at;
});

function goBack(hash) {
  if (places.from === hash && places.fromAt === places.at - 1 && location.hash !== hash) return history.back();
  go(hash);
}

function back() {
  if (state.view === 'keeping' && /^#\/settings\/keeping(?:\/|$)/.test(location.hash)) return goBack(/^#\/settings(?:\/[^/]+)?\/?$/.test(places.from) ? places.from : places.before || '#/settings');
  if (asideHash(location.hash) && places.before) return goBack(places.before);
  if (state.view === 'note') return goBack('#/');
  goBack('#/');
}

const PAGE_NAMES = { '': 'Notes', f: 'Notes', search: 'Search', trash: 'Trash', calendar: 'Calendar', boards: 'Boards', settings: 'Settings', record: 'Record', new: 'New note' };

function pageName() {
  const [, kind = '', rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [];
  if (kind === 'settings') return settingsTitle(settingsSection(rest));
  return PAGE_NAMES[kind] || '';
}

function chrome({ dir = '', showBack = false, fab = null, actions = null }) {
  $('#back').hidden = !showBack;
  const crumbs = $('#crumbs');
  let trail;
  if (dir) {
    const parts = dir.split('/');
    trail = `<button data-action="home" title="Top level · Drop a folder here to move it to the top level">Top level</button>` + parts
      .map((part, i) => {
        const path = parts.slice(0, i + 1).join('/');
        return `<span class="sep">/</span><button data-action="open-folder" data-dir="${esc(path)}" draggable="true">${esc(part)}</button>`;
      })
      .join('');
  } else {
    const name = pageName();
    trail = name ? `<span class="page-name">${esc(name)}</span>` : '';
  }
  if (crumbs.innerHTML !== trail) crumbs.innerHTML = trail;
  floating.innerHTML = (fab || '') + (actions || '');
  document.title = dir ? `${folderLabel(dir)} · leo` : 'leo';
}

const CRUMB_EDGES = ['.note-columns > .note', '.workspace-empty', '.cal-head', '.kanban-head'];

function fitCrumbs() {
  const body = document.body;
  const row = $('#bar .bar-row');
  const target = !body.classList.contains('on-settings') && row ? CRUMB_EDGES.map((selector) => app.querySelector(selector)).find(Boolean) : null;
  const box = target && target.getBoundingClientRect();
  if (!box || !box.width) {
    if (body.classList.contains('crumb-fit')) body.classList.remove('crumb-fit');
    return;
  }
  const edge = getComputedStyle(target);
  const left = box.left + (parseFloat(edge.paddingLeft) || 0);
  const right = box.right - (parseFloat(edge.paddingRight) || 0);
  const bar = $('#bar');
  const start = bar.getBoundingClientRect().left + (parseFloat(getComputedStyle(bar).paddingLeft) || 0);
  body.style.setProperty('--crumb-shift', `${Math.max(0, Math.round(left - start))}px`);
  body.style.setProperty('--crumb-width', `${Math.max(0, Math.round(right - left))}px`);
  if (!body.classList.contains('crumb-fit')) body.classList.add('crumb-fit');
}

if (typeof MutationObserver === 'function') {
  new MutationObserver(fitCrumbs).observe(app, { childList: true });
  new MutationObserver(fitCrumbs).observe(app, { attributes: true, subtree: true, attributeFilter: ['data-margin'] });
  new MutationObserver(fitCrumbs).observe(document.body, { attributes: true, attributeFilter: ['class'] });
}
if (typeof ResizeObserver === 'function') new ResizeObserver(fitCrumbs).observe(app);
window.addEventListener('resize', fitCrumbs);

async function showWorkspace(dir = null) {
  const mine = ++seq;
  if (dir !== null) {
    const folders = await api('/api/folders');
    if (mine !== seq) return;
    side.folders = folders;
    side.loaded = true;
    if (dir && !folders.some((folder) => folder.name === dir)) toast('That folder is not there any more. Choose another folder.', { bad: true });
    else revealFolder(dir);
    history.replaceState(history.state, '', '#/');
  }
  if (state.session?.doc) state.session.doc.destroy();
  state = { view: 'workspace', dir: '' };
  document.body.classList.remove('on-note');
  chrome({});
  swap('workspace', () => {
    app.innerHTML = `<section class="workspace-empty">${empty(ICON.note, 'Choose a note or create one', 'Your folders and notes are in Folders.')}<button class="btn primary" data-action="new">${ICON.filePlus}New note</button>${!sideWide() ? '<button class="btn plain" data-action="folders">Folders</button>' : ''}</section>`;
    scrollToTop();
  });
}

let moreWatch = null;

async function undoTrashMove(done) {
  try {
    const back = await api('/api/trash/restore', { method: 'POST', body: { ids: done.ids, dirs: done.dirs || [] } });
    toast(back.restored === done.ids.length ? 'Brought back' : `Brought back ${plural(back.restored, 'note')}`);
    await refreshSideTree();
  } catch (e) { fail(e); }
}

const cleanTitle = (text) => text.replace(/\s*\n\s*/g, ' ').trim();

function mark(s, text) {
  if (state.session !== s) return;
  const el = $('#save-state');
  if (el) el.textContent = text;
}

let local;
try { local = window.localStorage; } catch (_) { local = null; }
const saving = window.leoSaving.create({
  api, storage: local,
  mark,
  storageError: () => toast('This browser cannot keep a draft. Keep the page open until your note says Saved.', { bad: true }),
  recovered: (count) => {
    toast(`Saving ${count} edit${count === 1 ? '' : 's'} that had not reached leo yet.`);
    setTimeout(() => saving.retry().then(() => {
      const left = saving.drafts().length;
      if (left) toast(`${plural(left, 'edit')} still could not be saved. Try again or discard ${left === 1 ? 'it' : 'them'} in Settings → Keeping.`, { bad: true });
    }), 0);
  },
  created: (s, note) => {
    if (state.session === s) history.replaceState(history.state, '', noteHash(note.id));
  },
  conflict: (s, note) => {
    if (state.session === s) {
      history.replaceState(history.state, '', noteHash(note.id));
      s.title.textContent = s.edit.title;
    }
    toast('This note changed elsewhere. Your edits were kept in a separate copy.', { action: 'Open copy', run: () => go(noteHash(note.id)) });
  },
  retryable: (error) => error instanceof Offline || error.status >= 500,
});

function snapshot(s) {
  const body = s.doc.source();
  const title = cleanTitle(s.title.textContent) || (s.note.id ? s.note.title : md.plain(body).slice(0, 60));
  return { title, body, tags: [...s.tags] };
}

function changed(s) { saving.changed(s, snapshot(s)); }
function flush(s = state.session) { return saving.flush(s); }

let citedLines = null;

function showCitedLines() {
  const want = citedLines;
  citedLines = null;
  if (!want || !state.session || !state.session.doc) return;
  if (!state.session.doc.highlight(want.from, want.to)) toast('Those lines have changed since Felix read the note.');
}

async function showPassage(note, name, at) {
  const mine = seq;
  const found = await api(`/api/notes/${enc(note)}/originals/${enc(name)}/passage?at=${enc(at || 'p1')}`);
  if (mine !== seq) return;
  const lines = found.lines.map((l) => `<div class="source-row"><span class="rec-at">${l.n}</span><span class="passage-text">${esc(l.text)}</span></div>`).join('');
  const box = sheet(`<h3>${esc(name)}</h3>
    <p>${esc(found.label[0].toUpperCase() + found.label.slice(1))}, word for word from the file you uploaded.</p>
    <div class="source-list passage-list">${lines || '<p class="hint">That part of the file is empty.</p>'}</div>
    <div class="buttons"><button class="btn plain" data-action="close">Close</button>${viewKind(name) ? '<button class="btn primary" id="passage-open">Open the file</button>' : ''}</div>`);
  const open = $('#passage-open', box);
  if (open) open.addEventListener('click', () => { closeSheet(true); viewOriginal(note, name, found.page).catch(fail); });
}

function noteActions(id, recorded = false) {
  return `<nav class="actions" aria-label="Note">
      ${id && recorded ? `<button type="button" data-action="note-sources" aria-label="Transcript">${ICON.mic}</button>` : ''}
      ${id ? `<button type="button" data-action="note-tasks" aria-label="Tasks">${ICON.calendar}</button>` : ''}
      <button type="button" data-action="note-picture" aria-label="Picture">${ICON.image}</button>
      <button type="button" data-action="move" aria-label="Move">${ICON.move}</button>
      <button type="button" data-action="share" aria-label="PDF">${ICON.share}</button>
      <button type="button" data-action="delete" class="danger" aria-label="Delete">${ICON.trash}</button>
    </nav>`;
}

async function showNote(id, { fresh = null, draft = null } = {}) {
  const passage = searchPassage?.hash === noteHash(id) ? searchPassage : null;
  searchPassage = null;
  clearSearchHighlight();
  const mine = ++seq;
  const note = fresh || (await api(`/api/notes/${enc(id)}`));
  await marginLoadPrefs();
  if (mine !== seq) return;
  let s;
  swap(`n:${note.id || 'new'}`, () => {
    scrollToTop();
    document.body.classList.toggle('on-note', true);
    if (source.note && source.note !== note.id) closeSource();
    if (state.session && state.session.doc) state.session.doc.destroy();
    s = draft || saving.open(note);
    s.tags = [...s.edit.tags];
    if (side.openedNote !== note.id) side.target = null;
    side.openedNote = note.id;
    state = { view: 'note', dir: note.directory, session: s };
    chrome({ dir: note.directory, showBack: true, actions: noteActions(note.id, Boolean(note.recorded)) });
    document.title = `${note.title || 'New note'} · leo`;
    chat.setContext(note.id ? { id: note.id, title: note.title, directory: note.directory } : null);
    const where = `<button class="chip accent" data-action="open-folder" data-dir="${esc(note.directory)}">${ICON.folder.replace('<svg', '<svg width="13" height="13"')} ${esc(folderLabel(note.directory))}</button>`;
    const when = note.id ? `Edited ${rel(note.updated_at)}` : 'New note';
    app.innerHTML = `<div class="note-layout"><div class="note-columns"><aside class="note-outline" aria-label="Note outline" hidden><details class="source-more"><summary aria-label="Headings in this note" title="Headings in this note"><span class="outline-icon">${ICON.list}</span>${ICON.chevron}</summary><nav aria-label="Headings"></nav></details></aside><article class="note">
    <header class="note-head"><div class="note-context">${where}<div class="margin-mode-wrap"><button type="button" id="margin-mode" class="margin-mode" aria-haspopup="menu" aria-expanded="false" aria-controls="margin-mode-menu" disabled><span id="margin-mode-icon" class="margin-mode-icon" aria-hidden="true">${ICON.pen}</span><span id="margin-mode-label">When you ask</span>${ICON.chevron}</button><div id="margin-mode-menu" class="margin-mode-menu" role="menu" aria-label="Note reading mode" hidden><button type="button" role="menuitemradio" aria-checked="false" data-margin-mode="along">Reads along<small>Reads changes after a pause.</small></button><button type="button" role="menuitemradio" aria-checked="true" data-margin-mode="ask">When you ask<small>Reads nothing until you ask.</small></button></div></div><span id="margin-status" role="status" class="hint"></span></div><h1 id="title" contenteditable="plaintext-only" spellcheck="true" data-placeholder="Title" enterkeyhint="next">${esc(s.edit.title)}</h1>
    <div class="note-meta"><span id="save-state">${when}</span></div></header>
    <div class="prose doc" id="doc"></div>
    <section class="related" id="related" hidden aria-label="Related notes"></section>
  </article></div></div>`;
    s.title = $('#title');
    drawOriginals(note);
    hideAskFloat();
    s.doc = leoDoc.mount($('#doc'), { source: s.edit.body, outline: $('.note-outline'), onChange: () => { changed(s); marginEdited(s); }, placeholder: 'Tap here to write', dir: note.directory || '', onPictures: (files) => addPictures(s, files), onSelection: (has) => placeAskFloat(has) });
    marginMount(s, mine).catch(fail);
    if (citedLines && citedLines.id === note.id) showCitedLines();
    else if (passage && highlightSearch(app.querySelector('article.note'), passage.query)) {
      clearTimeout(reading.timer);
      reading.id = note.id;
      reading.user = false;
    } else restoreReading(note.id, mine);
  }, {
    from: () => morphTitle(note.id),
    to: () => ($('#title')?.textContent.trim() ? $('#title') : null),
  });
  arriveOnPage(app.querySelector('article.note'));
  drawRelated(note.id, mine);
  if (s.dirty) {
    mark(s, 'Recovered draft · saving…');
    flush(s).catch(fail);
  }
  const blank = () => {
    const empty = !cleanTitle(s.title.textContent);
    if (empty && s.title.innerHTML !== '') s.title.textContent = '';
    s.title.classList.toggle('blank', empty);
  };
  blank();
  s.title.addEventListener('input', () => {
    blank();
    document.title = `${cleanTitle(s.title.textContent) || 'Untitled'} · leo`;
    changed(s);
  });
  s.title.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      s.doc.editStart();
    }
  });
  s.title.addEventListener('paste', (e) => {
    e.preventDefault();
    const text = cleanTitle((e.clipboardData || window.clipboardData).getData('text'));
    document.execCommand('insertText', false, text);
  });
  if (!note.id) {
    s.title.focus();
  }
}


function newNote(dir) {
  return showNote(null, { fresh: { id: null, title: '', body: '', tags: [], directory: dir || '', pinned: false, version: null } });
}

async function ready() {
  const s = state.session;
  if (!s) return null;
  s.doc.stop();
  await flush(s);
  return s.note.id ? s : null;
}

async function showSearch(query) {
  const mine = ++seq;
  state = { view: 'search', dir: '', query };
  chrome({});
  openSearch(query);
  if (!query.trim()) {
    app.innerHTML = empty(ICON.search, 'Search every note', 'Titles, text and the ideas. Abbreviations like BFS work too.');
    return;
  }
  const results = await api(`/api/search?q=${enc(query)}&brief=true`);
  if (mine !== seq) return;
  const words = query.split(/\s+/).map((w) => w.replace(/^#/, '')).filter(Boolean);
  const draw = (list) => list.map((n) => card(n, { words, showFolder: true, why: n.why })).join('');
  const count = results.length >= SEARCH_MOST ? `The first ${SEARCH_MOST} notes` : plural(results.length, 'note');
  app.innerHTML = results.length
    ? `<div class="section-title">${count}</div><div class="cards">${draw(results.slice(0, SEARCH_STEP))}</div>${results.length > SEARCH_STEP ? '<div class="more-notes" id="more-notes" aria-hidden="true">Loading more notes…</div>' : ''}`
    : empty(ICON.search, 'Nothing found', `No note mentions “${query}”.`);
  cascade($('.cards'));
  let shown = SEARCH_STEP;
  if (moreWatch) moreWatch.disconnect();
  const sentinel = $('#more-notes');
  if (!sentinel || typeof IntersectionObserver !== 'function') return;
  moreWatch = new IntersectionObserver((seen) => {
    if (mine !== seq || !seen.some((x) => x.isIntersecting)) return;
    $('.cards').insertAdjacentHTML('beforeend', draw(results.slice(shown, shown + SEARCH_STEP)));
    shown += SEARCH_STEP;
    if (shown >= results.length) {
      moreWatch.disconnect();
      sentinel.remove();
    }
  }, { rootMargin: '600px 0px' });
  moreWatch.observe(sentinel);
}

const SEARCH_MOST = 300;
const SEARCH_STEP = 60;

async function showRecord(dir) {
  const mine = ++seq;
  state = { view: 'record', dir: '' };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Record</button>';
  document.title = 'Record · leo';
  app.innerHTML = '<div id="rec-box"></div>';
  let folders = [];
  try {
    folders = await api('/api/folders');
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  if (mine !== seq) return;
  await recorder.show($('#rec-box'), { dir, folders: folders.map((f) => f.name).filter(Boolean) });
}

window.addEventListener('beforeprint', () => {
  for (const el of document.body.querySelectorAll('*')) {
    if (getComputedStyle(el).position === 'fixed') el.setAttribute('data-print-off', '');
  }
});
window.addEventListener('afterprint', () => {
  document.body.querySelectorAll('[data-print-off]').forEach((el) => el.removeAttribute('data-print-off'));
});
