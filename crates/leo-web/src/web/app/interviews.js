const interviews = window.leoInterview;
let interviewPage = null;
let interviewStart = null;
let interviewCodeLoad = null;
const MOST_BOARD_UNDO = 20;
const MOST_CODE_HISTORY = 40;

function loadInterviewCode() {
  if (window.leoCode) return Promise.resolve(window.leoCode);
  if (!interviewCodeLoad) interviewCodeLoad = new Promise((resolve) => {
    const script = document.createElement('script');
    const finish = (code = null) => {
      clearTimeout(timer);
      script.onload = script.onerror = null;
      script.remove();
      resolve(code);
    };
    const timer = setTimeout(finish, 10000);
    script.src = '/code-tools.js';
    script.onload = () => finish(window.leoCode || null);
    script.onerror = () => finish();
    document.head.append(script);
  });
  return interviewCodeLoad;
}

let interviewTypeScriptLoad = null;

function interviewTypeScript() {
  if (!interviewTypeScriptLoad) interviewTypeScriptLoad = new Promise((resolve) => {
    const script = document.createElement('script');
    const finish = (strip = null) => {
      clearTimeout(timer);
      script.onload = script.onerror = null;
      script.remove();
      if (!strip) interviewTypeScriptLoad = null;
      resolve(strip);
    };
    const timer = setTimeout(finish, 10000);
    script.src = '/typescript.js';
    script.onload = () => finish(window.leoTypeScript || null);
    script.onerror = () => finish();
    document.head.append(script);
  });
  return interviewTypeScriptLoad;
}

async function showInterviews(id = '') {
  const mine = ++seq;
  state = { view: 'interviews', dir: '' };
  interviewPage?.codeResize?.disconnect();
  if (interviewPage?.boardFrame) cancelAnimationFrame(interviewPage.boardFrame);
  interviewPage = null;
  if (interviewStart && interviewStart.id !== id) interviewStart = null;
  chrome({ showBack: Boolean(id) });
  app.innerHTML = '<p class="hint">Opening interview practice…</p>';
  if (!id) {
    const plans = await api('/api/interviews');
    if (mine !== seq) return;
    app.innerHTML = `<div class="interview-home"><div class="interview-heading"><div><p class="eyebrow">With Felix</p><h1>Interview practice</h1><p class="hint">Think it through, try it yourself, and build on what you learn.</p></div></div>
      <h2>Start a practice</h2><div class="interview-tracks">${interviews.PRESETS.map((p) => `<button class="set-card interview-track" data-interview="create" data-track="${p.id}"><strong>${esc(p.label)}</strong><span class="hint">${esc(p.about)}</span>${ICON.chevron}</button>`).join('')}</div>
      ${plans.length ? `<h2>Your practice</h2><div class="set-card">${plans.map((p) => `<div class="set-row"><button class="interview-plan grow" data-interview="open" data-id="${esc(p.id)}"><b>${esc(p.framework.title)}</b><span class="hint">${esc([p.framework.company, p.framework.role].filter(Boolean).join(' · ') || interviews.preset(p.framework.track).label)} · ${p.attempts} submitted</span></button><button class="icon-btn" data-interview="delete" data-id="${esc(p.id)}" aria-label="Delete ${esc(p.framework.title)}">${ICON.trash}</button></div>`).join('')}</div>` : ''}</div>`;
    return;
  }
  let item;
  try { item = await api(`/api/interviews/${enc(id)}`); }
  catch (e) {
    if (mine !== seq) return;
    if (e.status !== 404) throw e;
    app.innerHTML = `<div class="interview-home"><h1>Set up your interview practice</h1><p class="hint">Choose a starting point. Your Felix conversation stays here.</p><div class="interview-tracks">${interviews.PRESETS.map((p) => `<button class="set-card interview-track" data-interview="create" data-id="${esc(id)}" data-track="${p.id}"><strong>${esc(p.label)}</strong><span class="hint">${esc(p.about)}</span>${ICON.chevron}</button>`).join('')}</div></div>`;
    return;
  }
  if (mine !== seq) return;
  const session = { item, mine, draft: interviews.workspace(item.workspace), generation: 0, saved: 0, timer: 0, queue: Promise.resolve(), error: '', tab: item.framework.track === 'coding' ? 'code' : 'write', tool: 'select', selection: [], stroke: null, submitting: false, pane: 'question', paneFor: '' };
  interviewPage = session;
  const local = item.challenge && interviews.readDraft(localStorage, id, item.challenge.id);
  if (local && JSON.stringify(local) !== JSON.stringify(interviews.workspace(item.workspace))) {
    session.draft = local;
    session.generation = 1;
    session.error = 'An unsaved draft was recovered on this device. Review it, then save it when you are ready.';
  }
  drawInterview(session);
  if (chat.isOpen()) {
    chat.useInterview(id, () => interviewHere(session)).then((found) => {
      if (!interviewHere(session)) return;
      if (found === 'switched') toast(`Felix switched to this practice’s chat: ${item.framework.title}`);
      else if (found === 'busy') toast('Felix is answering in another chat. Press the Felix button here when he finishes to see this practice’s chat.');
    }).catch(() => {});
  }
  if (interviewStart?.id === id) {
    interviewStart = null;
    prepareInterview(session).catch(fail);
  }
}

function interviewHere(session) {
  return interviewPage === session && session.mine === seq && state.view === 'interviews';
}

function drawInterview(session) {
  if (!interviewHere(session)) return;
  session.codeResize?.disconnect();
  if (session.boardFrame) { cancelAnimationFrame(session.boardFrame); session.boardFrame = 0; }
  const active = document.activeElement;
  const focus = active && app.contains(active) && (active instanceof HTMLTextAreaElement || (active instanceof HTMLInputElement && active.type === 'text')) ? { id: active.id, from: active.selectionStart, to: active.selectionEnd, scroll: active.scrollTop } : null;
  const item = session.item;
  const coding = item.framework.track === 'coding';
  const tabs = coding ? [['code', 'Code'], ['write', 'Approach'], ['board', 'Whiteboard']] : [['write', 'Write'], ['board', 'Whiteboard']];
  app.innerHTML = `<div class="interview-home interview-practice${coding ? ' interview-coding' : ''}"><div class="interview-heading"><div><p class="eyebrow">${esc(interviews.preset(item.framework.track).label)} · ${item.framework.style === 'mock' ? 'Mock interview' : 'Coach'}</p><h1>${esc(item.framework.title)}</h1><p class="hint">${esc([item.framework.company || 'General practice', item.framework.role].filter(Boolean).join(' · '))}</p></div><div class="buttons"><button class="btn sm" data-interview="configure">Preferences</button></div></div>
    <div id="interview-problems" class="interview-problems" role="group" aria-label="Practice problems"></div>
    <div class="interview-solve" style="--split: ${interviewSplit()}%"><div class="interview-question"><section id="interview-challenge" class="set-card interview-problem"><div id="interview-panes" class="interview-panes"></div><div id="interview-pane-question" role="tabpanel"></div><div id="interview-pane-cases" role="tabpanel" hidden></div><div id="interview-pane-result" role="tabpanel" hidden><div id="interview-results" class="interview-results" aria-live="polite"></div></div><div id="interview-pane-submissions" role="tabpanel" hidden><section id="interview-attempts"></section><section id="interview-progress"></section></div></section></div>
    ${item.challenge ? `<section class="set-card interview-workspace"><div class="interview-toolbar"><div class="library-views" role="tablist" aria-label="Your answer workspace">${tabs.map(([id, label]) => `<button class="btn sm plain${session.tab === id ? ' on' : ''}" role="tab" aria-selected="${session.tab === id}" data-interview="tab" data-tab="${id}">${label}</button>`).join('')}</div><span id="interview-save" class="hint" role="status"></span></div>
    <div id="interview-editor"${session.tab !== (coding ? 'code' : 'write') ? ' hidden' : ''}><div class="interview-toolbar"><label class="interview-label" for="interview-answer">${coding ? esc(item.challenge.coding?.language || item.framework.language || 'Code') : 'Your answer'}</label></div><div class="interview-code-shell${coding ? ' numbered' : ''}">${coding ? '<pre id="interview-lines" aria-hidden="true"></pre>' : ''}<div class="interview-code-pane">${coding ? '<pre id="interview-colors" aria-hidden="true"></pre>' : ''}<textarea id="interview-answer" class="set-input${coding ? ' interview-code' : ''}" maxlength="32000" spellcheck="${!coding}" autocapitalize="off" autocomplete="off" aria-label="${coding ? 'Code editor' : 'Your answer'}" placeholder="${coding ? 'Write your implementation here…' : 'Clarify the problem, outline an approach, and work through it…'}">${esc(session.draft.answer)}</textarea></div></div></div>
    ${coding ? `<div id="interview-reasoning-editor"${session.tab !== 'write' ? ' hidden' : ''}><label class="interview-label" for="interview-reasoning">Your approach</label><p class="hint">Explain your idea, complexity, and edge cases. Your code stays in the Code tab.</p><textarea id="interview-reasoning" class="set-input" maxlength="8000" placeholder="What’s your approach?">${esc(session.draft.reasoning)}</textarea></div>` : ''}
    <div id="interview-whiteboard"${session.tab !== 'board' ? ' hidden' : ''}><div class="interview-toolbar board-toolbar"><div class="library-views board-tools" role="toolbar" aria-label="Drawing tools">${BOARD_TOOLS.map(([id, label, icon, key]) => `<button class="btn sm plain${session.tool === id ? ' on' : ''}" aria-pressed="${session.tool === id}" data-interview="tool" data-tool="${id}" aria-label="${label}" title="${label} (${key})">${ICON[icon]}</button>`).join('')}</div><div class="library-views board-tools" role="toolbar" aria-label="Board actions"><button class="btn sm plain" data-interview="undo-board" aria-label="Undo" title="Undo (⌘Z)">${ICON.undo}</button><button class="btn sm plain" data-interview="redo-board" aria-label="Redo" title="Redo (⇧⌘Z)">${ICON.redo}</button><button class="btn sm plain" data-interview="delete-board" aria-label="Delete selected" title="Delete (⌫)">${ICON.trash}</button><button class="btn sm plain" data-interview="zoom-board" data-step="-1" aria-label="Zoom out" title="Zoom out">${ICON.minus}</button><button class="btn sm plain" data-interview="zoom-board" data-step="1" aria-label="Zoom in" title="Zoom in">${ICON.plus}</button><button class="btn sm plain" data-interview="fit-board" aria-label="Fit everything" title="Fit everything">${ICON.fit}</button></div></div><div class="interview-board-frame"><div id="interview-board" tabindex="0" role="group" aria-label="Whiteboard. Click a shape to select it, drag empty space to select several, and double-click to write. Hold Space, scroll or drag with one finger to move around."></div></div><p class="hint">Drag empty space to select several things. Double-click to write or edit text. Hold Space or scroll to move around.</p></div>
    <div id="interview-checks" class="interview-checks"></div>
    <div id="interview-save-error" class="set-note warn" hidden></div><div class="interview-toolbar interview-submit"><p class="hint" id="interview-submit-hint"></p><div class="buttons"><button class="btn sm" data-interview="reset">Reset</button><button class="btn sm" data-interview="hint">Hint</button><button class="btn sm primary" data-interview="submit">Submit</button></div></div></section><div class="interview-split" role="separator" aria-orientation="vertical" aria-label="Resize the question and workspace" tabindex="0" aria-valuemin="${SPLIT_LEAST}" aria-valuemax="${SPLIT_MOST}" aria-valuenow="${interviewSplit()}" title="Drag to resize · double-click to reset"></div>` : ''}</div>
    </div>`;
  drawInterviewContext(session);
  drawInterviewProblems(session);
  drawInterviewChallenge(session);
  drawInterviewBoard(session);
  drawInterviewStatus(session);
  drawInterviewProgress(session);
  drawInterviewChecks(session);
  drawInterviewResults(session);
  drawInterviewPanes(session);
  wireInterviewResults(session);
  wireInterviewSplit();
  loadInterviewKey(session);
  const answer = $('#interview-answer');
  if (answer) {
    answer.addEventListener('input', (e) => { session.draft.answer = e.target.value; interviewChanged(session); drawInterviewLines(session); });
    answer.addEventListener('scroll', syncInterviewCodeScroll);
    session.codeHistory = [];
    answer.addEventListener('input', e => {
      const key = e.inputType === 'historyUndo' ? 'before' : e.inputType === 'historyRedo' ? 'after' : '';
      if (!key) return;
      const match = [...session.codeHistory].reverse().find(change => change[key].value === answer.value);
      if (match) answer.setSelectionRange(match[key].from, match[key].to, match[key].direction);
      syncInterviewCodeScroll();
    });
    let engine = null, changing = false;
    const type = (e, key) => {
      if (!coding || answer.readOnly || e.isComposing || changing || session.codeApplying || (e.type === 'beforeinput' && !e.cancelable) || ((e.ctrlKey || e.metaKey || e.altKey) && !e.getModifierState?.('AltGraph'))) return;
      const language = session.item.challenge?.coding?.language || session.item.framework.language;
      if (!engine && window.leoCode?.editor) engine = window.leoCode.editor(language);
      const edit = engine?.edit(answer.value, answer.selectionStart, answer.selectionEnd, key, e.shiftKey, answer.selectionDirection) || interviews.codeEdit(answer.value, answer.selectionStart, answer.selectionEnd, key, e.shiftKey, language);
      if (!edit) { if (key === 'Tab') e.preventDefault(); return; }
      e.preventDefault();
      changing = true;
      try { applyInterviewTyping(session, answer, edit); } finally { changing = false; }
    };
    answer.addEventListener('keydown', e => { if (['Tab', 'Enter', 'Backspace', 'Delete', '(', ')', '[', ']', '{', '}', "'", '"', '`'].includes(e.key)) type(e, e.key); });
    answer.addEventListener('beforeinput', e => {
      const key = e.inputType === 'insertLineBreak' || e.inputType === 'insertParagraph' ? 'Enter' : e.inputType === 'deleteContentBackward' ? 'Backspace' : e.inputType === 'deleteContentForward' ? 'Delete' : e.inputType === 'insertText' && e.data?.length === 1 && '()[]{}\'"`'.includes(e.data) ? e.data : '';
      if (key) type(e, key);
    });
    if (coding) {
      session.codeResize = new ResizeObserver(() => syncInterviewCodeScroll());
      session.codeResize.observe(answer);
    }
    drawInterviewLines(session);
    if (coding) loadInterviewCode().then((code) => {
      if (!code || !interviewHere(session)) return;
      drawInterviewLines(session);
    });
    $('#interview-reasoning')?.addEventListener('input', (e) => { session.draft.reasoning = e.target.value; interviewChanged(session); });
    $('#interview-checks')?.addEventListener('input', (e) => {
      const field = e.target.closest('[data-ask]');
      if (!field) return;
      session.draft.values = { ...(session.draft.values || {}), [field.dataset.ask]: field.value };
      interviewChanged(session);
    });
    wireInterviewBoard(session);
  }
  if (focus) {
    const again = document.getElementById(focus.id);
    if (again && !again.disabled) { again.focus({ preventScroll: true }); again.setSelectionRange(focus.from, focus.to); again.scrollTop = focus.scroll; syncInterviewCodeScroll(); }
  }
}

