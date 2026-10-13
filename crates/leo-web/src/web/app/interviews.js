const interviews = window.leoInterview;
let interviewPage = null;
let interviewStart = null;
let interviewCodeLoad = null;
const MOST_BOARD_UNDO = 20;

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
  const session = { item, mine, draft: interviews.workspace(item.workspace), generation: 0, saved: 0, timer: 0, queue: Promise.resolve(), error: '', tab: item.framework.track === 'coding' ? 'code' : 'write', tool: 'pen', stroke: null, submitting: false, calendar: [] };
  interviewPage = session;
  const local = item.challenge && interviews.readDraft(localStorage, id, item.challenge.id);
  if (local && JSON.stringify(local) !== JSON.stringify(interviews.workspace(item.workspace))) {
    session.draft = local;
    session.generation = 1;
    session.error = 'An unsaved draft was recovered on this device. Review it, then save it when you are ready.';
  }
  drawInterview(session);
  if (interviewStart?.id === id) {
    interviewStart = null;
    prepareInterview(session).catch(fail);
  }
  api('/api/calendar').then((calendar) => {
    if (interviewPage !== session || mine !== seq) return;
    session.calendar = calendar.events || [];
    drawInterviewSchedule(session);
  }).catch(() => {});
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
    <div class="interview-solve"><section id="interview-challenge" class="set-card"></section>
    ${item.challenge ? `<section class="set-card interview-workspace"><div class="interview-toolbar"><div class="library-views" role="tablist" aria-label="Your answer workspace">${tabs.map(([id, label]) => `<button class="btn sm plain${session.tab === id ? ' on' : ''}" role="tab" aria-selected="${session.tab === id}" data-interview="tab" data-tab="${id}">${label}</button>`).join('')}</div><span id="interview-save" class="hint" role="status"></span></div>
    <div id="interview-editor"${session.tab !== (coding ? 'code' : 'write') ? ' hidden' : ''}><div class="interview-toolbar"><label class="interview-label" for="interview-answer">${coding ? esc(item.challenge.coding?.language || item.framework.language || 'Code') : 'Your answer'}</label></div><div class="interview-code-shell${coding ? ' numbered' : ''}">${coding ? '<pre id="interview-lines" aria-hidden="true"></pre>' : ''}<div class="interview-code-pane">${coding ? '<pre id="interview-colors" aria-hidden="true"></pre>' : ''}<textarea id="interview-answer" class="set-input${coding ? ' interview-code' : ''}" maxlength="32000" spellcheck="${!coding}" autocapitalize="off" autocomplete="off" aria-label="${coding ? 'Code editor' : 'Your answer'}" placeholder="${coding ? 'Write your implementation here…' : 'Clarify the problem, outline an approach, and work through it…'}">${esc(session.draft.answer)}</textarea></div></div></div>
    ${coding ? `<div id="interview-reasoning-editor"${session.tab !== 'write' ? ' hidden' : ''}><label class="interview-label" for="interview-reasoning">Your approach</label><p class="hint">Explain your idea, complexity, and edge cases. Your code stays in the Code tab.</p><textarea id="interview-reasoning" class="set-input" maxlength="8000" placeholder="What’s your approach?">${esc(session.draft.reasoning)}</textarea></div>` : ''}
    <div id="interview-whiteboard"${session.tab !== 'board' ? ' hidden' : ''}><div class="interview-toolbar"><div class="pick-chips" aria-label="Drawing tools">${[['select', 'Select'], ['pan', 'Pan'], ['pen', 'Pen'], ['box', 'Box'], ['arrow', 'Arrow'], ['text', 'Text']].map(([id, label]) => `<button class="pick-chip${session.tool === id ? ' on' : ''}" aria-pressed="${session.tool === id}" data-interview="tool" data-tool="${id}">${label}</button>`).join('')}</div><div class="buttons"><button class="btn sm" data-interview="undo-board">Undo</button><button class="btn sm" data-interview="zoom-board" data-step="-1" aria-label="Zoom out">${ICON.minus}</button><button class="btn sm" data-interview="zoom-board" data-step="1" aria-label="Zoom in">${ICON.plus}</button><button class="btn sm" data-interview="fit-board">${ICON.fit}<span>Fit</span></button></div></div><label class="interview-label" for="interview-board-label">Board label</label><div class="interview-board-label-row"><input class="set-input" id="interview-board-label" value="${esc(session.label || '')}" maxlength="300" placeholder="Type a label and press Enter"><button class="btn sm" data-interview="label-board">Add label</button></div><div id="interview-board" tabindex="0" role="group" aria-label="Whiteboard. Drag labels to move them. Use Pan or hold Space to drag the canvas."></div><p class="hint">Drag labels to move them. Pan or hold Space to move the canvas; scroll to explore. Felix reviews the whole board when you submit.</p></div>
    <div id="interview-save-error" class="set-note warn" hidden></div><div class="interview-toolbar interview-submit"><p class="hint">${coding ? 'Felix reviews your code and reasoning; examples are not executed.' : 'Work at your pace. Submit when you’re ready for feedback.'}</p><div class="buttons"><button class="btn sm" data-interview="reset">Reset</button><button class="btn sm" data-interview="hint">Hint</button><button class="btn sm primary" data-interview="submit">Submit for review</button></div></div></section>` : ''}</div>
    <details class="interview-details source-more"${item.attempts.length ? ' open' : ''}><summary>${ICON.chevron}<span>Progress & previous attempts</span></summary><section id="interview-progress"></section><section id="interview-attempts"></section></details><details class="interview-details source-more"${item.schedule.length ? ' open' : ''}><summary>${ICON.chevron}<span>Practice schedule</span></summary><section id="interview-schedule"></section></details></div>`;
  drawInterviewContext(session);
  drawInterviewProblems(session);
  drawInterviewChallenge(session);
  drawInterviewBoard(session);
  drawInterviewStatus(session);
  drawInterviewProgress(session);
  drawInterviewSchedule(session);
  const answer = $('#interview-answer');
  if (answer) {
    answer.addEventListener('input', (e) => { session.draft.answer = e.target.value; interviewChanged(session); drawInterviewLines(session); });
    answer.addEventListener('scroll', syncInterviewCodeScroll);
    answer.addEventListener('keydown', (e) => {
      if (!coding || answer.readOnly || e.isComposing || !['Tab', 'Enter', 'Backspace', 'Delete'].includes(e.key) || e.ctrlKey || e.metaKey || e.altKey) return;
      const edit = interviews.codeEdit(answer.value, answer.selectionStart, answer.selectionEnd, e.key, e.shiftKey, session.item.challenge?.coding?.language || session.item.framework.language);
      if (!edit) { if (e.key === 'Tab') e.preventDefault(); return; }
      e.preventDefault();
      answer.setSelectionRange(0, answer.value.length);
      if (!document.execCommand('insertText', false, edit.value)) answer.setRangeText(edit.value, 0, answer.value.length, 'preserve');
      answer.setSelectionRange(edit.from, edit.to);
      if (session.draft.answer !== answer.value) answer.dispatchEvent(new Event('input', { bubbles: true }));
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
    $('#interview-board-label').addEventListener('input', (e) => { session.label = e.target.value; });
    $('#interview-board-label').addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); addInterviewLabel(session); } });
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
  box.innerHTML = `${problems.slice(0, interviews.MOST_PROBLEMS).map((p, i) => `<button class="pick-chip${p.challenge.id === item.challenge?.id ? ' on' : ''}" data-interview="problem" data-id="${esc(p.challenge.id)}" aria-pressed="${p.challenge.id === item.challenge?.id}" title="${esc(p.challenge.title)}"${busy ? ' disabled' : ''}>Problem ${i + 1}${item.attempts.some((a) => a.challenge.id === p.challenge.id) ? ' · Submitted' : ''}</button>`).join('')}<button class="btn sm plain" data-interview="next"${busy || problems.length >= interviews.MOST_PROBLEMS ? ' disabled' : ''}>${session.preparing ? 'Preparing…' : '+ Problem'}</button>`;
}

function useInterviewProblem(session, item) {
  if (session.item.challenge?.id !== item.challenge?.id) { session.boardUndo = []; session.selected = -1; session.gesture = null; session.stroke = null; }
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
  const fields = [...app.querySelectorAll('#interview-answer, #interview-reasoning, #interview-board-label')];
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
  $('#interview-challenge').innerHTML = c ? `<div class="interview-toolbar"><h2>${esc(c.title)}</h2><div class="buttons">${coding ? `<span class="chip">${esc(coding.difficulty)}</span>` : ''}<span class="chip">${c.minutes} min</span></div></div><p class="hint">${esc(c.origin === 'generated' ? 'Original practice' : c.origin === 'official' ? 'Official preparation' : 'Candidate report · unverified')}</p><div class="prose">${md.render(c.question, { interactive: false })}</div>${coding ? `<h3>Examples</h3>${coding.examples.map((e, i) => `<div class="interview-example"><b>Example ${i + 1}</b><pre>Input: ${esc(e.input)}\nOutput: ${esc(e.output)}</pre>${e.explanation ? `<p class="hint">${esc(e.explanation)}</p>` : ''}</div>`).join('')}<h3>Constraints</h3><ul class="interview-constraints">${coding.constraints.map((s) => `<li>${esc(s)}</li>`).join('')}</ul>` : ''}${c.sources.length ? `<details class="interview-research source-more"><summary>${ICON.chevron}<span>Research & sources</span></summary><p class="hint">Reported questions are unverified. This exercise is labelled separately above.</p><div class="interview-sources">${c.sources.map((s) => `<a class="chip" href="${esc(s.url)}" target="_blank" rel="noopener noreferrer">${esc(s.title)} · ${esc(s.kind)}</a>`).join('')}</div></details>` : ''}<button class="btn sm" data-interview="next"${busy ? ' disabled' : ''}>${busy ? 'Preparing…' : 'Next challenge'}</button>` : `<div class="interview-welcome"><p class="eyebrow">${session.item.framework.company ? 'Company preparation' : 'General practice'}</p><h2>${busy ? 'Preparing your first challenge…' : 'Ready when you are'}</h2><p class="hint" role="status">${busy ? session.waiting ? 'Felix is finishing the previous reply. Your practice will start automatically after it.' : session.item.framework.company ? 'Felix is researching the company and role, then preparing an exercise. You can follow along in chat.' : 'Felix is choosing an exercise for your goals. You can follow along in chat.' : 'Start once. Felix will research your company if you have one, then put a challenge and workspace here.'}</p><div class="buttons"><button class="btn sm primary" data-interview="next"${busy ? ' disabled' : ''}>${busy ? 'Preparing…' : 'Start practice'}</button></div></div>`;
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
      const fields = [...app.querySelectorAll('#interview-answer, #interview-reasoning, #interview-board-label')];
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
      drawInterviewSchedule(session);
    }
  });
}

function drawInterviewProgress(session) {
  if (!interviewHere(session)) return;
  const attempts = session.item.attempts;
  const assessed = attempts.filter((a) => a.feedback).length;
  if (attempts.length) $('#interview-progress').closest('details').open = true;
  const scores = interviews.progress(attempts);
  $('#interview-progress').innerHTML = `<h2>What you’re learning</h2><div class="set-card"><p class="hint">${assessed ? `${assessed} reviewed attempt${assessed === 1 ? '' : 's'}. AI feedback is provisional; averages use your last five observations per skill.` : 'Complete an attempt to start seeing strengths and areas to practise. Felix won’t guess from an empty history.'}</p>${scores.map((s) => `<div class="set-row"><div class="grow"><b>${esc(s.skill)}</b><span class="hint">${s.n} observation${s.n === 1 ? '' : 's'}${s.n < 3 ? ' · early signal' : ''}</span></div><span class="chip">${s.value.toFixed(1)} / 4</span></div>`).join('')}</div>`;
  $('#interview-attempts').innerHTML = attempts.length ? `<h2>Your attempts</h2><div class="set-card">${[...attempts].reverse().map((a) => `<details class="interview-attempt source-more"><summary>${ICON.chevron}<span class="grow"><b>${esc(a.challenge.title)}</b><span class="hint">${new Date(a.at).toLocaleDateString()} · ${a.feedback ? 'Reviewed' : 'Awaiting feedback'}</span></span></summary>${a.feedback ? `<div class="prose">${md.render(a.feedback.summary, { interactive: false })}</div>${a.feedback.scores.map((s) => `<p><b>${esc(s.skill)} · ${s.value}/4</b><br>${esc(s.evidence)}<br><span class="hint">Next: ${esc(s.next)}</span></p>`).join('')}` : `<button class="btn sm" data-interview="review-attempt" data-id="${esc(a.id)}">Ask Felix to review</button>`}<details class="source-more interview-work"><summary>${ICON.chevron}<span>Your submitted work</span></summary><pre class="interview-submitted">${esc(a.workspace.answer)}</pre>${a.workspace.reasoning ? `<h3>Approach</h3><div class="prose">${md.render(a.workspace.reasoning, { interactive: false })}</div>` : ''}${a.workspace.board.length ? `<div class="interview-board-snapshot">${interviews.boardSvg(a.workspace.board)}</div>` : ''}</details></details>`).join('')}</div>` : '';
}

function drawInterviewSchedule(session) {
  if (!interviewHere(session)) return;
  const rows = interviews.agenda(session.item.schedule, session.calendar);
  let day = '';
  $('#interview-schedule').innerHTML = `<div class="interview-toolbar"><h2>Your practice schedule</h2><div class="buttons"><button class="btn sm" data-interview="add-session">Add session</button><button class="btn sm" data-interview="plan">Plan with Felix</button></div></div><p class="hint">${session.item.framework.weekly_minutes} minutes per week${session.item.framework.date ? ` · Interview ${esc(session.item.framework.date)}` : ''}. Connected calendar events appear here too.</p><div class="set-card">${rows.length ? rows.map((r) => {
    const heading = day !== r.day ? `<h3 class="interview-day">${esc(new Date(`${r.day}T12:00:00`).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' }))}</h3>` : '';
    day = r.day;
    return `${heading}<div class="set-row interview-agenda">${r.kind === 'practice' ? `<input type="checkbox" data-interview="complete" data-id="${esc(r.id)}" aria-label="Complete ${esc(r.title)}"${r.done ? ' checked' : ''}>` : `<span class="cal-icon">${ICON.calendar}</span>`}<div class="grow${r.done ? ' interview-complete' : ''}"><b>${esc(r.title)}</b><span class="hint">${esc(r.at || 'Any time')}${r.kind === 'practice' ? ` · ${r.minutes} min · ${esc(r.skill)}` : ' · Calendar event'}</span></div>${r.kind === 'practice' ? `<button class="icon-btn" data-interview="edit-session" data-id="${esc(r.id)}" aria-label="Edit ${esc(r.title)}">${ICON.more}</button>` : ''}</div>`;
  }).join('') : '<p class="hint">A realistic plan, a little at a time. Ask Felix to focus on the skills you want to strengthen.</p>'}</div>`;
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

function rememberInterviewBoard(session, board = session.draft.board, view = false) {
  session.boardUndo ||= [];
  session.boardUndo.push({ board: board.map(s => ({ ...s, points: s.points.map(p => [...p]) })), view: view ? { ...interviewBoardView(session) } : null });
  if (session.boardUndo.length > MOST_BOARD_UNDO) session.boardUndo.shift();
}

function drawInterviewBoard(session) {
  if (!interviewHere(session)) return;
  const board = $('#interview-board');
  if (board) {
    board.dataset.tool = session.tool;
    board.innerHTML = interviews.boardSvg(session.stroke ? [...session.draft.board, session.stroke] : session.draft.board, { view: interviewBoardView(session), interactive: true, selected: session.selected ?? -1 });
    const selected = board.querySelector(`[data-board-shape="${session.selected}"]`);
    const outline = selected?.querySelector('.board-selection');
    if (outline) { const bounds = selected.firstElementChild.getBBox(); outline.setAttribute('x', bounds.x - 6); outline.setAttribute('y', bounds.y - 6); outline.setAttribute('width', bounds.width + 12); outline.setAttribute('height', bounds.height + 12); }
  }
}

function queueInterviewBoard(session) {
  if (session.boardFrame) return;
  session.boardFrame = requestAnimationFrame(() => { session.boardFrame = 0; drawInterviewBoard(session); });
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
    if (gesture.kind === 'move') {
      if (cancel) session.draft.board[gesture.index] = gesture.shape;
      else if (JSON.stringify(session.draft.board[gesture.index]) !== JSON.stringify(gesture.shape)) {
        const before = [...session.draft.board]; before[gesture.index] = gesture.shape;
        rememberInterviewBoard(session, before); interviewChanged(session);
      }
    } else if (session.stroke && !cancel) {
      const next = [...session.draft.board, session.stroke];
      if (interviews.validBoard(next)) { rememberInterviewBoard(session); session.draft.board = next; interviewChanged(session); }
      else toast('Your board is full. Undo some strokes to make room.', { bad: true });
    }
    session.stroke = null;
    session.gesture = null;
    board.classList.remove('panning');
    if (board.hasPointerCapture(gesture.pointer)) board.releasePointerCapture(gesture.pointer);
    drawInterviewBoard(session);
  };
  board.addEventListener('pointerdown', (e) => {
    if (session.switching || session.gesture || !session.item.challenge || ![0, 1].includes(e.button)) return;
    e.preventDefault();
    const first = locate(e);
    const target = e.target.closest('[data-board-shape]');
    const index = target ? Number(target.dataset.boardShape) : -1;
    const shape = session.draft.board[index];
    if (session.tool === 'pan' || session.panHeld || e.button === 1) {
      session.gesture = { kind: 'pan', pointer: e.pointerId, x: e.clientX, y: e.clientY, view: { ...interviewBoardView(session) } };
      board.classList.add('panning');
    } else if (shape && (session.tool === 'select' || shape.kind === 'text')) {
      session.selected = index;
      session.gesture = { kind: 'move', pointer: e.pointerId, first, index, shape };
    } else if (session.tool === 'select') { session.selected = -1; drawInterviewBoard(session); return; }
    else if (session.tool === 'text') { addInterviewLabel(session, first); return; }
    else {
      if (session.draft.board.length >= interviews.MOST_SHAPES) { toast('Your board is full. Undo a shape first.', { bad: true }); return; }
      session.selected = -1;
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
    if (gesture.kind === 'move') session.draft.board[gesture.index] = interviews.moveShape(gesture.shape, p[0] - gesture.first[0], p[1] - gesture.first[1]);
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
    const v = interviewBoardView(session), rect = board.getBoundingClientRect();
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? rect.height : 1;
    if (e.ctrlKey || e.metaKey) {
      const p = locate(e), w = Math.max(100, Math.min(interviews.BOARD_LIMIT * 2, v.w * Math.exp(Math.max(-1, Math.min(1, e.deltaY * unit / 500)))));
      const ratio = w / v.w;
      setInterviewBoardView(session, { x: p[0] - (p[0] - v.x) * ratio, y: p[1] - (p[1] - v.y) * ratio, w, h: w * 7 / 10 });
    } else setInterviewBoardView(session, { ...v, x: v.x + e.deltaX * unit / rect.width * v.w, y: v.y + e.deltaY * unit / rect.height * v.h });
  }, { passive: false });
  board.addEventListener('keydown', e => {
    if (e.code === 'Space') { e.preventDefault(); session.panHeld = true; board.classList.add('panning'); return; }
    if (e.key === 'Escape') { finish(true); session.selected = -1; drawInterviewBoard(session); return; }
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key) || session.switching || session.gesture) return;
    e.preventDefault();
    const target = e.target.closest('[data-board-shape]');
    const index = target ? Number(target.dataset.boardShape) : -1;
    const step = (e.shiftKey ? 50 : 10) * interviewBoardView(session).w / 1000;
    const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0;
    const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0;
    if (session.draft.board[index]) {
      rememberInterviewBoard(session); session.selected = index;
      session.draft.board[index] = interviews.moveShape(session.draft.board[index], dx, dy);
      interviewChanged(session); drawInterviewBoard(session);
      board.querySelector(`[data-board-shape="${index}"]`)?.focus({ preventScroll: true });
    } else { const v = interviewBoardView(session); setInterviewBoardView(session, { ...v, x: v.x + dx, y: v.y + dy }); }
  });
  board.addEventListener('keyup', e => { if (e.code === 'Space') { session.panHeld = false; if (session.gesture?.kind !== 'pan') board.classList.remove('panning'); } });
  board.addEventListener('blur', e => { if (!board.contains(e.relatedTarget)) { session.panHeld = false; if (session.gesture?.kind !== 'pan') board.classList.remove('panning'); } }, true);
}

function addInterviewLabel(session, at = null) {
  if (session.switching || !session.item.challenge) return;
  const field = $('#interview-board-label');
  const text = field.value.trim();
  if (!text) { field.focus(); return; }
  const v = interviewBoardView(session);
  const rows = Math.max(1, Math.min(6, Math.floor((v.h - 80) / 40)));
  const position = at || interviews.point(v.x + Math.max(30, 50 * v.w / 1000), v.y + Math.max(50, 60 * v.w / 1000) + session.draft.board.length % rows * Math.max(40, 40 * v.w / 1000));
  const board = [...session.draft.board, { kind: 'text', points: [position], text }];
  if (!interviews.validBoard(board)) { toast('Your board is full. Undo a shape first.', { bad: true }); return; }
  rememberInterviewBoard(session);
  session.draft.board = board;
  session.selected = board.length - 1;
  field.value = ''; session.label = '';
  interviewChanged(session); drawInterviewBoard(session);
  const placed = $('#interview-board').querySelector(`[data-board-shape="${session.selected}"]`);
  placed?.focus({ preventScroll: true });
  placed?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
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
  if (!interviews.hasWork(workspace, session.item.challenge)) { $('#interview-answer').focus(); toast('Add your implementation, approach or drawing first.'); return; }
  session.submitting = true;
  drawInterviewProblems(session);
  drawInterviewStatus(session);
  try {
    await saveInterviewWorkspace(session);
    const challenge = session.item.challenge.id;
    const attempt = felix.newId();
    await interviewQueue(session, async () => {
      session.item = await api(`/api/interviews/${enc(session.item.id)}`, { method: 'PUT', body: { action: 'submit', revision: session.item.revision, challenge, attempt, workspace } });
    });
    if (JSON.stringify(session.draft) === JSON.stringify(workspace)) {
      interviews.clearDraft(localStorage, session.item.id, challenge);
      session.saved = session.generation;
    } else {
      session.saved = -1;
      interviews.keepDraft(localStorage, session.item.id, challenge, session.draft);
    }
    drawInterviewProblems(session);
    drawInterviewProgress(session);
    await talkInterview(session, `Review my submitted answer${workspace.board.length ? ' and whiteboard' : ''}. Explain my strengths, gaps and what to practise next. Keep the full solution for when I ask.`, workspace.board, `Review my submitted attempt ${attempt}. Read interview_progress, assess the submitted answer and board against the challenge criteria with interview_feedback, explain my strengths and gaps, and suggest the next practice step. Do not reveal a complete solution unless I ask for it.`);
  } finally { session.submitting = false; drawInterviewProblems(session); drawInterviewStatus(session); }
}

function interviewForm(track, id = felix.newId(), item = null) {
  const f = item ? item.framework : interviews.framework(track);
  const coding = f.track === 'coding';
  sheet(`<h2>${item ? 'Practice preferences' : `Start ${esc(interviews.preset(track).label.toLowerCase())} practice`}</h2><p class="hint">${item ? 'Keep your workspace and change what Felix focuses on next.' : 'One step to your first challenge. Felix researches your target and prepares an exercise.'}</p>
    <label class="interview-label" for="interview-company">Company <span class="hint">Optional · leave blank if you’re just practising</span></label><div class="field"><input id="interview-company" maxlength="120" value="${esc(f.company)}" placeholder="e.g. Pinterest"></div>
    <label class="interview-label" for="interview-role">Role & level <span class="hint">Optional</span></label><div class="field"><input id="interview-role" maxlength="120" value="${esc(f.role)}" placeholder="e.g. Software engineer intern"></div>
    <label class="interview-label" for="interview-language">${coding ? 'Coding language' : 'Specialism'} <span class="hint">${coding ? 'The starting function will use this language' : 'Optional'}</span></label><div class="field"><input id="interview-language" maxlength="80" value="${esc(f.language)}" placeholder="${coding ? 'Python' : 'Your area of focus'}"></div>
    ${coding ? `<div class="pick-chips" aria-label="Coding language">${['Python', 'JavaScript', 'TypeScript', 'Java', 'C++', 'Go', 'Rust', 'SQL'].map((language) => `<button class="pick-chip${f.language === language ? ' on' : ''}" data-interview="form-language" data-language="${language}" aria-pressed="${f.language === language}">${language}</button>`).join('')}</div>` : ''}
    <p class="interview-label">How would you like to practise?</p><div class="pick-chips" role="group" aria-label="Interview style"><button class="pick-chip${f.style === 'coach' ? ' on' : ''}" data-interview="form-style" data-style="coach" aria-pressed="${f.style === 'coach'}">Coach</button><button class="pick-chip${f.style === 'mock' ? ' on' : ''}" data-interview="form-style" data-style="mock" aria-pressed="${f.style === 'mock'}">Mock interview</button></div><p class="hint">Coach helps when you ask. Mock saves feedback for your debrief.</p>
    <label class="interview-label" for="interview-wants">What would you like to practise? <span class="hint">Optional</span></label><textarea class="set-input" id="interview-wants" maxlength="2000" rows="2" placeholder="e.g. I’m just practising, start with easy array problems…">${esc(f.wants)}</textarea>
    <details class="interview-details source-more"><summary>${ICON.chevron}<span>Schedule & assessment preferences</span></summary><label class="interview-label" for="interview-title">Practice title</label><div class="field"><input id="interview-title" maxlength="120" value="${esc(f.title)}"></div><label class="interview-label" for="interview-date">Interview date (optional)</label><div class="field"><input id="interview-date" type="date" value="${esc(f.date)}"></div><label class="interview-label" for="interview-budget">Practice minutes per week</label><div class="field"><input id="interview-budget" type="number" min="15" max="2400" value="${f.weekly_minutes}"></div><label class="interview-label" for="interview-skills">Skills to assess · one per line, up to 12</label><textarea class="set-input" id="interview-skills" rows="4" maxlength="1000">${esc(f.skills.join('\n'))}</textarea></details>
    <p class="set-note warn" id="interview-form-error" hidden></p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn primary" id="interview-form-save" data-interview="save-framework" data-id="${esc(id)}" data-track="${esc(f.track)}" data-revision="${item ? item.revision : 0}" data-style="${esc(f.style)}">${item ? 'Save preferences' : 'Start practice'}</button></div>`);
  $('#interview-company').focus();
  $('#interview-language').addEventListener('input', (e) => {
    document.querySelectorAll('[data-interview="form-language"]').forEach((b) => { const selected = b.dataset.language === e.target.value; b.classList.toggle('on', selected); b.setAttribute('aria-pressed', String(selected)); });
  });
}

