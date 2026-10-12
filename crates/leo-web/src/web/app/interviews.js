const interviews = window.leoInterview;
let interviewPage = null;

async function showInterviews(id = '') {
  const mine = ++seq;
  state = { view: 'interviews', dir: '' };
  interviewPage = null;
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
  const session = { item, mine, draft: structuredClone(item.workspace), generation: 0, saved: 0, timer: 0, queue: Promise.resolve(), error: '', tab: item.framework.track === 'coding' ? 'code' : 'write', tool: 'pen', stroke: null, submitting: false, calendar: [] };
  interviewPage = session;
  const local = item.challenge && interviews.readDraft(localStorage, id, item.challenge.id);
  if (local && JSON.stringify(local) !== JSON.stringify(item.workspace)) {
    session.draft = local;
    session.generation = 1;
    session.error = 'An unsaved draft was recovered on this device. Review it, then save it when you are ready.';
  }
  drawInterview(session);
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
  const active = document.activeElement;
  const focus = active && app.contains(active) && (active instanceof HTMLTextAreaElement || (active instanceof HTMLInputElement && active.type === 'text')) ? { id: active.id, from: active.selectionStart, to: active.selectionEnd } : null;
  const item = session.item;
  app.innerHTML = `<div class="interview-home"><div class="interview-heading"><div><p class="eyebrow">${esc(interviews.preset(item.framework.track).label)} · ${item.framework.style === 'mock' ? 'Mock interview' : 'Coach'}</p><h1>${esc(item.framework.title)}</h1><p class="hint">${esc([item.framework.company, item.framework.role, item.framework.language].filter(Boolean).join(' · ') || 'A space to work through your next interview.')}</p></div><div class="buttons"><button class="btn sm" data-interview="configure">Customize</button><button class="btn sm primary" data-interview="chat">Talk with Felix</button></div></div>
    <section id="interview-challenge" class="set-card"></section>
    <section class="set-card interview-workspace"><div class="interview-toolbar"><div class="library-views" role="tablist" aria-label="Your answer workspace">${[['write', 'Write'], ['code', 'Code'], ['board', 'Whiteboard']].map(([id, label]) => `<button class="btn sm plain${session.tab === id ? ' on' : ''}" role="tab" aria-selected="${session.tab === id}" data-interview="tab" data-tab="${id}">${label}</button>`).join('')}</div><span id="interview-save" class="hint" role="status"></span></div>
    <div id="interview-editor"${session.tab === 'board' ? ' hidden' : ''}><label class="interview-label" for="interview-answer">Your answer <span class="hint">Include your reasoning and checks.</span></label><textarea id="interview-answer" class="set-input${session.tab === 'code' ? ' interview-code' : ''}" maxlength="32000" spellcheck="${session.tab !== 'code'}" placeholder="${session.tab === 'code' ? 'Write your code, then explain your approach and test cases…' : 'Clarify the problem, outline an approach, and work through it…'}"${item.challenge ? '' : ' disabled'}>${esc(session.draft.answer)}</textarea></div>
    <div id="interview-whiteboard"${session.tab !== 'board' ? ' hidden' : ''}><div class="interview-toolbar"><div class="pick-chips" aria-label="Drawing tools">${[['pen', 'Pen'], ['box', 'Box'], ['arrow', 'Arrow'], ['text', 'Text']].map(([id, label]) => `<button class="pick-chip${session.tool === id ? ' on' : ''}" aria-pressed="${session.tool === id}" data-interview="tool" data-tool="${id}">${label}</button>`).join('')}</div><div class="buttons"><button class="btn sm" data-interview="undo-board">Undo</button><button class="btn sm" data-interview="clear-board">Clear</button></div></div><label class="interview-label" for="interview-board-label">Board label</label><input class="set-input" id="interview-board-label" value="${esc(session.label || '')}" maxlength="300" placeholder="Type a label, then tap the board to place it"><button class="btn sm" data-interview="label-board">Add label to board</button><div id="interview-board" tabindex="0" role="group" aria-label="Whiteboard. Draw with a pointer; add text with the Add label button."></div><p class="hint">Draw boxes and arrows to explain your thinking. Felix can review a snapshot when you submit.</p></div>
    <div id="interview-save-error" class="set-note warn" hidden></div><div class="interview-toolbar"><p class="hint">Felix reviews your work and reasoning. Code stays in this workspace.</p><div class="buttons"><button class="icon-btn" data-interview="clear-workspace" aria-label="Clear your draft" title="Clear your draft">${ICON.trash}</button><button class="btn sm" data-interview="hint">Ask for a hint</button><button class="btn sm primary" data-interview="submit"${item.challenge ? '' : ' disabled'}>Review with Felix</button></div></div></section>
    <section id="interview-progress"></section><section id="interview-schedule"></section><section id="interview-attempts"></section></div>`;
  drawInterviewChallenge(session);
  drawInterviewBoard(session);
  drawInterviewStatus(session);
  drawInterviewProgress(session);
  drawInterviewSchedule(session);
  $('#interview-answer').addEventListener('input', (e) => {
    session.draft.answer = e.target.value;
    interviewChanged(session);
  });
  $('#interview-answer').addEventListener('keydown', (e) => {
    if (e.key !== 'Tab' || e.shiftKey || session.tab !== 'code') return;
    e.preventDefault();
    const field = e.target;
    field.setRangeText('  ', field.selectionStart, field.selectionEnd, 'end');
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
  $('#interview-board-label').addEventListener('input', (e) => { session.label = e.target.value; });
  wireInterviewBoard(session);
  if (focus) {
    const again = document.getElementById(focus.id);
    if (again && !again.disabled) { again.focus({ preventScroll: true }); again.setSelectionRange(focus.from, focus.to); }
  }
}

function drawInterviewChallenge(session) {
  if (!interviewHere(session)) return;
  const c = session.item.challenge;
  $('#interview-challenge').innerHTML = c ? `<div class="interview-toolbar"><h2>${esc(c.title)}</h2><span class="chip">${c.minutes} min · ${esc(c.origin === 'generated' ? 'Generated practice' : c.origin === 'official' ? 'Official preparation' : 'Candidate report · unverified')}</span></div><div class="prose">${md.render(c.question, { interactive: false })}</div><div class="interview-sources">${c.sources.map((s) => `<a class="chip" href="${esc(s.url)}" target="_blank" rel="noopener noreferrer">${esc(s.title)} · ${esc(s.kind)}</a>`).join('')}</div><button class="btn sm" data-interview="next">Ask for another challenge</button>` : `<h2>Your next challenge</h2><p class="hint">Felix will tailor an exercise to your framework. You can ask for a topic or a publicly reported company problem.</p><div class="buttons"><button class="btn sm primary" data-interview="next">Prepare a challenge</button><button class="btn sm" data-interview="research">Research company interviews</button></div>`;
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
    const latest = await api(`/api/interviews/${enc(id)}`);
    if (!interviewHere(session)) return;
    const changed = latest.challenge?.id !== session.item.challenge?.id;
    if (changed) {
      if (session.generation !== session.saved) {
        session.blocked = true;
        session.error = 'The challenge changed while you were writing. Your draft is kept; reopen this practice to continue.';
        drawInterviewStatus(session);
      } else {
        session.item = latest;
        session.draft = structuredClone(latest.workspace);
        drawInterview(session);
      }
    } else {
      session.item = latest;
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
  const scores = interviews.progress(attempts);
  $('#interview-progress').innerHTML = `<h2>What you’re learning</h2><div class="set-card"><p class="hint">${assessed ? `${assessed} reviewed attempt${assessed === 1 ? '' : 's'}. AI feedback is provisional; averages use your last five observations per skill.` : 'Complete an attempt to start seeing strengths and areas to practise. Felix won’t guess from an empty history.'}</p>${scores.map((s) => `<div class="set-row"><div class="grow"><b>${esc(s.skill)}</b><span class="hint">${s.n} observation${s.n === 1 ? '' : 's'}${s.n < 3 ? ' · early signal' : ''}</span></div><span class="chip">${s.value.toFixed(1)} / 4</span></div>`).join('')}</div>`;
  $('#interview-attempts').innerHTML = attempts.length ? `<h2>Your attempts</h2><div class="set-card">${[...attempts].reverse().map((a) => `<details class="interview-attempt"><summary><b>${esc(a.challenge.title)}</b><span class="hint">${new Date(a.at).toLocaleDateString()} · ${a.feedback ? 'Reviewed' : 'Awaiting feedback'}</span></summary>${a.feedback ? `<div class="prose">${md.render(a.feedback.summary, { interactive: false })}</div>${a.feedback.scores.map((s) => `<p><b>${esc(s.skill)} · ${s.value}/4</b><br>${esc(s.evidence)}<br><span class="hint">Next: ${esc(s.next)}</span></p>`).join('')}` : `<button class="btn sm" data-interview="review-attempt" data-id="${esc(a.id)}">Ask Felix to review</button>`}<details><summary>Your submitted work</summary><pre class="interview-submitted">${esc(a.workspace.answer)}</pre>${a.workspace.board.length ? `<div class="interview-board-snapshot">${interviews.boardSvg(a.workspace.board)}</div>` : ''}</details></details>`).join('')}</div>` : '';
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

function drawInterviewBoard(session) {
  if (!interviewHere(session)) return;
  const board = $('#interview-board');
  if (board) board.innerHTML = interviews.boardSvg(session.stroke ? [...session.draft.board, session.stroke] : session.draft.board);
}

function wireInterviewBoard(session) {
  const board = $('#interview-board');
  const locate = (e) => {
    const rect = board.getBoundingClientRect();
    return interviews.point((e.clientX - rect.left) / rect.width * 1000, (e.clientY - rect.top) / rect.height * 700);
  };
  board.addEventListener('pointerdown', (e) => {
    if (!session.item.challenge || e.button !== 0 || session.draft.board.length >= interviews.MOST_SHAPES) return;
    e.preventDefault();
    const first = locate(e);
    if (session.tool === 'text') { addInterviewLabel(session, first); return; }
    session.stroke = { kind: session.tool, points: [first], text: '' };
    board.setPointerCapture(e.pointerId);
    drawInterviewBoard(session);
  });
  board.addEventListener('pointermove', (e) => {
    if (!session.stroke) return;
    const p = locate(e);
    const shape = session.stroke;
    if (shape.kind === 'pen') {
      const last = shape.points.at(-1);
      if (shape.points.length < 128 && Math.hypot(p[0] - last[0], p[1] - last[1]) > 4) shape.points.push(p);
    } else shape.points = [shape.points[0], p];
    if (interviews.validBoard([...session.draft.board, shape])) drawInterviewBoard(session);
  });
  const finish = () => {
    if (!session.stroke) return;
    const board = [...session.draft.board, session.stroke];
    if (interviews.validBoard(board)) { session.draft.board = board; interviewChanged(session); }
    else toast('Your board is full. Undo some strokes to make room.', { bad: true });
    session.stroke = null;
    drawInterviewBoard(session);
  };
  board.addEventListener('pointerup', finish);
  board.addEventListener('pointercancel', finish);
}

function addInterviewLabel(session, point = [50, 50 + Math.min(session.draft.board.length * 30, 600)]) {
  if (!session.item.challenge) return;
  const field = $('#interview-board-label');
  const text = field.value.trim();
  if (!text) { field.focus(); return; }
  const board = [...session.draft.board, { kind: 'text', points: [point], text }];
  if (!interviews.validBoard(board)) { toast('Your board is full. Undo a shape first.', { bad: true }); return; }
  session.draft.board = board;
  field.value = '';
  session.label = '';
  interviewChanged(session);
  drawInterviewBoard(session);
  field.focus();
}

async function interviewPicture(board) {
  if (!board.length) return null;
  const url = URL.createObjectURL(new Blob([interviews.boardSvg(board)], { type: 'image/svg+xml' }));
  try {
    const image = new Image();
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('The board snapshot could not be made. Try again.')), 3000);
      image.onload = () => { clearTimeout(timeout); resolve(); };
      image.onerror = () => { clearTimeout(timeout); reject(new Error('The board snapshot could not be made. Try again.')); };
      image.src = url;
    });
    const canvas = document.createElement('canvas');
    canvas.width = 1000; canvas.height = 700;
    canvas.getContext('2d').drawImage(image, 0, 0, 1000, 700);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
    return blob ? new File([blob], `interview-whiteboard-${felix.newId()}.png`, { type: 'image/png' }) : null;
  } finally { URL.revokeObjectURL(url); }
}