function drawInterviewContext(session) {
  const item = session.item, c = item.challenge;
  const number = (item.problems || []).findIndex(p => p.challenge.id === c?.id) + 1;
  chat.setInterviewContext(c ? { id: item.id, challenge: c.id, number: number || 1, title: c.title, tab: session.tab === 'board' ? 'Whiteboard' : session.tab === 'code' ? 'Code' : item.framework.track === 'coding' ? 'Approach' : 'Answer' } : null);
}

function drawInterviewProblems(session) {
  if (!interviewHere(session)) return;
  const box = $('#interview-problems');
  if (!box) return;
  const item = session.item;
  const problems = item.problems?.length ? item.problems : item.challenge ? [{ challenge: item.challenge }] : [];
  const busy = Boolean(session.preparing || session.submitting || session.switching);
  const filter = interviews.topicFilter(problems.slice(0, interviews.MOST_PROBLEMS), interviewTopic(session));
  const count = Math.min(problems.length, interviews.MOST_PROBLEMS);
  const row = (label, n, attrs, on) => `<button type="button" class="pick-model${on ? ' on' : ''}" role="menuitemradio" aria-checked="${on}" data-interview="topic" ${attrs}><span class="pick-text"><span class="pick-name">${label}</span><span class="pick-price">${n} problem${n === 1 ? '' : 's'}</span></span>${on ? ICON.check : ''}</button>`;
  const named = filter.chosen === null ? 'All topics' : filter.chosen === interviews.NO_TOPIC ? 'No topic' : esc(filter.topics.find((t) => t.name.toLowerCase() === filter.chosen.toLowerCase())?.name || filter.chosen);
  const menu = `${row('All topics', count, 'data-all="1"', filter.chosen === null)}${filter.topics.map((t) => row(esc(t.name), t.count, `data-topic="${esc(t.name)}"`, filter.chosen !== null && filter.chosen !== interviews.NO_TOPIC && filter.chosen.toLowerCase() === t.name.toLowerCase())).join('')}${filter.untagged && filter.topics.length ? row('No topic', filter.untagged, 'data-none="1"', filter.chosen === interviews.NO_TOPIC) : ''}${filter.untagged ? `<div class="pick-split"></div><button type="button" class="pick-model interview-sort-topics" role="menuitem" data-interview="sort-topics"${busy ? ' disabled' : ''}><span class="pick-text"><span class="pick-name">Sort into topics with Felix</span><span class="pick-price">${filter.untagged} without a topic</span></span></button>` : ''}`;
  const topics = count ? `<div class="picker interview-topic-picker"><button type="button" class="picker-button${filter.chosen === null ? '' : ' filtered'}" data-interview="topic-menu" aria-haspopup="menu" aria-expanded="false" aria-label="Filter problems by topic: ${named}">${ICON.filter}<span class="picker-text">${named}</span>${ICON.chevron}</button><div class="picker-menu" role="menu" aria-label="Topics" hidden><div class="pick-head">Show problems about</div>${menu}</div></div>` : '';
  box.innerHTML = `${topics}${problems.slice(0, interviews.MOST_PROBLEMS).map((p, i) => !filter.shown.includes(i) ? '' : `<button class="pick-chip${p.challenge.id === item.challenge?.id ? ' on' : ''}" data-interview="problem" data-id="${esc(p.challenge.id)}" aria-pressed="${p.challenge.id === item.challenge?.id}" title="${esc(p.challenge.title)}"${busy ? ' disabled' : ''}>Problem ${i + 1}${item.attempts.some((a) => a.challenge.id === p.challenge.id) ? ' · Submitted' : ''}</button>`).join('')}<button class="btn sm plain" data-interview="next"${busy || problems.length >= interviews.MOST_PROBLEMS ? ' disabled' : ''}>${session.preparing ? 'Preparing…' : '+ Problem'}</button>`;
}

function interviewTopic(session) {
  if (session.topic === undefined) {
    try {
      const saved = localStorage.getItem(`leo-interview-topic:${session.item.id}`);
      session.topic = saved === null ? null : saved === '\u0000' ? interviews.NO_TOPIC : saved;
    } catch (e) {
      session.topic = null;
    }
  }
  return session.topic;
}

function interviewTopicMenu(open) {
  const menu = $('.interview-topic-picker .picker-menu');
  const button = $('.interview-topic-picker .picker-button');
  if (!menu || !button) return;
  const opening = open ?? (menu.hidden || Boolean(menu.dataset.folding));
  if (opening) {
    unfold(menu);
    (menu.querySelector('.pick-model.on') || menu.querySelector('.pick-model'))?.focus({ preventScroll: true });
  } else {
    fold(menu);
  }
  button.setAttribute('aria-expanded', String(opening));
}