async function saveInterviewForm(el) {
  if (el.disabled) return;
  el.disabled = true;
  const framework = { title: $('#interview-title').value.trim(), track: el.dataset.track, company: $('#interview-company').value.trim(), role: $('#interview-role').value.trim(), language: $('#interview-language').value.trim(), date: $('#interview-date').value, weekly_minutes: Number($('#interview-budget').value), skills: $('#interview-skills').value.split('\n').map((s) => s.trim()).filter(Boolean), wants: $('#interview-wants').value, style: el.dataset.style };
  try {
    await api(`/api/interviews/${enc(el.dataset.id)}`, { method: 'PUT', body: { action: 'framework', revision: Number(el.dataset.revision), framework } });
    if (Number(el.dataset.revision) === 0) interviewStart = { id: el.dataset.id };
    closeSheet();
    go(`#/interviews/${enc(el.dataset.id)}`);
  } catch (e) {
    const error = $('#interview-form-error');
    if (error) { error.hidden = false; error.textContent = e.message; }
  } finally { el.disabled = false; }
}

function interviewSlotForm(session, id = '') {
  const old = session.item.schedule.find((s) => s.id === id);
  const today = new Date();
  const day = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
  const slot = old || { id: felix.newId(), title: '', date: day, time: '', minutes: 30, skill: session.item.framework.skills[0] };
  sheet(`<h2>${old ? 'Edit practice session' : 'Add practice session'}</h2><div class="field"><input id="interview-slot-title" maxlength="160" aria-label="Session title" placeholder="What will you practise?" value="${esc(slot.title)}"></div><label class="interview-label" for="interview-slot-date">Date</label><div class="field"><input id="interview-slot-date" type="date" value="${esc(slot.date)}"></div><label class="interview-label" for="interview-slot-time">Time (optional)</label><div class="field"><input id="interview-slot-time" type="time" value="${esc(slot.time)}"></div><label class="interview-label" for="interview-slot-minutes">Minutes</label><div class="field"><input id="interview-slot-minutes" type="number" min="5" max="180" value="${slot.minutes}"></div><p class="interview-label">Skill focus</p><div class="pick-chips">${session.item.framework.skills.map((s) => `<button class="pick-chip${s === slot.skill ? ' on' : ''}" data-interview="slot-skill" data-skill="${esc(s)}" aria-pressed="${s === slot.skill}">${esc(s)}</button>`).join('')}</div><p id="interview-slot-error" class="set-note warn" hidden></p><div class="buttons">${old ? `<button class="btn danger" data-interview="remove-session" data-id="${esc(slot.id)}">Remove</button>` : '<button class="btn" data-action="close">Cancel</button>'}<button class="btn primary" id="interview-slot-save" data-interview="save-session" data-id="${esc(slot.id)}" data-skill="${esc(slot.skill)}">Save</button></div>`);
  $('#interview-slot-title').focus();
}