async function talkInterview(session, text, board = []) {
  await saveInterviewWorkspace(session);
  const ready = await chat.openInterview(session.item.id);
  if (!ready) { toast('Felix is finishing another answer. Try again when it finishes.', { bad: true }); return; }
  const picture = await interviewPicture(board);
  if (picture && !(await chat.attachBoard(picture))) throw new Error('Felix could not read your board snapshot. Your attempt is saved; try its review button again.');
  chat.send(text);
}

async function submitInterview(session) {
  if (session.submitting) return;
  const workspace = structuredClone(session.draft);
  if (!workspace.answer.trim() && !workspace.board.length) { $('#interview-answer').focus(); toast('Write an answer or draw on the board first.'); return; }
  session.submitting = true;
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
      session.draft = { answer: '', board: [] };
      session.saved = session.generation;
      if (interviewHere(session)) { $('#interview-answer').value = ''; drawInterviewBoard(session); }
    }
    else {
      session.saved = -1;
      interviews.keepDraft(localStorage, session.item.id, challenge, session.draft);
    }
    drawInterviewProgress(session);
    await talkInterview(session, `Review my submitted attempt ${attempt}. Read interview_progress, assess the submitted answer and board against the challenge criteria with interview_feedback, explain my strengths and gaps, and suggest the next practice step. Do not reveal a complete solution unless I ask for it.`, workspace.board);
  } finally { session.submitting = false; drawInterviewStatus(session); }
}