document.addEventListener('click', (e) => {
  if (state.view === 'interviews' && !e.target.closest('.interview-topic-picker')) {
    const menu = $('.interview-topic-picker .picker-menu');
    if (menu && !menu.hidden) interviewTopicMenu(false);
  }
});
document.addEventListener('keydown', (e) => {
  const menu = $('.interview-topic-picker .picker-menu');
  if (!menu || menu.hidden || state.view !== 'interviews') return;
  if (e.key === 'Escape') {
    e.preventDefault();
    e.stopPropagation();
    interviewTopicMenu(false);
    $('.interview-topic-picker .picker-button')?.focus();
    return;
  }
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  const items = [...menu.querySelectorAll('.pick-model:not([disabled])')];
  const at = items.indexOf(document.activeElement);
  e.preventDefault();
  items[(at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
}, true);

function pickInterviewTopic(session, el) {
  session.topic = el.dataset.all ? null : el.dataset.none ? interviews.NO_TOPIC : el.dataset.topic;
  try {
    const key = `leo-interview-topic:${session.item.id}`;
    if (session.topic === null) localStorage.removeItem(key);
    else localStorage.setItem(key, session.topic === interviews.NO_TOPIC ? '\u0000' : session.topic);
  } catch (e) {}
  drawInterviewProblems(session);
  $('.interview-topic-picker .picker-button')?.focus({ preventScroll: true });
}

function useInterviewProblem(session, item) {
  if (session.item.challenge?.id !== item.challenge?.id) { session.boardUndo = []; session.boardRedo = []; session.selection = []; closeInterviewBoardText(session, false); session.gesture = null; session.stroke = null; }
  session.item = item;
  session.draft = interviews.workspace(item.workspace);
  session.generation = 0;
  session.saved = 0;
  session.blocked = false;
  session.error = '';
  const local = item.challenge && interviews.readDraft(localStorage, item.id, item.challenge.id);
  if (local && JSON.stringify(local) !== JSON.stringify(session.draft)) {
    session.draft = local;
    session.generation = 1;
    session.error = 'An unsaved draft was recovered on this device. Review it, then save it when you are ready.';
  }
}

async function selectInterviewProblem(session, id) {
  if (session.switching || session.preparing || session.submitting || id === session.item.challenge?.id) return;
  session.switching = true;
  drawInterviewProblems(session);
  const fields = [...app.querySelectorAll('#interview-answer, #interview-reasoning')];
  fields.forEach((field) => { field.readOnly = true; });
  try {
    await saveInterviewWorkspace(session);
    await interviewQueue(session, async () => {
      const item = await api(`/api/interviews/${enc(session.item.id)}`, { method: 'PUT', body: { action: 'select', revision: session.item.revision, challenge: id } });
      if (!interviewHere(session)) return;
      useInterviewProblem(session, item);
    });
    session.switching = false;
    if (interviewHere(session)) drawInterview(session);
  } finally {
    session.switching = false;
    fields.forEach((field) => { field.readOnly = false; });
    drawInterviewProblems(session);
  }
}

function applyInterviewTyping(session, answer, edit) {
  const pageX = scrollX, pageY = scrollY;
  const before = { value: answer.value, from: answer.selectionStart, to: answer.selectionEnd, direction: answer.selectionDirection };
  let from = 0, to = answer.value.length, end = edit.value.length;
  while (from < to && from < end && answer.value[from] === edit.value[from]) from++;
  while (to > from && end > from && answer.value[to - 1] === edit.value[end - 1]) { to--; end--; }
  const change = edit.change || { from, to, insert: edit.value.slice(from, end) };
  session.codeApplying = true;
  try {
    if (answer.value !== edit.value) {
      answer.setSelectionRange(change.from, change.to);
      if (document.activeElement !== answer || !document.execCommand('insertText', false, change.insert)) answer.setRangeText(change.insert, change.from, change.to, 'end');
    }
    answer.setSelectionRange(edit.from, edit.to, edit.direction || 'forward');
    if (before.value !== answer.value) {
      session.codeHistory ||= [];
      session.codeHistory.push({ before, after: { value: answer.value, from: edit.from, to: edit.to, direction: edit.direction || 'forward' } });
      if (session.codeHistory.length > MOST_CODE_HISTORY) session.codeHistory.shift();
    }
    if (session.draft.answer !== answer.value) answer.dispatchEvent(new Event('input', { bubbles: true }));
    window.scrollTo({ left: pageX, top: pageY, behavior: 'instant' });
    syncInterviewCodeScroll();
  } finally { session.codeApplying = false; }
}

function drawInterviewLines(session) {
  if (!interviewHere(session)) return;
  const lines = $('#interview-lines');
  if (lines && (session.linesNode !== lines || session.linesValue !== session.draft.answer)) {
    lines.innerHTML = session.draft.answer.split('\n').slice(0, 32001).map((line, i) => `<span class="interview-line"><span class="interview-line-number">${i + 1}</span><span class="interview-line-measure">${esc(line || ' ')}</span></span>`).join('');
    session.linesNode = lines;
    session.linesValue = session.draft.answer;
  }
  syncInterviewCodeScroll();
  if (!lines || !window.leoCode) return;
  if (session.codeFrame) cancelAnimationFrame(session.codeFrame);
  session.codeFrame = requestAnimationFrame(() => {
    session.codeFrame = 0;
    if (!interviewHere(session)) return;
    const colors = $('#interview-colors');
    const answer = $('#interview-answer');
    if (!colors || !answer) return;
    const language = session.item.challenge?.coding?.language || session.item.framework.language;
    const value = session.draft.answer;
    if (session.colorsNode === colors && session.colorsLanguage === language && session.colorsValue === value) { syncInterviewCodeScroll(); return; }
    let html = null;
    try { html = window.leoCode.highlight(value, language); } catch {}
    colors.parentElement.classList.toggle('syntax-on', html !== null);
    colors.innerHTML = html === null ? '' : html + (value.endsWith('\n') || !value ? ' ' : '');
    session.colorsNode = colors;
    session.colorsLanguage = language;
    session.colorsValue = value;
    syncInterviewCodeScroll();
  });
}

function syncInterviewCodeScroll() {
  const answer = $('#interview-answer');
  if (!answer) return;
  const lines = $('#interview-lines');
  if (lines) { lines.style.setProperty('--code-scrollbar', `${answer.offsetWidth - answer.clientWidth}px`); lines.scrollTop = answer.scrollTop; }
  const colors = $('#interview-colors');
  if (colors) { colors.style.width = `${answer.clientWidth}px`; colors.style.height = `${answer.clientHeight}px`; colors.scrollTop = answer.scrollTop; colors.scrollLeft = answer.scrollLeft; }
}

function drawInterviewChallenge(session) {
  if (!interviewHere(session)) return;
  const c = session.item.challenge;
  const coding = c?.coding;
  const busy = session.preparing;
  keepOpen($('#interview-pane-question'), c ? `<div class="interview-toolbar"><h2>${esc(c.title)}</h2><div class="buttons">${(c.topics || []).map((t) => `<span class="chip interview-topic">${esc(t)}</span>`).join('')}${coding ? `<span class="chip">${esc(coding.difficulty)}</span>` : ''}<span class="chip">${c.minutes} min</span></div></div><p class="hint">${esc(c.origin === 'generated' ? 'Original practice' : c.origin === 'official' ? 'Official preparation' : 'Candidate report · unverified')}</p><div class="prose">${md.render(c.question, { interactive: false })}</div>${coding ? `<h3>Examples</h3>${coding.examples.map((e, i) => `<div class="interview-example"><b>Example ${i + 1}</b><pre>Input: ${esc(e.input)}\nOutput: ${esc(e.output)}</pre>${e.explanation ? `<p class="hint">${esc(e.explanation)}</p>` : ''}${e.diagram ? `<div class="prose interview-example-diagram">${md.render(e.diagram, { interactive: false })}</div>` : ''}</div>`).join('')}<h3>Constraints</h3><ul class="interview-constraints">${coding.constraints.map((s) => `<li>${esc(s)}</li>`).join('')}</ul>` : ''}${c.sources.length ? `<details class="interview-research source-more"><summary>${ICON.chevron}<span>Research & sources</span></summary><p class="hint">Reported questions are unverified. This exercise is labelled separately above.</p><div class="interview-sources">${c.sources.map((s) => `<a class="chip" href="${esc(s.url)}" target="_blank" rel="noopener noreferrer">${esc(s.title)} · ${esc(s.kind)}</a>`).join('')}</div></details>` : ''}<button class="btn sm" data-interview="next"${busy ? ' disabled' : ''}>${busy ? 'Preparing…' : 'Next challenge'}</button>` : `<div class="interview-welcome"><p class="eyebrow">${session.item.framework.company ? 'Company preparation' : 'General practice'}</p><h2>${busy ? 'Preparing your first challenge…' : 'Ready when you are'}</h2><p class="hint" role="status">${busy ? session.waiting ? 'Felix is finishing the previous reply. Your practice will start automatically after it.' : session.item.framework.company ? 'Felix is researching the company and role, then preparing an exercise. You can follow along in chat.' : 'Felix is choosing an exercise for your goals. You can follow along in chat.' : 'Start once. Felix will research your company if you have one, then put a challenge and workspace here.'}</p><div class="buttons"><button class="btn sm primary" data-interview="next"${busy ? ' disabled' : ''}>${busy ? 'Preparing…' : 'Start practice'}</button></div></div>`);
}

function interviewChanged(session) {
  if (!session.item.challenge) return;
  session.generation++;
  session.error = '';
  const kept = interviews.keepDraft(localStorage, session.item.id, session.item.challenge.id, session.draft);
  if (!kept) session.error = 'Your browser could not keep a backup draft. Leave this page open until it is saved.';
  clearTimeout(session.timer);
  session.timer = setTimeout(() => saveInterviewWorkspace(session).catch(() => {}), 700);
  drawInterviewStatus(session);
}

function interviewQueue(session, work) {
  const next = session.queue.then(work);
  session.queue = next.catch(() => {});
  return next;
}

function drawInterviewStatus(session) {
  if (!interviewHere(session)) return;
  const text = $('#interview-save');
  if (text) text.textContent = session.submitting ? 'Submitting…' : session.generation === session.saved ? 'Saved' : 'Saving your draft…';
  const error = $('#interview-save-error');
  if (error) {
    error.hidden = !session.error;
    error.innerHTML = session.error ? `${esc(session.error)} <button class="btn sm" data-interview="retry">Reload and save my draft</button>` : '';
  }
}

async function saveInterviewWorkspace(session) {
  clearTimeout(session.timer);
  return interviewQueue(session, async () => {
    if (session.generation === session.saved || !session.item.challenge) return;
    if (session.blocked) throw new Error('The challenge changed. Copy your draft or reopen this practice before continuing.');
    const generation = session.generation;
    const workspace = structuredClone(session.draft);
    if (session.gesture?.kind === 'move') workspace.board[session.gesture.index] = structuredClone(session.gesture.shape);
    const challenge = session.item.challenge.id;
    try {
      const item = await api(`/api/interviews/${enc(session.item.id)}`, { method: 'PUT', body: { action: 'workspace', revision: session.item.revision, challenge, workspace } });
      session.item = item;
      session.saved = generation;
      session.error = '';
      if (session.generation === generation) interviews.clearDraft(localStorage, item.id, challenge);
    } catch (e) {
      session.error = e.message || 'Your draft could not be saved. Keep this page open and try again.';
      throw e;
    } finally { drawInterviewStatus(session); }
  });
}

async function retryInterviewSave(session) {
  const latest = await api(`/api/interviews/${enc(session.item.id)}`);
  if (latest.challenge?.id !== session.item.challenge?.id) {
    toast('The challenge changed. Your previous draft is kept on this device.', { bad: true });
    return;
  }
  session.item = latest;
  await saveInterviewWorkspace(session);
}

async function applyInterviewCodeSuggestion(action) {
  const session = interviewPage;
  if (!session || session.item.id !== action.interview || !interviewHere(session)) return api('/api/felix/act', { method: 'POST', body: action });
  if (session.switching) throw new Error('Wait for the problem to finish switching, then accept this change again.');
  for (let tries = 0; tries < 2 && session.generation !== session.saved; tries++) await saveInterviewWorkspace(session);
  if (session.generation !== session.saved) throw new Error('Your code is still being saved. Wait a moment, then accept this change again.');
  return interviewQueue(session, async () => {
    const challenge = action.challenge;
    const generation = session.generation;
    const shown = session.item.challenge?.id === challenge;
    const result = await api('/api/felix/act', { method: 'POST', body: action });
    session.item.revision = result.revision;
    if (shown && interviewHere(session) && session.item.challenge?.id === challenge) {
      const code = session.draft.answer;
      const start = code.indexOf(action.find);
      if ((!action.find && code) || start < 0 || (action.find && code.indexOf(action.find, start + 1) >= 0)) {
        session.error = 'Your code changed while this suggestion was being accepted. Your draft is kept; review it or use Undo on the change card.';
        drawInterviewStatus(session);
      } else {
        const next = code.slice(0, start) + action.replace + code.slice(start + action.find.length);
        if (next.length > interviews.MOST_ANSWER) { session.error = 'Your newer draft plus this change exceeds the code limit. Your draft is kept; review it or use Undo on the card.'; drawInterviewStatus(session); return result; }
        const area = $('#interview-answer');
        const map = position => position <= start ? position : position >= start + action.find.length ? position + action.replace.length - action.find.length : start + Math.min(position - start, action.replace.length);
        applyInterviewTyping(session, area, { value: next, from: map(area.selectionStart), to: map(area.selectionEnd), direction: area.selectionDirection, change: { from: start, to: start + action.find.length, insert: action.replace } });
        session.item.workspace.answer = next;
        if (session.generation === generation + 1) { session.saved = session.generation; clearTimeout(session.timer); interviews.clearDraft(localStorage, session.item.id, challenge); }
        drawInterviewStatus(session);
      }
    }
    return result;
  });
}

function refreshInterviewTests(id, challenge) {
  const session = interviewPage;
  if (!session || session.item.id !== id || !interviewHere(session)) return Promise.resolve();
  if (session.ran?.challenge === challenge) session.ran = null;
  return loadInterviewKey(session, true);
}

async function refreshInterview(id) {
  const session = interviewPage;
  if (!session || session.item.id !== id || !interviewHere(session)) return;
  await interviewQueue(session, async () => {
    let latest = await api(`/api/interviews/${enc(id)}`);
    if (!interviewHere(session)) return;
    const changed = latest.challenge?.id !== session.item.challenge?.id;
    if (changed) {
      session.switching = true;
      drawInterviewProblems(session);
      const fields = [...app.querySelectorAll('#interview-answer, #interview-reasoning')];
      fields.forEach((field) => { field.readOnly = true; });
      try {
        if (session.generation !== session.saved) {
          const challenge = session.item.challenge.id;
          try {
            latest = await api(`/api/interviews/${enc(id)}`, { method: 'PUT', body: { action: 'workspace', revision: latest.revision, challenge, workspace: structuredClone(session.draft) } });
            if (!interviewHere(session)) return;
            interviews.clearDraft(localStorage, id, challenge);
            useInterviewProblem(session, latest);
            drawInterview(session);
          } catch (e) {
            session.blocked = true;
            session.error = 'Your earlier problem has an unsaved draft on this device. Reopen this practice to recover it before continuing.';
            drawInterviewStatus(session);
          }
        } else {
          useInterviewProblem(session, latest);
          drawInterview(session);
        }
      } finally {
        session.switching = false;
        fields.forEach((field) => { field.readOnly = false; });
        drawInterviewProblems(session);
      }
    } else {
      session.item = latest;
      drawInterviewProblems(session);
      drawInterviewChallenge(session);
      drawInterviewProgress(session);
      drawInterviewResults(session);
      drawInterviewPanes(session);
    }
  });
}

function drawInterviewProgress(session) {
  paintInterviewProgress(session);
  drawInterviewPanes(session);
}

function paintInterviewProgress(session) {
  if (!interviewHere(session)) return;
  const attempts = session.item.attempts;
  const assessed = attempts.filter((a) => a.feedback).length;
  const scores = interviews.progress(attempts);
  keepOpen($('#interview-progress'), `<h2>What you’re learning</h2><div class="set-card"><p class="hint">${assessed ? `${assessed} reviewed attempt${assessed === 1 ? '' : 's'}. AI feedback is provisional; averages use your last five observations per skill.` : 'Complete an attempt to start seeing strengths and areas to practise. Felix won’t guess from an empty history.'}</p>${scores.map((s) => `<div class="set-row"><div class="grow"><b>${esc(s.skill)}</b><span class="hint">${s.n} observation${s.n === 1 ? '' : 's'}${s.n < 3 ? ' · early signal' : ''}</span></div><span class="chip">${s.value.toFixed(1)} / 4</span></div>`).join('')}</div>`);
  const current = attempts.filter(a => a.challenge.id === session.item.challenge?.id);
  keepOpen($('#interview-attempts'), current.length ? `<h2>Your attempts for this problem</h2><div class="interview-attempts">${[...current].reverse().map((a) => { const status = interviews.attemptStatus(a); return `<details class="interview-attempt source-more ${status.kind}" data-keep="attempt-${esc(a.id)}"><summary>${ICON.chevron}<span class="chip interview-tally ${status.kind}">${status.kind === 'passed' ? ICON.check : status.kind === 'failed' ? ICON.close : ''}${esc(status.label)}</span><span class="grow"><b>${esc(a.challenge.title)}</b><span class="hint">${new Date(a.at).toLocaleDateString()} · ${[...interviews.checkSummary(a.checks).map((g) => `${g.pass}/${g.total} ${g.name}`), a.grade ? `quick review ${a.grade.criteria.filter((v) => v.verdict === 'met').length}/${a.grade.criteria.length} met` : '', a.feedback ? 'Reviewed' : 'Awaiting feedback'].filter(Boolean).join(' · ')}</span></span></summary>${a.feedback ? `<div class="prose">${md.render(a.feedback.summary, { interactive: false })}</div>${a.feedback.scores.map((s) => `<p><b>${esc(s.skill)} · ${s.value}/4</b><br>${esc(s.evidence)}<br><span class="hint">Next: ${esc(s.next)}</span></p>`).join('')}` : `<button class="btn sm" data-interview="review-attempt" data-id="${esc(a.id)}">Ask Felix to review</button>`}<details class="source-more interview-work"><summary>${ICON.chevron}<span>Your submitted work</span></summary><pre class="interview-submitted">${esc(a.workspace.answer)}</pre>${a.workspace.reasoning ? `<h3>Approach</h3><div class="prose">${md.render(a.workspace.reasoning, { interactive: false })}</div>` : ''}${a.workspace.board.length ? `<div class="interview-board-snapshot">${interviews.boardSvg(a.workspace.board)}</div>` : ''}</details></details>`; }).join('')}</div>` : session.item.challenge ? '<p class="hint">No submissions for this problem yet.</p>' : '');
}


function interviewBoardView(session) {
  session.boardViews ||= new Map();
  const id = session.item.challenge?.id || '';
  if (!session.boardViews.has(id)) {
    if (session.boardViews.size >= interviews.MOST_PROBLEMS) session.boardViews.delete(session.boardViews.keys().next().value);
    const width = $('#interview-board')?.clientWidth || 0;
    const view = interviews.fitBoard(session.draft.board, width ? width * 1.5 : 1000);
    if (!width) return view;
    session.boardViews.set(id, view);
  }
  return session.boardViews.get(id);
}

function setInterviewBoardView(session, view) {
  const next = { ...view, x: interviews.point(view.x, view.y)[0], y: interviews.point(view.x, view.y)[1] };
  session.boardViews.set(session.item.challenge.id, next);
  const svg = $('#interview-board > svg');
  if (svg) svg.setAttribute('viewBox', `${next.x} ${next.y} ${next.w} ${next.h}`);
}

const BOARD_TOOLS = [['select', 'Select and move', 'pointer', 'V'], ['pen', 'Pen', 'pen', 'P'], ['box', 'Box', 'box', 'R'], ['arrow', 'Arrow', 'arrow', 'A'], ['text', 'Text', 'text', 'T']];
const BOARD_KEYS = { v: 'select', p: 'pen', r: 'box', a: 'arrow', t: 'text' };
const copyBoard = (board) => board.map((s) => ({ ...s, points: s.points.map((p) => [...p]) }));

function rememberInterviewBoard(session, board = session.draft.board, view = false) {
  session.boardUndo ||= [];
  session.boardUndo.push({ board: copyBoard(board), view: view ? { ...interviewBoardView(session) } : null });
  if (session.boardUndo.length > MOST_BOARD_UNDO) session.boardUndo.shift();
  session.boardRedo = [];
}

function changeInterviewBoard(session, next, selection = session.selection) {
  if (!interviews.validBoard(next)) { toast('Your board is full. Delete or undo something first.', { bad: true }); return false; }
  rememberInterviewBoard(session);
  session.draft.board = next;
  session.selection = selection.filter((i) => next[i]);
  interviewChanged(session);
  drawInterviewBoard(session);
  return true;
}

function stepInterviewBoard(session, back) {
  const from = back ? (session.boardUndo ||= []) : (session.boardRedo ||= []);
  const to = back ? (session.boardRedo ||= []) : (session.boardUndo ||= []);
  const step = from.pop();
  if (!step) return;
  to.push({ board: copyBoard(session.draft.board), view: step.view ? { ...interviewBoardView(session) } : null });
  session.draft.board = step.board;
  if (step.view) setInterviewBoardView(session, step.view);
  session.selection = [];
  interviewChanged(session);
  drawInterviewBoard(session);
}

function setInterviewTool(session, tool) {
  session.tool = tool;
  app.querySelectorAll('[data-interview="tool"]').forEach((b) => { const on = b.dataset.tool === tool; b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on)); });
  const board = $('#interview-board');
  if (board) board.dataset.tool = tool;
}

function drawInterviewBoard(session) {
  if (!interviewHere(session)) return;
  const board = $('#interview-board');
  if (!board) return;
  const focused = board.contains(document.activeElement) ? document.activeElement.closest('[data-board-shape]')?.dataset.boardShape : null;
  board.dataset.tool = session.tool;
  board.innerHTML = interviews.boardSvg(session.stroke ? [...session.draft.board, session.stroke] : session.draft.board, { view: interviewBoardView(session), interactive: true, selected: session.selection || [], marquee: session.marquee || null, editing: session.boardEditing?.index ?? -1 });
  if (focused != null) board.querySelector(`[data-board-shape="${focused}"]`)?.focus({ preventScroll: true });
  const remove = $('[data-interview="delete-board"]');
  if (remove) remove.disabled = !(session.selection || []).length;
}

function queueInterviewBoard(session) {
  if (session.boardFrame) return;
  session.boardFrame = requestAnimationFrame(() => { session.boardFrame = 0; drawInterviewBoard(session); });
}

function boardScreen(board, at) {
  const svg = board.querySelector('svg');
  const matrix = svg?.getScreenCTM();
  if (!matrix) return { x: 0, y: 0, scale: 1 };
  const p = new DOMPoint(at[0], at[1]).matrixTransform(matrix);
  const frame = board.parentElement.getBoundingClientRect();
  return { x: p.x - frame.left, y: p.y - frame.top, scale: matrix.a };
}

function editInterviewBoardText(session, index, at = null) {
  const board = $('#interview-board');
  if (!board || session.switching || !session.item.challenge) return;
  closeInterviewBoardText(session, true);
  const shape = session.draft.board[index];
  if (index >= 0 && (!shape || shape.kind === 'pen')) return;
  const editor = document.createElement('input');
  editor.className = 'interview-board-editor';
  editor.maxLength = 300;
  editor.setAttribute('aria-label', shape ? 'Edit text' : 'New label');
  editor.value = shape ? shape.text : '';
  session.boardEditing = { index, at, editor };
  session.selection = index >= 0 ? [index] : [];
  drawInterviewBoard(session);
  let anchor = at, middle = false;
  if (shape) {
    const b = interviews.shapeBounds(shape);
    if (shape.kind === 'text') anchor = shape.points[0];
    else if (shape.kind === 'box') { anchor = [b.x + b.w / 2, b.y + b.h / 2]; middle = true; }
    else { anchor = [b.x + b.w / 2, b.y + b.h / 2 - 16]; middle = true; }
  }
  const place = boardScreen(board, anchor);
  const size = Math.max(12, Math.min(40, 22 * place.scale));
  editor.style.fontSize = `${size}px`;
  editor.style.left = `${place.x}px`;
  editor.style.top = `${place.y - (middle ? size * .7 : size * 1.05)}px`;
  editor.classList.toggle('middle', middle);
  const fit = () => { editor.style.width = `${Math.max(90, [...editor.value].length * size * .6 + 24)}px`; };
  fit();
  editor.addEventListener('input', fit);
  editor.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.isComposing) return;
    if (e.key === 'Enter') { e.preventDefault(); closeInterviewBoardText(session, true); board.focus({ preventScroll: true }); }
    else if (e.key === 'Escape') { e.preventDefault(); closeInterviewBoardText(session, false); board.focus({ preventScroll: true }); }
  });
  editor.addEventListener('blur', () => closeInterviewBoardText(session, true));
  board.parentElement.append(editor);
  editor.focus({ preventScroll: true });
  editor.select();
}

function closeInterviewBoardText(session, keep) {
  const editing = session.boardEditing;
  if (!editing) return;
  session.boardEditing = null;
  editing.editor.remove();
  const text = editing.editor.value.trim();
  const board = session.draft.board;
  const shape = board[editing.index];
  if (keep && shape && text !== shape.text) {
    const next = text || shape.kind !== 'text' ? board.map((s, i) => i === editing.index ? { ...s, text } : s) : board.filter((_, i) => i !== editing.index);
    changeInterviewBoard(session, next, text || shape.kind !== 'text' ? [editing.index] : []);
  } else if (keep && !shape && text && editing.at) {
    if (board.length >= interviews.MOST_SHAPES) toast('Your board is full. Delete or undo something first.', { bad: true });
    else changeInterviewBoard(session, [...board, { kind: 'text', points: [editing.at], text }], [board.length]);
  } else drawInterviewBoard(session);
  if (session.tool === 'text') setInterviewTool(session, 'select');
}

