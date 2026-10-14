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
      { text: 'Start a study session on this note', needs: 'note', ask: 'Which note should the session cover?' },
      { text: 'Quiz me across my classes', needs: 'classes', ask: 'Which classes should the quiz cover?' },
      { text: 'Make me a 3-day review plan', needs: 'classes', ask: 'Which classes should the plan cover?' },
    ] },
    { id: 'interview', label: 'Interview', hint: 'Practise coding, system design or a role-specific interview with a workspace and feedback', starters: [
      { text: 'Help me set up interview practice', ask: 'What kind of interview are you preparing for?' },
      { text: 'Research interviews for my target company', ask: 'Which company and role are you preparing for?' },
      { text: 'Plan my interview practice', ask: 'What is your interview date and weekly practice time?' },
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
      <g class="felix-rawr"><text x="38" y="2" font-size="7" font-weight="900" font-family="system-ui, sans-serif">RAWR!</text></g>
      <g class="felix-body">
        <g class="felix-arm felix-left"><rect class="felix-arm-skin" x="0" y="18" width="5" height="6"/><g class="felix-claws"><rect x="0" y="24" width="1" height="2"/><rect x="2" y="24" width="1" height="2"/><rect x="4" y="24" width="1" height="2"/></g></g>
        <g class="felix-arm felix-right"><rect class="felix-arm-skin" x="39" y="18" width="5" height="6"/><g class="felix-claws"><rect x="39" y="24" width="1" height="2"/><rect x="41" y="24" width="1" height="2"/><rect x="43" y="24" width="1" height="2"/></g></g>
        <rect class="felix-skin" x="5" y="2" width="34" height="28"/>
        <g class="felix-cheeks"><rect x="8" y="21" width="4" height="2"/><rect x="24" y="21" width="4" height="2"/></g>
        <g class="felix-gaze"><g class="felix-eyes" shape-rendering="geometricPrecision"><rect x="11" y="16" width="4" height="4"/><rect x="21" y="16" width="4" height="4"/></g></g>
        <g class="felix-sweat"><rect x="34" y="5" width="2" height="2"/><rect x="33" y="7" width="4" height="3"/></g>
        <g class="felix-tool felix-tool-search"><rect class="glass" x="20" y="14" width="8" height="8"/><rect class="metal" x="20" y="12" width="8" height="2"/><rect class="metal" x="20" y="22" width="8" height="2"/><rect class="metal" x="18" y="14" width="2" height="8"/><rect class="metal" x="28" y="14" width="2" height="8"/><rect class="wood" x="30" y="23" width="2" height="2"/><rect class="wood" x="32" y="25" width="2" height="2"/><rect class="wood" x="34" y="27" width="3" height="3"/></g>
        <g class="felix-tool felix-tool-open"><rect class="paper-old" x="10" y="22" width="24" height="8"/><rect class="ink-old" x="13" y="24" width="14" height="1"/><rect class="ink-old" x="13" y="27" width="10" height="1"/><rect class="roll" x="7" y="21" width="4" height="10"/><rect class="roll" x="33" y="21" width="4" height="10"/></g>
        <g class="felix-tool felix-tool-map"><rect class="paper-old" x="30" y="5" width="15" height="11"/><rect class="fold" x="35" y="5" width="1" height="11"/><rect class="fold" x="40" y="5" width="1" height="11"/><rect class="pin" x="32" y="8" width="2" height="2"/><rect class="pin" x="42" y="7" width="2" height="2"/><rect class="pin" x="37" y="12" width="2" height="2"/></g>
        <g class="felix-tool felix-tool-edit"><rect class="paper" x="8" y="21" width="16" height="10"/><rect class="ink" x="10" y="24" width="11" height="1"/><rect class="ink" x="10" y="27" width="8" height="1"/><g class="pen"><rect class="cap" x="29" y="18" width="2" height="2"/><rect class="barrel" x="27" y="20" width="2" height="2"/><rect class="barrel" x="25" y="22" width="2" height="2"/><rect class="tip" x="23" y="24" width="2" height="2"/></g></g>
        <g class="felix-monster"><g class="felix-horns"><rect x="9" y="-1" width="4" height="3"/><rect x="10" y="-3" width="2" height="2"/><rect x="31" y="-1" width="4" height="3"/><rect x="32" y="-3" width="2" height="2"/></g><g class="felix-spikes"><rect x="16" y="0" width="2" height="2"/><rect x="20" y="-1" width="3" height="3"/><rect x="25" y="0" width="2" height="2"/></g><g class="felix-brows"><rect x="9" y="12" width="2" height="1"/><rect x="11" y="13" width="2" height="1"/><rect x="13" y="14" width="2" height="1"/><rect x="25" y="12" width="2" height="1"/><rect x="23" y="13" width="2" height="1"/><rect x="21" y="14" width="2" height="1"/></g><rect class="felix-maw" x="13" y="23" width="12" height="4"/><g class="felix-fangs"><rect x="14" y="23" width="2" height="2"/><rect x="22" y="23" width="2" height="2"/></g></g>
        <g class="felix-tool felix-tool-create"><rect class="wood" x="42" y="5" width="2" height="15"/><rect class="metal" x="38" y="1" width="10" height="5"/><rect class="shine" x="39" y="2" width="2" height="1"/></g>
      </g>
    </svg>`;
  }

  const TAPS = ['boop', 'hop', 'spin', 'giggle', 'tickle'];
  const MONSTER_ONE_IN = 10;
  const POSES = { search_notes: 'tool-search', open_note: 'tool-open', connected_notes: 'tool-map', edit_note: 'tool-edit', create_note: 'tool-create', web_search: 'tool-search', open_page: 'tool-open', read_document: 'tool-open', read_transcript: 'tool-open', look_at_picture: 'tool-search', calculate: 'tool-edit' };
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

  const VERDICT_WITHIN = 240;

  function grade(text) {
    const value = String(text || '');
    const match = /\[\[(correct|incorrect)\]\]\s*/i.exec(value);
    if (match && match.index <= VERDICT_WITHIN) return { verdict: match[1].toLowerCase(), text: value.slice(match.index + match[0].length) };
    const head = value.slice(0, VERDICT_WITHIN + 16);
    if (/\[\[?[a-z]*\]?$/i.test(head.trimEnd()) && head.length < VERDICT_WITHIN + 16) return { verdict: null, text: value.slice(0, value.search(/\[\[?[a-z]*\]?$/i)) };
    return { verdict: null, text: value };
  }

  const MOST_REPLY_STEPS = 64;

  function timeline(text, steps) {
    const value = String(text || '');
    const parts = [];
    let at = 0;
    for (const raw of (Array.isArray(steps) ? steps : []).slice(0, MOST_REPLY_STEPS)) {
      const item = typeof raw === 'string' ? { text: raw } : raw;
      if (!item || typeof item.text !== 'string' || !item.text) continue;
      let end = Number.isInteger(item.at) ? Math.max(at, Math.min(value.length, item.at)) : at;
      if (end > 0 && end < value.length && /[\uD800-\uDBFF]/.test(value[end - 1]) && /[\uDC00-\uDFFF]/.test(value[end])) end++;
      if (end > at) parts.push({ text: value.slice(at, end) });
      const step = { text: item.text, tool: typeof item.tool === 'string' ? item.tool : '', found: Array.isArray(item.found) ? item.found.filter((f) => typeof f === 'string').slice(0, 12) : [] };
      if (parts.at(-1)?.steps) parts.at(-1).steps.push(step);
      else parts.push({ steps: [step] });
      at = end;
    }
    if (at < value.length) parts.push({ text: value.slice(at) });
    return parts;
  }

  const QUIZ_KINDS = { multiple_choice: 'Multiple choice', fill_blank: 'Fill in the blank', free_response: 'Free response' };

  function plainAnswer(text) {
    return String(text || '').toLowerCase().normalize('NFKD').replace(/[\u0300-\u036f]/g, '').replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
  }

  function accepted(quiz) {
    return (quiz.kind === 'fill_blank' ? String(quiz.answer || '').split('|') : [String(quiz.answer || '')]).map((a) => a.trim()).filter(Boolean);
  }

  function checkQuiz(quiz, given) {
    if (!quiz || quiz.kind !== 'multiple_choice') return null;
    const said = plainAnswer(given);
    return Boolean(said) && accepted(quiz).some((a) => plainAnswer(a) === said);
  }

  function quizSay(quiz, given, correct) {
    const next = 'If we are practising, ask the next question with the quiz tool so it appears as a card; never ask a practice question in plain text. If you keep a score, give it once, at the very end of your reply.';
    const right = 'praise me briefly in your own words, then explain why it is right: the idea behind it, and a quick way to see it or remember it next time';
    const lines = [`Quiz answer. Question: ${quiz.question}`, `My answer: ${given}`];
    if (correct === null) {
      const key = quiz.kind === 'fill_blank' ? `Accepted answers: ${accepted(quiz).join(' or ')}.` : `A model answer to compare with: ${quiz.answer}.`;
      lines.push(`Mark it: start your reply with [[correct]] or [[incorrect]]. Judge what my answer means, not how it is written: the same value or idea in other words or notation counts as right (sin x and sin(x), 1/2 and 0.5, f'(x) and df/dx). Be critical of the reasoning: a right result reached by a wrong or missing step, or an answer too vague to show I understand, is [[incorrect]]. If the question has more than one good answer (design, writing, interpretation), mark me right when I make a defensible case with the relevant principles, and treat the model answer as one strong answer, not the only one. Even if I am right, point out any gap, jump or loose term and what a complete answer would add. ${key} If I am right, ${right}. If I am wrong, I have already tried, so do not give a hint: give the right answer and explain it fully, say where my answer went wrong, and end with an intuitive shortcut or tip for keeping it in mind. ${next}`);
    } else if (correct) {
      lines.push(`leo checked it: right. Start your reply with [[correct]], then ${right}. ${next}`);
    } else {
      lines.push(`leo checked it: wrong; the right answer is ${accepted(quiz)[0] || quiz.answer}. Start your reply with [[incorrect]]. I have already tried, so do not give a hint: explain why the right answer is right, then why each of the other choices is wrong, and end with a quick way to tell them apart next time. ${next}`);
    }
    return lines.join('\n');
  }

  function hintsOf(quiz) {
    const list = Array.isArray(quiz.hints) ? quiz.hints : [];
    return quiz.hint && typeof quiz.hint === 'object' ? [quiz.hint, ...list] : list;
  }

  function hintSay(quiz) {
    const choices = quiz.kind === 'multiple_choice' && (quiz.options || []).length ? `\nChoices: ${quiz.options.join(' | ')}` : '';
    const given = hintsOf(quiz).map((h) => grade(String(h.text || '')).text.trim()).filter(Boolean);
    const before = given.length ? `\nHints you already gave me:\n${given.map((h, n) => `${n + 1}. ${h}`).join('\n')}\nI still need help, so give the next hint: one step further than those, without repeating them.` : '\nGive me one short hint that helps me work it out myself: point me at the idea or first step.';
    return `Hint, please. Question: ${quiz.question}${choices}${before}\nDo not give the answer, do not say which choice is right or wrong, do not mark anything and do not ask a new question.`;
  }

  const CITED = /\s?\[((?:[nf]\d+(?:\s+[LlPpSs]\d+(?:\s*[-–]\s*\d+)?)?)(?:\s*[,;]\s*[nf]\d+(?:\s+[LlPpSs]\d+(?:\s*[-–]\s*\d+)?)?)*)\]/g;

  function citations(list) {
    return list
      .split(/\s*[,;]\s*/)
      .map((item) => {
        const m = /^([nf])(\d+)(?:\s+([LlPpSs])(\d+)(?:\s*[-–]\s*(\d+))?)?$/.exec(item.trim());
        if (!m) return null;
        const kind = m[3] ? m[3].toUpperCase() : '';
        const from = m[4] ? Number(m[4]) : null;
        const to = m[5] ? Number(m[5]) : from;
        return { id: `${m[1]}${m[2]}`, file: m[1] === 'f', kind, from, to };
      })
      .filter(Boolean);
  }

  function placeWord(c, name) {
    if (c.from === null) return '';
    if (c.kind === 'L') return c.to !== c.from ? `lines ${c.from}–${c.to}` : `line ${c.from}`;
    const slide = /\.pptx?$/i.test(name || '') || c.kind === 'S';
    return `${slide ? 'slide' : 'p.'} ${c.from}`.replace('p. ', 'p.');
  }

  function cite(html, sources, escape, files) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    const byF = new Map((files || []).map((f) => [`f${f.n}`, f]));
    const short = (t) => (t.length > 28 ? t.slice(0, 27) + '…' : t);
    return html.replace(CITED, (whole, list) => {
      const chips = citations(list)
        .map((c) => {
          if (c.file) {
            const f = byF.get(c.id);
            if (!f) return '';
            const at = c.from === null ? '' : `${c.kind === 'L' ? 'L' : 'p'}${c.from}${c.to !== c.from ? `-${c.to}` : ''}`;
            const where = placeWord(c, f.name);
            return `<button class="cite cite-file" data-chat="original" data-note="${escape(f.note)}" data-name="${escape(f.name)}" data-at="${escape(at)}" title="${escape(`${f.name}${where ? `, ${where}` : ''} (from “${f.title}”)`)}">${escape(short(f.name))}${where ? `<span class="cite-at">${escape(where)}</span>` : ''}</button>`;
          }
          const s = byN.get(c.id);
          if (!s) return '';
          const lines = c.kind === 'L' && c.from !== null ? `${c.from}-${c.to}` : '';
          const where = lines ? placeWord(c) : '';
          return `<button class="cite" data-chat="open" data-id="${escape(s.id)}"${lines ? ` data-line="${lines}"` : ''} title="${escape(`${s.title}${where ? `, ${where}` : ''}`)}">${escape(short(s.title))}${where ? `<span class="cite-at">${escape(where)}</span>` : ''}</button>`;
        })
        .filter(Boolean);
      return chips.length ? `${whole.startsWith(' ') ? ' ' : ''}${chips.join('')}` : '';
    });
  }

  function cited(text, sources) {
    const used = new Set();
    for (const m of String(text).matchAll(CITED)) for (const c of citations(m[1])) if (!c.file) used.add(c.id);
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
    const cached = Number(spent.cached || 0) > 0 ? ` (${Number(spent.cached).toLocaleString('en-US')} from the cache)` : '';
    const title = `${about}${Number(spent.input || 0).toLocaleString('en-US')} tokens in${cached}, ${Number(spent.output || 0).toLocaleString('en-US')} out${steps}`;
    let parts;
    if (spent.plan) parts = [spent.by, model, effort || 'default effort', 'on your plan'];
    else if (spent.local) parts = [spent.by, model, 'free on this computer'];
    else if (typeof spent.cost === 'number') parts = [`${spent.estimated ? '≈ ' : ''}${money(spent.cost)}`, spent.by, model];
    else parts = [spent.by, model, 'price unknown'];
    return { text: parts.filter(Boolean).join(' · '), title };
  }

  function linkCitations(text, sources) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    return String(text || '').replace(/\[(n\d+(?:\s*,\s*n\d+)*)(?:\s+L\d+(?:-\d+)?)?\]/g, (whole, list) => {
      const links = list
        .split(/\s*,\s*/)
        .map((n) => byN.get(n))
        .filter(Boolean)
        .map((s) => `[[${s.title.replace(/[[\]|]/g, '')}]]`);
      return links.length ? links.join(' ') : whole;
    });
  }

  function squash(text) {
    return String(text || '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '');
  }

  function wholeBlocks(lines, a, b) {
    let fence = -1;
    let math = -1;
    const opens = [];
    for (let k = 0; k < lines.length; k++) {
      const line = lines[k].trim();
      if (math < 0 && /^(```|~~~)/.test(line)) {
        if (fence < 0) fence = k;
        else {
          opens.push([fence, k]);
          fence = -1;
        }
      } else if (fence < 0 && line === '$$') {
        if (math < 0) math = k;
        else {
          opens.push([math, k]);
          math = -1;
        }
      }
    }
    for (const [start, end] of opens) {
      if (a > start && a <= end) a = start;
      if (b >= start && b < end) b = end;
    }
    return [a, b];
  }

  function answerLines(source, picked) {
    const lines = String(source || '').split('\n');
    const want = squash(picked);
    if (!want) return '';
    let joined = '';
    const ends = [];
    for (const line of lines) {
      joined += squash(line);
      ends.push(joined.length);
    }
    const lineAt = (pos) => ends.findIndex((e) => e > pos);
    let from = joined.indexOf(want);
    let to = from + want.length - 1;
    if (from < 0) {
      const ends = Math.max(4, Math.min(16, Math.floor(want.length / 2)));
      let start = -1;
      for (let k = 0; k + ends <= want.length && k <= 200 && start < 0; k++) start = joined.indexOf(want.slice(k, k + ends));
      if (start < 0) return String(picked).trim();
      let end = -1;
      for (let k = want.length; k - ends >= 0 && k >= want.length - 200 && end < 0; k--) {
        const tail = want.slice(k - ends, k);
        const at = joined.indexOf(tail, start);
        if (at >= 0) end = at + tail.length - 1;
      }
      from = start;
      to = end < 0 ? start + ends - 1 : end;
    }
    const [a, b] = wholeBlocks(lines, lineAt(from), lineAt(to));
    return lines.slice(a, b + 1).join('\n').trim();
  }

  function asNote(question, text, sources) {
    const answer = linkCitations(grade(String(text || '')).text, sources).trim();
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

  function recentNotes(messages, most = 6) {
    const out = [];
    for (const m of [...messages].reverse().filter((x) => x.role === 'assistant').slice(0, 3)) {
      const opened = (m.sources || []).filter((s) => s && /^(opened|changed) by Felix|its pictures looked at/.test(String(s.why || '')));
      for (const s of [...cited(m.text || '', m.sources || []), ...opened]) {
        if (s && s.id && !out.includes(s.id) && out.length < most) out.push(s.id);
      }
    }
    return out;
  }

  const clipped = (text, most) => {
    const flat = String(text || '').replace(/\s+/g, ' ').trim();
    return flat.length > most ? `${flat.slice(0, most - 1)}…` : flat;
  };

  const MOST_IDEAS = 12;
  const MOST_SESSION_QUESTIONS = 20;

  function startSession(ideas) {
    const list = (Array.isArray(ideas) ? ideas : []).filter((i) => typeof i === 'string' && i.trim()).slice(0, MOST_IDEAS).map((i) => i.trim().slice(0, 80));
    return { ideas: list, outcomes: list.map(() => ({ state: 'open', misses: 0 })), asked: 0, state: 'active' };
  }

  function scoreSession(session, idea, correct, more) {
    if (!session || session.state !== 'active' || !idea || correct === null || correct === undefined) return session;
    const outcome = session.outcomes[idea - 1];
    if (!outcome || outcome.state !== 'open') return session;
    if (correct === false) outcome.misses += 1;
    if (outcome.misses >= 2) outcome.state = 'review';
    else if (!more && correct === true) outcome.state = outcome.misses ? 'done' : 'first';
    if (session.outcomes.every((o) => o.state !== 'open') || session.asked >= MOST_SESSION_QUESTIONS) session.state = 'finishing';
    return session;
  }

  function sessionBody(session) {
    if (!session || (session.state !== 'active' && session.state !== 'finishing')) return null;
    return { ideas: session.ideas, closed: session.outcomes.map((o) => o.state !== 'open'), asked: session.asked };
  }

  function sessionSummary(session) {
    const pick = (test) => session.ideas.filter((_, i) => test(session.outcomes[i]));
    return {
      total: session.ideas.length,
      covered: session.outcomes.filter((o) => o.state !== 'open').length,
      first: pick((o) => o.state === 'first').length,
      review: pick((o) => o.state === 'review'),
      skipped: pick((o) => o.state === 'open'),
    };
  }

  function passageLine(p) {
    if (!p || typeof p.text !== 'string') return '';
    const lines = p.from ? ` lines ${p.from}${p.to > p.from ? `–${p.to}` : ''}` : '';
    return `[Selected passage from “${clipped(p.title || 'a note', 80)}”${lines}: “${clipped(p.text, 300)}”]`;
  }

  function historyText(m) {
    if (m.role === 'user') return [interviewLine(m.interview), passageLine(m.selection), m.say || m.text].filter(Boolean).join('\n');
    const lines = [];
    const by = m.spent && typeof m.spent.by === 'string' ? [m.spent.by, m.spent.model].filter(Boolean).join(' · ') : '';
    if (by) lines.push(`[Written by ${by}${m.mode ? `, in ${m.mode} style` : ''}]`);
    const steps = (m.steps || []).map((s) => (typeof s === 'string' ? { text: s, found: [] } : s)).filter((s) => s && s.text);
    if (steps.length) {
      lines.push(`[What Felix did for this answer: ${steps.slice(0, 12).map((s) => {
        const found = (s.found || []).filter((f) => typeof f === 'string' && f !== s.text).slice(0, 5);
        return clipped(s.text, 120) + (found.length ? ` (found: ${found.map((f) => clipped(f, 60)).join(', ')})` : '');
      }).join('; ')}]`);
    }
    for (const a of m.asks || []) {
      lines.push(`[Felix asked: ${clipped(a.question, 300)}${(a.options || []).length ? ` (choices: ${a.options.join(' | ')})` : ''}${typeof a.answered === 'string' ? `; the user answered: ${clipped(a.answered, 200)}` : ''}]`);
    }
    for (const q of m.quizzes || []) {
      const how = typeof q.given !== 'string' ? 'not answered yet' : q.correct === true ? `answered “${clipped(q.given, 120)}”, right` : q.correct === false ? `answered “${clipped(q.given, 120)}”, wrong` : `answered “${clipped(q.given, 200)}”`;
      const replied = q.reply && q.reply.text ? `; Felix replied: ${clipped(grade(q.reply.text).text, 400)}` : '';
      lines.push(`[Practice question (${(QUIZ_KINDS[q.kind] || 'question').toLowerCase()}): ${clipped(q.question, 300)}${(q.options || []).length ? `; choices: ${q.options.join(' | ')}` : ''}; answer: ${clipped(q.answer, 200)}; ${how}${replied}]`);
    }
    for (const p of m.proposals || []) {
      const state = p.state === 'applied' ? 'applied' : p.state === 'dismissed' ? 'dismissed by the user' : 'waiting for the user';
      const what = p.kind === 'create' ? `Suggested a new note “${clipped(p.title, 80)}”` : p.kind === 'edit' ? `Suggested a change to “${clipped(p.title, 80)}”: “${clipped(p.find, 80)}” → “${clipped(p.replace, 120)}”` : `Suggested: ${clipped(p.summary, 160)}`;
      lines.push(`[${what}; ${state}]`);
    }
    lines.push(grade(m.text || '').text || m.text || '');
    return lines.filter(Boolean).join('\n');
  }

  function interviewLine(view) {
    if (!view || !Number.isInteger(view.number) || view.number < 1 || view.number > MOST_INTERVIEW_PROBLEMS || typeof view.challenge !== 'string' || !/^[a-zA-Z0-9-]{8,64}$/.test(view.challenge) || typeof view.title !== 'string') return '';
    const title = [];
    for (const c of view.title) { if (title.length >= MOST_INTERVIEW_TITLE) break; title.push(c); }
    const data = JSON.stringify({ number: view.number, challenge: view.challenge, title: title.join(''), tab: ['Code', 'Approach', 'Whiteboard', 'Answer'].includes(view.tab) ? view.tab : '' }).replace(/</g, '\\u003c');
    return `<interview_workspace_reference>\n${data}\n</interview_workspace_reference>`;
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
    const boards = new Set();
    for (const m of messages || []) {
      if (m.role !== 'user') continue;
      for (const card of m.cards || []) if (card.interviewBoard) boards.add(card.name);
      for (const id of m.files || []) ids.add(id);
      if (!m.files) for (const name of m.docs || []) names.add(name);
    }
    const sent = [];
    const waiting = [];
    for (const d of docs || []) (ids.has(d.id) || names.has(d.name) ? sent : waiting).push(boards.has(d.name) ? { ...d, interviewBoard: true } : d);
    return { sent, waiting };
  }
  const FILE_TYPES = '.pdf,.docx,.pptx,.txt,.md,image/*';
  const NOTE_DRAG = 'application/x-leo-note';

  const CHAT_LEAVE_MS = 160;
  const PERMISSION_MS = 5000;
  const MOST_INTERVIEW_PROBLEMS = 128;
  const MOST_INTERVIEW_TITLE = 160;

  function create({ render, escape, icons = {}, onOpen = () => {}, onOriginal = () => {}, storage = root.localStorage, prepare = null, notify = () => {}, onSaved = () => {}, onChanged = () => {}, onUndone = () => {}, onToggle = () => {}, onInterview = () => {}, onInterviewChanged = () => {}, onInterviewReply = () => {}, onAdd = () => {}, onOrganized = () => {}, onBeforeSend = () => true, onCodeEdit = null }) {
    const saved = load(storage);
    const stepChevron = (icons.chevron || '').replace('<svg ', '<svg class="msg-step-chevron" ');
    const state = { open: false, id: saved.id || newId(), mode: modeOf(saved.mode), messages: saved.messages, refs: saved.refs, context: null, dropped: null, busy: null, streak: 0, pick: null, chats: null, sidebar: null, doomed: null, asking: null, files: [], sent: [], filesFor: null, review: [], drafts: {}, found: null, selection: null };
    const panel = document.createElement('aside');
    panel.className = 'chat';
    panel.id = 'chat';
    panel.hidden = true;
    panel.setAttribute('aria-label', 'Ask Felix');
    panel.innerHTML = `
      <nav class="chat-history" id="chat-history" aria-label="Your chats">
        <div class="chat-history-head"><button class="icon-btn chat-history-back" data-chat="history" aria-label="Back to the chat"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg></button><b>Chats</b><button class="chat-history-new" data-chat="new"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>New chat</button></div>
        <div class="chat-history-find"><input id="chat-history-search" type="search" placeholder="Search your chats" autocomplete="off" aria-label="Search your chats"></div>
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
      <button class="chat-workspace" data-chat="interview-workspace" hidden><span>Open interview workspace</span>${icons.chevron || ''}</button>
      <div class="chat-modes" role="tablist" aria-label="How Felix helps">${MODES.map((m) => `<button role="tab" class="chat-mode" data-chat="mode" data-mode="${m.id}" title="${escape(m.hint)}">${m.label}</button>`).join('')}</div>
      <div class="study-progress" id="study-progress" hidden></div>
      <div class="chat-body" id="chat-body" aria-live="polite"></div>
      <footer class="chat-foot">
        <div class="chat-context" id="chat-context"></div>
        <div class="chat-pick" id="chat-pick" hidden></div>
        <div class="chat-refs" id="chat-refs"></div>
        <form class="chat-compose" id="chat-form">
          <div class="chat-attach-menu" id="chat-attach-menu" role="menu" hidden><button type="button" role="menuitem" data-chat="attach-note">${icons.note || ''}<span>A note from leo</span></button>${prepare ? `<button type="button" role="menuitem" data-chat="attach-file">${icons.paperclip || ''}<span>A file from this device</span></button>` : ''}</div>
          <input type="file" id="chat-file" multiple accept="${FILE_TYPES}" hidden>
          <button class="chat-attach" type="button" data-chat="attach" aria-haspopup="menu" aria-controls="chat-attach-menu" aria-expanded="false" aria-label="Add a note or a file" title="Add a note or a file to the conversation (or type @ for a note)"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11l-8.5 8.5a5 5 0 0 1-7-7L14 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 7"/></svg></button>
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

    function attachMenu(open) {
      $('#chat-attach-menu').hidden = !open;
      $('[data-chat="attach"]').setAttribute('aria-expanded', String(open));
    }

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
        f.classList.remove('dance', 'droop', 'wave', 'cheer', 'nod', 'wake', 'perk', 'monster', 'grow-down', ...TAPS);
        const box = f.getBoundingClientRect();
        if (name === 'monster' && box.top < box.height * 2.5) f.classList.add('grow-down');
        f.classList.add(name);
        setTimeout(() => f.classList.remove(name, ...(name === 'monster' ? ['grow-down'] : [])), ms);
      }
    }
    const working = new WeakMap();
    function workingFor(since) {
      const secs = Math.max(0, Math.round((Date.now() - since) / 1000));
      return `Working · ${secs >= 60 ? `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}` : `${secs} s`}`;
    }
    const ticking = setInterval(() => {
      if (typeof document === 'undefined') return;
      for (const el of document.querySelectorAll('.msg-clock[data-since]')) el.textContent = workingFor(Number(el.dataset.since));
    }, 1000);
    if (ticking.unref) ticking.unref();

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
    let transformed = 0;
    function tapped() {
      if (Date.now() < transformed) return;
      if (Math.random() < 1 / MONSTER_ONE_IN) {
        transformed = Date.now() + 2800;
        mood('monster', 2800);
        return;
      }
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
      if (state.busy) { state.busy.permissionWanted = state.access; postPermission(state.busy); }
    }
    drawAccess();

    async function postPermission(work) {
      if (state.busy !== work || !work.answerId || work.permissionWorking || work.permissionPosted === work.permissionWanted) return;
      work.permissionWorking = true;
      const wanted = work.permissionWanted;
      let failed = false;
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), PERMISSION_MS);
      try {
        const reply = await fetch(`/api/chat/${encodeURIComponent(work.answerId)}/access`, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ access: wanted }), signal: controller.signal });
        if (!reply.ok && reply.status !== 410) throw new Error('Permissions could not be changed for the current reply. Send your message again with the access you want.');
        work.permissionPosted = wanted;
      } catch (e) {
        failed = true;
        if (state.busy === work) { work.abort(); notify('Permissions could not be changed for the current reply. Send your message again with the access you want.'); }
      } finally {
        clearTimeout(timer); work.permissionWorking = false;
        if (state.busy === work && work.permissionPosted !== work.permissionWanted && !failed && !controller.signal.aborted) setTimeout(() => postPermission(work), 0);
      }
    }

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
            state.chats = [summary, ...state.chats.filter((c) => c.id !== summary.id)].sort((a, b) => (Date.parse(b.updated_at) || 0) - (Date.parse(a.updated_at) || 0));
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

    let findTimer = 0;
    let findTurn = 0;
    async function findChats(query) {
      const turn = ++findTurn;
      if (!query.trim()) {
        state.found = null;
        drawChats();
        return;
      }
      try {
        const response = await fetch(`/api/chats?q=${encodeURIComponent(query.trim())}`, { credentials: 'same-origin' });
        if (!response.ok || turn !== findTurn) return;
        state.found = await response.json();
      } catch (e) {
        state.found = [];
      }
      if (turn === findTurn) drawChats();
    }

    function drawChats() {
      const box = $('#chat-history-list');
      if (!box) return;
      if (!state.chats) {
        box.innerHTML = '<div class="chat-history-none">Loading…</div>';
        return;
      }
      if (state.found) {
        box.innerHTML = state.found.length
          ? state.found.map((c) => chatRow(c)).join('')
          : '<div class="chat-history-none">No chat mentions that.</div>';
        return;
      }
      if (!state.chats.length) {
        box.innerHTML = '<div class="chat-history-none">Your conversations with Felix are kept here, on this computer.</div>';
        return;
      }
      box.innerHTML = groups(state.chats)
        .map((g) => `<div class="chat-history-group">${escape(g.name)}</div>${g.chats
          .map((c) => chatRow(c))
          .join('')}`)
        .join('');
    }

    function chatRow(c) {
      return `<div class="chat-history-row${c.id === state.id ? ' on' : ''}"><button class="chat-history-item" data-chat="resume" data-id="${escape(c.id)}" title="${escape(c.about ? `${c.title}\n${c.about}` : c.title)}"><span class="chat-history-title">${escape(c.title)}</span>${c.about ? `<span class="chat-history-about">${escape(c.about)}</span>` : ''}</button><button class="chat-history-x${state.doomed === c.id ? ' sure' : ''}" data-chat="forget" data-id="${escape(c.id)}" aria-label="${state.doomed === c.id ? 'Confirm delete' : 'Delete'} ${escape(c.title)}">${state.doomed === c.id ? 'Delete' : '×'}</button></div>`;
    }

    let resumeSeq = 0;

    function begin(id = newId()) {
      resumeSeq++;
      if (state.busy) state.busy.abort();
      state.id = id;
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
        return true;
      }
      if (state.busy) state.busy.abort();
      const mine = ++resumeSeq;
      let chat;
      try {
        const response = await fetch(`/api/chats/${encodeURIComponent(id)}`, { credentials: 'same-origin' });
        if (mine !== resumeSeq) return null;
        if (response.status === 404) {
          state.chats = (state.chats || []).filter((c) => c.id !== id);
          drawChats();
          return false;
        }
        if (!response.ok) throw new Error('Leo could not open that conversation. Try again.');
        chat = await response.json();
        if (mine !== resumeSeq) return null;
      } catch (e) {
        if (mine === resumeSeq) notify('Leo could not open that conversation. Check your connection and try again.');
        return null;
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
      await loadFiles();
      if (mine !== resumeSeq || state.id !== id) return null;
      drawModes();
      drawRefs();
      draw();
      drawChats();
      if (!wide()) showSidebar(false);
      return true;
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
      const p = state.selection;
      const passage = p
        ? `<span class="chat-ref chat-passage"><button class="chat-ref-open" data-chat="open" data-id="${escape(p.note)}" data-line="${p.from}-${p.to}" title="${escape(clipped(p.text, 400))}"><svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M7 7h4v4c0 2.5-1.5 4-4 5M14 7h4v4c0 2.5-1.5 4-4 5"/></svg>${escape(p.title || 'Note')}<span class="chat-passage-text">${escape(clipped(p.text.replace(/\s+/g, ' '), 60))}</span></button><button class="chat-ref-x" data-chat="unselect" aria-label="Remove the selected passage">×</button></span>`
        : '';
      const notes = passage + state.refs
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

    function addFile(file, interviewBoard = false) {
      const key = `${Date.now()}-${Math.random()}`;
      state.files.push({ key, name: file.name, status: 'reading', thumb: null });
      drawRefs();
      const work = readFile(file, key, state.id, interviewBoard);
      reading.set(key, work);
      work.then(() => reading.delete(key));
      return work;
    }

    async function readFile(file, key, chat, interviewBoard = false) {
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
      const doc = { ...(await reply.json()), key, status: 'ready', thumb, ...(interviewBoard ? { interviewBoard: true } : {}) };
      if (state.id === chat) {
        state.files = state.files.map((f) => (f.key === key ? doc : f));
        drawRefs();
      }
      return doc;
    }

    function cardOf(f) {
      const card = { name: f.name, ...(f.interviewBoard ? { interviewBoard: true } : {}) };
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

    function currentSession() {
      for (let k = state.messages.length - 1; k >= 0; k--) {
        const s = state.messages[k].session;
        if (s && typeof s === 'object' && Array.isArray(s.ideas) && Array.isArray(s.outcomes)) return s.dismissed ? null : s;
      }
      return null;
    }

    function drawProgress() {
      const box = $('#study-progress');
      const s = currentSession();
      if (!s || (s.state !== 'active' && s.state !== 'finishing')) {
        box.hidden = true;
        box.innerHTML = '';
        return;
      }
      const c = sessionSummary(s);
      const ideas = s.ideas.map((t, i) => {
        const o = s.outcomes[i];
        const mark = o.state === 'first' || o.state === 'done' ? '✓ ' : '';
        return `<span class="study-idea ${o.state}" title="${escape(t)}">${mark}${escape(t)}</span>`;
      }).join('<span class="study-sep" aria-hidden="true">·</span>');
      box.hidden = false;
      box.innerHTML = `<div class="study-ideas" aria-label="Ideas in this session">${ideas}</div><span class="study-count">${c.covered} of ${c.total}</span><button type="button" class="msg-act" data-chat="session-finish">Finish</button>`;
    }

    function recapCard(s) {
      const c = sessionSummary(s);
      const parts = [`${c.covered} of ${c.total} ${c.total === 1 ? 'idea' : 'ideas'} covered`];
      if (c.first) parts.push(`${c.first} right the first time`);
      const lines = [];
      if (c.review.length) lines.push(`<p><b>Look at again:</b> ${c.review.map(escape).join(', ')}</p>`);
      if (c.skipped.length) lines.push(`<p><b>Not reached:</b> ${c.skipped.map(escape).join(', ')}</p>`);
      const again = [...c.review, ...c.skipped].length;
      return `<div class="session-recap" role="status"><h4>Study session done</h4><p>${parts.join(' · ')}</p>${lines.join('')}<div class="session-acts">${again ? '<button type="button" class="btn primary sm" data-chat="session-again">Study those again</button>' : ''}<button type="button" class="btn plain sm" data-chat="session-done">Done</button></div></div>`;
    }

    function drawModes() {
      $('.chat-workspace').hidden = state.mode !== 'interview';
      for (const b of panel.querySelectorAll('.chat-mode')) {
        const on = b.dataset.mode === state.mode;
        b.classList.toggle('on', on);
        b.setAttribute('aria-selected', String(on));
      }
      drawContext();
      input.placeholder = state.mode === 'study' ? 'Answer, or ask for a question…' : 'Message Felix, or type @ to add a note…';
    }

    function drawContext() {
      const box = $('#chat-context');
      const ctx = state.context && state.dropped !== state.context.id ? state.context : null;
      const interview = state.mode === 'interview' && state.interview?.id === state.id ? state.interview : null;
      box.innerHTML = interview ? `<span class="chat-using">Problem ${interview.number} · <b>${escape(interview.title)}</b> · ${escape(interview.tab)}</span>` : ctx
        ? `<span class="chat-using">Also reading <b>${escape(ctx.title || 'Untitled')}</b> and notes connected to it</span><button class="chat-drop" data-chat="drop" aria-label="Stop using this note">×</button>`
        : '<span class="chat-using">Looks through your notes when they help</span>';
    }

    const modeName = (id) => (MODES.find((x) => x.id === id) || MODES[0]).label;
    const writer = (m) => (m && m.spent && typeof m.spent.by === 'string' ? [m.spent.by, m.spent.model].filter(Boolean).join(' · ') : '');

    function switches(list, i) {
      const m = list[i];
      const out = [];
      if (m.role === 'user' && m.mode) {
        const before = list.slice(0, i).reverse().find((x) => x.role === 'user' && x.mode);
        if (before && before.mode !== m.mode) out.push(`Now in ${modeName(m.mode)}`);
      }
      if (m.role === 'assistant' && writer(m)) {
        const before = list.slice(0, i).reverse().find((x) => x.role === 'assistant' && writer(x));
        if (before && writer(before) !== writer(m)) out.push(`Now answered by ${writer(m)}, with everything said so far`);
      }
      return out.map((t) => `<div class="chat-switch">${escape(t)}</div>`).join('');
    }

    function lastSwitch() {
      const last = [...state.messages].reverse().find((x) => x.role === 'user' && x.mode);
      return last && last.mode !== state.mode ? `<div class="chat-switch">Switched to ${escape(modeName(state.mode))}; the conversation carries on</div>` : '';
    }

    function bubble(m, i) {
      if (m.role === 'user') {
        const notes = (m.refs || []).map((r) => `<button class="cite" data-chat="open" data-id="${escape(r.id)}">${escape(r.title)}</button>`);
        const cards = Array.isArray(m.cards)
          ? m.cards.filter((c) => c && typeof c.name === 'string')
          : [...(m.docs || []).map((name) => ({ name })), ...(m.pics || []).filter((p) => p && typeof p.name === 'string')];
        const refs = notes.length ? `<div class="msg-refs">${notes.join('')}</div>` : '';
        const p = m.selection && typeof m.selection.text === 'string' ? m.selection : null;
        const quote = p
          ? `<div class="msg-passage"><button class="msg-passage-from" data-chat="open" data-id="${escape(p.note || '')}" data-line="${Number(p.from) || 1}-${Number(p.to) || Number(p.from) || 1}">${escape(p.title || 'Note')}${p.from ? ` · ${p.to > p.from ? `lines ${p.from}–${p.to}` : `line ${p.from}`}` : ''}</button><div class="msg-passage-text">${escape(clipped(p.text, 280))}</div></div>`
          : '';
        const files = cards.length ? `<div class="file-cards sent">${cards.filter(Boolean).map((c) => fileCard(c, escape, { reading: c.reading === true })).join('')}</div>` : '';
        const quiz = m.quiz && typeof m.quiz === 'object'
          ? `<div class="msg-note">${m.quiz.correct === true ? 'Practice answer · correct' : m.quiz.correct === false ? 'Practice answer · not quite' : 'Practice answer'}</div>`
          : '';
        const steer = m.steer === 'waiting' ? '<div class="msg-note">Felix reads this at his next step</div>' : m.steer === 'read' ? '<div class="msg-note">Sent while Felix worked</div>' : '';
        return `<div class="msg user${m.steer === 'waiting' ? ' queued' : ''}">${quote}${refs}${files}<div class="bubble">${escape(m.text).replace(/\n/g, '<br>')}</div>${quiz}${steer}</div>`;
      }
      const shown = grade(m.text).text;
      const html = !shown && m.pending ? '<span class="typing"><i></i><i></i><i></i></span>' : '';
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
          : `<button class="msg-act" data-chat="save" data-i="${i}">Save as note</button>`) + `<button class="msg-act" data-chat="add" data-i="${i}">Add to note</button>`
        : '';
      const keep = save || meta ? `<div class="msg-acts">${save}${meta}</div>` : '';
      const removed = verdict ? m.text.length - shown.length : 0;
      const parts = timeline(shown, (Array.isArray(m.steps) ? m.steps : []).slice(0, MOST_REPLY_STEPS).map((s) => typeof s === 'string' || !s ? s : { ...s, at: Number.isInteger(s.at) ? s.at - removed : 0 }));
      const body = parts.length ? parts.map((part, k) => part.steps
        ? `<div class="msg-steps">${part.steps.map((step, j) => stepLine(step, m.pending && k === parts.length - 1 && j === part.steps.length - 1)).join('')}</div>`
        : `<div class="prose">${cite(render(part.text), m.sources, escape, m.files).replace(/<input /g, '<input disabled ')}</div>`).join('') : `<div class="prose">${html}</div>`;
      const offers = (m.proposals || []).map((p, j) => proposalCard(p, i, j)).join('');
      const asks = (m.asks || []).map((a, k) => askCard(a, i, k, m.pending)).join('');
      const quizzes = (m.quizzes || []).map((q, k) => quizCard(q, i, k, m.pending, m.sources)).join('');
      const since = m.pending ? working.get(m) : 0;
      const clock = since ? `<div class="msg-note msg-clock" data-since="${since}" role="timer">${workingFor(since)}</div>` : '';
      return `<div class="msg leo${m.pending ? ' pending' : ''}" data-i="${i}">${badge}${body}${quizzes}${asks}${offers}${clock}${error}${from}${keep}</div>`;
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
      read_document: '<path d="M6 3h8l4 4v14H6z"/><path d="M9 12h6M9 16h6"/>',
      read_transcript: '<path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
      look_at_picture: '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="9" cy="11" r="2"/><path d="M21 17l-5-5-8 7"/>',
      calculate: '<rect x="5" y="3" width="14" height="18" rx="2"/><path d="M8 7h8M8 11h2M12 11h2M16 11v6M8 15h2M12 15h2M8 18h2M12 18h2"/>',
      ask_user: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17.2v.1"/>',
      quiz: '<path d="M9 6h11M9 12h11M9 18h11"/><path d="M4 6l1 1 2-2M4 12l1 1 2-2M4 18l1 1 2-2"/>',
    };

    function stepLine(step, working) {
      const item = typeof step === 'string' ? { text: step, tool: '', found: [] } : step;
      const icon = `<svg class="msg-step-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${TOOL_ICONS[item.tool] || '<circle cx="12" cy="12" r="3"/>'}</svg>`;
      const label = `${icon}<span>${escape(item.text || '')}</span>${working ? '<span class="msg-step-busy" aria-label="working"></span>' : ''}`;
      const found = (item.found || []).filter((f) => f && f !== item.text);
      if (!found.length || item.tool === 'open_note') return `<div class="msg-step">${label}</div>`;
      return `<details class="msg-step"><summary>${label}<span class="msg-step-count">${found.length}</span>${stepChevron}</summary><ul>${found.map((f) => `<li>${escape(f)}</li>`).join('')}</ul></details>`;
    }

    function proposalCard(p, i, j) {
      const where = `data-i="${i}" data-j="${j}"`;
      if (p.kind === 'interview_code_edit') {
        const actions = p.state === 'applied' ? `<div class="proposal-done">Accepted · <button class="msg-act" data-chat="undo" ${where}>Undo</button></div>` : p.state === 'dismissed' ? '<div class="proposal-done">Declined</div>' : p.state === 'applying' || p.state === 'undoing' ? '<div class="proposal-done" role="status">Saving code…</div>' : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>Accept</button><button class="btn plain sm" data-chat="dismiss" ${where}>Decline</button></div>`;
        return `<div class="proposal proposal-code"><div class="proposal-title">${escape(p.title || 'Code change')}</div><div class="proposal-why">${escape(p.summary || '')}</div><span class="proposal-note">Before</span><pre class="proposal-old">${escape(p.find || 'Empty code')}</pre><span class="proposal-note">After</span><pre class="proposal-new">${escape(p.replace || 'Remove this code')}</pre>${actions}</div>`;
      }
      if (p.kind === 'interview_problem_edit') {
        const before = p.before || {};
        const after = p.after || {};
        const shown = (v) => (typeof v === 'string' ? v : JSON.stringify(v ?? ''));
        const fields = [['title', 'Title'], ['question', 'Question'], ['minutes', 'Minutes'], ['skills', 'Skills']].map(([k, label]) => [label, before[k], after[k]])
          .concat([['starter', 'Starter'], ['difficulty', 'Difficulty'], ['examples', 'Examples'], ['constraints', 'Constraints'], ['language', 'Language']].map(([k, label]) => [label, (before.coding || {})[k], (after.coding || {})[k]]))
          .filter(([, a, b]) => JSON.stringify(a) !== JSON.stringify(b));
        const clip = (text) => { const t = shown(text); return t.length > 600 ? `${t.slice(0, 600)}…` : t; };
        const changes = fields.map(([label, a, b]) => `<span class="proposal-note">${escape(label)}</span><pre class="proposal-old">${escape(clip(a) || 'Empty')}</pre><pre class="proposal-new">${escape(clip(b) || 'Empty')}</pre>`).join('');
        const actions = p.state === 'applied' ? `<div class="proposal-done">Saved · <button class="msg-act" data-chat="undo" ${where}>Undo</button></div>` : p.state === 'dismissed' ? '<div class="proposal-done">Dismissed</div>' : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>Change problem</button><button class="btn plain sm" data-chat="dismiss" ${where}>Dismiss</button></div>`;
        return `<div class="proposal proposal-code"><div class="proposal-title">${escape(p.title || 'Problem')}</div><div class="proposal-why">${escape(p.summary || '')}</div>${changes}${actions}</div>`;
      }
      if (p.kind === 'interview_test') {
        const t = p.test || {};
        const call = (x) => `(${(Array.isArray(x.args) ? x.args : []).map((a) => JSON.stringify(a)).join(', ')}) → ${JSON.stringify(x.expected)}${x.hidden ? ' · hidden' : ''}`;
        const verb = p.action === 'add' ? 'Add' : p.action === 'delete' ? 'Delete' : 'Correct';
        const actions = p.state === 'applied' ? `<div class="proposal-done">Saved · <button class="msg-act" data-chat="undo" ${where}>Undo</button></div>` : p.state === 'dismissed' ? '<div class="proposal-done">Dismissed</div>' : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>${verb} test</button><button class="btn plain sm" data-chat="dismiss" ${where}>Dismiss</button></div>`;
        const before = p.before ? `<span class="proposal-note">Before</span><pre class="proposal-old">${escape(call(p.before))}</pre><span class="proposal-note">After</span>` : '';
        return `<div class="proposal proposal-code"><div class="proposal-title">${escape(`${p.title || 'Test case'} · ${verb} test ${t.id || ''}`)}</div><div class="proposal-why">${escape(p.summary || '')}</div>${before}<pre class="${p.action === 'delete' ? 'proposal-old' : 'proposal-new'}">${escape(call(t))}</pre>${actions}</div>`;
      }
      if (p.kind !== 'create' && p.kind !== 'edit') {
        const acts = p.kind === 'interview_schedule'
          ? '<div class="proposal-done">Practice schedules are no longer part of leo</div>'
          : p.state === 'applied'
          ? `<div class="proposal-done">Done · <button class="msg-act" data-chat="undo" ${where}>Undo</button></div>`
          : p.state === 'dismissed'
            ? '<div class="proposal-done">Dismissed</div>'
            : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>${p.kind === 'delete_note' || p.kind === 'delete_folder' ? 'Move to trash' : p.kind === 'restore_note' ? 'Bring it back' : 'Do it'}</button><button class="btn plain sm" data-chat="dismiss" ${where}>Dismiss</button></div>`;
        return `<div class="proposal organize ${escape(p.kind)}"><div class="proposal-title">${escape(p.summary || 'A change')}</div>${acts}</div>`;
      }
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

    const inline = (text) => render(String(text || '')).replace(/^<p>/, '').replace(/<\/p>$/, '');

    function askCard(a, i, k, pending) {
      const where = `data-i="${i}" data-k="${k}"`;
      const answered = typeof a.answered === 'string';
      const off = answered || pending ? ' disabled' : '';
      const options = (a.options || []).map((o, n) => `<button type="button" class="ask-option${answered && a.answered === o ? ' chosen' : ''}" data-chat="ask-pick" ${where} data-o="${n}"${off}>${inline(o)}</button>`).join('');
      const hint = answered ? `You answered: ${escape(a.answered)}` : options ? 'Tap one, or type your own answer below.' : 'Type your answer below.';
      return `<div class="ask-card"><div class="ask-q prose">${inline(a.question)}</div>${options ? `<div class="ask-options">${options}</div>` : ''}<div class="ask-hint">${hint}</div></div>`;
    }

    const BLANK = 'QUIZBLANKMARK';

    function quizCard(q, i, k, pending, sources) {
      const where = `data-i="${i}" data-k="${k}"`;
      const done = typeof q.given === 'string';
      const reply = q.reply && typeof q.reply === 'object' ? q.reply : null;
      const marking = Boolean(reply && reply.pending);
      const off = done || pending ? ' disabled' : '';
      const asked = cite(q.kind === 'fill_blank'
        ? render(String(q.question || '').replace(/_{3,}/g, BLANK)).split(BLANK).join('<span class="quiz-blank" aria-label="blank"></span>')
        : render(String(q.question || '')), sources, escape);
      const draft = escape(done ? q.given : state.drafts[`${i}:${k}`] || '');
      let answer = '';
      if (q.kind === 'multiple_choice') {
        answer = `<div class="quiz-options">${(q.options || []).map((o, n) => {
          const mark = !done ? '' : plainAnswer(o) === plainAnswer(q.answer) ? ' right' : o === q.given ? ' wrong' : '';
          return `<button type="button" class="quiz-option${mark}" data-chat="quiz-pick" ${where} data-o="${n}"${off}><span class="quiz-letter">${String.fromCharCode(65 + n)}</span><span>${inline(o)}</span></button>`;
        }).join('')}</div>`;
      } else if (q.kind === 'fill_blank') {
        answer = `<div class="quiz-row"><input class="quiz-input" ${where} placeholder="The missing words" autocomplete="off" value="${draft}"${off}><button type="button" class="btn primary sm" data-chat="quiz-check" ${where}${off}>Submit</button></div>`;
      } else {
        answer = `<div class="quiz-row column"><textarea class="quiz-input" ${where} rows="3" placeholder="Your answer, in your own words"${off}>${draft}</textarea><button type="button" class="btn primary sm" data-chat="quiz-check" ${where}${off}>Submit</button></div>`;
      }
      const result = marking && !(reply && reply.text) ? '<div class="quiz-result">Felix is reading your answer…</div>' : '';
      const hints = hintsOf(q);
      const hinted = hints
        .map((hint, n) => `<div class="quiz-hint prose">${hints.length > 1 ? `<span class="quiz-hint-n">Hint ${n + 1}</span>` : ''}${grade(hint.text || '').text ? cite(render(grade(hint.text).text), hint.sources || sources, escape, hint.files) : hint.pending ? '<span class="typing"><i></i><i></i><i></i></span>' : ''}</div>`)
        .join('');
      const waiting = hints.some((h) => h.pending);
      const askHint = !done && !pending && !waiting ? `<button type="button" class="quiz-hint-ask" data-chat="quiz-hint" ${where}>${hints.length ? 'Another hint' : 'Get a hint'}</button>` : '';
      const why = done && q.correct !== null && q.correct !== undefined && q.explain && !reply ? `<div class="quiz-explain">${cite(inline(q.explain), sources, escape)}</div>` : '';
      const said = reply
        ? (grade(reply.text || '').text
          ? `<div class="quiz-reply prose">${cite(render(grade(reply.text).text), reply.sources || sources, escape, reply.files)}</div>`
          : reply.pending ? '<div class="quiz-reply"><span class="typing"><i></i><i></i><i></i></span></div>' : '')
        : '';
      const tone = q.correct === true ? ' right' : q.correct === false ? ' wrong' : '';
      return `<div class="quiz-card${done ? ' done' : ''}${marking ? ' marking' : ''}${tone}"><div class="quiz-kind">${escape(QUIZ_KINDS[q.kind] || 'Question')}</div><div class="quiz-q prose">${asked}</div>${hinted}${answer}${askHint}${result}${why}${said}</div>`;
    }

    function askHint(i, k) {
      const m = state.messages[i];
      const q = m && m.quizzes && m.quizzes[k];
      if (!q || typeof q.given === 'string' || state.busy || m.pending || hintsOf(q).some((h) => h.pending)) return;
      send('Hint, please.', { say: hintSay(q), card: q, hint: true });
    }

    function answerAsk(i, k, given) {
      const m = state.messages[i];
      const a = m && m.asks && m.asks[k];
      if (!a || typeof a.answered === 'string' || state.busy) return;
      send(given);
    }

    function answerQuiz(i, k, given) {
      const m = state.messages[i];
      const q = m && m.quizzes && m.quizzes[k];
      const text = String(given || '').trim();
      if (!q || typeof q.given === 'string' || state.busy || m.pending || !text) return;
      const correct = checkQuiz(q, text);
      q.given = text;
      q.correct = correct;
      scoreSession(currentSession(), q.idea, correct, q.more);
      delete state.drafts[`${i}:${k}`];
      if (correct === true) {
        state.streak += 1;
        mood(state.streak >= 3 ? 'cheer' : 'dance', state.streak >= 3 ? 2400 : 1800);
      } else if (correct === false) {
        state.streak = 0;
        mood('droop', 1600);
        setTimeout(() => mood('perk', 700), 1650);
      }
      send(text, { say: quizSay(q, text, correct), quiz: { question: q.question, given: text, correct, kind: q.kind }, card: q });
    }

    async function postSteer(work, m) {
      if (!work.answerId || m.posted) return;
      m.posted = true;
      try {
        const response = await fetch(`/api/chat/${encodeURIComponent(work.answerId)}/steer`, {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ text: m.text }),
        });
        if (!response.ok) m.late = true;
      } catch (e) {
        m.late = true;
      }
    }

    function steerNow(text) {
      const said = text.trim();
      const work = state.busy;
      if (!said || !work) return;
      const m = { role: 'user', text: said, steer: 'waiting' };
      work.thread.messages.push(m);
      work.waiting.push(m);
      input.value = '';
      fit();
      draw();
      postSteer(work, m);
    }

    function steered(work, texts) {
      for (const text of texts) {
        const m = work.waiting.find((w) => w.steer === 'waiting' && w.text.slice(0, 4000) === text);
        if (!m) continue;
        m.steer = 'read';
        const list = work.thread.messages;
        list.splice(list.indexOf(m), 1);
        const at = list.indexOf(work.answer);
        list.splice(at < 0 ? list.length : at, 0, m);
      }
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
      if (p.kind !== 'create' && p.kind !== 'edit') return organize(p, p, true);
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

    async function organize(p, action, forward) {
      const sending = Object.fromEntries(Object.entries(action || {}).filter(([k]) => !['state', 'undo', 'task', 'path'].includes(k)));
      let done;
      const previous = p.state;
      if (p.kind === 'interview_code_edit') { p.state = forward ? 'applying' : 'undoing'; draw(false); }
      try {
        if (p.kind === 'interview_code_edit' && onCodeEdit) done = await onCodeEdit(sending);
        else {
          const response = await fetch('/api/felix/act', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(sending) });
          done = await response.json().catch(() => ({}));
          if (!response.ok) throw new Error(done.error || 'That could not be done.');
        }
      } catch (e) {
        p.state = previous; draw(false);
        notify(e.message || "Can't reach leo. Is leo serve still running?");
        return;
      }
      if (forward) {
        p.state = 'applied';
        p.undo = done.undo;
      } else {
        p.state = 'new';
        delete p.undo;
      }
      onOrganized(p);
      remember();
      draw(false);
    }

    async function undo(i, j) {
      const m = state.messages[i];
      const p = m && m.proposals && m.proposals[j];
      if (!p || p.state !== 'applied') return;
      if (p.kind !== 'create' && p.kind !== 'edit') {
        if (!p.undo) return notify('This change was made before leo could undo it.');
        return organize(p, p.undo, false);
      }
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

    function addAnswer(i, picked) {
      const m = state.messages[i];
      if (!m || m.role !== 'assistant' || m.pending) return;
      const source = grade(m.text || '').text;
      const chosen = picked ? answerLines(source, picked) : source;
      const text = linkCitations(chosen, m.sources).trim();
      if (text) onAdd({ text, folder: m.folder || '' });
    }

    const addFloat = root.document.createElement('button');
    addFloat.type = 'button';
    addFloat.className = 'float-act';
    addFloat.dataset.chat = 'add-picked';
    addFloat.textContent = 'Add to note';
    addFloat.hidden = true;
    addFloat.addEventListener('mousedown', (e) => e.preventDefault());
    panel.appendChild(addFloat);

    function hideAddFloat() {
      addFloat.hidden = true;
      state.picked = null;
    }

    function pickedInAnswer() {
      const selection = root.getSelection ? root.getSelection() : null;
      if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
      const range = selection.getRangeAt(0);
      const start = range.startContainer.nodeType === 1 ? range.startContainer : range.startContainer.parentElement;
      const end = range.endContainer.nodeType === 1 ? range.endContainer : range.endContainer.parentElement;
      const msg = start && start.closest('.msg.leo:not(.pending)');
      if (!msg || !body.contains(msg) || !end || end.closest('.msg.leo') !== msg || !start.closest('.prose') || !end.closest('.prose')) return null;
      const copy = range.cloneContents();
      for (const chip of copy.querySelectorAll('.cite, .katex-mathml, .msg-steps')) chip.remove();
      const text = copy.textContent || selection.toString();
      if (!text.trim()) return null;
      const box = range.getBoundingClientRect();
      return { i: Number(msg.dataset.i), text, box };
    }

    function placeAddFloat() {
      const picked = pickedInAnswer();
      if (!picked) return hideAddFloat();
      state.picked = picked;
      const area = panel.getBoundingClientRect();
      addFloat.hidden = false;
      const width = addFloat.offsetWidth;
      const left = Math.max(8, Math.min(picked.box.right - area.left - width / 2, area.width - width - 8));
      addFloat.style.left = `${left}px`;
      addFloat.style.top = `${picked.box.bottom - area.top + 6}px`;
    }

    root.document.addEventListener('selectionchange', () => {
      if (!state.open) return;
      clearTimeout(state.pickWait);
      state.pickWait = setTimeout(placeAddFloat, 120);
    });
    body.addEventListener('scroll', () => hideAddFloat(), { passive: true });

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
      const typing = document.activeElement && document.activeElement.classList && document.activeElement.classList.contains('quiz-input') && body.contains(document.activeElement)
        ? { i: document.activeElement.dataset.i, k: document.activeElement.dataset.k, from: document.activeElement.selectionStart, to: document.activeElement.selectionEnd }
        : null;
      const kept = body.scrollTop;
      const session = currentSession();
      const recap = session && session.state === 'finished' ? recapCard(session) : '';
      body.innerHTML = state.messages.length ? `${state.messages.map((m, i) => switches(state.messages, i) + bubble(m, i)).join('')}${recap}${lastSwitch()}` : welcome();
      drawProgress();
      if (typing) {
        const again = body.querySelector(`.quiz-input[data-i="${typing.i}"][data-k="${typing.k}"]`);
        if (again && !again.disabled) {
          again.focus();
          again.setSelectionRange(typing.from, typing.to);
          body.scrollTop = kept;
        }
      } else if (stick || near) body.scrollTop = body.scrollHeight;
      drawSend();
    }

    function drawSend() {
      const stop = Boolean(state.busy) && !input.value.trim();
      $('#chat-send').classList.toggle('stop', stop);
      $('#chat-send').setAttribute('aria-label', stop ? 'Stop' : state.busy ? 'Send to Felix while he works' : 'Send');
      $('#chat-send').innerHTML = stop
        ? '<svg viewBox="0 0 24 24"><rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor"/></svg>'
        : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg>';
      input.placeholder = state.busy ? 'Add to what Felix is doing…' : 'Message Felix, or type @ to add a note…';
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

    async function send(text, extra = {}) {
      const question = text.trim();
      if (!question || state.busy || state.preparingSend) return;
      const viewing = state.mode === 'interview' && state.interview?.id === state.id ? state.interview : null;
      if (state.mode === 'interview') {
        const id = state.id;
        let prepared = false;
        state.preparingSend = true;
        try { if (await onBeforeSend({ id, mode: state.mode, challenge: viewing?.challenge }) === false || state.id !== id || state.mode !== 'interview' || state.busy) return; prepared = true; }
        catch (e) { notify(e.message || 'Your practice could not be saved. Try saving it again before asking Felix.'); return; }
        finally { state.preparingSend = false; if (!prepared) onInterviewReply(id); }
      }
      const card = extra.card || null;
      if (!extra.quiz) {
        const last = [...state.messages].reverse().find((m) => m.role === 'assistant');
        for (const a of (last && last.asks) || []) if (typeof a.answered !== 'string') a.answered = question;
      }
      closePick();
      const ctx = state.context && state.dropped !== state.context.id ? state.context.id : null;
      let access = state.access;
      const going = card ? [] : state.files.filter((f) => f.status === 'ready' || f.status === 'reading');
      const attached = card ? [] : state.refs.slice();
      const passage = card ? null : state.selection;
      if (!card) {
        state.files = state.files.filter((f) => !going.includes(f));
        state.refs = [];
        state.selection = null;
        drawRefs();
      }
      const asked = { role: 'user', text: question, refs: attached, docs: [], files: [], cards: going.map(cardOf), ...(viewing ? { interview: { ...viewing } } : {}), ...(passage ? { selection: passage } : {}), ...(extra.say ? { say: extra.say } : {}), ...(extra.quiz ? { quiz: extra.quiz } : {}) };
      const settle = (i, doc) => {
        if (!doc) {
          asked.cards[i] = null;
          return;
        }
        asked.cards[i] = cardOf(doc);
        asked.files.push(doc.id);
        if (!doc.thumb) asked.docs.push(doc.name);
        state.sent = [...state.sent, { id: doc.id, name: doc.name, ...(doc.interviewBoard ? { interviewBoard: true } : {}) }];
      };
      going.forEach((f, i) => {
        if (f.status === 'ready') settle(i, f);
      });
      asked.mode = state.mode;
      const answer = { role: 'assistant', text: '', sources: [], pending: true, folder: (state.context && state.context.directory) || '', mode: state.mode };
      working.set(answer, Date.now());
      if (card && extra.hint) card.hints = [...(Array.isArray(card.hints) ? card.hints : []), answer];
      else if (card) card.reply = answer;
      else {
        state.messages.push(asked);
        state.messages.push(answer);
        if (input.value.trim() === question) input.value = '';
        fit();
      }
      const controller = new AbortController();
      const thread = { id: state.id, mode: state.mode, refs: state.refs.slice(), messages: state.messages };
      const work = { abort: () => controller.abort(), thread, answer, waiting: [], answerId: null, permissionWanted: state.access, permissionPosted: access, permissionWorking: false };
      state.busy = work;
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
      const history = state.messages.filter((m) => m !== answer && !m.error && m.steer !== 'waiting').map((m) => ({ role: m.role, text: historyText(m) }));
      if (card) history.push({ role: 'user', text: historyText(asked) });
      if (passage && history.length) history[history.length - 1] = { role: 'user', text: asked.say || asked.text };
      const sentRefs = threadRefs(thread.messages).map((r) => r.id);
      if (passage && passage.note && !sentRefs.includes(passage.note)) sentRefs.unshift(passage.note);
      try {
        access = state.access; work.permissionPosted = access;
        const response = await fetch('/api/chat', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ messages: history, mode: thread.mode, note: ctx, refs: sentRefs.slice(0, MOST_REFS), recent: recentNotes(thread.messages.filter((m) => m !== answer)), chat: thread.id, interview_problem: viewing?.challenge || null, files, access, practice: Boolean(card), mark: Boolean(card && !extra.hint && card.correct === null), selection: passage ? { title: passage.title, from: passage.from, to: passage.to, text: passage.text } : null, session: sessionBody(currentSession()) }),
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
            if (e.interview && typeof e.interview.id === 'string') onInterviewChanged(e.interview.id);
            if (typeof e.answer === 'string') {
              work.answerId = e.answer;
              postPermission(work);
              for (const m of work.waiting) postSteer(work, m);
            }
            if (Array.isArray(e.steered)) steered(work, e.steered);
            if (e.ask && typeof e.ask === 'object' && typeof e.ask.question === 'string') {
              answer.asks = [...(answer.asks || []), { question: e.ask.question, options: Array.isArray(e.ask.options) ? e.ask.options.filter((o) => typeof o === 'string') : [] }];
              pose('');
            }
            if (e.plan && typeof e.plan === 'object' && Array.isArray(e.plan.ideas)) {
              const before = currentSession();
              if (before && before.state !== 'finished') before.dismissed = true;
              answer.session = startSession(e.plan.ideas);
            }
            if (e.session_end === true) {
              const s = currentSession();
              if (s && s.state !== 'finished') s.state = 'finished';
            }
            if (e.quiz && typeof e.quiz === 'object' && typeof e.quiz.question === 'string') {
              const idea = Number(e.quiz.idea) || 0;
              const s = currentSession();
              if (s && s.state === 'active' && idea) s.asked += 1;
              answer.quizzes = [...(answer.quizzes || []), { kind: String(e.quiz.kind || ''), question: e.quiz.question, options: Array.isArray(e.quiz.options) ? e.quiz.options.filter((o) => typeof o === 'string') : [], answer: String(e.quiz.answer || ''), explain: String(e.quiz.explain || ''), ...(idea ? { idea } : {}), ...(e.quiz.more ? { more: true } : {}) }];
            }
            if (e.reset) {
              answer.text = '';
              answer.steps = [];
              delete answer.quizzes;
              delete answer.asks;
              answer.proposals = (answer.proposals || []).filter((p) => p.state === 'applied');
            }
            if (e.verdict === 'correct' || e.verdict === 'incorrect') answer.verdict = e.verdict;
            if (e.sources) answer.sources = e.sources;
            if (Array.isArray(e.files)) answer.files = e.files;
            if (e.restart) answer.text = '';
            if (typeof e.step === 'string') pose(e.tool);
            if (typeof e.step === 'string') answer.steps = [...(answer.steps || []), { text: e.step, at: answer.text.length, tool: typeof e.tool === 'string' ? e.tool : '', found: Array.isArray(e.found) ? e.found.filter((f) => typeof f === 'string').slice(0, 12) : [] }];
            if (e.spent && typeof e.spent === 'object') answer.spent = e.spent;
            if (e.proposal && typeof e.proposal === 'object') {
              const done = e.proposal.state === 'applied';
              const { auto, ...shown } = e.proposal;
              const card = { ...shown, state: done ? 'applied' : 'new' };
              answer.proposals = [...(answer.proposals || []), card];
              if (auto && card.kind === 'interview_code_edit') queueMicrotask(() => organize(card, card, true));
              if (done && e.proposal.kind === 'create' && e.proposal.made) onSaved({ id: e.proposal.made, title: e.proposal.title, directory: e.proposal.folder || '' });
              else if (done && e.proposal.kind === 'edit') onChanged({ id: e.proposal.note, title: e.proposal.title });
              else if (done) onOrganized(e.proposal);
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
      const asking = (answer.asks || []).length || (answer.quizzes || []).length;
      if (!answer.text && !answer.error && !asking) answer.error = 'Stopped.';
      const stopped = answer.error === 'Stopped.';
      if (card) {
        const extras = ['quizzes', 'asks', 'proposals'].filter((key) => (answer[key] || []).length);
        if (extras.length) {
          const next = { role: 'assistant', text: '', sources: answer.sources, files: answer.files, mode: answer.mode, at: answer.at };
          for (const key of extras) {
            next[key] = answer[key];
            delete answer[key];
          }
          thread.messages.push(next);
        }
        if (card.correct === null || card.correct === undefined) {
          const verdict = grade(answer.text).verdict || answer.verdict;
          card.correct = verdict === 'correct' ? true : verdict === 'incorrect' ? false : null;
          scoreSession(currentSession(), card.idea, card.correct, card.more);
        }
      }
      const ending = currentSession();
      if (ending && ending.state === 'finishing') ending.state = 'finished';
      const left = work.waiting.filter((m) => m.steer === 'waiting');
      for (const m of left) thread.messages.splice(thread.messages.indexOf(m), 1);
      for (const m of work.waiting) {
        delete m.posted;
        delete m.late;
      }
      state.busy = null;
      onInterviewReply(thread.id);
      thinking(false);
      if (stopped && left.length && thread.id === state.id) {
        input.value = [input.value, ...left.map((m) => m.text)].filter((t) => t.trim()).join('\n\n');
        fit();
      }
      draw(false);
      remember({ ...thread, messages: thread.messages.slice() });
      if (thread.id === state.id && !(asked.quiz && asked.quiz.correct !== null && asked.quiz.correct !== undefined)) react(answer);
      awake();
      if (!stopped && left.length && thread.id === state.id) send(left.map((m) => m.text).join('\n\n'));
    }

    let leaving = 0;

    function toggle(force) {
      const was = state.open;
      state.open = force === undefined ? !state.open : force;
      clearTimeout(leaving);
      const still = root.matchMedia && root.matchMedia('(prefers-reduced-motion: reduce)').matches;
      if (state.open) {
        panel.classList.remove('chat-leaving');
        panel.hidden = false;
        document.body.classList.add('chat-open');
        onToggle(true);
      } else if (was && !panel.hidden && !still) {
        panel.classList.add('chat-leaving');
        leaving = setTimeout(() => {
          panel.hidden = true;
          panel.classList.remove('chat-leaving');
          document.body.classList.remove('chat-open');
          onToggle(false);
        }, CHAT_LEAVE_MS);
      } else {
        panel.hidden = true;
        document.body.classList.remove('chat-open');
        onToggle(false);
      }
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
      if (!e.target.closest('[data-chat^="attach"]')) attachMenu(false);
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
      else if (what === 'interview-workspace') onInterview(state.id);
      else if (what === 'new') {
        begin();
        if (!wide()) showSidebar(false);
      } else if (what === 'history') showSidebar(!panel.classList.contains('with-history'));
      else if (what === 'resume') resume(el.dataset.id);
      else if (what === 'save') saveAnswer(Number(el.dataset.i));
      else if (what === 'add') addAnswer(Number(el.dataset.i), '');
      else if (what === 'add-picked') {
        const picked = state.picked;
        hideAddFloat();
        if (picked) addAnswer(picked.i, picked.text);
      }
      else if (what === 'session-finish') {
        const s = currentSession();
        if (s) s.state = 'finished';
        draw();
        remember();
      } else if (what === 'session-done') {
        const s = currentSession();
        if (s) s.dismissed = true;
        draw();
        remember();
      } else if (what === 'session-again') {
        const s = currentSession();
        if (!s) return;
        const c = sessionSummary(s);
        s.dismissed = true;
        const ideas = [...c.review, ...c.skipped];
        if (state.mode !== 'study') {
          state.mode = 'study';
          drawModes();
        }
        send(`Let's do a study session on just these ideas: ${ideas.join('; ')}.`);
      }
      else if (what === 'unselect') {
        state.selection = null;
        drawRefs();
      }
      else if (what === 'apply' || what === 'dismiss') decide(Number(el.dataset.i), Number(el.dataset.j), what === 'apply');
      else if (what === 'undo') undo(Number(el.dataset.i), Number(el.dataset.j));
      else if (what === 'ask-pick') {
        const m = state.messages[Number(el.dataset.i)];
        const a = m && m.asks && m.asks[Number(el.dataset.k)];
        if (a) answerAsk(Number(el.dataset.i), Number(el.dataset.k), a.options[Number(el.dataset.o)]);
      } else if (what === 'quiz-pick') {
        const m = state.messages[Number(el.dataset.i)];
        const q = m && m.quizzes && m.quizzes[Number(el.dataset.k)];
        if (q) answerQuiz(Number(el.dataset.i), Number(el.dataset.k), q.options[Number(el.dataset.o)]);
      } else if (what === 'quiz-hint') {
        askHint(Number(el.dataset.i), Number(el.dataset.k));
      } else if (what === 'quiz-check') {
        const box = body.querySelector(`.quiz-input[data-i="${el.dataset.i}"][data-k="${el.dataset.k}"]`);
        if (box) answerQuiz(Number(el.dataset.i), Number(el.dataset.k), box.value);
      }
      else if (what === 'review') startReview();
      else if (what === 'forget') forget(el.dataset.id);
      else if (what === 'mode') {
        if (el.dataset.mode === state.mode) return;
        state.asking = null;
        closePick();
        state.mode = el.dataset.mode;
        if (state.messages.length) remember();
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
        else attachMenu(menu.hidden);
      } else if (what === 'attach-note') {
        attachMenu(false);
        openPick('button', '', 0);
      } else if (what === 'attach-file') {
        attachMenu(false);
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
        const [from, to] = (el.dataset.line || '').split('-').map(Number);
        onOpen(el.dataset.id, from ? { from, to: to || from } : null);
      } else if (what === 'original') {
        if (!root.matchMedia || !root.matchMedia('(min-width: 900px)').matches) toggle(false);
        onOriginal(el.dataset.note, el.dataset.name, el.dataset.at || '');
      }
    });
    $('#chat-form').addEventListener('submit', (e) => {
      e.preventDefault();
      if (state.busy && input.value.trim()) steerNow(input.value);
      else if (state.busy) state.busy.abort();
      else send(input.value);
    });
    body.addEventListener('input', (e) => {
      if (e.target.classList.contains('quiz-input')) state.drafts[`${e.target.dataset.i}:${e.target.dataset.k}`] = e.target.value;
    });
    body.addEventListener('keydown', (e) => {
      if (!e.target.classList.contains('quiz-input') || e.key !== 'Enter' || e.shiftKey || e.isComposing) return;
      e.preventDefault();
      answerQuiz(Number(e.target.dataset.i), Number(e.target.dataset.k), e.target.value);
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
      drawSend();
    });
    input.addEventListener('click', watchMention);
    $('#chat-file').addEventListener('change', (e) => {
      attachMenu(false);
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
      if (e.target.id === 'chat-history-search') {
        clearTimeout(findTimer);
        const query = e.target.value;
        findTimer = setTimeout(() => findChats(query), 180);
      }
    });
    panel.addEventListener('keydown', (e) => {
      const attach = !$('#chat-attach-menu').hidden;
      const access = !$('#chat-access-menu').hidden;
      if (!attach && !access) return;
      const menu = $(attach ? '#chat-attach-menu' : '#chat-access-menu');
      const trigger = $(attach ? '[data-chat="attach"]' : '#chat-access');
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (attach) attachMenu(false);
        else { menu.hidden = true; trigger.setAttribute('aria-expanded', 'false'); }
        trigger.focus();
      } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key) && (menu.contains(e.target) || e.target === trigger)) {
        const items = [...menu.querySelectorAll('button')].slice(0, ACCESS.length);
        if (!items.length) return;
        e.preventDefault();
        const at = items.indexOf(document.activeElement);
        let next;
        if (e.key === 'Home') next = 0;
        else if (e.key === 'End') next = items.length - 1;
        else if (at < 0) next = e.key === 'ArrowUp' ? items.length - 1 : 0;
        else next = (at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
        items[next].focus();
      }
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
        if (state.busy) steerNow(input.value);
        else send(input.value);
      }
      if (e.key === 'Escape') {
        e.stopPropagation();
        if (state.busy) state.busy.abort();
        else toggle(false);
      }
    });
    panel.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || !state.busy || e.target === input || e.target.id === 'chat-pick-search') return;
      e.stopPropagation();
      state.busy.abort();
    });

    return {
      toggle,
      attachSelection(selection) {
        if (!selection || typeof selection.text !== 'string' || !selection.text.trim()) return;
        state.selection = { note: String(selection.note || ''), title: String(selection.title || ''), from: Number(selection.from) || 0, to: Number(selection.to) || 0, text: selection.text };
        if (state.mode === 'interview') {
          state.mode = 'chat';
          drawModes();
        }
        toggle(true);
        drawRefs();
        input.focus();
      },
      isOpen: () => state.open,
      isBusy: () => Boolean(state.busy || state.preparingSend),
      async openSaved(id) {
        if (state.busy || state.preparingSend) return false;
        const opened = await resume(id);
        if (opened) toggle(true);
        return opened;
      },
      forgetBrowserCache() {
        if (state.busy || state.preparingSend) return false;
        begin();
        input.value = '';
        state.drafts = {};
        storage.removeItem(KEY);
        return true;
      },
      send,
      async attachBoard(file) {
        const id = state.id;
        const old = [...state.files, ...state.sent].filter((f) => f.interviewBoard);
        for (const doc of old) {
          const reply = await fetch(`/api/chats/${encodeURIComponent(id)}/files/${encodeURIComponent(doc.id)}`, { method: 'DELETE', credentials: 'same-origin' });
          if (!reply.ok) throw new Error('The previous board snapshot could not be replaced. Try again.');
        }
        if (state.id !== id) return false;
        const previous = new Set(old.map((f) => f.id));
        state.files = state.files.filter((f) => !previous.has(f.id));
        state.sent = state.sent.filter((f) => !previous.has(f.id));
        if (state.files.length + state.sent.length >= MOST_FILES) throw new Error('This chat already has 10 files. Remove a file or start a new practice before sending a board snapshot.');
        const doc = await addFile(file, true);
        return Boolean(doc && state.id === id);
      },
      async openInterview(id, available = () => true) {
        if (!available()) return false;
        if (state.busy && state.id !== id) return false;
        if (state.id !== id) {
          const opened = await resume(id);
          if (opened === null || !available()) return false;
          if (!opened) begin(id);
        }
        if (state.busy) { toggle(true); return false; }
        state.mode = 'interview';
        drawModes();
        draw();
        persist();
        toggle(true);
        return true;
      },
      setInterviewContext(context) { state.interview = context; if (state.open) drawContext(); },
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

  root.leoChat = { timeline, startSession, scoreSession, sessionBody, sessionSummary, linkCitations, answerLines, passageLine, historyText, checkQuiz, quizSay, hintSay, plainAnswer, QUIZ_KINDS, recentNotes, threadRefs, ACCESS, accessOf, nextAccess, NOTE_DRAG, spentLabel, fileKind, fileCard, create, felix, splitLines, grade, cite, cited, load, save, mentionAt, addRef, modeOf, groups, newId, starterWords, splitFiles, asNote, reviewPrompt, pastedNames, poseOf, MODES, MOST_REFS };
})(typeof window !== 'undefined' ? window : globalThis);
