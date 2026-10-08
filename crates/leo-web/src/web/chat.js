(function (root) {
  'use strict';

  const KEY = 'leo-chat-v1';
  const MOST_KEPT = 40;
  const MOST_REFS = 8;
  const MODES = [
    { id: 'chat', label: 'Chat', hint: 'Ask anything, get things explained, or go over meeting notes; type @ to bring in a note', starters: [
      { text: 'What are the key ideas here?', needs: 'note', ask: 'Which note should I pull the key ideas from?' },
      { text: 'Explain this simply', needs: 'note', ask: 'Which note should I explain?' },
      { text: 'Summarize this meeting and list the action items', needs: 'note', ask: 'Which meeting notes should I summarize?' },
    ] },
    { id: 'study', label: 'Study', hint: 'Quizzes you one question at a time, with hints, a score and review plans', starters: [
      { text: 'Quiz me on this note', needs: 'note', ask: 'Which note should I quiz you on?' },
      { text: 'Quiz me across my classes', needs: 'classes', ask: 'Which classes should the quiz cover?' },
      { text: 'Make me a 3-day review plan', needs: 'classes', ask: 'Which classes should the plan cover?' },
    ] },
  ];
  const OLD_MODES = { ask: 'chat', explain: 'chat', meeting: 'chat', quiz: 'study', coach: 'study' };
  const modeOf = (id) => (MODES.some((m) => m.id === id) ? id : OLD_MODES[id] || 'chat');

  function felix(width, extra = '') {
    return `<svg class="felix ${extra}" viewBox="-2 -10 50 42" width="${width}" height="${Math.round((width * 42) / 50)}" shape-rendering="crispEdges" aria-hidden="true">
      <g class="felix-spark"><rect x="-1" y="-6" width="2" height="2"/><rect x="42" y="-4" width="2" height="2"/><rect x="21" y="-8" width="2" height="2"/></g>
      <g class="felix-heart"><rect x="17" y="-9" width="2" height="1"/><rect x="21" y="-9" width="2" height="1"/><rect x="16" y="-8" width="8" height="2"/><rect x="17" y="-6" width="6" height="1"/><rect x="18" y="-5" width="4" height="1"/><rect x="19" y="-4" width="2" height="1"/></g>
      <g class="felix-thought"><rect x="37" y="-1" width="2" height="2"/><rect x="40" y="-4" width="2" height="2"/><rect x="43" y="-7" width="3" height="3"/></g>
      <g class="felix-z"><text x="37" y="-2" font-size="10" font-weight="800" font-family="system-ui, sans-serif">z</text></g>
      <g class="felix-body">
        <rect class="felix-arm felix-left" x="0" y="18" width="5" height="6"/>
        <rect class="felix-arm felix-right" x="39" y="18" width="5" height="6"/>
        <rect class="felix-skin" x="5" y="2" width="34" height="28"/>
        <g class="felix-cheeks"><rect x="8" y="21" width="4" height="2"/><rect x="24" y="21" width="4" height="2"/></g>
        <g class="felix-gaze"><g class="felix-eyes" shape-rendering="geometricPrecision"><rect x="11" y="16" width="4" height="4"/><rect x="21" y="16" width="4" height="4"/></g></g>
        <g class="felix-sweat"><rect x="34" y="5" width="2" height="2"/><rect x="33" y="7" width="4" height="3"/></g>
      </g>
    </svg>`;
  }

  const TAPS = ['boop', 'hop', 'spin', 'giggle'];

  function splitLines(buffer) {
    const parts = buffer.split('\n');
    const rest = parts.pop();
    const events = [];
    for (const line of parts) {
      if (!line.trim()) continue;
      try {
        events.push(JSON.parse(line));
      } catch (e) {
        events.push({ error: 'leo sent something it could not read.' });
      }
    }
    return { events, rest };
  }

  function grade(text) {
    const match = /^\s*\[\[(correct|incorrect)\]\]\s*/i.exec(text);
    if (match) return { verdict: match[1].toLowerCase(), text: text.slice(match[0].length) };
    if (/^\s*\[\[?[a-z]*\]?$/i.test(text) && text.trim()) return { verdict: null, text: '' };
    return { verdict: null, text };
  }

  function cite(html, sources, escape) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    return html.replace(/\[(n\d+(?:\s*,\s*n\d+)*)\]/g, (whole, list) => {
      const chips = list
        .split(/\s*,\s*/)
        .map((n) => byN.get(n))
        .filter(Boolean)
        .map((s) => `<button class="cite" data-chat="open" data-id="${escape(s.id)}" title="${escape(s.title)}">${escape(s.title.length > 28 ? s.title.slice(0, 27) + '…' : s.title)}</button>`);
      return chips.length ? chips.join('') : whole;
    });
  }

  function cited(text, sources) {
    const used = new Set();
    for (const m of String(text).matchAll(/\[(n\d+(?:\s*,\s*n\d+)*)\]/g)) for (const n of m[1].split(/\s*,\s*/)) used.add(n);
    return (sources || []).filter((s) => used.has(`n${s.n}`));
  }

  function mentionAt(text, caret) {
    const before = text.slice(0, caret);
    const at = before.lastIndexOf('@');
    if (at < 0 || (at > 0 && !/\s/.test(before[at - 1]))) return null;
    const query = before.slice(at + 1);
    if (/\n/.test(query) || query.length > 40 || /^\s/.test(query)) return null;
    return { start: at, query };
  }

  function addRef(refs, note) {
    if (!note || !note.id || refs.some((r) => r.id === note.id) || refs.length >= MOST_REFS) return refs;
    return [...refs, { id: note.id, title: note.title || 'Untitled' }];
  }

  function newId() {
    if (root.crypto && typeof root.crypto.randomUUID === 'function') return root.crypto.randomUUID();
    return `chat-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  }

  function load(storage) {
    const fresh = { id: null, mode: 'chat', messages: [], refs: [] };
    try {
      const saved = JSON.parse(storage.getItem(KEY) || 'null');
      if (saved && Array.isArray(saved.messages)) return { id: typeof saved.id === 'string' ? saved.id : null, mode: modeOf(saved.mode), messages: saved.messages, refs: Array.isArray(saved.refs) ? saved.refs.slice(0, MOST_REFS) : [] };
    } catch (e) {
      return fresh;
    }
    return fresh;
  }

  function save(storage, mode, messages, refs = [], id = null) {
    try {
      storage.setItem(KEY, JSON.stringify({ id, mode, messages: messages.slice(-MOST_KEPT), refs }));
    } catch (e) {
      return;
    }
  }

  function groups(list, now = new Date()) {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const day = 86400000;
    const order = ['Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older'];
    const out = new Map(order.map((name) => [name, []]));
    for (const chat of list) {
      const t = new Date(chat.updated_at).getTime();
      const name = t >= start ? 'Today' : t >= start - day ? 'Yesterday' : t >= start - 7 * day ? 'Previous 7 days' : t >= start - 30 * day ? 'Previous 30 days' : 'Older';
      out.get(name).push(chat);
    }
    return order.filter((name) => out.get(name).length).map((name) => ({ name, chats: out.get(name) }));
  }

  function starterWords(starter, picked) {
    if (starter.needs !== 'classes' || !picked.length) return starter.text;
    const list = picked.length === 1 ? picked[0] : `${picked.slice(0, -1).join(', ')} and ${picked[picked.length - 1]}`;
    return starter.text === 'Quiz me across my classes' ? `Quiz me across ${list}` : `${starter.text} for ${list}`;
  }

  const MOST_FILES = 10;

  function splitFiles(docs, messages) {
    const ids = new Set();
    const names = new Set();
    for (const m of messages || []) {
      if (m.role !== 'user') continue;
      for (const id of m.files || []) ids.add(id);
      if (!m.files) for (const name of m.docs || []) names.add(name);
    }
    const sent = [];
    const waiting = [];
    for (const d of docs || []) (ids.has(d.id) || names.has(d.name) ? sent : waiting).push(d);
    return { sent, waiting };
  }
  const FILE_TYPES = '.pdf,.docx,.pptx,.txt,.md,image/*';

  function create({ render, escape, onOpen = () => {}, storage = root.localStorage, prepare = null, notify = () => {} }) {
    const saved = load(storage);
    const state = { open: false, id: saved.id || newId(), mode: modeOf(saved.mode), messages: saved.messages, refs: saved.refs, context: null, dropped: null, busy: null, streak: 0, pick: null, chats: null, sidebar: null, doomed: null, asking: null, files: [], sent: [], filesFor: null };
    const panel = document.createElement('aside');
    panel.className = 'chat';
    panel.id = 'chat';
    panel.hidden = true;
    panel.setAttribute('aria-label', 'Ask Felix');
    panel.innerHTML = `
      <nav class="chat-history" id="chat-history" aria-label="Your chats">
        <div class="chat-history-head"><button class="icon-btn chat-history-back" data-chat="history" aria-label="Back to the chat"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg></button><b>Chats</b><button class="chat-history-new" data-chat="new"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>New chat</button></div>
        <div class="chat-history-list" id="chat-history-list"></div>
      </nav>
      <section class="chat-main">
      <header class="chat-head">
        <button class="icon-btn chat-history-toggle" data-chat="history" aria-label="Your chats" title="Your chats"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg></button>
        <span class="chat-face" id="chat-face">${felix(40, 'idle')}</span>
        <span class="chat-name"><b>Felix</b><span>your study buddy</span></span>
        <button class="icon-btn" data-chat="new" aria-label="New chat" title="New chat"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button>
        <button class="icon-btn" data-chat="close" aria-label="Close"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button>
      </header>
      <div class="chat-modes" role="tablist" aria-label="How Felix helps">${MODES.map((m) => `<button role="tab" class="chat-mode" data-chat="mode" data-mode="${m.id}" title="${escape(m.hint)}">${m.label}</button>`).join('')}</div>
      <div class="chat-body" id="chat-body" aria-live="polite"></div>
      <footer class="chat-foot">
        <div class="chat-context" id="chat-context"></div>
        <div class="chat-pick" id="chat-pick" hidden></div>
        <div class="chat-refs" id="chat-refs"></div>
        <form class="chat-compose" id="chat-form">
          <div class="chat-attach-menu" id="chat-attach-menu" hidden><button type="button" data-chat="attach-note">A note from leo</button>${prepare ? '<button type="button" data-chat="attach-file">A file from this device</button>' : ''}</div>
          <input type="file" id="chat-file" multiple accept="${FILE_TYPES}" hidden>
          <button class="chat-attach" type="button" data-chat="attach" aria-label="Add a note or a file" title="Add a note or a file to the conversation (or type @ for a note)"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11l-8.5 8.5a5 5 0 0 1-7-7L14 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 7"/></svg></button>
          <textarea id="chat-input" rows="1" placeholder="Message Felix, or type @ to add a note…" enterkeyhint="send"></textarea>
          <button class="chat-send" id="chat-send" type="submit" aria-label="Send"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg></button>
        </form>
      </footer>
      </section>`;
    document.body.appendChild(panel);
    const $ = (sel) => panel.querySelector(sel);
    const body = $('#chat-body');
    const input = $('#chat-input');

    const faces = () => [...document.querySelectorAll('.felix')];
    let blinkTimer = 0;
    function blink() {
      clearTimeout(blinkTimer);
      blinkTimer = setTimeout(() => {
        for (const f of faces()) {
          f.classList.remove('blink');
          void f.getBoundingClientRect();
          f.classList.add('blink');
          setTimeout(() => f.classList.remove('blink'), 200);
        }
        blink();
      }, 2200 + Math.random() * 3600);
    }
    function mood(name, ms) {
      for (const f of faces()) {
        f.classList.remove('dance', 'droop', 'wave', 'cheer', 'nod', 'wake', 'perk', ...TAPS);
        void f.getBoundingClientRect();
        f.classList.add(name);
        setTimeout(() => f.classList.remove(name), ms);
      }
    }
    function thinking(on) {
      for (const f of faces()) {
        f.classList.toggle('think', on);
        if (!on) f.classList.remove('talk');
      }
    }

    function talking() {
      for (const f of faces()) {
        f.classList.remove('think');
        f.classList.add('talk');
      }
    }

    let sleepTimer = 0;
    function awake() {
      clearTimeout(sleepTimer);
      const sleeping = faces().some((f) => f.classList.contains('sleep'));
      for (const f of faces()) f.classList.remove('sleep');
      if (sleeping) mood('wake', 900);
      sleepTimer = setTimeout(() => {
        if (state.open && !state.busy) for (const f of faces()) f.classList.add('sleep');
      }, 60000);
    }

    let lastTap = -1;
    function tapped() {
      let pick = Math.floor(Math.random() * TAPS.length);
      if (pick === lastTap) pick = (pick + 1) % TAPS.length;
      lastTap = pick;
      mood(TAPS[pick], 900);
    }

    function gaze(e) {
      for (const f of faces()) {
        const box = f.getBoundingClientRect();
        if (!box.width) continue;
        const dx = e.clientX - (box.left + box.width / 2);
        const dy = e.clientY - (box.top + box.height / 2);
        const far = Math.max(1, Math.hypot(dx, dy));
        f.style.setProperty('--gaze-x', `${((dx / far) * Math.min(1, far / 160) * 2).toFixed(2)}px`);
        f.style.setProperty('--gaze-y', `${((dy / far) * Math.min(1, far / 160) * 1.5).toFixed(2)}px`);
      }
    }
    blink();

    const SIDEBAR = 'leo-chat-sidebar';
    const wide = () => Boolean(root.matchMedia && root.matchMedia('(min-width: 900px)').matches);

    function sidebarOpen() {
      if (state.sidebar !== null) return state.sidebar;
      try {
        const kept = storage.getItem(SIDEBAR);
        if (kept !== null) return kept === '1' && wide();
      } catch (e) {
        return wide();
      }
      return wide();
    }

    function showSidebar(on) {
      state.sidebar = on;
      if (wide()) {
        try {
          storage.setItem(SIDEBAR, on ? '1' : '0');
        } catch (e) {
          state.sidebar = on;
        }
      }
      panel.classList.toggle('with-history', on);
      if (on) loadChats();
    }

    function stored() {
      return state.messages.filter((m) => !m.pending);
    }

    function persist() {
      save(storage, state.mode, stored(), state.refs, state.id);
    }

    let upload = Promise.resolve();
    function snapshot() {
      return { id: state.id, mode: state.mode, refs: state.refs, messages: stored() };
    }

    function remember(chat = snapshot()) {
      if (chat.id === state.id) persist();
      const messages = chat.messages.filter((m) => !m.pending);
      if (!messages.length) return upload;
      const id = chat.id;
      const sending = { mode: chat.mode, refs: chat.refs, messages: messages.map(({ pending, ...m }) => m) };
      upload = upload.then(async () => {
        try {
          const response = await fetch(`/api/chats/${encodeURIComponent(id)}`, {
            method: 'PUT',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(sending),
          });
          if (!response.ok) return;
          const summary = await response.json();
          if (state.chats) {
            state.chats = [summary, ...state.chats.filter((c) => c.id !== summary.id)];
            drawChats();
          }
        } catch (e) {
          return;
        }
      });
      return upload;
    }

    async function loadChats() {
      try {
        const response = await fetch('/api/chats', { credentials: 'same-origin' });
        if (response.ok) state.chats = await response.json();
      } catch (e) {
        state.chats = state.chats || [];
      }
      drawChats();
    }

    function drawChats() {
      const box = $('#chat-history-list');
      if (!box) return;
      if (!state.chats) {
        box.innerHTML = '<div class="chat-history-none">Loading…</div>';
        return;
      }
      if (!state.chats.length) {
        box.innerHTML = '<div class="chat-history-none">Your conversations with Felix are kept here, on this computer.</div>';
        return;
      }
      box.innerHTML = groups(state.chats)
        .map((g) => `<div class="chat-history-group">${escape(g.name)}</div>${g.chats
          .map((c) => `<div class="chat-history-row${c.id === state.id ? ' on' : ''}"><button class="chat-history-item" data-chat="resume" data-id="${escape(c.id)}" title="${escape(c.title)}">${escape(c.title)}</button><button class="chat-history-x${state.doomed === c.id ? ' sure' : ''}" data-chat="forget" data-id="${escape(c.id)}" aria-label="${state.doomed === c.id ? 'Confirm delete' : 'Delete'} ${escape(c.title)}">${state.doomed === c.id ? 'Delete' : '×'}</button></div>`)
          .join('')}`)
        .join('');
    }

    function begin() {
      if (state.busy) state.busy.abort();
      state.id = newId();
      state.messages = [];
      state.refs = [];
      state.files = [];
      state.sent = [];
      state.filesFor = state.id;
      state.streak = 0;
      state.asking = null;
      closePick();
      drawRefs();
      persist();
      draw();
      drawChats();
      mood('wave', 1500);
    }

    async function resume(id) {
      if (id === state.id) {
        if (!wide()) showSidebar(false);
        return;
      }
      if (state.busy) state.busy.abort();
      let chat;
      try {
        const response = await fetch(`/api/chats/${encodeURIComponent(id)}`, { credentials: 'same-origin' });
        if (!response.ok) throw new Error('gone');
        chat = await response.json();
      } catch (e) {
        state.chats = (state.chats || []).filter((c) => c.id !== id);
        drawChats();
        return;
      }
      state.id = chat.id;
      state.mode = modeOf(chat.mode);
      state.messages = Array.isArray(chat.messages) ? chat.messages : [];
      state.refs = Array.isArray(chat.refs) ? chat.refs.slice(0, MOST_REFS) : [];
      state.files = [];
      state.sent = [];
      state.streak = 0;
      closePick();
      persist();
      loadFiles();
      drawModes();
      drawRefs();
      draw();
      drawChats();
      if (!wide()) showSidebar(false);
    }

    async function forget(id) {
      if (state.doomed !== id) {
        state.doomed = id;
        drawChats();
        return;
      }
      state.doomed = null;
      try {
        await fetch(`/api/chats/${encodeURIComponent(id)}`, { method: 'DELETE', credentials: 'same-origin' });
      } catch (e) {
        return;
      }
      state.chats = (state.chats || []).filter((c) => c.id !== id);
      if (id === state.id) begin();
      else drawChats();
    }

    function drawRefs() {
      const notes = state.refs
        .map((r) => `<span class="chat-ref"><button class="chat-ref-open" data-chat="open" data-id="${escape(r.id)}" title="${escape(r.title)}">${escape(r.title)}</button><button class="chat-ref-x" data-chat="unref" data-id="${escape(r.id)}" aria-label="Remove ${escape(r.title)}">×</button></span>`)
        .join('');
      const files = state.files
        .map((f) => f.status === 'reading'
          ? `<span class="chat-ref doc reading" title="Felix is reading ${escape(f.name)}"><span class="chat-ref-open">${escape(f.name)} · reading…</span></span>`
          : `<span class="chat-ref doc"><span class="chat-ref-open" title="${escape(f.name)}">${escape(f.name)}</span><button class="chat-ref-x" data-chat="unfile" data-id="${escape(f.id)}" aria-label="Remove ${escape(f.name)}">×</button></span>`)
        .join('');
      $('#chat-refs').innerHTML = notes + files;
    }

    async function loadFiles() {
      const chat = state.id;
      state.filesFor = chat;
      try {
        const response = await fetch(`/api/chats/${encodeURIComponent(chat)}/files`, { credentials: 'same-origin' });
        if (!response.ok || state.id !== chat) return;
        const split = splitFiles(await response.json(), state.messages);
        state.sent = split.sent;
        state.files = split.waiting.map((d) => ({ ...d, status: 'ready' }));
        drawRefs();
      } catch (e) {
        state.files = state.id === chat ? state.files : [];
      }
    }

    async function addFile(file) {
      const chat = state.id;
      const key = `${Date.now()}-${Math.random()}`;
      state.files.push({ key, name: file.name, status: 'reading' });
      drawRefs();
      const drop = () => {
        state.files = state.files.filter((f) => f.key !== key);
        drawRefs();
      };
      let reply;
      try {
        const body = await prepare(file);
        reply = await fetch(`/api/chats/${encodeURIComponent(chat)}/files`, {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
      } catch (e) {
        if (state.id === chat) drop();
        notify(e.message || `Felix could not read ${file.name}.`);
        return;
      }
      if (state.id !== chat) return;
      if (!reply.ok) {
        let said = '';
        try {
          said = (await reply.json()).error || '';
        } catch (e) {
          said = '';
        }
        drop();
        notify(said ? `Felix could not read ${file.name}: ${said}` : reply.status === 413 ? `${file.name} is too large for Felix.` : `Felix could not read ${file.name}.`);
        return;
      }
      const doc = await reply.json();
      state.files = state.files.map((f) => (f.key === key ? { ...doc, status: 'ready' } : f));
      drawRefs();
    }

    function addFiles(list) {
      const room = MOST_FILES - state.files.length - state.sent.length;
      const files = [...list];
      if (files.length > room) notify(`A chat holds up to ${MOST_FILES} documents, so ${files.length - Math.max(room, 0)} ${files.length - Math.max(room, 0) === 1 ? 'was' : 'were'} left out.`);
      for (const file of files.slice(0, Math.max(room, 0))) addFile(file);
    }

    async function removeFile(id) {
      const chat = state.id;
      state.files = state.files.filter((f) => f.id !== id);
      drawRefs();
      try {
        await fetch(`/api/chats/${encodeURIComponent(chat)}/files/${encodeURIComponent(id)}`, { method: 'DELETE', credentials: 'same-origin' });
      } catch (e) {
        return;
      }
    }

    let pickTimer = 0;
    let pickSeq = 0;
    async function findNotes(query) {
      const url = query.trim() ? `/api/search?q=${encodeURIComponent(query.trim())}` : '/api/notes?limit=12';
      const response = await fetch(url, { credentials: 'same-origin' });
      if (!response.ok) return [];
      const notes = await response.json();
      return notes.filter((n) => !state.refs.some((r) => r.id === n.id)).slice(0, 8);
    }

    function closePick() {
      state.pick = null;
      const box = $('#chat-pick');
      box.hidden = true;
      box.innerHTML = '';
    }

    function drawPick() {
      const box = $('#chat-pick');
      const p = state.pick;
      if (!p) return closePick();
      box.hidden = false;
      const search = p.from === 'button' ? `<input class="chat-pick-search" id="chat-pick-search" placeholder="Find a note…" autocomplete="off" value="${escape(p.query)}">` : '';
      const rows = p.notes.length
        ? p.notes.map((n, i) => `<button class="chat-pick-row${i === p.at ? ' on' : ''}" data-chat="pick" data-i="${i}" role="option" aria-selected="${i === p.at}"><b>${escape(n.title || 'Untitled')}</b><span>${escape(n.directory || 'All notes')}</span></button>`).join('')
        : `<div class="chat-pick-none">${p.loading ? 'Looking…' : state.refs.length >= MOST_REFS ? `Up to ${MOST_REFS} notes at once.` : 'No notes match.'}</div>`;
      box.innerHTML = `${search}<div class="chat-pick-list" role="listbox" aria-label="Notes">${rows}</div>`;
      const field = $('#chat-pick-search');
      if (field && p.focusSearch) {
        field.focus();
        field.setSelectionRange(field.value.length, field.value.length);
        p.focusSearch = false;
      }
    }

    function openPick(from, query, start) {
      if (state.refs.length >= MOST_REFS) {
        state.pick = { from, query, start, notes: [], at: 0, loading: false };
        return drawPick();
      }
      const same = state.pick && state.pick.from === from && state.pick.query === query;
      if (same) return;
      state.pick = { from, query, start, notes: state.pick ? state.pick.notes : [], at: 0, loading: true, focusSearch: from === 'button' && !state.pick };
      drawPick();
      clearTimeout(pickTimer);
      const mine = ++pickSeq;
      pickTimer = setTimeout(async () => {
        let notes = [];
        try {
          notes = await findNotes(query);
        } catch (e) {
          notes = [];
        }
        if (mine !== pickSeq || !state.pick) return;
        state.pick.notes = notes;
        state.pick.loading = false;
        state.pick.at = 0;
        drawPick();
      }, 120);
    }

    function choose(i) {
      const p = state.pick;
      const note = p && p.notes[i];
      if (!note) return;
      state.refs = addRef(state.refs, note);
      if (p.from === 'mention') {
        const caret = input.selectionStart;
        input.value = input.value.slice(0, p.start) + input.value.slice(caret);
        input.setSelectionRange(p.start, p.start);
        fit();
      }
      closePick();
      drawRefs();
      remember();
      if (state.asking && state.asking.starter.needs === 'note') {
        const starter = state.asking.starter;
        state.asking = null;
        send(starter.text);
        return;
      }
      if (!state.messages.length) draw();
      input.focus();
    }

    function watchMention() {
      const m = mentionAt(input.value, input.selectionStart);
      if (m) openPick('mention', m.query, m.start);
      else if (state.pick && state.pick.from === 'mention') closePick();
    }

    function modeInfo() {
      return MODES.find((m) => m.id === state.mode) || MODES[0];
    }

    function drawModes() {
      for (const b of panel.querySelectorAll('.chat-mode')) {
        const on = b.dataset.mode === state.mode;
        b.classList.toggle('on', on);
        b.setAttribute('aria-selected', String(on));
      }
      input.placeholder = state.mode === 'study' ? 'Answer, or ask for a question…' : 'Message Felix, or type @ to add a note…';
    }

    function drawContext() {
      const box = $('#chat-context');
      const ctx = state.context && state.dropped !== state.context.id ? state.context : null;
      box.innerHTML = ctx
        ? `<span class="chat-using">Also reading <b>${escape(ctx.title || 'Untitled')}</b> and notes connected to it</span><button class="chat-drop" data-chat="drop" aria-label="Stop using this note">×</button>`
        : '<span class="chat-using">Looks through your notes when they help</span>';
    }

    function bubble(m, i) {
      if (m.role === 'user') {
        const notes = (m.refs || []).map((r) => `<button class="cite" data-chat="open" data-id="${escape(r.id)}">${escape(r.title)}</button>`);
        const docs = (m.docs || []).map((name) => `<span class="cite doc">${escape(name)}</span>`);
        const refs = notes.length || docs.length ? `<div class="msg-refs">${[...notes, ...docs].join('')}</div>` : '';
        return `<div class="msg user">${refs}<div class="bubble">${escape(m.text).replace(/\n/g, '<br>')}</div></div>`;
      }
      const shown = grade(m.text).text;
      let html = shown ? cite(render(shown), m.sources, escape).replace(/<input /g, '<input disabled ') : '';
      if (!html && m.pending) html = '<span class="typing"><i></i><i></i><i></i></span>';
      const used = cited(m.text, m.sources);
      const from = !m.pending && used.length > 1
        ? `<div class="msg-from">${used.map((s) => `<button class="cite" data-chat="open" data-id="${escape(s.id)}">${escape(s.title)}</button>`).join('')}</div>`
        : '';
      const error = m.error ? `<div class="msg-error">${escape(m.error)}</div>` : '';
      const verdict = grade(m.text).verdict;
      const badge = verdict ? `<span class="verdict ${verdict}">${verdict === 'correct' ? 'Correct' : 'Not quite'}</span>` : '';
      return `<div class="msg leo${m.pending ? ' pending' : ''}" data-i="${i}">${badge}<div class="prose">${html}</div>${error}${from}</div>`;
    }

    function onNote() {
      return Boolean(state.context && state.dropped !== state.context.id);
    }

    function asking() {
      const a = state.asking;
      if (!a) return '';
      const body = a.starter.needs === 'note'
        ? '<p class="chat-ask-how">Pick one below, or type @ and its name.</p>'
        : a.folders === null
          ? '<p class="chat-ask-how">Looking at your folders…</p>'
          : a.folders.length
            ? `<div class="chat-ask-classes">${a.folders.map((f) => `<button class="chat-class${a.picked.includes(f) ? ' on' : ''}" data-chat="class" data-name="${escape(f)}" aria-pressed="${a.picked.includes(f)}">${escape(f)}</button>`).join('')}</div>
              <div class="chat-ask-go"><button class="btn sm plain" data-chat="classes-all">All of them</button><button class="btn sm primary" data-chat="classes-go"${a.picked.length ? '' : ' disabled'}>Go</button></div>`
            : '<p class="chat-ask-how">You have no folders yet, so I will use all your notes.</p><div class="chat-ask-go"><button class="btn sm primary" data-chat="classes-all">Go</button></div>';
      return `<div class="chat-ask"><div class="chat-ask-top">${felix(34, 'idle')}<b>${escape(a.starter.ask)}</b><button class="chat-ask-x" data-chat="ask-cancel" aria-label="Never mind">×</button></div>${body}</div>`;
    }

    function welcome() {
      const info = modeInfo();
      const where = onNote() ? 'this note' : 'your notes';
      return `<div class="chat-hello">
        ${felix(96, 'idle big')}
        <h3>Hi, I'm Felix!</h3>
        <p>${escape(info.hint)}. I also read ${where}, and the notes connected to it on your map.</p>
        <div class="chat-starters">${info.starters.map((s, i) => `<button class="starter${state.asking && state.asking.starter === s ? ' on' : ''}" data-chat="starter" data-i="${i}">${escape(s.text)}</button>`).join('')}</div>
        ${asking()}
      </div>`;
    }

    async function startWith(starter) {
      const ready = onNote() || state.refs.length > 0;
      if (!starter.needs || ready) {
        state.asking = null;
        return send(starter.text);
      }
      state.asking = { starter, folders: starter.needs === 'classes' ? null : [], picked: [] };
      draw();
      if (starter.needs === 'note') {
        openPick('button', '', 0);
        return;
      }
      let folders = [];
      try {
        const response = await fetch('/api/folders', { credentials: 'same-origin' });
        if (response.ok) folders = (await response.json()).map((f) => f.name).filter((n) => n && !n.includes('/'));
      } catch (e) {
        folders = [];
      }
      if (state.asking && state.asking.starter === starter) {
        state.asking.folders = folders;
        draw();
      }
    }

    function answerClasses(all) {
      const a = state.asking;
      if (!a) return;
      const picked = all ? [] : a.picked;
      state.asking = null;
      send(all ? (a.starter.text === 'Quiz me across my classes' ? 'Quiz me across all my classes' : `${a.starter.text} for all my classes`) : starterWords(a.starter, picked));
    }

    function draw(stick = true) {
      const near = body.scrollHeight - body.scrollTop - body.clientHeight < 80;
      body.innerHTML = state.messages.length ? state.messages.map(bubble).join('') : welcome();
      if (stick || near) body.scrollTop = body.scrollHeight;
      $('#chat-send').classList.toggle('stop', Boolean(state.busy));
      $('#chat-send').setAttribute('aria-label', state.busy ? 'Stop' : 'Send');
      $('#chat-send').innerHTML = state.busy
        ? '<svg viewBox="0 0 24 24"><rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor"/></svg>'
        : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg>';
    }

    function fit() {
      input.style.height = 'auto';
      input.style.height = `${Math.min(input.scrollHeight, 160)}px`;
    }

    function react(message) {
      const verdict = grade(message.text).verdict;
      if (verdict === 'correct') {
        state.streak += 1;
        mood(state.streak >= 3 ? 'cheer' : 'dance', state.streak >= 3 ? 2400 : 1800);
      } else if (verdict === 'incorrect') {
        state.streak = 0;
        mood('droop', 1600);
        setTimeout(() => mood('perk', 700), 1650);
      }
    }

    async function send(text) {
      const question = text.trim();
      if (!question || state.busy) return;
      closePick();
      const ready = state.files.filter((f) => f.status === 'ready');
      state.messages.push({ role: 'user', text: question, refs: state.refs.slice(), docs: ready.map((f) => f.name), files: ready.map((f) => f.id) });
      state.sent = [...state.sent, ...ready.map((f) => ({ id: f.id, name: f.name }))];
      state.files = state.files.filter((f) => f.status !== 'ready');
      drawRefs();
      const files = state.sent.map((f) => f.id);
      const answer = { role: 'assistant', text: '', sources: [], pending: true };
      state.messages.push(answer);
      input.value = '';
      fit();
      const controller = new AbortController();
      state.busy = controller;
      const thread = { id: state.id, mode: state.mode, refs: state.refs.slice(), messages: state.messages };
      awake();
      mood('nod', 450);
      thinking(true);
      draw();
      const ctx = state.context && state.dropped !== state.context.id ? state.context.id : null;
      const history = state.messages.filter((m) => m !== answer && !m.error).map((m) => ({ role: m.role, text: grade(m.text).text || m.text }));
      try {
        const response = await fetch('/api/chat', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ messages: history, mode: state.mode, note: ctx, refs: state.refs.map((r) => r.id), chat: thread.id, files }),
          signal: controller.signal,
        });
        if (response.status === 401) throw new Error('This page needs its link again. Open the link leo serve printed.');
        if (!response.ok || !response.body) {
          let said = '';
          try {
            said = (await response.json()).error || '';
          } catch (e) {
            said = '';
          }
          throw new Error(said || `leo answered ${response.status}.`);
        }
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const { events, rest } = splitLines(buffer);
          buffer = rest;
          for (const e of events) {
            if (e.sources) answer.sources = e.sources;
            if (e.restart) answer.text = '';
            if (typeof e.t === 'string') {
              if (!answer.text) talking();
              answer.text += e.t;
            }
            if (e.error) answer.error = e.error;
          }
          draw(false);
        }
      } catch (e) {
        if (e.name !== 'AbortError') answer.error = e.message || "Can't reach leo. Is leo serve still running?";
      }
      answer.pending = false;
      if (!answer.text && !answer.error) answer.error = 'Stopped.';
      state.busy = null;
      thinking(false);
      draw(false);
      remember({ ...thread, messages: thread.messages.slice() });
      if (thread.id === state.id) react(answer);
      awake();
    }

    function toggle(force) {
      state.open = force === undefined ? !state.open : force;
      panel.hidden = !state.open;
      document.body.classList.toggle('chat-open', state.open);
      if (state.open) {
        panel.classList.toggle('with-history', sidebarOpen());
        if (sidebarOpen()) loadChats();
        drawModes();
        drawContext();
        if (state.filesFor !== state.id) loadFiles();
        drawRefs();
        draw();
        mood('wave', 1500);
        awake();
        if (root.matchMedia && root.matchMedia('(pointer: fine)').matches) input.focus();
      }
    }

    panel.addEventListener('pointermove', gaze);
    panel.addEventListener('keydown', awake);
    panel.addEventListener('click', (e) => {
      awake();
      if (!e.target.closest('[data-chat^="attach"]')) $('#chat-attach-menu').hidden = true;
      if (e.target.closest('.felix') && !e.target.closest('[data-chat]')) {
        tapped();
        return;
      }
      const el = e.target.closest('[data-chat]');
      if (!el) return;
      e.preventDefault();
      const what = el.dataset.chat;
      if (what === 'close') toggle(false);
      else if (what === 'new') {
        begin();
        if (!wide()) showSidebar(false);
      } else if (what === 'history') showSidebar(!panel.classList.contains('with-history'));
      else if (what === 'resume') resume(el.dataset.id);
      else if (what === 'forget') forget(el.dataset.id);
      else if (what === 'mode') {
        if (el.dataset.mode === state.mode) return;
        const refs = state.refs;
        if (state.messages.length) begin();
        state.asking = null;
        closePick();
        state.mode = el.dataset.mode;
        state.refs = refs;
        drawModes();
        drawRefs();
        draw();
        persist();
      } else if (what === 'starter') startWith(modeInfo().starters[Number(el.dataset.i)]);
      else if (what === 'ask-cancel') {
        state.asking = null;
        closePick();
        draw();
      } else if (what === 'class') {
        const a = state.asking;
        if (!a) return;
        const name = el.dataset.name;
        a.picked = a.picked.includes(name) ? a.picked.filter((n) => n !== name) : [...a.picked, name];
        draw();
      } else if (what === 'classes-go') answerClasses(false);
      else if (what === 'classes-all') answerClasses(true);
      else if (what === 'attach') {
        const menu = $('#chat-attach-menu');
        if (state.pick && state.pick.from === 'button') closePick();
        else if (!prepare) openPick('button', '', 0);
        else menu.hidden = !menu.hidden;
      } else if (what === 'attach-note') {
        $('#chat-attach-menu').hidden = true;
        openPick('button', '', 0);
      } else if (what === 'attach-file') {
        $('#chat-attach-menu').hidden = true;
        if (state.files.length + state.sent.length >= MOST_FILES) notify(`A chat holds up to ${MOST_FILES} documents; remove one first.`);
        else $('#chat-file').click();
      } else if (what === 'unfile') removeFile(el.dataset.id); else if (what === 'pick') choose(Number(el.dataset.i));
      else if (what === 'unref') {
        state.refs = state.refs.filter((r) => r.id !== el.dataset.id);
        drawRefs();
        remember();
      }
      else if (what === 'drop') {
        state.dropped = state.context && state.context.id;
        drawContext();
        if (!state.messages.length) draw();
      } else if (what === 'open') {
        if (!root.matchMedia || !root.matchMedia('(min-width: 900px)').matches) toggle(false);
        onOpen(el.dataset.id);
      }
    });
    $('#chat-form').addEventListener('submit', (e) => {
      e.preventDefault();
      if (state.busy) state.busy.abort();
      else send(input.value);
    });
    input.addEventListener('input', () => {
      fit();
      watchMention();
    });
    input.addEventListener('click', watchMention);
    $('#chat-file').addEventListener('change', (e) => {
      $('#chat-attach-menu').hidden = true;
      addFiles(e.target.files);
      e.target.value = '';
    });
    function steer(e) {
      const p = state.pick;
      if (!p || !p.notes.length) return false;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        p.at = (p.at + (e.key === 'ArrowDown' ? 1 : p.notes.length - 1)) % p.notes.length;
        drawPick();
        return true;
      }
      if ((e.key === 'Enter' || e.key === 'Tab') && !e.isComposing) {
        e.preventDefault();
        choose(p.at);
        return true;
      }
      return false;
    }
    panel.addEventListener('input', (e) => {
      if (e.target.id === 'chat-pick-search') openPick('button', e.target.value, 0);
    });
    panel.addEventListener('keydown', (e) => {
      if (e.target.id !== 'chat-pick-search') return;
      if (e.key === 'Escape') {
        e.stopPropagation();
        closePick();
        input.focus();
      } else steer(e);
    });
    input.addEventListener('keydown', (e) => {
      if (state.pick && e.key === 'Escape') {
        e.stopPropagation();
        closePick();
        return;
      }
      if (state.pick && state.pick.from === 'mention' && steer(e)) return;
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
        e.preventDefault();
        if (!state.busy) send(input.value);
      }
      if (e.key === 'Escape') {
        e.stopPropagation();
        toggle(false);
      }
    });

    return {
      toggle,
      isOpen: () => state.open,
      setContext(ctx) {
        const next = ctx && ctx.id ? { id: ctx.id, title: ctx.title } : null;
        if ((next && next.id) === (state.context && state.context.id) && (!next || next.title === state.context.title)) return;
        state.context = next;
        if (state.open) {
          drawContext();
          if (!state.messages.length) draw();
        }
      },
      button: (width) => felix(width, 'idle'),
    };
  }

  root.leoChat = { create, felix, splitLines, grade, cite, cited, load, save, mentionAt, addRef, modeOf, groups, newId, starterWords, splitFiles, MODES, MOST_REFS };
})(typeof window !== 'undefined' ? window : globalThis);