function moveInterviewSelection(session, dx, dy) {
  const chosen = session.selection || [];
  if (!chosen.length) return false;
  changeInterviewBoard(session, session.draft.board.map((s, i) => chosen.includes(i) ? interviews.moveShape(s, dx, dy) : s));
  return true;
}

function pasteInterviewShapes(session, shapes) {
  if (!shapes?.length) return;
  const unit = interviewBoardView(session).w / 1000;
  const board = session.draft.board;
  const placed = shapes.map((s) => interviews.moveShape(s, 24 * unit, 24 * unit));
  if (board.length + placed.length > interviews.MOST_SHAPES) { toast('Your board is full. Delete or undo something first.', { bad: true }); return; }
  if (changeInterviewBoard(session, [...board, ...placed], placed.map((_, i) => board.length + i))) session.boardClip = copyBoard(placed);
}

function deleteInterviewSelection(session) {
  const chosen = session.selection || [];
  if (!chosen.length) return;
  changeInterviewBoard(session, session.draft.board.filter((_, i) => !chosen.includes(i)), []);
}

function wireInterviewBoard(session) {
  const board = $('#interview-board');
  const locate = (e) => {
    const svg = board.querySelector('svg');
    const matrix = svg?.getScreenCTM();
    if (!matrix) return [0, 0];
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(matrix.inverse());
    return interviews.point(p.x, p.y);
  };
  const finish = (cancel = false) => {
    const gesture = session.gesture;
    if (!gesture) return;
    if (gesture.kind === 'move' || gesture.kind === 'resize') {
      if (cancel) session.draft.board = gesture.before;
      else if (JSON.stringify(session.draft.board) !== JSON.stringify(gesture.before)) {
        const after = session.draft.board;
        session.draft.board = gesture.before;
        rememberInterviewBoard(session);
        session.draft.board = after;
        interviewChanged(session);
      }
    } else if (gesture.kind === 'marquee') {
      const found = session.marquee ? interviews.shapesWithin(session.draft.board, session.marquee) : [];
      session.selection = gesture.add ? [...new Set([...gesture.base, ...found])] : found;
      session.marquee = null;
    } else if (gesture.kind === 'draw' && session.stroke && !cancel) {
      let stroke = session.stroke;
      const unit = interviewBoardView(session).w / 1000;
      if (stroke.kind !== 'pen') {
        const a = stroke.points[0], b = stroke.points[1] || a;
        if (Math.hypot(b[0] - a[0], b[1] - a[1]) < 6 * unit) stroke = { ...stroke, points: [a, interviews.point(a[0] + (stroke.kind === 'box' ? 160 : 140) * unit, a[1] + (stroke.kind === 'box' ? 90 : 0) * unit)] };
      }
      const index = session.draft.board.length;
      session.stroke = null;
      if (changeInterviewBoard(session, [...session.draft.board, stroke], stroke.kind === 'pen' ? [] : [index]) && stroke.kind !== 'pen') setInterviewTool(session, 'select');
    }
    session.stroke = null;
    session.gesture = null;
    board.classList.remove('panning');
    if (board.hasPointerCapture(gesture.pointer)) board.releasePointerCapture(gesture.pointer);
    drawInterviewBoard(session);
  };
  board.addEventListener('pointerdown', (e) => {
    if (session.switching || session.gesture || !session.item.challenge || ![0, 1].includes(e.button)) return;
    if (session.boardEditing) closeInterviewBoardText(session, true);
    e.preventDefault();
    const first = locate(e);
    const handle = e.target.closest('[data-board-handle]')?.dataset.boardHandle;
    const target = e.target.closest('[data-board-shape]');
    const index = target ? Number(target.dataset.boardShape) : -1;
    const shape = session.draft.board[index];
    const selection = session.selection || [];
    const empty = !shape && !handle;
    const last = session.lastPress;
    const double = last && e.timeStamp - last.at < 400 && Math.hypot(e.clientX - last.x, e.clientY - last.y) < 6 && last.index === index;
    session.lastPress = double ? null : { at: e.timeStamp, x: e.clientX, y: e.clientY, index };
    if (double && session.tool === 'select' && !handle && !e.shiftKey && e.button === 0 && !session.panHeld) {
      if (shape && shape.kind !== 'pen') { editInterviewBoardText(session, index); return; }
      if (!shape) { editInterviewBoardText(session, -1, first); return; }
    }
    if (e.button === 1 || session.panHeld || (session.tool === 'select' && empty && e.pointerType === 'touch')) {
      session.gesture = { kind: 'pan', pointer: e.pointerId, x: e.clientX, y: e.clientY, view: { ...interviewBoardView(session) } };
      board.classList.add('panning');
    } else if (session.tool === 'text') {
      if (shape && shape.kind !== 'pen') editInterviewBoardText(session, index);
      else editInterviewBoardText(session, -1, first);
      return;
    } else if (handle && selection.length === 1) {
      session.gesture = { kind: 'resize', pointer: e.pointerId, index: selection[0], handle, shape: session.draft.board[selection[0]], before: copyBoard(session.draft.board) };
    } else if (session.tool === 'select' && shape) {
      if (e.shiftKey) session.selection = selection.includes(index) ? selection.filter((i) => i !== index) : [...selection, index];
      else if (!selection.includes(index)) session.selection = [index];
      session.gesture = { kind: 'move', pointer: e.pointerId, first, before: copyBoard(session.draft.board), chosen: [...session.selection] };
    } else if (session.tool === 'select') {
      session.gesture = { kind: 'marquee', pointer: e.pointerId, first, add: e.shiftKey, base: e.shiftKey ? [...selection] : [] };
      if (!e.shiftKey) session.selection = [];
    } else {
      if (session.draft.board.length >= interviews.MOST_SHAPES) { toast('Your board is full. Delete or undo something first.', { bad: true }); return; }
      session.selection = [];
      session.stroke = { kind: session.tool, points: [first], text: '' };
      session.gesture = { kind: 'draw', pointer: e.pointerId };
    }
    board.setPointerCapture(e.pointerId);
    board.focus({ preventScroll: true });
    drawInterviewBoard(session);
  });
  board.addEventListener('pointermove', (e) => {
    const gesture = session.gesture;
    if (!gesture || e.pointerId !== gesture.pointer) return;
    if (gesture.kind === 'pan') {
      const rect = board.getBoundingClientRect(), v = gesture.view;
      setInterviewBoardView(session, { ...v, x: v.x - (e.clientX - gesture.x) / rect.width * v.w, y: v.y - (e.clientY - gesture.y) / rect.height * v.h });
      return;
    }
    const p = locate(e);
    if (gesture.kind === 'move') session.draft.board = gesture.before.map((s, i) => gesture.chosen.includes(i) ? interviews.moveShape(s, p[0] - gesture.first[0], p[1] - gesture.first[1]) : s);
    else if (gesture.kind === 'resize') session.draft.board = gesture.before.map((s, i) => i === gesture.index ? interviews.resizeShape(gesture.shape, gesture.handle, p) : s);
    else if (gesture.kind === 'marquee') session.marquee = interviews.normalRect(gesture.first, p);
    else if (session.stroke.kind === 'pen') {
      const last = session.stroke.points.at(-1);
      if (session.stroke.points.length < 128 && Math.hypot(p[0] - last[0], p[1] - last[1]) > 4 * interviewBoardView(session).w / 1000) {
        session.stroke.points.push(p);
        if (!interviews.validBoard([...session.draft.board, session.stroke])) session.stroke.points.pop();
      }
    } else session.stroke.points = [session.stroke.points[0], p];
    queueInterviewBoard(session);
  });
  board.addEventListener('pointerup', e => { if (e.pointerId === session.gesture?.pointer) finish(); });
  board.addEventListener('pointercancel', e => { if (e.pointerId === session.gesture?.pointer) finish(true); });
  board.addEventListener('lostpointercapture', () => finish(true));
  board.addEventListener('wheel', e => {
    if (session.gesture) return;
    e.preventDefault();
    if (session.boardEditing) closeInterviewBoardText(session, true);
    const v = interviewBoardView(session), rect = board.getBoundingClientRect();
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? rect.height : 1;
    if (e.ctrlKey || e.metaKey) {
      const p = locate(e), w = Math.max(100, Math.min(interviews.BOARD_LIMIT * 2, v.w * Math.exp(Math.max(-1, Math.min(1, e.deltaY * unit / 500)))));
      const ratio = w / v.w;
      setInterviewBoardView(session, { x: p[0] - (p[0] - v.x) * ratio, y: p[1] - (p[1] - v.y) * ratio, w, h: w * 7 / 10 });
    } else setInterviewBoardView(session, { ...v, x: v.x + e.deltaX * unit / rect.width * v.w, y: v.y + e.deltaY * unit / rect.height * v.h });
  }, { passive: false });
  board.addEventListener('focusin', (e) => {
    const target = e.target.closest('[data-board-shape]');
    if (!target || session.gesture) return;
    const index = Number(target.dataset.boardShape);
    if (!(session.selection || []).includes(index)) { session.selection = [index]; drawInterviewBoard(session); }
  });
  board.addEventListener('keydown', e => {
    if (session.switching || e.isComposing) return;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (e.code === 'Space' && !mod) { e.preventDefault(); e.stopPropagation(); session.panHeld = true; board.classList.add('panning'); return; }
    if (e.key === 'Escape') {
      if (!session.gesture && !(session.selection || []).length) return;
      e.stopPropagation();
      finish(true); session.selection = []; session.marquee = null; drawInterviewBoard(session); board.focus({ preventScroll: true }); return;
    }
    if (session.gesture) return;
    const handled = () => { e.preventDefault(); e.stopPropagation(); };
    if (mod && key === 'z') { handled(); stepInterviewBoard(session, !e.shiftKey); return; }
    if (mod && key === 'y') { handled(); stepInterviewBoard(session, false); return; }
    if (mod && key === 'a') { handled(); session.selection = session.draft.board.map((_, i) => i); drawInterviewBoard(session); return; }
    const chosen = (session.selection || []).map((i) => session.draft.board[i]).filter(Boolean);
    if (mod && (key === 'c' || key === 'x') && chosen.length) { handled(); session.boardClip = copyBoard(chosen); if (key === 'x') deleteInterviewSelection(session); return; }
    if (mod && key === 'v' && session.boardClip?.length) { handled(); pasteInterviewShapes(session, session.boardClip); return; }
    if (mod && key === 'd' && chosen.length) { handled(); pasteInterviewShapes(session, copyBoard(chosen)); return; }
    if ((e.key === 'Delete' || e.key === 'Backspace') && chosen.length) { handled(); deleteInterviewSelection(session); board.focus({ preventScroll: true }); return; }
    if (e.key === 'Enter' && chosen.length === 1 && chosen[0].kind !== 'pen') { handled(); editInterviewBoardText(session, session.selection[0]); return; }
    if (!mod && !e.altKey && BOARD_KEYS[key] && !e.target.closest('input')) { handled(); setInterviewTool(session, BOARD_KEYS[key]); return; }
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
    handled();
    const step = (e.shiftKey ? 50 : 10) * interviewBoardView(session).w / 1000;
    const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0;
    const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0;
    if (!moveInterviewSelection(session, dx, dy)) { const v = interviewBoardView(session); setInterviewBoardView(session, { ...v, x: v.x + dx, y: v.y + dy }); }
  });
  board.addEventListener('keyup', e => { if (e.code === 'Space') { session.panHeld = false; if (session.gesture?.kind !== 'pan') board.classList.remove('panning'); } });
  board.addEventListener('blur', e => { if (!board.contains(e.relatedTarget)) { session.panHeld = false; if (session.gesture?.kind !== 'pan') board.classList.remove('panning'); } }, true);
}

async function interviewPicture(board) {
  if (!board.length) return null;
  const source = await base64(new Blob([interviews.boardSvg(board)], { type: 'image/svg+xml' }));
  const image = new Image();
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('The board snapshot could not be made. Try again.')), 3000);
    image.onload = () => { clearTimeout(timeout); resolve(); };
    image.onerror = () => { clearTimeout(timeout); reject(new Error('The board snapshot could not be made. Try again.')); };
    image.src = `data:image/svg+xml;base64,${source}`;
  });
  const canvas = document.createElement('canvas');
  canvas.width = 1000; canvas.height = 700;
  canvas.getContext('2d').drawImage(image, 0, 0, 1000, 700);
  const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
  return blob ? new File([blob], `interview-whiteboard-${felix.newId()}.png`, { type: 'image/png' }) : null;
}

async function talkInterview(session, text, board = [], instruction = text) {
  if (session.talking) return;
  session.talking = true;
  try {
    await saveInterviewWorkspace(session);
    if (!interviewHere(session)) return;
    const ready = await chat.openInterview(session.item.id, () => interviewHere(session));
    if (!ready) {
      if (chat.isBusy()) toast('Felix is finishing another answer. Try again when it finishes.', { bad: true });
      return;
    }
    let read = !board.length;
    if (board.length) {
      try {
        const picture = await interviewPicture(board);
        if (!interviewHere(session)) return;
        read = Boolean(picture && await chat.attachBoard(picture));
      } catch (e) { toast('Your work is saved. Felix can review the written answer and board labels, but could not read the drawing.', { bad: true }); }
    }
    if (!read) instruction += '\nThe board snapshot could not be read. Review only the submitted written answer, labels and geometry; explicitly say that freehand drawing details could not be assessed. Do not invent visual observations.';
    if (interviewHere(session)) chat.send(text, { say: instruction });
  } finally { session.talking = false; }
}

async function prepareInterview(session) {
  if ((session.preparing && !session.waiting) || session.talking) return;
  if (chat.isBusy()) {
    session.waiting = true;
    session.preparing = true;
    drawInterviewProblems(session);
    drawInterviewChallenge(session);
    return;
  }
  session.waiting = false;
  if (session.switching || session.submitting) return;
  session.preparing = true;
  drawInterviewProblems(session);
  drawInterviewChallenge(session);
  try {
    await saveInterviewWorkspace(session);
    if (!interviewHere(session)) return;
    const request = interviews.preparation(session.item.framework);
    await talkInterview(session, request.text, [], request.say);
  } finally { if (!chat.isBusy()) { session.preparing = false; drawInterviewProblems(session); drawInterviewChallenge(session); } }
}

function interviewReplyFinished(id) {
  const session = interviewPage;
  if (!session || !interviewHere(session)) return;
  if (session.waiting) { prepareInterview(session).catch(fail); return; }
  if (session.item.id !== id) return;
  session.preparing = false;
  drawInterviewProblems(session);
  refreshInterview(id).catch(fail);
}

