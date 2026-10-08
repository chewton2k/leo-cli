(function (root) {
  'use strict';

  const KEY = 'leo-chat-v1';
  const MOST_KEPT = 40;
  const MODES = [
    { id: 'ask', label: 'Ask', hint: 'Answers from your notes, with sources', starters: ['What are the key ideas here?', 'How does this connect to my other classes?', 'What should I review before the exam?'] },
    { id: 'coach', label: 'Coach', hint: 'Teaches with recall questions and hints', starters: ['Help me learn this note', 'Make me a 3-day review plan', 'Check whether I really understand this'] },
    { id: 'quiz', label: 'Quiz', hint: 'One question at a time, with a score', starters: ['Quiz me on this note', 'Quiz me across my classes', 'Give me 5 hard questions'] },
    { id: 'explain', label: 'Explain', hint: 'Plain words, an analogy, an example', starters: ['Explain this simply', 'What do people usually get wrong here?', 'Give me an analogy'] },
    { id: 'meeting', label: 'Meeting', hint: 'Decisions, action items, follow-ups', starters: ['Summarize this meeting', 'List the action items and owners', 'Draft a follow-up message'] },
  ];

  function felix(width, extra = '') {
    return `<svg class="felix ${extra}" viewBox="-2 -8 64 38" width="${width}" height="${Math.round((width * 38) / 64)}" shape-rendering="crispEdges" aria-hidden="true">
      <g class="felix-spark"><rect x="-1" y="-6" width="2" height="2"/><rect x="58" y="-4" width="2" height="2"/><rect x="29" y="-8" width="2" height="2"/></g>
      <g class="felix-body">
        <rect class="felix-arm felix-left" x="0" y="17" width="5" height="6"/>
        <rect class="felix-arm felix-right" x="55" y="17" width="5" height="6"/>
        <rect class="felix-skin" x="5" y="2" width="50" height="26"/>
        <g class="felix-eyes"><rect x="12" y="16" width="4" height="4"/><rect x="26" y="16" width="4" height="4"/></g>
      </g>
    </svg>`;
  }

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

  function load(storage) {
    try {
      const saved = JSON.parse(storage.getItem(KEY) || 'null');
      if (saved && Array.isArray(saved.messages)) return { mode: saved.mode || 'ask', messages: saved.messages };
    } catch (e) {
      return { mode: 'ask', messages: [] };
    }
    return { mode: 'ask', messages: [] };
  }

  function save(storage, mode, messages) {
    try {
      storage.setItem(KEY, JSON.stringify({ mode, messages: messages.slice(-MOST_KEPT) }));
    } catch (e) {
      return;
    }
  }

  function create({ render, escape, onOpen = () => {}, storage = root.localStorage }) {
    const saved = load(storage);
    const state = { open: false, mode: MODES.some((m) => m.id === saved.mode) ? saved.mode : 'ask', messages: saved.messages, context: null, dropped: null, busy: null, streak: 0 };
    const panel = document.createElement('aside');
    panel.className = 'chat';
    panel.id = 'chat';
    panel.hidden = true;
    panel.setAttribute('aria-label', 'Ask Felix');
    panel.innerHTML = `
      <header class="chat-head">
        <span class="chat-face" id="chat-face">${felix(40, 'idle')}</span>
        <span class="chat-name"><b>Felix</b><span>your study buddy</span></span>
        <button class="icon-btn" data-chat="new" aria-label="New chat" title="New chat"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button>
        <button class="icon-btn" data-chat="close" aria-label="Close"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button>
      </header>
      <div class="chat-modes" role="tablist" aria-label="How Felix helps">${MODES.map((m) => `<button role="tab" class="chat-mode" data-chat="mode" data-mode="${m.id}" title="${escape(m.hint)}">${m.label}</button>`).join('')}</div>
      <div class="chat-body" id="chat-body" aria-live="polite"></div>
      <footer class="chat-foot">
        <div class="chat-context" id="chat-context"></div>
        <form class="chat-compose" id="chat-form">
          <textarea id="chat-input" rows="1" placeholder="Ask Felix about your notes…" enterkeyhint="send"></textarea>
          <button class="chat-send" id="chat-send" type="submit" aria-label="Send"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg></button>
        </form>
      </footer>`;
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
        f.classList.remove('dance', 'droop', 'wave', 'cheer');
        void f.getBoundingClientRect();
        f.classList.add(name);
        setTimeout(() => f.classList.remove(name), ms);
      }
    }
    function thinking(on) {
      for (const f of faces()) f.classList.toggle('think', on);
    }
    blink();

    function persist() {
      save(storage, state.mode, state.messages.filter((m) => !m.pending));
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
      input.placeholder = state.mode === 'quiz' ? 'Answer, or ask for a question…' : state.mode === 'meeting' ? 'Ask about your meeting notes…' : 'Ask Felix about your notes…';
    }

    function drawContext() {
      const box = $('#chat-context');
      const ctx = state.context && state.dropped !== state.context.id ? state.context : null;
      box.innerHTML = ctx
        ? `<span class="chat-using">Reading <b>${escape(ctx.title || 'Untitled')}</b> and notes connected to it</span><button class="chat-drop" data-chat="drop" aria-label="Stop using this note">×</button>`
        : '<span class="chat-using">Searching all your notes</span>';
    }

    function bubble(m, i) {
      if (m.role === 'user') return `<div class="msg user"><div class="bubble">${escape(m.text).replace(/\n/g, '<br>')}</div></div>`;
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

    function welcome() {
      const info = modeInfo();
      const where = state.context && state.dropped !== state.context.id ? 'this note' : 'your notes';
      return `<div class="chat-hello">
        ${felix(96, 'idle big')}
        <h3>Hi, I'm Felix!</h3>
        <p>${escape(info.hint)}. I read ${where}, and the notes connected to it on your map.</p>
        <div class="chat-starters">${info.starters.map((s) => `<button class="starter" data-chat="starter">${escape(s)}</button>`).join('')}</div>
      </div>`;
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
        mood('droop', 1400);
      }
    }

    async function send(text) {
      const question = text.trim();
      if (!question || state.busy) return;
      state.messages.push({ role: 'user', text: question });
      const answer = { role: 'assistant', text: '', sources: [], pending: true };
      state.messages.push(answer);
      input.value = '';
      fit();
      const controller = new AbortController();
      state.busy = controller;
      thinking(true);
      draw();
      const ctx = state.context && state.dropped !== state.context.id ? state.context.id : null;
      const history = state.messages.filter((m) => m !== answer && !m.error).map((m) => ({ role: m.role, text: grade(m.text).text || m.text }));
      try {
        const response = await fetch('/api/chat', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ messages: history, mode: state.mode, note: ctx }),
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
            if (typeof e.t === 'string') answer.text += e.t;
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
      persist();
      react(answer);
    }

    function toggle(force) {
      state.open = force === undefined ? !state.open : force;
      panel.hidden = !state.open;
      document.body.classList.toggle('chat-open', state.open);
      if (state.open) {
        drawModes();
        drawContext();
        draw();
        mood('wave', 1500);
        if (root.matchMedia && root.matchMedia('(pointer: fine)').matches) input.focus();
      }
    }

    panel.addEventListener('click', (e) => {
      const el = e.target.closest('[data-chat]');
      if (!el) return;
      e.preventDefault();
      const what = el.dataset.chat;
      if (what === 'close') toggle(false);
      else if (what === 'new') {
        if (state.busy) state.busy.abort();
        state.messages = [];
        state.streak = 0;
        persist();
        draw();
        mood('wave', 1500);
      } else if (what === 'mode') {
        state.mode = el.dataset.mode;
        drawModes();
        if (!state.messages.length) draw();
        persist();
      } else if (what === 'starter') send(el.textContent);
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
    input.addEventListener('input', fit);
    input.addEventListener('keydown', (e) => {
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

  root.leoChat = { create, felix, splitLines, grade, cite, cited, load, save, MODES };
})(typeof window !== 'undefined' ? window : globalThis);