function interviewForm(track, id = felix.newId(), item = null) {
  const f = item ? item.framework : interviews.framework(track);
  sheet(`<h2>${item ? 'Customize practice' : 'Start interview practice'}</h2><p class="hint">Choose what matters for your role. You can change this later.</p><div class="field"><input id="interview-title" aria-label="Practice title" maxlength="120" value="${esc(f.title)}" placeholder="Practice title"></div><div class="field"><input id="interview-company" aria-label="Company" maxlength="120" value="${esc(f.company)}" placeholder="Company (optional)"></div><div class="field"><input id="interview-role" aria-label="Role and level" maxlength="120" value="${esc(f.role)}" placeholder="Role and level (optional)"></div><div class="field"><input id="interview-language" aria-label="Coding language or specialism" maxlength="80" value="${esc(f.language)}" placeholder="Language or specialism (optional)"></div><div class="pick-chips" role="group" aria-label="Interview style"><button class="pick-chip${f.style === 'coach' ? ' on' : ''}" data-interview="form-style" data-style="coach" aria-pressed="${f.style === 'coach'}">Coach</button><button class="pick-chip${f.style === 'mock' ? ' on' : ''}" data-interview="form-style" data-style="mock" aria-pressed="${f.style === 'mock'}">Mock interview</button></div><p class="hint">Coach helps you work it out. Mock saves coaching for your debrief.</p><label class="interview-label" for="interview-date">Interview date (optional)</label><div class="field"><input id="interview-date" type="date" value="${esc(f.date)}"></div><label class="interview-label" for="interview-budget">Practice minutes per week</label><div class="field"><input id="interview-budget" type="number" min="15" max="2400" value="${f.weekly_minutes}"></div><label class="interview-label" for="interview-skills">Skills to assess · one per line, up to 12</label><textarea class="set-input" id="interview-skills" rows="4" maxlength="1000">${esc(f.skills.join('\n'))}</textarea><label class="interview-label" for="interview-wants">Anything Felix should know?</label><textarea class="set-input" id="interview-wants" maxlength="2000" rows="2" placeholder="Topics, experience, accessibility needs, or how you like to practise…">${esc(f.wants)}</textarea><p class="set-note warn" id="interview-form-error" hidden></p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn primary" id="interview-form-save" data-interview="save-framework" data-id="${esc(id)}" data-track="${esc(f.track)}" data-revision="${item ? item.revision : 0}" data-style="${esc(f.style)}">${item ? 'Save' : 'Start practice'}</button></div>`);
  $('#interview-title').focus();
}