async function submitInterview(session) {
  if (session.submitting) return;
  const workspace = structuredClone(session.draft);
  if (!interviews.hasWork(workspace, session.item.challenge)) { $('#interview-answer')?.focus(); toast('Add your implementation, approach or drawing first.'); return; }
  session.submitting = true;
  drawInterviewProblems(session);
  drawInterviewStatus(session);
  let attempt = null;
  try {
    await saveInterviewWorkspace(session);
    const challenge = session.item.challenge.id;
    await checkInterviewKey(session).catch((e) => { if (e instanceof Locked) throw e; });
    const key = interviewKey(session) || (session.key?.challenge === challenge && session.key.status === 'ready' ? session.key.key : null);
    let tests = [];
    if (key?.tests?.length && interviewRunnable(session)) {
      session.running = 'submit';
      drawInterviewChecks(session);
      const hidden = await api(`/api/interviews/${enc(session.item.id)}/hidden-tests`, { method: 'POST', body: { challenge } }).catch(() => ({ tests: [] }));
      const run = await runInterviewTests(session, workspace.answer, key.entry, [...key.tests, ...(hidden.tests || []).filter((t) => !key.tests.some((k) => k.id === t.id))]);
      tests = run.reports;
      session.running = '';
    }
    attempt = felix.newId();
    const made = attempt;
    await interviewQueue(session, async () => {
      session.item = await api(`/api/interviews/${enc(session.item.id)}`, { method: 'PUT', body: { action: 'submit', revision: session.item.revision, challenge, attempt: made, workspace, tests } });
    });
    session.latest = attempt;
    if (JSON.stringify(session.draft) === JSON.stringify(workspace)) {
      interviews.clearDraft(localStorage, session.item.id, challenge);
      session.saved = session.generation;
    } else {
      session.saved = -1;
      interviews.keepDraft(localStorage, session.item.id, challenge, session.draft);
    }
    drawInterviewProblems(session);
    drawInterviewProgress(session);
  } finally { session.submitting = false; session.running = ''; drawInterviewProblems(session); drawInterviewStatus(session); drawInterviewChecks(session); drawInterviewResults(session); }
  if (attempt) showInterviewPane(session, 'result', true);
  if (attempt) loadInterviewKey(session, true).catch(() => {});
  if (attempt) gradeAndReview(session, attempt, workspace.board).catch(fail);
}

async function gradeAndReview(session, attempt, board) {
  const id = session.item.id;
  if (interviewKey(session)) {
    session.grading = attempt;
    session.gradeProgress = null;
    drawInterviewResults(session);
    const stop = watchProgress(`grade:${attempt}`, (view) => { session.gradeProgress = { attempt, ...view }; setMeter('interview-grade-meter', view); });
    try {
      await interviewQueue(session, async () => {
        const item = await api(`/api/interviews/${enc(id)}/grade`, { method: 'POST', body: { attempt } });
        if (interviewPage === session && session.item.id === id) session.item = { ...session.item, attempts: item.attempts, revision: item.revision };
      });
      await finishMeter('interview-grade-meter');
    } catch (e) {
      if (e instanceof Locked) throw e;
      session.gradeError = { attempt, message: e.message };
    } finally {
      stop();
      session.grading = '';
      if (interviewHere(session)) { drawInterviewResults(session); drawInterviewProgress(session); }
    }
  }
  if (!interviewHere(session) || session.item.id !== id) return;
  const done = session.item.attempts.find((a) => a.id === attempt);
  const note = done ? interviews.reviewNote(done) : '';
  await talkInterview(session, `Review my submitted answer${board.length ? ' and whiteboard' : ''}. Explain my strengths, gaps and what to practise next. Keep the full solution for when I ask.`, board, `Review my submitted attempt ${attempt}. Read interview_progress, assess the submitted answer and board against the challenge criteria with interview_feedback, explain my strengths and gaps, and suggest the next practice step. Do not reveal a complete solution unless I ask for it.${note ? `\n${note}` : ''}`);
}

function interviewKey(session) {
  const c = session.item.challenge;
  return c && session.key?.challenge === c.id && session.key.status === 'ready' && !session.key.key?.verify ? session.key.key : null;
}

function checkInterviewKey(session) {
  const c = session.item.challenge;
  const id = session.item.id;
  if (!c || session.key?.challenge !== c.id || session.key.status !== 'ready' || !session.key.key?.verify) return Promise.resolve();
  if (session.checking?.challenge === c.id) return session.checking.done;
  const done = (async () => {
    const job = await api(`/api/interviews/${enc(id)}/key/check`, { method: 'POST', body: { challenge: c.id } });
    let view;
    if (job.done) view = await api(`/api/interviews/${enc(id)}/key?challenge=${enc(c.id)}`);
    else {
      const out = await runInSandbox({ code: job.solution, entry: job.entry, language: job.language === 'sql' ? 'sql' : 'javascript', tests: job.tests.map((t) => ({ id: t.id, args: t.args })) });
      const byId = new Map((out.results || []).map((r) => [r.id, r]));
      const reports = job.tests.map((t) => {
        const r = byId.get(t.id);
        return { id: t.id, pass: Boolean(r && !r.error && interviews.sameValue(t.expected, r.got, t.unordered)), got: r ? (r.error ? `Error: ${r.error}` : JSON.stringify(r.got) || '') : 'Did not run' };
      });
      view = await api(`/api/interviews/${enc(id)}/key/checked`, { method: 'POST', body: { challenge: c.id, stamp: job.stamp, reports, error: out.error || '' } });
    }
    if (session.item.id === id && session.item.challenge?.id === c.id) {
      session.key = { challenge: c.id, ...view };
      if (interviewHere(session)) drawInterviewChecks(session);
    }
  })().finally(() => { if (session.checking?.done === done) session.checking = null; });
  session.checking = { challenge: c.id, done };
  return done;
}

function interviewLanguage(session) {
  return String(session.item.challenge?.coding?.language || session.item.framework.language || '').trim();
}

function interviewRunLanguage(session) {
  const language = interviewLanguage(session);
  if (/^(javascript|js|node(\.?js)?)$/i.test(language)) return 'javascript';
  if (/^(typescript|ts)$/i.test(language)) return 'typescript';
  if (/^(python|py)\s*3?$/i.test(language)) return 'python';
  if (/^(sql|sqlite|postgres(ql)?|mysql|t-?sql|pl\/?sql)$/i.test(language)) return 'sql';
  if (/^(c\+\+|cpp|cxx|cc)(\s*\d+)?$/i.test(language)) return 'cpp';
  if (/^(go|golang)$/i.test(language)) return 'go';
  if (/^java(\s*\d+)?$/i.test(language)) return 'java';
  if (/^(rust|rs)$/i.test(language)) return 'rust';
  return '';
}

function interviewRunnable(session) {
  return Boolean(interviewRunLanguage(session));
}

const RUNTIMES = {
  python: { path: '/api/python', label: 'Getting Python ready, once', none: 'Python can’t run on this copy of leo.', failed: 'Python could not be downloaded. Check the connection and try again.', mb: 12 },
  cpp: { path: '/api/cpp', label: 'Getting C++ ready, once', none: 'C++ can’t run on this copy of leo.', failed: 'The C++ compiler could not be downloaded. Check the connection and try again.', mb: 27 },
  go: { path: '/api/go', label: 'Getting Go ready, once', none: 'Go can’t run on this copy of leo.', failed: 'Go could not be downloaded. Check the connection and try again.', mb: 21 },
  java: { path: '/api/java', label: 'Getting Java ready, once', none: 'Java can’t run on this copy of leo.', failed: 'Java could not be downloaded. Check the connection and try again.', mb: 5 },
  rust: { path: '/api/rust', label: 'Getting Rust ready, once', none: 'Rust can’t run on this copy of leo.', failed: 'Rust could not be downloaded. Check the connection and try again.', mb: 46 },
};

async function interviewRuntimeReady(session, name) {
  const runtime = RUNTIMES[name];
  let view = await api(runtime.path);
  if (view.status === 'ready') return true;
  if (view.status === 'unavailable') { session.runtimeNote = runtime.none; return false; }
  if (view.status !== 'fetching') view = await api(runtime.path, { method: 'POST' });
  const started = Date.now();
  let drawn = false;
  while (view.status === 'fetching' && Date.now() - started < 600000) {
    const total = Number(view.total) || (view.mb || runtime.mb) * 1e6;
    session.runtimeFetch = { label: `${runtime.label} (about ${view.mb || runtime.mb} MB)`, fraction: Math.min(0.99, (Number(view.done) || 0) / total), seconds: Math.round((Date.now() - started) / 1000) };
    if (!drawn && interviewHere(session)) { drawInterviewChecks(session); drawn = true; } else setMeter('interview-runtime-meter', session.runtimeFetch);
    await new Promise((resolve) => setTimeout(resolve, 800));
    view = await api(runtime.path);
  }
  session.runtimeFetch = null;
  session.runtimeNote = view.status === 'ready' ? '' : view.error || runtime.failed;
  return view.status === 'ready';
}

async function loadInterviewKey(session, force = false) {
  const c = session.item.challenge;
  if (!c || !interviewHere(session)) return;
  if (!force && session.key?.challenge === c.id && session.key.status !== 'preparing') return;
  clearTimeout(session.keyTimer);
  try {
    const view = await api(`/api/interviews/${enc(session.item.id)}/key?challenge=${enc(c.id)}`);
    if (!interviewHere(session) || session.item.challenge?.id !== c.id) return;
    const changed = session.key?.challenge !== c.id || session.key.status !== view.status || JSON.stringify(session.key.key) !== JSON.stringify(view.key);
    if (session.key?.challenge === c.id && session.key.status === 'preparing' && view.status !== 'preparing') await finishMeter('interview-key-meter');
    if (!interviewHere(session) || session.item.challenge?.id !== c.id) return;
    session.key = { challenge: c.id, ...view };
    if (changed) drawInterviewChecks(session);
    else if (view.progress) setMeter('interview-key-meter', view.progress);
    if (view.key?.verify) checkInterviewKey(session).catch(() => {});
    if (view.status === 'preparing') session.keyTimer = setTimeout(() => loadInterviewKey(session, true).catch(() => {}), 1500);
  } catch (e) { if (e instanceof Locked) throw e; }
}

const runner = { frame: null, ready: null, cppWarm: false, clangBundle: '', goWarm: false, javaWarm: false, rustWarm: false };

function runnerFrame() {
  if (runner.frame?.isConnected && runner.ready) return runner.ready;
  runner.cppWarm = false;
  runner.goWarm = false;
  runner.javaWarm = false;
  runner.rustWarm = false;
  const frame = document.createElement('iframe');
  frame.setAttribute('sandbox', 'allow-scripts');
  frame.hidden = true;
  frame.title = 'Code runner';
  runner.frame = frame;
  runner.ready = new Promise((resolve) => {
    const heard = (e) => {
      if (e.source !== frame.contentWindow || e.data?.kind !== 'ready') return;
      window.removeEventListener('message', heard);
      resolve(frame);
    };
    window.addEventListener('message', heard);
  });
  frame.src = '/runner.html';
  document.body.appendChild(frame);
  return runner.ready;
}

async function runInSandbox(job, onProgress = null, transfer = []) {
  const limit = job.language === 'python' ? 75000 : ['cpp', 'go', 'java', 'rust'].includes(job.language) ? 270000 : 15000;
  const frame = await Promise.race([runnerFrame(), new Promise((resolve) => setTimeout(() => resolve(null), 10000))]);
  if (!frame) { runner.frame?.remove(); runner.frame = null; runner.ready = null; return { error: 'The code runner did not start. Try Run again.', results: [], logs: [] }; }
  return new Promise((resolve) => {
    const run = felix.newId();
    let timer = 0;
    const finish = (result) => {
      clearTimeout(timer);
      window.removeEventListener('message', heard);
      resolve(result);
    };
    const heard = (e) => {
      if (e.source !== frame.contentWindow || e.data?.run !== run) return;
      if (e.data.kind === 'progress' && onProgress) onProgress(Number(e.data.fraction) || 0);
      if (e.data.kind === 'done') finish(e.data);
    };
    window.addEventListener('message', heard);
    timer = setTimeout(() => { frame.remove(); runner.frame = null; runner.ready = null; finish({ error: 'The code runner did not answer. Try Run again.', results: [], logs: [] }); }, limit);
    frame.contentWindow.postMessage({ ...job, kind: 'run', run, base: `${location.origin}/` }, '*', transfer);
  });
}

async function interviewGoToolchain(session) {
  const started = Date.now();
  const show = (fraction) => {
    const first = !session.runtimeFetch;
    session.runtimeFetch = { label: 'Loading Go into this page, once per visit', fraction, seconds: Math.round((Date.now() - started) / 1000) };
    if (first && interviewHere(session)) drawInterviewChecks(session); else setMeter('interview-runtime-meter', session.runtimeFetch);
  };
  show(0);
  const get = async (path, text) => {
    const response = await fetch(path, { credentials: 'same-origin' });
    if (!response.ok) throw new Error(`${path} ${response.status}`);
    return text ? response.text() : response.arrayBuffer();
  };
  const shim = await get('/wasi-shim.js', true);
  const config = await get('/goroot/importcfg');
  const names = ['compile.wasm', 'link.wasm', ...new TextDecoder().decode(config).split('\n').map((line) => line.match(/^packagefile \S+=\/go\/(std\/[\w./-]+\.a)$/)?.[1]).filter(Boolean)];
  const files = { importcfg: config };
  let done = 0;
  const queue = [...names];
  await Promise.all(Array.from({ length: 6 }, async () => {
    while (queue.length) {
      const name = queue.shift();
      files[name] = await get(`/goroot/${name}`);
      done += name.endsWith('compile.wasm') ? 50 : name.endsWith('link.wasm') ? 12 : 0.4;
      show(Math.min(0.95, done / (62 + 0.4 * (names.length - 2))));
    }
  }));
  return { shim, files };
}

async function interviewJavaToolchain(session) {
  const started = Date.now();
  const show = (fraction) => {
    const first = !session.runtimeFetch;
    session.runtimeFetch = { label: 'Loading Java into this page, once per visit', fraction, seconds: Math.round((Date.now() - started) / 1000) };
    if (first && interviewHere(session)) drawInterviewChecks(session); else setMeter('interview-runtime-meter', session.runtimeFetch);
  };
  show(0);
  const get = async (name, text) => {
    const response = await fetch(`/javaroot/${name}`, { credentials: 'same-origin' });
    if (!response.ok) throw new Error(`${name} ${response.status}`);
    return text ? response.text() : response.arrayBuffer();
  };
  const runtime = await get('compiler.wasm-runtime.js', true);
  show(0.05);
  const files = {};
  const sizes = { 'compiler.wasm': 0.6, 'runtime-classlib-teavm.bin': 0.33, 'compile-classlib-teavm.bin': 0.03 };
  let done = 0.05;
  await Promise.all(Object.keys(sizes).map(async (name) => {
    files[name] = await get(name);
    done += sizes[name];
    show(Math.min(0.97, done));
  }));
  return { runtime, files };
}

