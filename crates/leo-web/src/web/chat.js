(function (root) {
  'use strict';

  const KEY = 'leo-chat-v1';
  const MOST_KEPT = 40;
  const MOST_REFS = 8;
  const ACCESS = [
    { id: 'ask', label: 'Ask before changing notes', short: 'Ask first', hint: 'Felix suggests changes and new notes; you apply each one.' },
    { id: 'auto', label: 'Change notes automatically', short: 'Auto', hint: 'Felix makes changes and new notes himself; each one can be undone.' },
    { id: 'read', label: 'Read only', short: 'Read only', hint: 'Felix reads your notes but never changes or makes any.' },
  ];
  const ACCESS_KEY = 'leo-felix-access';
  const accessOf = (id) => (ACCESS.some((a) => a.id === id) ? id : 'ask');
  const nextAccess = (id) => ACCESS[(ACCESS.findIndex((a) => a.id === accessOf(id)) + 1) % ACCESS.length].id;
  const ACCESS_GLYPHS = {
    ask: '<path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z"/><path d="M9.5 12l2 2 3.5-4"/>',
    auto: '<path d="M5 12h9M10 7l5 5-5 5"/><path d="M15 7l5 5-5 5"/>',
    read: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  };

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
        <g class="felix-tool felix-tool-search"><rect class="glass" x="20" y="14" width="8" height="8"/><rect class="metal" x="20" y="12" width="8" height="2"/><rect class="metal" x="20" y="22" width="8" height="2"/><rect class="metal" x="18" y="14" width="2" height="8"/><rect class="metal" x="28" y="14" width="2" height="8"/><rect class="wood" x="30" y="23" width="2" height="2"/><rect class="wood" x="32" y="25" width="2" height="2"/><rect class="wood" x="34" y="27" width="3" height="3"/></g>
        <g class="felix-tool felix-tool-open"><rect class="paper-old" x="10" y="22" width="24" height="8"/><rect class="ink-old" x="13" y="24" width="14" height="1"/><rect class="ink-old" x="13" y="27" width="10" height="1"/><rect class="roll" x="7" y="21" width="4" height="10"/><rect class="roll" x="33" y="21" width="4" height="10"/></g>
        <g class="felix-tool felix-tool-map"><rect class="paper-old" x="30" y="5" width="15" height="11"/><rect class="fold" x="35" y="5" width="1" height="11"/><rect class="fold" x="40" y="5" width="1" height="11"/><rect class="pin" x="32" y="8" width="2" height="2"/><rect class="pin" x="42" y="7" width="2" height="2"/><rect class="pin" x="37" y="12" width="2" height="2"/></g>
        <g class="felix-tool felix-tool-edit"><rect class="paper" x="8" y="21" width="16" height="10"/><rect class="ink" x="10" y="24" width="11" height="1"/><rect class="ink" x="10" y="27" width="8" height="1"/><g class="pen"><rect class="cap" x="29" y="18" width="2" height="2"/><rect class="barrel" x="27" y="20" width="2" height="2"/><rect class="barrel" x="25" y="22" width="2" height="2"/><rect class="tip" x="23" y="24" width="2" height="2"/></g></g>
        <g class="felix-tool felix-tool-create"><rect class="wood" x="42" y="5" width="2" height="15"/><rect class="metal" x="38" y="1" width="10" height="5"/><rect class="shine" x="39" y="2" width="2" height="1"/></g>
      </g>
    </svg>`;
  }

  const TAPS = ['boop', 'hop', 'spin', 'giggle'];
  const POSES = { search_notes: 'tool-search', open_note: 'tool-open', connected_notes: 'tool-map', edit_note: 'tool-edit', create_note: 'tool-create', web_search: 'tool-search', open_page: 'tool-open' };
  const poseOf = (tool) => POSES[tool] || null;

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

  const THUMB = /^data:image\/(?:jpeg|png);base64,[A-Za-z0-9+/=]+$/;
  const glyph = (paths) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
  const PAGE = '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/>';
  const FILE_GLYPHS = {
    pdf: glyph(`${PAGE}<path d="M9 13h6M9 17h4"/>`),
    doc: glyph(`${PAGE}<path d="M9 11h6M9 14h6M9 17h4"/>`),
    slides: glyph('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M12 16v4M8 20h8M7 9h6"/>'),
    sheet: glyph('<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M4 10h16M4 15h16M10 4v16"/>'),
    image: glyph('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/>'),
    text: glyph(`${PAGE}<path d="M9 12h6M9 16h6"/>`),
    other: glyph(PAGE),
  };

  function fileKind(name) {
    const found = String(name || '').match(/\.([a-z0-9]{1,8})$/i);
    const ext = found ? found[1].toLowerCase() : '';
    const tone = ext === 'pdf' ? 'pdf'
      : /^(docx?|rtf|odt|pages)$/.test(ext) ? 'doc'
        : /^(pptx?|key|odp)$/.test(ext) ? 'slides'
          : /^(xlsx?|csv|numbers|ods)$/.test(ext) ? 'sheet'
            : /^(png|jpe?g|gif|webp|heic|heif|bmp|tiff?|avif)$/.test(ext) ? 'image'
              : /^(md|markdown|txt|text)$/.test(ext) ? 'text' : 'other';
    return { label: ext ? (ext === 'jpeg' ? 'JPG' : ext.toUpperCase()) : 'FILE', tone };
  }

  function fileCard(f, escape, { remove = '', reading = false } = {}) {
    const kind = fileKind(f.name);
    const thumb = f.thumb && THUMB.test(f.thumb) ? f.thumb : '';
    const excerpt = typeof f.excerpt === 'string' ? f.excerpt.trim() : '';
    const peek = thumb
      ? `<img src="${escape(thumb)}" alt="">`
      : excerpt
        ? `<span class="file-page">${escape(excerpt)}</span>`
        : `<span class="file-glyph">${FILE_GLYPHS[kind.tone]}</span>`;
    const busy = reading ? '<span class="file-reading">Reading…</span>' : '';
    const x = remove ? `<button class="file-x" data-chat="unfile" data-id="${escape(remove)}" aria-label="Remove ${escape(f.name)}">×</button>` : '';
    return `<span class="file-card tone-${kind.tone}${reading ? ' reading' : ''}" title="${escape(f.name)}"><span class="file-peek">${peek}${busy}</span><span class="file-foot"><span class="file-badge">${FILE_GLYPHS[kind.tone]}${escape(kind.label)}</span><span class="file-name">${escape(f.name)}</span></span>${x}</span>`;
  }

  function pastedNames(files) {
    return [...files].map((f, i) => (f.name && !/^image\.\w+$/i.test(f.name)
      ? f
      : new File([f], `pasted-${i + 1}.${(f.type.split('/')[1] || 'png').replace('jpeg', 'jpg')}`, { type: f.type })));
  }

  function money(cost) {
    if (cost >= 0.995) return `$${cost.toFixed(2)}`;
    if (cost >= 0.01) return `$${cost.toFixed(3)}`;
    if (cost >= 0.0001) return `$${cost.toFixed(4)}`;
    return 'under $0.0001';
  }

  function spentLabel(spent) {
    if (!spent || typeof spent !== 'object' || typeof spent.by !== 'string') return null;
    const model = typeof spent.model === 'string' && spent.model ? spent.model : '';
    const effort = typeof spent.effort === 'string' && spent.effort ? `${spent.effort} effort` : '';
    const about = spent.estimated ? 'about ' : '';
    const steps = spent.steps > 1 ? ` over ${spent.steps} steps` : '';
    const title = `${about}${Number(spent.input || 0).toLocaleString('en-US')} tokens in, ${Number(spent.output || 0).toLocaleString('en-US')} out${steps}`;
    let parts;
    if (spent.plan) parts = [spent.by, model, effort || 'default effort', 'on your plan'];
    else if (spent.local) parts = [spent.by, model, 'free on this computer'];
    else if (typeof spent.cost === 'number') parts = [`${spent.estimated ? '≈ ' : ''}${money(spent.cost)}`, spent.by, model];
    else parts = [spent.by, model, 'price unknown'];
    return { text: parts.filter(Boolean).join(' · '), title };
  }

  function asNote(question, text, sources) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    const answer = grade(String(text || '')).text.replace(/\[(n\d+(?:\s*,\s*n\d+)*)\]/g, (whole, list) => {
      const links = list
        .split(/\s*,\s*/)
        .map((n) => byN.get(n))
        .filter(Boolean)
        .map((s) => `[[${s.title.replace(/[[\]|]/g, '')}]]`);
      return links.length ? links.join(' ') : whole;
    }).trim();
    const asked = String(question || '').replace(/\s+/g, ' ').trim();
    let title = asked;
    if (title.length > 70) {
      const cut = title.slice(0, 70);
      title = `${cut.slice(0, cut.lastIndexOf(' ') > 30 ? cut.lastIndexOf(' ') : 70)}…`;
    }
    return { title: title || 'From Felix', body: asked ? `**Q:** ${asked}\n\n${answer}` : answer };
  }

  function reviewPrompt(items) {
    const lines = items.map((m, i) => `${i + 1}. ${m.question}${m.answer ? ` (last time I said: ${m.answer})` : ''}`);
    return `Let's review questions I got wrong before. Ask me each one again, one at a time, and wait for my answer:\n${lines.join('\n')}`;
  }

  function mentionAt(text, caret) {
    const before = text.slice(0, caret);
    const at = before.lastIndexOf('@');
    if (at < 0 || (at > 0 && !/\s/.test(before[at - 1]))) return null;
    const query = before.slice(at + 1);
    if (/\n/.test(query) || query.length > 40 || /^\s/.test(query)) return null;
    return { start: at, query };
  }

  function threadRefs(messages) {
    const out = [];
    for (const m of [...messages].reverse()) {
      if (m.role !== 'user' || !Array.isArray(m.refs)) continue;
      for (const r of m.refs) {
        if (r && r.id && !out.some((x) => x.id === r.id) && out.length < MOST_REFS) out.push(r);
      }
    }
    return out;
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
  const NOTE_DRAG = 'application/x-leo-note';

  function create({ render, escape, onOpen = () => {}, storage = root.localStorage, prepare = null, notify = () => {}, onSaved = () => {}, onChanged = () => {}, onUndone = () => {}, onToggle = () => {} }) {
    const saved = load(storage);
    const state = { open: false, id: saved.id || newId(), mode: modeOf(saved.mode), messages: saved.messages, refs: saved.refs, context: null, dropped: null, busy: null, streak: 0, pick: null, chats: null, sidebar: null, doomed: null, asking: null, files: [], sent: [], filesFor: null, review: [] };
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
        <div class="chat-under">
          <button type="button" class="chat-access" id="chat-access" data-chat="access" aria-haspopup="menu" aria-expanded="false"></button>
          <span class="chat-under-hint">⇧Tab to switch</span>
          <div class="chat-access-menu" id="chat-access-menu" role="menu" hidden>${ACCESS.map((a) => `<button type="button" role="menuitemradio" data-chat="access-pick" data-access="${a.id}"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ACCESS_GLYPHS[a.id]}</svg><span><b>${escape(a.label)}</b><small>${escape(a.hint)}</small></span></button>`).join('')}</div>
        </div>
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
      if (!on) pose(null);
      for (const f of faces()) {
        f.classList.toggle('think', on);
        if (!on) f.classList.remove('talk');
      }
    }

    function talking() {
      pose(null);
      for (const f of faces()) {
        f.classList.remove('think');
        f.classList.add('talk');
      }
    }

    function pose(tool) {
      const wanted = poseOf(tool);
      for (const f of faces()) {
        for (const name of Object.values(POSES)) f.classList.toggle(name, name === wanted);
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

    try {
      state.access = accessOf(storage.getItem(ACCESS_KEY));
    } catch (e) {
      state.access = 'ask';
    }

    function drawAccess() {
      const now = ACCESS.find((a) => a.id === state.access);
      const button = $('#chat-access');
      button.className = `chat-access is-${now.id}`;
      button.title = `${now.label}: ${now.hint} Shift+Tab switches.`;
      button.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ACCESS_GLYPHS[now.id]}</svg><span>${escape(now.short)}</span><svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M7 10l5 5 5-5"/></svg>`;
      for (const item of panel.querySelectorAll('[data-chat="access-pick"]')) item.setAttribute('aria-checked', String(item.dataset.access === now.id));
    }

    function setAccess(id) {
      state.access = accessOf(id);
      try {
        storage.setItem(ACCESS_KEY, state.access);
      } catch (e) {}
      $('#chat-access-menu').hidden = true;
      $('#chat-access').setAttribute('aria-expanded', 'false');
      drawAccess();
    }
    drawAccess();

    const SIDEBAR = 'leo-chat-sidebar';
    const wide = () => Boolean(root.matchMedia && root.matchMedia('(min-width: 900px)').matches);

    const WIDTH = 'leo-chat-width';
    const LEAST_WIDTH = 420;
    const grip = document.createElement('div');
    grip.className = 'chat-resize';
    grip.tabIndex = 0;
    grip.title = 'Drag to make Felix wider or narrower; double-click to reset';
    grip.setAttribute('role', 'separator');
    grip.setAttribute('aria-orientation', 'vertical');
    grip.setAttribute('aria-label', 'Felix width');
    panel.prepend(grip);

    function keptWidth() {
      try {
        const kept = Number(storage.getItem(WIDTH));
        return kept > 0 ? kept : null;
      } catch (e) {
        return null;
      }
    }

    function widest() {
      const beside = parseFloat(root.getComputedStyle(document.body).paddingLeft) || 0;
      const room = root.innerWidth >= 1200 ? 380 : 120;
      return Math.max(LEAST_WIDTH, root.innerWidth - beside - room);
    }

    function setWidth(px, keep) {
      const style = document.documentElement.style;
      if (px === null) {
        style.removeProperty('--chat-w');
        grip.removeAttribute('aria-valuenow');
        if (keep) {
          try {
            storage.removeItem(WIDTH);
          } catch (e) {}
        }
        return;
      }
      const width = Math.round(Math.min(widest(), Math.max(LEAST_WIDTH, px)));
      style.setProperty('--chat-w', `${width}px`);
      grip.setAttribute('aria-valuenow', String(width));
      grip.setAttribute('aria-valuemin', String(LEAST_WIDTH));
      grip.setAttribute('aria-valuemax', String(widest()));
      if (keep) {
        try {
          storage.setItem(WIDTH, String(width));
        } catch (e) {}
      }
    }

    const fitWidth = () => setWidth(keptWidth(), false);
    fitWidth();
    root.addEventListener('resize', fitWidth);

    let dragFrom = null;
    let gripTap = null;
    grip.addEventListener('pointerdown', (e) => {
      if (!wide() || e.button !== 0) return;
      e.preventDefault();
      if (gripTap && e.timeStamp - gripTap.at < 400 && Math.abs(e.clientX - gripTap.x) < 6) {
        gripTap = null;
        setWidth(null, true);
        return;
      }
      dragFrom = { x: e.clientX, width: panel.getBoundingClientRect().width };
      grip.setPointerCapture(e.pointerId);
      document.body.classList.add('chat-resizing');
    });
    grip.addEventListener('pointermove', (e) => {
      if (!dragFrom || !grip.hasPointerCapture(e.pointerId)) return;
      setWidth(dragFrom.width + dragFrom.x - e.clientX, false);
    });
    const letGo = (e) => {
      if (!dragFrom || !grip.hasPointerCapture(e.pointerId)) return;
      const tapped = Math.abs(e.clientX - dragFrom.x) < 4;
      dragFrom = null;
      grip.releasePointerCapture(e.pointerId);
      document.body.classList.remove('chat-resizing');
      gripTap = tapped ? { at: e.timeStamp, x: e.clientX } : null;
      if (!tapped) setWidth(panel.getBoundingClientRect().width, true);
    };
    grip.addEventListener('pointerup', letGo);
    grip.addEventListener('pointercancel', letGo);
    grip.addEventListener('keydown', (e) => {
      const step = e.shiftKey ? 96 : 32;
      const now = panel.getBoundingClientRect().width;
      if (e.key === 'ArrowLeft') setWidth(now + step, true);
      else if (e.key === 'ArrowRight') setWidth(now - step, true);
      else if (e.key === 'Home') setWidth(null, true);
      else return;
      e.preventDefault();
    });

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
          if (!summary.named && summary.count >= 2 && !naming.has(summary.id)) {
            naming.add(summary.id);
            for (const wait of NAME_CHECKS) setTimeout(() => state.chats && loadChats(), wait);
          }
        } catch (e) {
          return;
        }
      });
      return upload;
    }

    const naming = new Set();
    const NAME_CHECKS = [8000, 25000, 60000];

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
          .map((c) => `<div class="chat-history-row${c.id === state.id ? ' on' : ''}"><button class="chat-history-item" data-chat="resume" data-id="${escape(c.id)}" title="${escape(c.about ? `${c.title}\n${c.about}` : c.title)}"><span class="chat-history-title">${escape(c.title)}</span>${c.about ? `<span class="chat-history-about">${escape(c.about)}</span>` : ''}</button><button class="chat-history-x${state.doomed === c.id ? ' sure' : ''}" data-chat="forget" data-id="${escape(c.id)}" aria-label="${state.doomed === c.id ? 'Confirm delete' : 'Delete'} ${escape(c.title)}">${state.doomed === c.id ? 'Delete' : '×'}</button></div>`)
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
      loadReview();
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
        .map((f) => (f.status === 'reading' ? fileCard(f, escape, { reading: true }) : fileCard(f, escape, { remove: f.id })))
        .join('');
      $('#chat-refs').innerHTML = (notes ? `<div class="chat-notes">${notes}</div>` : '') + (files ? `<div class="file-cards">${files}</div>` : '');
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

    async function thumbOf(file) {
      if (!/^image\//.test(file.type) || typeof createImageBitmap !== 'function') return null;
      try {
        const bitmap = await createImageBitmap(file);
        const scale = Math.min(1, 240 / Math.max(bitmap.width, bitmap.height));
        const canvas = document.createElement('canvas');
        canvas.width = Math.max(1, Math.round(bitmap.width * scale));
        canvas.height = Math.max(1, Math.round(bitmap.height * scale));
        canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height);
        const url = canvas.toDataURL('image/jpeg', 0.72);
        return THUMB.test(url) ? url : null;
      } catch (e) {
        return null;
      }
    }

    const reading = new Map();

    function addFile(file) {
      const key = `${Date.now()}-${Math.random()}`;
      state.files.push({ key, name: file.name, status: 'reading', thumb: null });
      drawRefs();
      const work = readFile(file, key, state.id);
      reading.set(key, work);
      work.then(() => reading.delete(key));
      return work;
    }

    async function readFile(file, key, chat) {
      const thumb = await thumbOf(file);
      if (thumb) {
        state.files = state.files.map((f) => (f.key === key ? { ...f, thumb } : f));
        drawRefs();
      }
      const drop = () => {
        state.files = state.files.filter((f) => f.key !== key);
        drawRefs();
        return null;
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
        notify(e.message || `Felix could not read ${file.name}.`);
        return drop();
      }
      if (!reply.ok) {
        let said = '';
        try {
          said = (await reply.json()).error || '';
        } catch (e) {
          said = '';
        }
        notify(said ? `Felix could not read ${file.name}: ${said}` : reply.status === 413 ? `${file.name} is too large for Felix.` : `Felix could not read ${file.name}.`);
        return drop();
      }
      const doc = { ...(await reply.json()), key, status: 'ready', thumb };
      if (state.id === chat) {
        state.files = state.files.map((f) => (f.key === key ? doc : f));
        drawRefs();
      }
      return doc;
    }

    function cardOf(f) {
      const card = { name: f.name };
      if (f.thumb) card.thumb = f.thumb;
      else if (f.excerpt) card.excerpt = String(f.excerpt).slice(0, 320);
      if (f.status === 'reading') card.reading = true;
      return card;
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
      const rows = p.notes.length
        ? p.notes.map((n, i) => `<button class="chat-pick-row${i === p.at ? ' on' : ''}" data-chat="pick" data-i="${i}" role="option" aria-selected="${i === p.at}"><b>${escape(n.title || 'Untitled')}</b><span>${escape(n.directory || 'All notes')}</span></button>`).join('')
        : `<div class="chat-pick-none">${p.loading ? 'Looking…' : state.refs.length >= MOST_REFS ? `Up to ${MOST_REFS} notes at once.` : 'No notes match.'}</div>`;
      const wantsSearch = p.from === 'button';
      let field = $('#chat-pick-search');
      let list = box.querySelector('.chat-pick-list');
      if (!list || Boolean(field) !== wantsSearch) {
        box.innerHTML = `${wantsSearch ? `<input class="chat-pick-search" id="chat-pick-search" placeholder="Find a note…" autocomplete="off" value="${escape(p.query)}">` : ''}<div class="chat-pick-list" role="listbox" aria-label="Notes"></div>`;
        field = $('#chat-pick-search');
        list = box.querySelector('.chat-pick-list');
      }
      list.innerHTML = rows;
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
        const cards = Array.isArray(m.cards)
          ? m.cards.filter((c) => c && typeof c.name === 'string')
          : [...(m.docs || []).map((name) => ({ name })), ...(m.pics || []).filter((p) => p && typeof p.name === 'string')];
        const refs = notes.length ? `<div class="msg-refs">${notes.join('')}</div>` : '';
        const files = cards.length ? `<div class="file-cards sent">${cards.filter(Boolean).map((c) => fileCard(c, escape, { reading: c.reading === true })).join('')}</div>` : '';
        return `<div class="msg user">${refs}${files}<div class="bubble">${escape(m.text).replace(/\n/g, '<br>')}</div></div>`;
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
      const label = m.pending ? null : spentLabel(m.spent);
      const meta = label ? `<span class="msg-meta" title="${escape(label.title)}">${escape(label.text)}</span>` : '';
      const save = !m.pending && !m.error && shown
        ? (m.saved
          ? `<button class="msg-act" data-chat="open" data-id="${escape(m.saved)}">Open the saved note</button>`
          : `<button class="msg-act" data-chat="save" data-i="${i}">Save as note</button>`)
        : '';
      const keep = save || meta ? `<div class="msg-acts">${save}${meta}</div>` : '';
      const steps = (m.steps || []).length ? `<div class="msg-steps">${m.steps.map((t, k) => stepLine(t, m.pending && !shown && k === m.steps.length - 1)).join('')}</div>` : '';
      const offers = (m.proposals || []).map((p, j) => proposalCard(p, i, j)).join('');
      return `<div class="msg leo${m.pending ? ' pending' : ''}" data-i="${i}">${badge}${steps}<div class="prose">${html}</div>${offers}${error}${from}${keep}</div>`;
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
        <p>${escape(info.hint)}. I also read ${where}, and the notes connected to it in your knowledge graph.</p>
        ${reviewCard()}
        <div class="chat-starters">${info.starters.map((s, i) => `<button class="starter${state.asking && state.asking.starter === s ? ' on' : ''}" data-chat="starter" data-i="${i}">${escape(s.text)}</button>`).join('')}</div>
        ${asking()}
      </div>`;
    }

    function reviewCard() {
      const all = state.review || [];
      const due = all.filter((m) => m.due);
      if (!due.length && !(state.mode === 'study' && all.length)) return '';
      const n = due.length || all.length;
      const what = `${n} question${n === 1 ? '' : 's'}`;
      return `<div class="chat-review">
        <b>${due.length ? `Time to review ${what} you missed` : `You missed ${what} recently`}</b>
        <span>From your study chats. Felix asks ${n === 1 ? 'it' : 'them'} again, one at a time.</span>
        <button class="btn primary" data-chat="review">Review now</button>
      </div>`;
    }

    async function loadReview() {
      try {
        const response = await fetch('/api/review', { credentials: 'same-origin' });
        if (!response.ok) return;
        state.review = await response.json();
      } catch (e) {
        return;
      }
      if (state.open && !state.messages.length) draw();
    }

    async function startReview() {
      const all = state.review || [];
      const due = all.filter((m) => m.due);
      const items = (due.length ? due : all).slice(0, 5);
      if (!items.length || state.busy) return;
      if (state.messages.length || state.mode !== 'study') begin();
      state.mode = 'study';
      const refs = [];
      for (const m of items) for (const r of m.notes || []) if (!refs.some((x) => x.id === r.id) && refs.length < MOST_REFS) refs.push(r);
      state.refs = refs;
      drawModes();
      drawRefs();
      const keys = items.map((m) => m.key);
      state.review = all.filter((m) => !keys.includes(m.key));
      send(reviewPrompt(items));
      try {
        await fetch('/api/review', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ done: keys }),
        });
      } catch (e) {
        return;
      }
    }

    const TOOL_ICONS = {
      search_notes: '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
      open_note: '<path d="M7 3h7l5 5v13H7z"/><path d="M14 3v5h5"/>',
      connected_notes: '<circle cx="6" cy="7" r="2.2"/><circle cx="18" cy="6" r="2.2"/><circle cx="12" cy="17.5" r="2.2"/><path d="M7.4 8.9l3.5 6.7M16.9 7.9l-3.8 7.8"/>',
      edit_note: '<path d="M4 20h4L19 9l-4-4L4 16z"/>',
      create_note: '<path d="M12 5v14M5 12h14"/>',
      web_search: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.2 3 14.8 0 18M12 3c-3 3.2-3 14.8 0 18"/>',
      open_page: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M7 13h10M7 16h6"/>',
    };

    function stepLine(step, working) {
      const item = typeof step === 'string' ? { text: step, tool: '', found: [] } : step;
      const icon = `<svg class="msg-step-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${TOOL_ICONS[item.tool] || '<circle cx="12" cy="12" r="3"/>'}</svg>`;
      const label = `${icon}<span>${escape(item.text || '')}</span>${working ? '<span class="msg-step-busy" aria-label="working"></span>' : ''}`;
      const found = (item.found || []).filter((f) => f && f !== item.text);
      if (!found.length || item.tool === 'open_note') return `<div class="msg-step">${label}</div>`;
      return `<details class="msg-step"><summary>${label}<span class="msg-step-count">${found.length}</span></summary><ul>${found.map((f) => `<li>${escape(f)}</li>`).join('')}</ul></details>`;
    }

    function proposalCard(p, i, j) {
      const where = `data-i="${i}" data-j="${j}"`;
      const done = p.state === 'applied'
        ? `<div class="proposal-done">${p.kind === 'create' ? 'Made' : 'Applied'} · <button class="msg-act" data-chat="open" data-id="${escape(p.made || p.note || '')}">Open the note</button><button class="msg-act" data-chat="undo" ${where}>Undo</button></div>`
        : p.state === 'dismissed'
          ? '<div class="proposal-done">Dismissed</div>'
          : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>${p.kind === 'create' ? 'Create' : 'Apply'}</button><button class="btn plain sm" data-chat="dismiss" ${where}>Dismiss</button></div>`;
      if (p.kind === 'create') {
        const body = String(p.body || '');
        return `<div class="proposal"><div class="proposal-title">New note: “${escape(p.title || 'Untitled')}”${p.folder ? ` in ${escape(p.folder)}` : ''}</div>
          <div class="proposal-body prose">${render(body.length > 900 ? `${body.slice(0, 900)}…` : body)}</div>${done}</div>`;
      }
      const old = p.find ? `<div class="proposal-old">${escape(p.find)}</div>` : '<div class="proposal-note">Added at the end:</div>';
      return `<div class="proposal"><div class="proposal-title">Change to “${escape(p.title || 'a note')}”</div>${p.why ? `<div class="proposal-why">${escape(p.why)}</div>` : ''}
        ${old}<div class="proposal-new">${escape(p.replace || '')}</div>${done}</div>`;
    }

    async function decide(i, j, apply) {
      const m = state.messages[i];
      const p = m && m.proposals && m.proposals[j];
      if (!p || p.state !== 'new') return;
      if (!apply) {
        p.state = 'dismissed';
        remember();
        draw(false);
        return;
      }
      const post = p.kind === 'create'
        ? ['/api/notes', { title: p.title, body: p.body || '', directory: p.folder || '' }]
        : [`/api/notes/${encodeURIComponent(p.note)}/suggestion`, { find: p.find || '', replace: p.replace || '' }];
      let note;
      try {
        const response = await fetch(post[0], {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(post[1]),
        });
        if (!response.ok) {
          let said = '';
          try {
            said = (await response.json()).error || '';
          } catch (e) {
            said = '';
          }
          throw new Error(said || (p.kind === 'create' ? 'The note could not be made.' : 'The change could not be applied.'));
        }
        note = await response.json();
      } catch (e) {
        notify(e.message);
        return;
      }
      p.state = 'applied';
      if (p.kind === 'create') {
        p.made = note.id;
        onSaved(note);
      } else {
        p.before = typeof note.before === 'string' ? note.before : null;
        p.after = note.version || null;
        onChanged(note);
      }
      remember();
      draw(false);
    }

    async function undo(i, j) {
      const m = state.messages[i];
      const p = m && m.proposals && m.proposals[j];
      if (!p || p.state !== 'applied') return;
      const create = p.kind === 'create';
      if (!create && (p.before === null || p.before === undefined || !p.after)) {
        notify('This change was applied before leo could undo it; open the note to change it back.');
        return;
      }
      const [url, method, body] = create
        ? [`/api/notes/${encodeURIComponent(p.made)}`, 'DELETE', undefined]
        : [`/api/notes/${encodeURIComponent(p.note)}`, 'PATCH', JSON.stringify({ body: p.before, base: p.after })];
      let response;
      try {
        response = await fetch(url, { method, credentials: 'same-origin', headers: body ? { 'Content-Type': 'application/json' } : {}, body });
      } catch (e) {
        notify("Can't reach leo. Is leo serve still running?");
        return;
      }
      if (response.status === 409) {
        notify('The note was edited after this change, so leo did not undo it. Open the note to change it back.');
        return;
      }
      if (!response.ok && response.status !== 404) {
        notify('The change could not be undone.');
        return;
      }
      const note = create ? { id: p.made, title: p.title } : await response.json().catch(() => ({ id: p.note, title: p.title }));
      p.state = 'new';
      delete p.before;
      delete p.after;
      delete p.made;
      remember();
      draw(false);
      onUndone(note, create);
    }

    async function saveAnswer(i) {
      const m = state.messages[i];
      if (!m || m.role !== 'assistant' || m.saved) return;
      let question = '';
      for (let j = i - 1; j >= 0; j--) {
        if (state.messages[j].role === 'user') {
          question = state.messages[j].text;
          break;
        }
      }
      const note = asNote(question, m.text, m.sources);
      const directory = typeof m.folder === 'string' ? m.folder : (state.context && state.context.directory) || '';
      let made;
      try {
        const response = await fetch('/api/notes', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ ...note, directory }),
        });
        if (!response.ok) throw new Error();
        made = await response.json();
      } catch (e) {
        notify('Felix could not save that answer as a note.');
        return;
      }
      m.saved = made.id;
      remember();
      draw(false);
      onSaved(made);
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
      const ctx = state.context && state.dropped !== state.context.id ? state.context.id : null;
      const access = state.access;
      const going = state.files.filter((f) => f.status === 'ready' || f.status === 'reading');
      const attached = state.refs.slice();
      state.files = state.files.filter((f) => !going.includes(f));
      state.refs = [];
      drawRefs();
      const asked = { role: 'user', text: question, refs: attached, docs: [], files: [], cards: going.map(cardOf) };
      const settle = (i, doc) => {
        if (!doc) {
          asked.cards[i] = null;
          return;
        }
        asked.cards[i] = cardOf(doc);
        asked.files.push(doc.id);
        if (!doc.thumb) asked.docs.push(doc.name);
        state.sent = [...state.sent, { id: doc.id, name: doc.name }];
      };
      going.forEach((f, i) => {
        if (f.status === 'ready') settle(i, f);
      });
      state.messages.push(asked);
      const answer = { role: 'assistant', text: '', sources: [], pending: true, folder: (state.context && state.context.directory) || '' };
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
      const waiting = going.map((f, i) => [f, i]).filter(([f]) => f.status === 'reading');
      if (waiting.length) {
        const docs = await Promise.all(waiting.map(([f]) => reading.get(f.key) || Promise.resolve(null)));
        waiting.forEach(([, i], k) => settle(i, docs[k]));
        draw();
      }
      asked.cards = asked.cards.filter(Boolean);
      if (!asked.cards.length) delete asked.cards;
      const files = state.sent.map((f) => f.id);
      const history = state.messages.filter((m) => m !== answer && !m.error).map((m) => ({ role: m.role, text: grade(m.text).text || m.text }));
      try {
        const response = await fetch('/api/chat', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ messages: history, mode: thread.mode, note: ctx, refs: threadRefs(thread.messages).map((r) => r.id), chat: thread.id, files, access }),
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
            if (typeof e.step === 'string') pose(e.tool);
            if (typeof e.step === 'string') answer.steps = [...(answer.steps || []), { text: e.step, tool: typeof e.tool === 'string' ? e.tool : '', found: Array.isArray(e.found) ? e.found.filter((f) => typeof f === 'string').slice(0, 12) : [] }];
            if (e.spent && typeof e.spent === 'object') answer.spent = e.spent;
            if (e.proposal && typeof e.proposal === 'object') {
              const done = e.proposal.state === 'applied';
              answer.proposals = [...(answer.proposals || []), { ...e.proposal, state: done ? 'applied' : 'new' }];
              if (done && e.proposal.kind === 'create' && e.proposal.made) onSaved({ id: e.proposal.made, title: e.proposal.title, directory: e.proposal.folder || '' });
              else if (done) onChanged({ id: e.proposal.note, title: e.proposal.title });
            }
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
      answer.at = new Date().toISOString();
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
      onToggle(state.open);
      if (state.open) {
        panel.classList.toggle('with-history', sidebarOpen());
        if (sidebarOpen()) loadChats();
        drawModes();
        drawContext();
        if (state.filesFor !== state.id) loadFiles();
        drawRefs();
        draw();
        loadReview();
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
      if (!e.target.closest('[data-chat^="access"]')) {
        $('#chat-access-menu').hidden = true;
        $('#chat-access').setAttribute('aria-expanded', 'false');
      }
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
      else if (what === 'save') saveAnswer(Number(el.dataset.i));
      else if (what === 'apply' || what === 'dismiss') decide(Number(el.dataset.i), Number(el.dataset.j), what === 'apply');
      else if (what === 'undo') undo(Number(el.dataset.i), Number(el.dataset.j));
      else if (what === 'review') startReview();
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
        loadReview();
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
      else if (what === 'access') {
        const menu = $('#chat-access-menu');
        menu.hidden = !menu.hidden;
        el.setAttribute('aria-expanded', String(!menu.hidden));
      } else if (what === 'access-pick') {
        setAccess(el.dataset.access);
        input.focus();
      } else if (what === 'attach') {
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
    const carries = (e, type) => Boolean(e.dataTransfer && [...e.dataTransfer.types].includes(type));
    panel.addEventListener('dragover', (e) => {
      if (!carries(e, NOTE_DRAG) && !(prepare && carries(e, 'Files'))) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      panel.classList.add('drop-here');
    });
    panel.addEventListener('dragleave', (e) => {
      if (!panel.contains(e.relatedTarget)) panel.classList.remove('drop-here');
    });
    panel.addEventListener('drop', (e) => {
      panel.classList.remove('drop-here');
      let note = null;
      try {
        note = carries(e, NOTE_DRAG) ? JSON.parse(e.dataTransfer.getData(NOTE_DRAG)) : null;
      } catch (err) {
        note = null;
      }
      const found = prepare ? [...e.dataTransfer.files] : [];
      if (!note && !found.length) return;
      e.preventDefault();
      e.stopPropagation();
      if (note) {
        if (state.refs.length >= MOST_REFS && !state.refs.some((r) => r.id === note.id)) notify(`A message can bring up to ${MOST_REFS} notes; remove one first.`);
        state.refs = addRef(state.refs, note);
        drawRefs();
        remember();
      }
      if (found.length) {
        if (state.files.length + state.sent.length >= MOST_FILES) notify(`A chat holds up to ${MOST_FILES} documents; remove one first.`);
        else addFiles(pastedNames(found));
      }
      input.focus();
    });

    input.addEventListener('paste', (e) => {
      const found = [...((e.clipboardData && e.clipboardData.files) || [])];
      if (!found.length) return;
      e.preventDefault();
      if (state.files.length + state.sent.length >= MOST_FILES) {
        notify(`A chat holds up to ${MOST_FILES} documents; remove one first.`);
        return;
      }
      addFiles(pastedNames(found));
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
      if (e.key === 'Tab' && e.shiftKey && !state.pick) {
        e.preventDefault();
        setAccess(nextAccess(state.access));
        return;
      }
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
        const next = ctx && ctx.id ? { id: ctx.id, title: ctx.title, directory: ctx.directory || '' } : null;
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

  root.leoChat = { threadRefs, ACCESS, accessOf, nextAccess, NOTE_DRAG, spentLabel, fileKind, fileCard, create, felix, splitLines, grade, cite, cited, load, save, mentionAt, addRef, modeOf, groups, newId, starterWords, splitFiles, asNote, reviewPrompt, pastedNames, poseOf, MODES, MOST_REFS };
})(typeof window !== 'undefined' ? window : globalThis);