async function saveInterviewForm(el) {
  if (el.disabled) return;
  el.disabled = true;
  const framework = { title: $('#interview-title').value.trim(), track: el.dataset.track, company: $('#interview-company').value.trim(), role: $('#interview-role').value.trim(), language: $('#interview-language').value.trim(), date: $('#interview-date').value, weekly_minutes: Number($('#interview-budget').value), skills: $('#interview-skills').value.split('\n').map((s) => s.trim()).filter(Boolean), wants: $('#interview-wants').value, style: el.dataset.style };
  try {
    await api(`/api/interviews/${enc(el.dataset.id)}`, { method: 'PUT', body: { action: 'framework', revision: Number(el.dataset.revision), framework } });
    closeSheet();
    go(`#/interviews/${enc(el.dataset.id)}`);
    if (interviewPage && interviewPage.item.id === el.dataset.id) refreshInterview(el.dataset.id).catch(fail);
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
  if (action === 'chat') return chat.openInterview(session.item.id);
  if (action === 'next') return talkInterview(session, 'Prepare one new interview challenge in my workspace using interview_challenge, suited to my framework and recent practice. Do not give its solution.');
  if (action === 'research') return talkInterview(session, 'Research common interview topics and publicly reported past questions for my target company and role. Ask my target if it is missing. Search the web, link sources, distinguish official guidance from candidate reports, and suggest an original practice exercise.');
  if (action === 'plan') return talkInterview(session, `Help me plan interview practice with interview_schedule. Read my progress, respect my weekly budget and interview date, and ask about availability if needed. Today on my device is ${new Date().toLocaleDateString('en-CA')}; timezone ${Intl.DateTimeFormat().resolvedOptions().timeZone}.`);
  if (action === 'hint') return talkInterview(session, 'Give me one small hint for the current interview challenge, without revealing the solution or assessing my draft.');
  if (action === 'submit') return submitInterview(session);
  if (action === 'review-attempt') {
    const attempt = session.item.attempts.find((a) => a.id === el.dataset.id);
    if (attempt) return talkInterview(session, `Review submitted attempt ${attempt.id} using interview_progress and interview_feedback. Explain strengths, gaps and the next practice step without giving a full solution.`, attempt.workspace.board);
  }
  if (action === 'retry') return retryInterviewSave(session);
  if (action === 'tab') {
    session.tab = el.dataset.tab;
    $('#interview-editor').hidden = session.tab === 'board';
    $('#interview-whiteboard').hidden = session.tab !== 'board';
    $('#interview-answer').classList.toggle('interview-code', session.tab === 'code');
    $('#interview-answer').spellcheck = session.tab !== 'code';
    app.querySelectorAll('[data-interview="tab"]').forEach((b) => { b.classList.toggle('on', b.dataset.tab === session.tab); b.setAttribute('aria-selected', String(b.dataset.tab === session.tab)); });
    if (session.tab !== 'board') $('#interview-answer').focus();
    return;
  }
  if (action === 'tool') {
    session.tool = el.dataset.tool;
    app.querySelectorAll('[data-interview="tool"]').forEach((b) => { b.classList.toggle('on', b === el); b.setAttribute('aria-pressed', String(b === el)); });
    return;
  }
  if (action === 'label-board') return addInterviewLabel(session);
  if (action === 'undo-board' || action === 'clear-board') {
    if (action === 'clear-board' && session.draft.board.length) {
      sheet('<h2>Clear your whiteboard?</h2><p>Your written answer stays here.</p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn danger" data-interview="confirm-clear-board">Clear board</button></div>');
      return;
    }
    session.draft.board.pop(); interviewChanged(session); drawInterviewBoard(session); return;
  }
  if (action === 'clear-workspace') {
    sheet('<h2>Clear your draft?</h2><p>Your current answer and whiteboard will be cleared. Submitted attempts stay in your history.</p><div class="buttons"><button class="btn" data-action="close">Cancel</button><button class="btn danger" data-interview="confirm-clear-workspace">Clear draft</button></div>');
    return;
  }
  if (action === 'confirm-clear-workspace') {
    session.draft = { answer: '', board: [] };
    closeSheet();
    if ($('#interview-answer')) $('#interview-answer').value = '';
    interviewChanged(session); drawInterviewBoard(session); return;
  }
  if (action === 'confirm-clear-board') { session.draft.board = []; closeSheet(); interviewChanged(session); drawInterviewBoard(session); return; }
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