async function runInterviewJava(session, code, entry, tests) {
  const made = interviews.javaDriver(code, entry, tests);
  const failed = (why) => ({ reports: tests.map((t) => ({ id: t.id, pass: false, got: why.split('\n')[0] })), error: why, logs: [] });
  if (made.error) return failed(made.error);
  let out = null;
  for (let tries = 0; tries < 2; tries++) {
    let extra = {};
    let transfer = [];
    if (!runner.javaWarm) {
      try {
        extra = await interviewJavaToolchain(session);
      } catch (e) {
        session.runtimeFetch = null;
        if (interviewHere(session)) drawInterviewChecks(session);
        return failed('Java could not load in this page. Try Run again.');
      }
      transfer = Object.values(extra.files);
    }
    out = await runInSandbox({ language: 'java', entry, source: made.source, main: made.main, ...extra }, null, transfer);
    if (session.runtimeFetch) { session.runtimeFetch = null; if (interviewHere(session)) drawInterviewChecks(session); }
    if (!out.missing) { runner.javaWarm = true; break; }
    runner.javaWarm = false;
  }
  if (out.missing) return failed('Java could not start in this page. Try Run again.');
  if (out.diagnostics) return failed(`Your code did not compile:\n${interviews.javaProblem(out.diagnostics, made.offset)}`);
  const parsed = interviews.driverResults(out.stdout || '');
  const byId = new Map(parsed.results.map((r) => [r.id, r]));
  const reports = tests.map((t) => {
    const r = byId.get(t.id);
    if (!r) return { id: t.id, pass: false, got: out.error || parsed.error || 'Did not run' };
    if (r.error) return { id: t.id, pass: false, got: `Error: ${r.error}` };
    return { id: t.id, pass: interviews.sameValue(t.expected, r.got, t.unordered), got: JSON.stringify(r.got) };
  });
  return { reports, error: out.error || parsed.error || '', logs: parsed.logs };
}

async function interviewRustToolchain(session) {
  const started = Date.now();
  const show = (fraction) => {
    const first = !session.runtimeFetch;
    session.runtimeFetch = { label: 'Loading Rust into this page, once per visit', fraction, seconds: Math.round((Date.now() - started) / 1000) };
    if (first && interviewHere(session)) drawInterviewChecks(session); else setMeter('interview-runtime-meter', session.runtimeFetch);
  };
  show(0);
  const get = async (path, text) => {
    const response = await fetch(path, { credentials: 'same-origin' });
    if (!response.ok) throw new Error(`${path} ${response.status}`);
    return text ? response.text() : response.arrayBuffer();
  };
  const shim = await get('/wasi-shim.js', true);
  const listed = (await get('/rustroot/files.sha256', true)).split('\n').map((line) => line.split('  ')[1]).filter((name) => name && (name.endsWith('.wasm') || name.endsWith('.rlib')));
  const files = {};
  let done = 0;
  const queue = [...listed];
  await Promise.all(Array.from({ length: 4 }, async () => {
    while (queue.length) {
      const name = queue.shift();
      files[name] = await get(`/rustroot/${name}`);
      done += files[name].byteLength;
      show(Math.min(0.97, done / 131000000));
    }
  }));
  return { shim, files };
}

async function runInterviewRust(session, code, entry, tests) {
  const made = interviews.rustDriver(code, entry, tests);
  const failed = (why) => ({ reports: tests.map((t) => ({ id: t.id, pass: false, got: why.split('\n')[0] })), error: why, logs: [] });
  if (made.error) return failed(made.error);
  const results = [];
  const logs = [];
  let from = 0;
  let error = '';
  for (let round = 0; round <= tests.length; round++) {
    let out = null;
    for (let tries = 0; tries < 2; tries++) {
      let extra = {};
      let transfer = [];
      if (!runner.rustWarm) {
        try {
          extra = await interviewRustToolchain(session);
        } catch (e) {
          session.runtimeFetch = null;
          if (interviewHere(session)) drawInterviewChecks(session);
          return failed('Rust could not load in this page. Try Run again.');
        }
        transfer = Object.values(extra.files);
      }
      out = await runInSandbox({ language: 'rust', entry, source: made.source, from, ...extra }, null, transfer);
      if (session.runtimeFetch) { session.runtimeFetch = null; if (interviewHere(session)) drawInterviewChecks(session); }
      if (!out.missing) { runner.rustWarm = true; break; }
      runner.rustWarm = false;
    }
    if (out.missing) return failed('Rust could not start in this page. Try Run again.');
    if (out.error && !out.stdout) { error = out.error; break; }
    const step = interviews.rustRound(out.stdout || '', out.stderr || '', made.ids, from);
    if (step.compile) return failed(`Your code did not compile:\n${step.compile}`);
    results.push(...step.results);
    logs.push(...step.logs);
    if (step.next === null) break;
    from = step.next;
  }
  const byId = new Map(results.map((r) => [r.id, r]));
  const reports = tests.map((t) => {
    const r = byId.get(t.id);
    if (!r) return { id: t.id, pass: false, got: error || 'Did not run' };
    if (r.error) return { id: t.id, pass: false, got: `Error: ${r.error}` };
    return { id: t.id, pass: interviews.sameValue(t.expected, r.got, t.unordered), got: JSON.stringify(r.got) };
  });
  return { reports, error, logs: logs.slice(0, 40) };
}

async function runInterviewGo(session, code, entry, tests) {
  const made = interviews.goDriver(code, entry, tests);
  const failed = (why) => ({ reports: tests.map((t) => ({ id: t.id, pass: false, got: why.split('\n')[0] })), error: why, logs: [] });
  if (made.error) return failed(made.error);
  let out = null;
  for (let tries = 0; tries < 2; tries++) {
    let extra = {};
    let transfer = [];
    if (!runner.goWarm) {
      try {
        extra = await interviewGoToolchain(session);
      } catch (e) {
        session.runtimeFetch = null;
        if (interviewHere(session)) drawInterviewChecks(session);
        return failed('Go could not load in this page. Try Run again.');
      }
      transfer = Object.values(extra.files);
    }
    out = await runInSandbox({ language: 'go', entry, sources: made.files, ...extra }, null, transfer);
    if (session.runtimeFetch) { session.runtimeFetch = null; if (interviewHere(session)) drawInterviewChecks(session); }
    if (!out.missing) { runner.goWarm = true; break; }
    runner.goWarm = false;
  }
  if (out.missing) return failed('Go could not start in this page. Try Run again.');
  if (out.compile !== undefined) return failed(`Your code did not compile:\n${interviews.goProblem(out.compile, made.offset)}`);
  const parsed = interviews.driverResults(out.stdout || '');
  const byId = new Map(parsed.results.map((r) => [r.id, r]));
  const reports = tests.map((t) => {
    const r = byId.get(t.id);
    if (!r) return { id: t.id, pass: false, got: out.error || parsed.error || 'Did not run' };
    if (r.error) return { id: t.id, pass: false, got: `Error: ${r.error}` };
    return { id: t.id, pass: interviews.sameValue(t.expected, r.got, t.unordered), got: JSON.stringify(r.got) };
  });
  return { reports, error: out.error || parsed.error || '', logs: parsed.logs };
}

async function runInterviewTests(session, code, entry, tests) {
  let language = interviewRunLanguage(session);
  if (['python', 'cpp', 'go', 'java', 'rust'].includes(language) && !(await interviewRuntimeReady(session, language))) return { reports: [], error: session.runtimeNote, logs: [] };
  if (language === 'rust') return runInterviewRust(session, code, entry, tests);
  if (language === 'go') return runInterviewGo(session, code, entry, tests);
  if (language === 'java') return runInterviewJava(session, code, entry, tests);
  if (language === 'cpp') {
    const made = interviews.cppDriver(code, entry, tests);
    const failed = (why) => ({ reports: tests.map((t) => ({ id: t.id, pass: false, got: why.split('\n')[0] })), error: why, logs: [] });
    if (made.error) return failed(made.error);
    const started = Date.now();
    let loaded = 0;
    const show = () => {
      const seconds = (Date.now() - started) / 1000;
      const first = !session.runtimeFetch;
      session.runtimeFetch = { label: 'Starting the C++ compiler, once per visit', fraction: Math.min(0.97, Math.max(loaded * 0.9, 0.9 * (1 - Math.exp(-seconds / 20)))), seconds: Math.round(seconds), expected: 20 };
      if (first && interviewHere(session)) drawInterviewChecks(session); else setMeter('interview-runtime-meter', session.runtimeFetch);
    };
    const ticking = runner.cppWarm ? 0 : setInterval(show, 500);
    if (!runner.clangBundle) {
      try {
        const response = await fetch('/clang/bundle.js', { credentials: 'same-origin' });
        if (!response.ok) throw new Error(String(response.status));
        runner.clangBundle = await response.text();
      } catch (e) {
        clearInterval(ticking);
        session.runtimeFetch = null;
        return failed('The C++ compiler could not load. Try Run again.');
      }
    }
    const out = await runInSandbox({ language: 'cpp', entry, files: made.files, bundle: runner.clangBundle }, (fraction) => { loaded = fraction; if (!runner.cppWarm) show(); });
    clearInterval(ticking);
    if (out.compile !== undefined || out.stdout !== undefined) runner.cppWarm = true;
    if (session.runtimeFetch) { session.runtimeFetch = null; if (interviewHere(session)) drawInterviewChecks(session); }
    if (out.compile !== undefined) return failed(`Your code did not compile:\n${interviews.cppProblem(out.compile)}`);
    const parsed = interviews.cppResults(out.stdout || '');
    const byId = new Map(parsed.results.map((r) => [r.id, r]));
    const reports = tests.map((t) => {
      const r = byId.get(t.id);
      if (!r) return { id: t.id, pass: false, got: out.error || 'Did not run' };
      return { id: t.id, pass: interviews.sameValue(t.expected, r.got, t.unordered), got: JSON.stringify(r.got) };
    });
    return { reports, error: out.error || '', logs: parsed.logs };
  }
  if (language === 'typescript') {
    const strip = await interviewTypeScript();
    if (!strip) return { reports: tests.map((t) => ({ id: t.id, pass: false, got: 'TypeScript could not load' })), error: 'TypeScript could not load in this browser. Try Run again.', logs: [] };
    try {
      code = strip(code);
    } catch (e) {
      const why = `Your code did not compile: ${String(e && e.message ? e.message : e).slice(0, 300)}`;
      return { reports: tests.map((t) => ({ id: t.id, pass: false, got: why })), error: why, logs: [] };
    }
    language = 'javascript';
  }
  const out = await runInSandbox({ code, entry, language, tests: tests.map((t) => ({ id: t.id, args: t.args })) });
  const byId = new Map((out.results || []).map((r) => [r.id, r]));
  const reports = tests.map((t) => {
    const r = byId.get(t.id);
    if (!r) return { id: t.id, pass: false, got: out.error || 'Did not run' };
    if (r.error) return { id: t.id, pass: false, got: `Error: ${r.error}` };
    return { id: t.id, pass: interviews.sameValue(t.expected, r.got, t.unordered), got: JSON.stringify(r.got) };
  });
  return { reports, error: out.error || '', logs: out.logs || [] };
}

async function runInterviewExamples(session) {
  const key = interviewKey(session);
  if (!key?.tests?.length || session.running) return;
  session.running = 'examples';
  drawInterviewChecks(session);
  try {
    const run = await runInterviewTests(session, session.draft.answer, key.entry, key.tests);
    const challenge = session.item.challenge.id;
    api(`/api/interviews/${enc(session.item.id)}/run`, { method: 'POST', body: { challenge, reports: run.reports, error: run.error || '' } }).catch(() => {});
    if (!interviewHere(session)) return;
    session.ran = { challenge, ...run };
  } finally { session.running = ''; if (interviewHere(session)) drawInterviewChecks(session); }
  if (session.ran?.challenge === session.item.challenge?.id) showInterviewPane(session, 'cases', true);
}

async function checkInterviewCoverage(session) {
  const c = session.item.challenge;
  if (!c || session.covering) return;
  session.covering = true;
  session.coverProgress = null;
  drawInterviewChecks(session);
  const stop = watchProgress(`coverage:${session.item.id}:${c.id}`, (view) => { session.coverProgress = view; setMeter('interview-coverage-meter', view); });
  try {
    const found = await api(`/api/interviews/${enc(session.item.id)}/coverage`, { method: 'POST', body: { challenge: c.id, workspace: interviews.workspace(session.draft) } });
    await finishMeter('interview-coverage-meter');
    if (interviewHere(session) && session.item.challenge?.id === c.id) session.coverage = { challenge: c.id, ...found };
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  } finally { stop(); session.covering = false; if (interviewHere(session)) drawInterviewChecks(session); }
}

const COVER_MARK = { yes: 'Covered', partly: 'Partly', no: 'Not yet' };

function drawInterviewChecks(session) {
  paintInterviewChecks(session);
  drawInterviewPanes(session);
}