async function saveInterviewSchedule(session, schedule) {
  await saveInterviewWorkspace(session);
  await interviewQueue(session, async () => {
    const next = typeof schedule === 'function' ? schedule(session.item.schedule) : schedule;
    session.item = await api(`/api/interviews/${enc(session.item.id)}`, { method: 'PUT', body: { action: 'schedule', revision: session.item.revision, schedule: next } });
  });
  drawInterviewSchedule(session);
}

async function interviewAction(el) {
  const action = el.dataset.interview;
  const session = interviewPage;
  if (action === 'open') return go(`#/interviews/${enc(el.dataset.id)}`);
  if (action === 'create') return interviewForm(el.dataset.track, el.dataset.id || felix.newId());
  if (action === 'save-framework') return saveInterviewForm(el);
  if (action === 'form-language') {
    $('#interview-language').value = el.dataset.language;
    $('#interview-language').dispatchEvent(new Event('input', { bubbles: true }));
    return;
  }
  if (action === 'form-style' || action === 'slot-skill') {
    const group = el.parentElement;
    group.querySelectorAll('.pick-chip').forEach((button) => { button.classList.toggle('on', button === el); button.setAttribute('aria-pressed', String(button === el)); });
    if (action === 'form-style') $('#interview-form-save').dataset.style = el.dataset.style;
    else $('#interview-slot-save').dataset.skill = el.dataset.skill;
    return;
  }
  if (action === 'delete') {
    const id = el.dataset.id;
    sheet(`<h2>Delete this interview practice?</h2><p>Its workspace, attempts and schedule will be removed. The Felix conversation stays in your chats.</p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn danger" data-interview="confirm-delete" data-id="${esc(id)}">Delete practice</button></div>`);
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
  if (action === 'plan') return talkInterview(session, 'Help me make a realistic practice schedule from my goals and progress.', [], `Help me plan interview practice with interview_schedule. Use interpretable language. Read interview_progress and list_schedule for the coming week before planning. Use my existing events, tasks and saved sessions to avoid conflicts, respect my weekly budget and interview date, and save a reasonable editable plan with interview_schedule. Do not ask me to repeat my schedule. If there is no interview date, plan the coming week. Briefly state timing assumptions and let me adjust them. Today on my device is ${new Date().toLocaleDateString('en-CA')}; timezone ${Intl.DateTimeFormat().resolvedOptions().timeZone}.`);
  if (action === 'hint') return talkInterview(session, 'Give me a small hint without the solution.', [], 'Give me one small hint for the current interview challenge, without revealing the solution or assessing my draft.');
  if (action === 'submit') return submitInterview(session);
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
    if (tab === 'board') { rememberInterviewBoard(session, session.draft.board, true); session.boardViews.delete(session.item.challenge.id); session.draft.board = []; session.selected = -1; session.stroke = null; session.gesture = null; drawInterviewBoard(session); }
    else if (tab === 'code' || session.item.framework.track !== 'coding') { session.draft.answer = tab === 'code' ? session.item.challenge.coding?.starter || '' : ''; $('#interview-answer').value = session.draft.answer; drawInterviewLines(session); }
    else { session.draft.reasoning = ''; $('#interview-reasoning').value = ''; }
    interviewChanged(session); return;
  }
  if (action === 'tool') {
    session.tool = el.dataset.tool;
    app.querySelectorAll('[data-interview="tool"]').forEach((b) => { b.classList.toggle('on', b === el); b.setAttribute('aria-pressed', String(b === el)); });
    $('#interview-board').dataset.tool = session.tool;
    return;
  }
  if (action === 'label-board') return addInterviewLabel(session);
  if (action === 'zoom-board') { const v = interviewBoardView(session), w = Math.max(100, Math.min(interviews.BOARD_LIMIT * 2, v.w * (Number(el.dataset.step) > 0 ? .8 : 1.25))), h = w * 7 / 10; setInterviewBoardView(session, { x: v.x + (v.w - w) / 2, y: v.y + (v.h - h) / 2, w, h }); return; }
  if (action === 'fit-board') { setInterviewBoardView(session, interviews.fitBoard(session.draft.board, $('#interview-board').clientWidth * 1.5)); return; }
  if (action === 'undo-board') {
    const before = session.boardUndo?.pop();
    if (before) { session.draft.board = before.board; if (before.view) setInterviewBoardView(session, before.view); }
    else if (session.draft.board.length) session.draft.board.pop();
    else return;
    session.selected = -1; interviewChanged(session); drawInterviewBoard(session); return;
  }
  if (action === 'add-session' || action === 'edit-session') return interviewSlotForm(session, el.dataset.id || '');
  if (action === 'complete') {
    const schedule = (slots) => slots.map((s) => s.id === el.dataset.id ? { ...s, done: el.checked } : s);
    try { return await saveInterviewSchedule(session, schedule); } catch (e) { drawInterviewSchedule(session); throw e; }
  }
  if (action === 'save-session' || action === 'remove-session') {
    if (el.disabled) return;
    el.disabled = true;
    try {
      const slot = action === 'save-session' ? { id: el.dataset.id, title: $('#interview-slot-title').value.trim(), date: $('#interview-slot-date').value, time: $('#interview-slot-time').value, minutes: Number($('#interview-slot-minutes').value), skill: el.dataset.skill, done: session.item.schedule.find((s) => s.id === el.dataset.id)?.done || false } : null;
      await saveInterviewSchedule(session, (slots) => [...slots.filter((s) => s.id !== el.dataset.id), ...(slot ? [slot] : [])]);
      closeSheet();
    } catch (e) { const error = $('#interview-slot-error'); if (error) { error.hidden = false; error.textContent = e.message; } }
    finally { el.disabled = false; }
  }
}

document.addEventListener('click', (e) => {
  const button = e.target.closest('[data-interview]');
  if (!button || button.dataset.interview === 'complete') return;
  e.preventDefault();
  interviewAction(button).catch(fail);
});
document.addEventListener('change', (e) => {
  if (e.target.dataset.interview === 'complete') interviewAction(e.target).catch(fail);
});