function paintInterviewChecks(session) {
  if (!interviewHere(session)) return;
  const box = $('#interview-checks');
  const pane = $('#interview-pane-cases');
  if (pane) pane.innerHTML = '';
  const hint = $('#interview-submit-hint');
  const c = session.item.challenge;
  if (!box || !c) return;
  const coding = Boolean(c.coding);
  const view = session.key?.challenge === c.id ? session.key : null;
  const key = interviewKey(session);
  const runnable = coding && interviewRunnable(session);
  if (hint) hint.textContent = key ? (coding ? (runnable ? `Run checks the examples in your browser. Submit adds ${key.hidden_tests} hidden test${key.hidden_tests === 1 ? '' : 's'} and a quick review; Felix’s full review follows in the chat.` : `${interviewLanguage(session) || 'This language'} doesn’t run in the browser yet, so Submit gives a quick review and Felix’s full review.`) : 'Submit checks your answers at once, then a quick review; Felix’s full review follows in the chat.') : 'Submit when you’re ready. Felix reviews your work in the chat.';
  if (!view || view.status === 'none') {
    box.innerHTML = session.item.framework.track ? `<p class="hint interview-checks-status">This problem has no instant checks. <button class="link-btn" data-interview="prepare-key">Prepare them</button></p>` : '';
    return;
  }
  if (view.status === 'preparing') { box.innerHTML = `<div class="interview-checks-status">${meterBar('interview-key-meter', 'Preparing instant checks', view.progress || { fraction: 0 })}<p class="hint">Start working; they’ll appear here.</p></div>`; return; }
  if (view.status === 'ready' && view.key?.verify) { box.innerHTML = '<p class="hint interview-checks-status" role="status">Checking the test cases against a reference solution…</p>'; return; }
  const python = session.runtimeFetch ? meterBar('interview-runtime-meter', session.runtimeFetch.label, session.runtimeFetch) : session.runtimeNote ? `<p class="hint interview-checks-status" role="status">${esc(session.runtimeNote)}</p>` : '';
  if (view.status === 'failed') { box.innerHTML = '<p class="hint interview-checks-status">Instant checks couldn’t be prepared. Submit still gets Felix’s review. <button class="link-btn" data-interview="prepare-key">Try again</button></p>'; return; }
  const values = session.draft.values || {};
  const cover = session.coverage?.challenge === c.id ? session.coverage : null;
  const coverOf = (id, list) => (list || []).find((x) => x.id === id)?.covered;
  const asks = key.asks.length ? `<div class="interview-asks"><h3>Your answers</h3>${key.asks.map((a) => a.kind === 'choice'
    ? `<div class="set-row interview-choice"><span class="set-label">${esc(a.label)}</span><div class="pick-chips" role="group" aria-label="${esc(a.label)}">${a.options.map((o) => `<button class="pick-chip${values[a.id] === o ? ' on' : ''}" aria-pressed="${values[a.id] === o}" data-interview="ask-choice" data-ask-id="${esc(a.id)}" data-option="${esc(o)}">${esc(o)}</button>`).join('')}</div></div>`
    : `<label class="set-row" for="interview-ask-${esc(a.id)}"><span class="set-label grow">${esc(a.label)}</span><span class="interview-ask">${a.unit && /^[$€£¥]$/.test(a.unit) ? `<span class="hint">${esc(a.unit)}</span>` : ''}<input class="set-input" id="interview-ask-${esc(a.id)}" data-ask="${esc(a.id)}" inputmode="decimal" autocomplete="off" maxlength="200" value="${esc(values[a.id] || '')}" placeholder="Your value">${a.unit && !/^[$€£¥]$/.test(a.unit) && !a.label.includes(a.unit) ? `<span class="hint">${esc(a.unit)}</span>` : ''}</span></label>`).join('')}</div>` : '';
  const sections = key.sections.length ? `<div class="interview-covers"><h3>A strong answer covers</h3><div class="pick-chips">${key.sections.map((x) => { const state = coverOf(x.id, cover?.sections); return `<span class="chip interview-cover${state ? ` ${state}` : ''}">${state === 'yes' ? ICON.check : ''}${esc(x.name)}</span>`; }).join('')}</div></div>` : '';
  const criteria = cover?.criteria?.length ? `<ul class="interview-coverage">${cover.criteria.map((x) => `<li class="${esc(x.covered)}"><span class="chip">${COVER_MARK[x.covered] || 'Not yet'}</span> ${esc(x.text)}</li>`).join('')}</ul>` : '';
  const ran = session.ran?.challenge === c.id ? session.ran : null;
  const tests = coding && key.tests.length ? `<div class="interview-tests"><div class="interview-toolbar"><h3>Test cases</h3>${ran?.reports.length ? `<span class="chip interview-tally ${ran.reports.every((r) => r.pass) ? 'good' : 'bad'}">${ran.reports.filter((r) => r.pass).length}/${ran.reports.length} passed</span>` : ''}</div><p class="hint">${key.revealed ? (runnable ? 'Run checks every test in your browser, including the ones hidden until you submitted.' : 'Every test, including the ones hidden until you submitted. Felix reviews your code against them.') : runnable ? 'Run checks these in your browser. Submit adds the hidden tests.' : 'These are the visible examples. Felix reviews your code against them.'}</p>${ran?.error ? `<p class="set-note warn">${esc(ran.error)}</p>` : ''}<ul>${key.tests.map((t) => { const r = ran?.reports.find((x) => x.id === t.id); return `<li class="${r ? (r.pass ? 'pass' : 'fail') : ''}"><span class="interview-mark">${r ? (r.pass ? ICON.check : ICON.close) : ''}</span><code>${esc(interviews.callText(key.entry, t.args))}</code>${t.hidden ? '<span class="chip">Hidden</span>' : ''}<span class="hint">→ ${esc(JSON.stringify(t.expected))}${r && !r.pass ? ` · yours: ${esc(r.got)}` : ''}</span></li>`; }).join('')}</ul>${ran?.logs?.length ? `<details class="source-more"><summary>${ICON.chevron}<span>Printed output</span></summary><pre class="interview-submitted">${esc(ran.logs.join('\n'))}</pre></details>` : ''}</div>` : '';
  const buttons = [
    runnable && key.tests.length ? `<button class="btn sm" data-interview="run"${session.running ? ' disabled' : ''}>${session.running === 'examples' ? 'Running…' : session.running === 'submit' ? 'Running tests…' : 'Run'}</button>` : '',
    !coding || key.sections.length ? `<button class="btn sm" data-interview="coverage"${session.covering ? ' disabled' : ''}>${session.covering ? 'Checking…' : 'Check coverage'}</button>` : '',
  ].filter(Boolean).join('');
  if (pane) pane.innerHTML = tests;
  const covering = session.covering ? meterBar('interview-coverage-meter', 'Checking coverage', session.coverProgress || { fraction: 0 }) : '';
  keepOpen(box, `${python}${asks}${sections}${criteria}${covering}${buttons ? `<div class="buttons">${buttons}</div>` : ''}`);
}

const PANES = ['question', 'cases', 'result', 'submissions'];

function interviewPanes(session) {
  const c = session.item.challenge;
  if (!c) return [];
  const key = interviewKey(session);
  const result = interviewResultAttempt(session);
  const submitted = session.item.attempts.filter((a) => a.challenge.id === c.id).length;
  const passed = result ? (result.checks || []).filter((x) => x.pass).length : 0;
  const total = result ? (result.checks || []).length : 0;
  return [
    ['question', 'Question'],
    ...(c.coding && key?.tests?.length ? [['cases', 'Test cases']] : []),
    ...(result ? [['result', total ? `Result · ${passed}/${total}` : 'Result']] : []),
    ['submissions', submitted ? `Submissions · ${submitted}` : 'Submissions'],
  ];
}

function drawInterviewPanes(session) {
  if (!interviewHere(session)) return;
  const bar = $('#interview-panes');
  if (!bar) return;
  const id = session.item.challenge?.id || '';
  if (session.paneFor !== id) { session.paneFor = id; session.pane = 'question'; }
  const panes = interviewPanes(session);
  if (!panes.some(([name]) => name === session.pane)) session.pane = 'question';
  const html = panes.length ? `<div class="library-views" role="tablist" aria-label="Problem">${panes.map(([name, label]) => `<button class="btn sm plain${session.pane === name ? ' on' : ''}" role="tab" id="interview-tab-${name}" aria-controls="interview-pane-${name}" aria-selected="${session.pane === name}" tabindex="${session.pane === name ? 0 : -1}" data-interview="pane" data-pane="${name}">${esc(label)}</button>`).join('')}</div>` : '';
  if (bar.dataset.drawn !== html) {
    const focused = bar.contains(document.activeElement) ? document.activeElement.dataset.pane : '';
    bar.innerHTML = html;
    bar.dataset.drawn = html;
    if (focused) bar.querySelector(`[data-pane="${focused}"]`)?.focus({ preventScroll: true });
  }
  bar.hidden = !panes.length;
  for (const name of PANES) {
    const pane = $(`#interview-pane-${name}`);
    if (!pane) continue;
    pane.hidden = panes.length ? session.pane !== name : name !== 'question';
    if (panes.length) pane.setAttribute('aria-labelledby', `interview-tab-${name}`);
  }
}

function showInterviewPane(session, name, reveal = false) {
  if (!interviewHere(session)) return;
  session.pane = name;
  drawInterviewPanes(session);
  const card = $('#interview-challenge');
  if (reveal && card && card.getBoundingClientRect().top < 0) card.scrollIntoView({ block: 'start' });
}

const VERDICT = { met: 'Met', partly: 'Partly', missed: 'Missed' };

function interviewResultAttempt(session) {
  const c = session.item.challenge;
  return c ? session.item.attempts.find((a) => a.id === session.latest && a.challenge.id === c.id) || null : null;
}

function interviewCaseDetail(chosen) {
  if (!chosen) return '';
  const status = `<span class="chip interview-tally ${chosen.pass ? 'good' : 'bad'}">${chosen.pass ? ICON.check : ICON.close}${chosen.pass ? 'Passed' : 'Failed'}</span>`;
  const block = (label, text, empty) => `<div class="interview-io"><span class="set-label">${label}</span>${text ? `<pre class="interview-output">${esc(text)}</pre>` : `<p class="hint">${empty}</p>`}</div>`;
  const body = chosen.group === 'section'
    ? `<p class="hint">${chosen.pass ? 'Your answer covers this part.' : 'This part was not found in your answer. A strong answer covers it.'}</p>`
    : `${chosen.input ? block('Input', chosen.input, '') : ''}<div class="interview-io-pair">${block(chosen.group === 'test' ? 'Your output' : 'Your answer', chosen.yours, chosen.group === 'test' ? 'Nothing was returned.' : 'Not answered.')}${block(chosen.group === 'test' ? 'Expected output' : 'Expected answer', chosen.expected, 'Not given.')}</div>`;
  return `<div class="interview-case-head"><b>${esc(chosen.title)}</b>${status}</div>${body}${chosen.pass ? '' : `<button class="link-btn" data-interview="check-wrong" data-check="${esc(chosen.id)}">This check looks wrong</button>`}`;
}

function drawInterviewResults(session) {
  paintInterviewResults(session);
  drawInterviewPanes(session);
}

function paintInterviewResults(session) {
  if (!interviewHere(session)) return;
  const box = $('#interview-results');
  if (!box || !session.item.challenge) return;
  const attempt = interviewResultAttempt(session);
  if (!attempt) { box.innerHTML = ''; return; }
  const cases = interviews.resultCases(attempt.checks);
  if (session.resultCase?.attempt !== attempt.id) session.resultCase = { attempt: attempt.id, check: '' };
  session.resultCase.check = interviews.pickCase(cases, session.resultCase.check);
  const chosen = cases.find((x) => x.id === session.resultCase.check);
  const passed = cases.filter((x) => x.pass).length;
  const failed = cases.length - passed;
  const groups = interviews.checkSummary(attempt.checks).map((g) => `${g.pass}/${g.total} ${g.name}`).join(' · ');
  const grade = attempt.grade;
  const counts = grade ? ['met', 'partly', 'missed'].map((v) => [v, grade.criteria.filter((x) => x.verdict === v).length]).filter(([, n]) => n) : [];
  const open = session.reviewOpen?.attempt === attempt.id && session.reviewOpen.open;
  const gradeError = session.gradeError?.attempt === attempt.id ? session.gradeError.message : '';
  const review = grade
    ? `<div class="interview-review"><h3>Quick review</h3>${counts.length ? `<div class="pick-chips">${counts.map(([v, n]) => `<span class="chip interview-tally ${v}">${n} ${VERDICT[v].toLowerCase()}</span>`).join('')}</div>` : ''}${grade.overall ? `<p>${esc(grade.overall)}</p>` : ''}<details class="source-more interview-review-more"${open ? ' open' : ''}><summary>${ICON.chevron}<span>Feedback by criterion · ${grade.criteria.length}</span></summary><ul class="interview-verdicts">${grade.criteria.map((v) => `<li class="${esc(v.verdict)}"><span class="chip">${VERDICT[v.verdict] || esc(v.verdict)}</span><span><b>${esc(v.text)}</b><span class="hint">${esc(v.why)}</span></span></li>`).join('')}</ul></details></div>`
    : session.grading === attempt.id ? meterBar('interview-grade-meter', 'Quick review', session.gradeProgress?.attempt === attempt.id ? session.gradeProgress : { fraction: 0 }) : gradeError ? `<p class="hint">${esc(gradeError)}</p>` : '';
  keepOpen(box, `<div class="interview-result"><div class="interview-toolbar"><h3>Your result</h3><span class="hint">${new Date(attempt.at).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</span></div>
    ${cases.length ? `<div class="interview-result-summary"><span class="chip interview-tally good">${ICON.check}${passed} passed</span>${failed ? `<span class="chip interview-tally bad">${ICON.close}${failed} failed</span>` : ''}<span class="hint">${esc(groups)}</span></div>
    <div class="interview-cases" role="tablist" aria-label="Checked cases">${cases.map((x) => { const on = x.id === chosen.id; return `<button class="pick-chip ${x.pass ? 'pass' : 'fail'}${on ? ' on' : ''}" role="tab" id="interview-case-${esc(x.id)}" aria-controls="interview-case" aria-selected="${on}" tabindex="${on ? 0 : -1}" aria-label="${esc(x.name)}, ${x.pass ? 'passed' : 'failed'}" data-interview="result-case" data-check="${esc(x.id)}"><span class="interview-mark">${x.pass ? ICON.check : ICON.close}</span><span>${esc(x.name)}</span></button>`; }).join('')}</div>
    <div class="interview-case" id="interview-case" role="tabpanel" aria-labelledby="interview-case-${esc(chosen.id)}">${interviewCaseDetail(chosen)}</div>` : ''}
    ${review}
    <p class="hint">Felix’s full review follows in the chat. You can keep working or start the next problem.</p></div>`);
}

function chooseInterviewCase(session, check, focus = false) {
  const attempt = interviewResultAttempt(session);
  const box = $('#interview-results');
  if (!attempt || !box) return;
  const cases = interviews.resultCases(attempt.checks);
  const chosen = cases.find((x) => x.id === check);
  if (!chosen) return;
  session.resultCase = { attempt: attempt.id, check: chosen.id };
  let picked = null;
  box.querySelectorAll('[data-interview="result-case"]').forEach((b) => {
    const on = b.dataset.check === chosen.id;
    b.classList.toggle('on', on);
    b.setAttribute('aria-selected', String(on));
    b.tabIndex = on ? 0 : -1;
    if (on) picked = b;
  });
  const panel = $('#interview-case');
  panel.innerHTML = interviewCaseDetail(chosen);
  panel.setAttribute('aria-labelledby', `interview-case-${chosen.id}`);
  if (picked) {
    const strip = picked.parentElement;
    if (picked.offsetLeft < strip.scrollLeft) strip.scrollLeft = picked.offsetLeft;
    else if (picked.offsetLeft + picked.offsetWidth > strip.scrollLeft + strip.clientWidth) strip.scrollLeft = picked.offsetLeft + picked.offsetWidth - strip.clientWidth;
    if (focus) picked.focus({ preventScroll: true });
  }
}

const SPLIT_KEY = 'leo-interview-split';
const SPLIT_DEFAULT = 45;
const SPLIT_LEAST = 25;
const SPLIT_MOST = 70;

function interviewSplit() {
  try {
    const kept = Number(localStorage.getItem(SPLIT_KEY));
    return kept >= SPLIT_LEAST && kept <= SPLIT_MOST ? kept : SPLIT_DEFAULT;
  } catch (e) { return SPLIT_DEFAULT; }
}

function setInterviewSplit(value, keep = true) {
  const split = Math.round(Math.max(SPLIT_LEAST, Math.min(SPLIT_MOST, value)) * 10) / 10;
  $('.interview-solve')?.style.setProperty('--split', `${split}%`);
  $('.interview-split')?.setAttribute('aria-valuenow', String(Math.round(split)));
  if (keep) { try { localStorage.setItem(SPLIT_KEY, String(split)); } catch (e) {} }
  return split;
}

function wireInterviewSplit() {
  const handle = $('.interview-split');
  if (!handle) return;
  let drag = null;
  handle.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    drag = { pointer: e.pointerId, box: handle.parentElement.getBoundingClientRect() };
    handle.setPointerCapture(e.pointerId);
    document.body.classList.add('interview-resizing');
  });
  handle.addEventListener('pointermove', (e) => {
    if (!drag || e.pointerId !== drag.pointer) return;
    setInterviewSplit((e.clientX - drag.box.left - 8) / drag.box.width * 100, false);
  });
  const stop = (e) => {
    if (!drag || e.pointerId !== drag.pointer) return;
    drag = null;
    document.body.classList.remove('interview-resizing');
    setInterviewSplit(parseFloat(getComputedStyle(handle.parentElement).getPropertyValue('--split')) || SPLIT_DEFAULT);
  };
  handle.addEventListener('pointerup', stop);
  handle.addEventListener('pointercancel', stop);
  handle.addEventListener('dblclick', () => setInterviewSplit(SPLIT_DEFAULT));
  handle.addEventListener('keydown', (e) => {
    const now = interviewSplit();
    const step = e.shiftKey ? 10 : 2;
    const next = e.key === 'ArrowLeft' ? now - step : e.key === 'ArrowRight' ? now + step : e.key === 'Home' ? SPLIT_DEFAULT : e.key === 'End' ? SPLIT_MOST : null;
    if (next === null) return;
    e.preventDefault();
    e.stopPropagation();
    setInterviewSplit(next);
  });
}

function wireInterviewResults(session) {
  $('#interview-panes')?.addEventListener('keydown', (e) => {
    const tab = e.target.closest('[data-interview="pane"]');
    if (!tab || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    const tabs = [...$('#interview-panes').querySelectorAll('[data-interview="pane"]')];
    const at = tabs.indexOf(tab);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : (at + (e.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
    e.preventDefault();
    showInterviewPane(session, tabs[next].dataset.pane);
    $('#interview-panes').querySelector(`[data-pane="${tabs[next].dataset.pane}"]`)?.focus({ preventScroll: true });
  });
  const box = $('#interview-results');
  if (!box) return;
  box.addEventListener('keydown', (e) => {
    const chip = e.target.closest('[data-interview="result-case"]');
    if (!chip || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    const chips = [...box.querySelectorAll('[data-interview="result-case"]')];
    const at = chips.indexOf(chip);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? chips.length - 1 : (at + (e.key === 'ArrowRight' ? 1 : -1) + chips.length) % chips.length;
    e.preventDefault();
    chooseInterviewCase(session, chips[next].dataset.check, true);
  });
  box.addEventListener('toggle', (e) => {
    if (!e.target.classList?.contains('interview-review-more')) return;
    const attempt = interviewResultAttempt(session);
    if (attempt) session.reviewOpen = { attempt: attempt.id, open: e.target.open };
  }, true);
}

function interviewForm(track, id = felix.newId(), item = null) {
  const f = item ? item.framework : interviews.framework(track);
  const coding = f.track === 'coding';
  sheet(`<h2>${item ? 'Practice preferences' : `Start ${esc(interviews.preset(track).label.toLowerCase())} practice`}</h2><p class="hint">${item ? 'Keep your workspace and change what Felix focuses on next.' : 'One step to your first challenge. Felix researches your target and prepares an exercise.'}</p>
    <label class="interview-label" for="interview-company">Company <span class="hint">Optional · leave blank if you’re just practising</span></label><div class="field"><input id="interview-company" maxlength="120" value="${esc(f.company)}" placeholder="e.g. Pinterest"></div>
    <label class="interview-label" for="interview-role">Role & level <span class="hint">Optional</span></label><div class="field"><input id="interview-role" maxlength="120" value="${esc(f.role)}" placeholder="e.g. Software engineer intern"></div>
    <label class="interview-label" for="interview-language">${coding ? 'Coding language' : 'Specialism'} <span class="hint">${coding ? 'The starting function will use this language' : 'Optional'}</span></label>${coding ? `<div class="field"><div class="select-control"><select id="interview-language">${['Python', 'C++', 'Rust', 'Java', 'SQL', 'TypeScript', 'JavaScript', 'Go'].map(language => `<option${language === (f.language || 'Python') ? ' selected' : ''}>${language}</option>`).join('')}</select>${ICON.chevron}</div></div>` : `<div class="field"><input id="interview-language" maxlength="80" value="${esc(f.language)}" placeholder="Your area of focus"></div>`}
    <p class="interview-label">How would you like to practise?</p><div class="pick-chips" role="group" aria-label="Interview style"><button class="pick-chip${f.style === 'coach' ? ' on' : ''}" data-interview="form-style" data-style="coach" aria-pressed="${f.style === 'coach'}">Coach</button><button class="pick-chip${f.style === 'mock' ? ' on' : ''}" data-interview="form-style" data-style="mock" aria-pressed="${f.style === 'mock'}">Mock interview</button></div><p class="hint">Coach helps when you ask. Mock saves feedback for your debrief.</p>
    <label class="interview-label" for="interview-wants">What would you like to practise? <span class="hint">Optional</span></label><textarea class="set-input" id="interview-wants" maxlength="8000" rows="4" placeholder="Describe it in your own words, e.g. I interview at Google for a SWE intern role in three weeks. Start with medium graph and array problems, ask me to explain complexity, and push me on edge cases…">${esc(f.wants)}</textarea>
    <details class="interview-details source-more"><summary>${ICON.chevron}<span>More preferences</span></summary><label class="interview-label" for="interview-title">Practice title</label><div class="field"><input id="interview-title" maxlength="120" value="${esc(f.title)}"></div><label class="interview-label" for="interview-date">Interview date (optional)</label><div class="field"><input id="interview-date" type="date" value="${esc(f.date)}"></div><label class="interview-label" for="interview-skills">Skills to assess · one per line, up to 12</label><textarea class="set-input" id="interview-skills" rows="4" maxlength="1000">${esc(f.skills.join('\n'))}</textarea></details>
    <p class="set-note warn" id="interview-form-error" hidden></p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn primary" id="interview-form-save" data-interview="save-framework" data-id="${esc(id)}" data-track="${esc(f.track)}" data-revision="${item ? item.revision : 0}" data-minutes="${f.weekly_minutes}" data-style="${esc(f.style)}">${item ? 'Save preferences' : 'Start practice'}</button></div>`);
  $('#interview-company').focus();

}

async function saveInterviewForm(el) {
  if (el.disabled) return;
  el.disabled = true;
  const framework = { title: $('#interview-title').value.trim(), track: el.dataset.track, company: $('#interview-company').value.trim(), role: $('#interview-role').value.trim(), language: $('#interview-language').value.trim(), date: $('#interview-date').value, weekly_minutes: Number(el.dataset.minutes) || 120, skills: $('#interview-skills').value.split('\n').map((s) => s.trim()).filter(Boolean), wants: $('#interview-wants').value, style: el.dataset.style };
  try {
    const saved = await api(`/api/interviews/${enc(el.dataset.id)}`, { method: 'PUT', body: { action: 'framework', revision: Number(el.dataset.revision), framework } });
    if (Number(el.dataset.revision) === 0) interviewStart = { id: el.dataset.id };
    const coding = saved?.challenge?.coding;
    if (framework.track === 'coding' && coding && coding.language.trim().toLowerCase() !== framework.language.toLowerCase()) await switchInterviewLanguage(el, saved, framework.language);
    closeSheet();
    go(`#/interviews/${enc(el.dataset.id)}`);
  } catch (e) {
    const error = $('#interview-form-error');
    if (error) { error.hidden = false; error.textContent = e.message; }
  } finally { el.disabled = false; }
}



async function switchInterviewLanguage(el, item, language) {
  const challenge = item.challenge.id;
  const label = `Rewriting the starting code in ${language}`;
  $('#interview-form-error')?.insertAdjacentHTML('afterend', meterBar('interview-language-meter', label));
  el.textContent = 'Switching language…';
  const stop = watchProgress(`language:${item.id}:${challenge}`, (view) => setMeter('interview-language-meter', view));
  let done;
  try {
    done = await api(`/api/interviews/${enc(item.id)}/language`, { method: 'POST', body: { challenge, language } });
    await finishMeter('interview-language-meter');
  } catch (e) {
    toast(e.message, { bad: true });
    return;
  } finally {
    stop();
    document.getElementById('interview-language-meter')?.remove();
  }
  if (!done?.changed) return;
  interviews.clearDraft(localStorage, item.id, challenge);
  const now = done.item.challenge.coding.starter;
  const back = done.previous;
  toast(done.replaced ? `This problem is now in ${language}. Your ${back.language} code is kept in Undo.` : `This problem is now in ${language}.`, {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/interviews/${enc(item.id)}/language`, { method: 'POST', body: { challenge, language: back.language, restore: { from: now, starter: back.starter, answer: back.answer } } });
        interviews.clearDraft(localStorage, item.id, challenge);
        if (location.hash === `#/interviews/${enc(item.id)}`) go(location.hash);
      } catch (e) {
        toast(e.message, { bad: true });
      }
    },
  });
}

async function interviewAction(el) {
  const action = el.dataset.interview;
  const session = interviewPage;
  if (action === 'open') return go(`#/interviews/${enc(el.dataset.id)}`);
  if (action === 'create') return interviewForm(el.dataset.track, el.dataset.id || felix.newId());
  if (action === 'save-framework') return saveInterviewForm(el);
  if (action === 'form-style') {
    const group = el.parentElement;
    group.querySelectorAll('.pick-chip').forEach((button) => { button.classList.toggle('on', button === el); button.setAttribute('aria-pressed', String(button === el)); });
    $('#interview-form-save').dataset.style = el.dataset.style;
    return;
  }
  if (action === 'delete') {
    const id = el.dataset.id;
    sheet(`<h2>Delete this interview practice?</h2><p>Its workspace and attempts will be removed. The Felix conversation stays in your chats.</p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn danger" data-interview="confirm-delete" data-id="${esc(id)}">Delete practice</button></div>`);
    return;
  }
  if (action === 'confirm-delete') {
    await api(`/api/interviews/${enc(el.dataset.id)}`, { method: 'DELETE' });
    closeSheet();
    return showInterviews();
  }
  if (!session || !interviewHere(session)) return;
  if (action === 'configure') { await saveInterviewWorkspace(session); return interviewForm(session.item.framework.track, session.item.id, session.item); }
  if (action === 'chat') return chat.openInterview(session.item.id, () => interviewHere(session));
  if (action === 'next') return prepareInterview(session);
  if (action === 'problem') return selectInterviewProblem(session, el.dataset.id);
  if (action === 'topic-menu') return interviewTopicMenu();
  if (action === 'topic') return pickInterviewTopic(session, el);
  if (action === 'sort-topics') { interviewTopicMenu(false); return talkInterview(session, 'Sort my problems into topics.', [], 'Give every problem in this practice that has no topics 1-3 topics with interview_topics, all in one call, reusing the spelling of topics_in_use. Every one of them must get at least one topic: choose the closest fit when unsure, never leave one out. Change nothing else.'); }
  if (action === 'hint') return talkInterview(session, 'Give me a small hint without the solution.', [], 'Give me one small hint for the current interview challenge, without revealing the solution or assessing my draft.');
  if (action === 'submit') return submitInterview(session);
  if (action === 'run') return runInterviewExamples(session);
  if (action === 'coverage') return checkInterviewCoverage(session);
  if (action === 'prepare-key') {
    const c = session.item.challenge;
    if (!c) return;
    const view = await api(`/api/interviews/${enc(session.item.id)}/key`, { method: 'POST', body: { challenge: c.id } });
    session.key = { challenge: c.id, ...view };
    drawInterviewChecks(session);
    return loadInterviewKey(session, true);
  }
  if (action === 'ask-choice') {
    session.draft.values = { ...(session.draft.values || {}), [el.dataset.askId]: el.dataset.option };
    interviewChanged(session);
    drawInterviewChecks(session);
    return;
  }
  if (action === 'result-case') return chooseInterviewCase(session, el.dataset.check);
  if (action === 'pane') return showInterviewPane(session, el.dataset.pane);
  if (action === 'check-wrong') {
    const attempt = session.item.attempts.find((a) => a.id === session.latest);
    const check = attempt?.checks?.find((x) => x.id === el.dataset.check);
    if (!check) return;
    return talkInterview(session, `I think this automatic check might be wrong: “${check.label}”${check.expected ? ` expects ${check.expected}` : ''}${check.given ? `, and I gave ${check.given}` : ''}. Can you check it?`, [], `The user disputes one automatic check from their submitted attempt ${attempt.id}: check ${check.id} (${check.kind}) “${check.label}”, expected ${check.expected || 'not given'}, the user gave ${check.given || 'nothing'}. Read the attempt with interview_attempt, work that check out carefully yourself and say plainly whether the expected result was right. The check was written by an AI in advance and can be wrong. Do not reveal the full solution unless asked.`);
  }
  if (action === 'review-attempt') {
    const attempt = session.item.attempts.find((a) => a.id === el.dataset.id);
    if (attempt) return talkInterview(session, `Review my “${attempt.challenge.title}” attempt from ${new Date(attempt.at).toLocaleDateString()}. What did I do well, and what should I practise next?`, attempt.workspace.board, `Review submitted attempt ${attempt.id} using interview_progress and interview_feedback. Explain strengths, gaps and the next practice step without giving a full solution.`);
  }
  if (action === 'retry') return retryInterviewSave(session);
  if (action === 'tab') {
    session.tab = el.dataset.tab;
    drawInterviewContext(session);
    const coding = session.item.framework.track === 'coding';
    $('#interview-editor').hidden = session.tab !== (coding ? 'code' : 'write');
    if ($('#interview-reasoning-editor')) $('#interview-reasoning-editor').hidden = session.tab !== 'write';
    $('#interview-whiteboard').hidden = session.tab !== 'board';
    app.querySelectorAll('[data-interview="tab"]').forEach((b) => { b.classList.toggle('on', b.dataset.tab === session.tab); b.setAttribute('aria-selected', String(b.dataset.tab === session.tab)); });
    if (session.tab === 'board') drawInterviewBoard(session);
    if (session.tab !== 'board') $(coding && session.tab === 'write' ? '#interview-reasoning' : '#interview-answer').focus();
    return;
  }
  if (action === 'reset') {
    const tab = session.tab;
    const name = tab === 'board' ? 'whiteboard' : tab === 'code' ? 'code' : 'answer';
    sheet(`<h2>Reset your ${name}?</h2><p>${tab === 'code' ? 'This code will return to its starting function.' : tab === 'board' ? 'This problem’s whiteboard will be cleared.' : 'This problem’s written answer will be cleared.'}</p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn danger" data-interview="confirm-reset" data-tab="${tab}" data-challenge="${esc(session.item.challenge.id)}">Reset</button></div>`);
    return;
  }
  if (action === 'confirm-reset') {
    closeSheet();
    if (session.switching || el.dataset.challenge !== session.item.challenge?.id) { toast('The active problem changed. Choose Reset again on the problem you want to reset.', { bad: true }); return; }
    const tab = el.dataset.tab;
    if (tab === 'board') { rememberInterviewBoard(session, session.draft.board, true); session.boardViews.delete(session.item.challenge.id); session.draft.board = []; session.selection = []; session.stroke = null; session.gesture = null; drawInterviewBoard(session); }
    else if (tab === 'code' || session.item.framework.track !== 'coding') { session.draft.answer = tab === 'code' ? session.item.challenge.coding?.starter || '' : ''; $('#interview-answer').value = session.draft.answer; drawInterviewLines(session); }
    else { session.draft.reasoning = ''; $('#interview-reasoning').value = ''; }
    interviewChanged(session); return;
  }
  if (action === 'tool') { setInterviewTool(session, el.dataset.tool); $('#interview-board')?.focus({ preventScroll: true }); return; }
  if (action === 'delete-board') { deleteInterviewSelection(session); $('#interview-board')?.focus({ preventScroll: true }); return; }
  if (action === 'redo-board') { stepInterviewBoard(session, false); return; }
  if (action === 'zoom-board') { const v = interviewBoardView(session), w = Math.max(100, Math.min(interviews.BOARD_LIMIT * 2, v.w * (Number(el.dataset.step) > 0 ? .8 : 1.25))), h = w * 7 / 10; setInterviewBoardView(session, { x: v.x + (v.w - w) / 2, y: v.y + (v.h - h) / 2, w, h }); return; }
  if (action === 'fit-board') { setInterviewBoardView(session, interviews.fitBoard(session.draft.board, $('#interview-board').clientWidth * 1.5)); return; }
  if (action === 'undo-board') { stepInterviewBoard(session, true); return; }
}

document.addEventListener('click', (e) => {
  const button = e.target.closest('[data-interview]');
  if (!button) return;
  e.preventDefault();
  interviewAction(button).catch(fail);
});
