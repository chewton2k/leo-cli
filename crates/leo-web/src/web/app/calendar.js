const S = window.leoSchedule;
const cal = { month: null, selected: null, data: null, loadedAt: 0, timer: 0, polls: 0, showDone: true, gridOpen: false };
const CAL_DONE_KEY = 'leo-calendar-hide-done';
try {
  cal.showDone = localStorage.getItem(CAL_DONE_KEY) !== '1';
} catch (e) {
  cal.showDone = true;
}
const CAL_POLL_MS = 150000;
const KIND_LABEL = { assignment: 'Assignment', exam: 'Exam', meeting: 'Meeting', 'follow-up': 'Follow-up', deadline: 'Deadline', other: 'Other' };
const KIND_ORDER = ['assignment', 'exam', 'meeting', 'follow-up', 'deadline', 'other'];

function calLocale() {
  return navigator.language || 'en-US';
}

function calZone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || '';
  } catch (e) {
    return '';
  }
}

function monthOf(key) {
  const d = S.fromKey(key);
  return new Date(d.getFullYear(), d.getMonth(), 1);
}

function calWindow() {
  const grid = S.monthGrid(cal.month.getFullYear(), cal.month.getMonth(), S.weekStart(calLocale()));
  const from = S.fromKey(grid[0].key);
  const to = S.addDays(S.fromKey(grid[grid.length - 1].key), 1);
  return { grid, from: from.toISOString(), to: to.toISOString() };
}

async function showCalendar() {
  const mine = ++seq;
  state = { view: 'calendar', dir: '' };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Calendar</button>';
  document.title = 'Calendar · leo';
  const today = S.dayKey(new Date());
  if (!cal.selected) cal.selected = today;
  cal.month = monthOf(cal.selected);
  app.innerHTML = '<div class="sched-page" id="sched-page"></div>';
  drawCalendar();
  await loadSchedule(mine);
}

async function loadSchedule(mine = seq) {
  if (state.view !== 'calendar') return;
  const { from, to } = calWindow();
  const asked = cal.month.getTime();
  let data;
  try {
    data = await api(`/api/schedule?from=${enc(from)}&to=${enc(to)}&live=1`);
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (mine === seq && state.view === 'calendar') {
      cal.failed = true;
      drawCalendar();
    }
    return;
  }
  if (mine !== seq || state.view !== 'calendar' || asked !== cal.month.getTime()) return;
  cal.failed = false;
  cal.data = data;
  tasksWin.tasks = data.tasks || tasksWin.tasks;
  cal.loadedAt = Date.now();
  drawCalendar();
  drawTasksWindow();
  clearTimeout(cal.timer);
  if (data.refreshing && cal.polls < 6) {
    cal.polls++;
    cal.timer = setTimeout(() => loadSchedule(mine), 4000);
  } else {
    cal.polls = 0;
    cal.timer = setTimeout(() => {
      if (document.visibilityState === 'visible') loadSchedule(mine);
    }, CAL_POLL_MS);
  }
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible' && state.view === 'calendar' && Date.now() - cal.loadedAt > 120000) loadSchedule(seq);
});

function calStatus() {
  const d = cal.data;
  if (cal.failed) return `<span>Couldn't update</span> · <button type="button" class="link-btn" data-action="cal-retry">Retry</button>`;
  if (!d) return '<span>Loading…</span>';
  if (!d.connected) return '';
  if (d.refreshing) return `<span class="activity-spin" aria-hidden="true"></span><span>Updating…</span>`;
  const problem = (d.calendars || []).some((c) => c.problem);
  const when = d.synced ? rel(d.synced) : 'not yet';
  return `<span>${problem ? 'A calendar could not be read' : `Updated ${esc(when)}`}</span> · <button type="button" class="link-btn" data-action="cal-retry">Retry</button>`;
}

const TONES = 8;
const TASK_TONES = { exam: '--hue-red', assignment: '--hue-indigo', meeting: '--hue-violet', 'follow-up': '--hue-teal', deadline: '--hue-amber', other: '--hue-slate' };

function toneOfEvent(event) {
  const key = String(event.title || '').toLowerCase().replace(/\s+/g, ' ').trim();
  let hash = 2166136261;
  for (const ch of key) hash = Math.imul(hash ^ ch.codePointAt(0), 16777619) >>> 0;
  return `--hue-${hash % TONES}`;
}

function toneOfTask(task) {
  return TASK_TONES[task.kind] || '--hue-slate';
}

function repeatBubbles(on) {
  return `<div class="repeat-days" role="group" aria-label="Repeat every">${[0, 1, 2, 3, 4, 5, 6].map((d) => {
    const day = new Date(2024, 0, 1 + d);
    const name = day.toLocaleDateString(calLocale(), { weekday: 'long' });
    const letter = day.toLocaleDateString(calLocale(), { weekday: 'narrow' });
    return `<button type="button" class="repeat-day${on.includes(d) ? ' on' : ''}" data-repeat-day="${d}" aria-pressed="${on.includes(d)}" aria-label="${esc(name)}" title="${esc(name)}">${esc(letter)}</button>`;
  }).join('')}</div>`;
}

function repeatHint(repeat) {
  return repeat.length ? S.repeatLabel(repeat, calLocale()) : 'Does not repeat';
}

function repeatPicker(box, start, onChange) {
  const days = [...start];
  box.addEventListener('click', (e) => {
    const b = e.target.closest('[data-repeat-day]');
    if (!b) return;
    const d = Number(b.dataset.repeatDay);
    const at = days.indexOf(d);
    if (at >= 0) days.splice(at, 1);
    else days.push(d);
    days.sort((a, c) => a - c);
    b.classList.toggle('on', at < 0);
    b.setAttribute('aria-pressed', String(at < 0));
    const hint = box.querySelector('.repeat-hint');
    if (hint) hint.textContent = repeatHint(days);
    onChange([...days]);
  });
  return days;
}

function taskLine(task, { showDate = false } = {}) {
  const time = task.time ? `<span class="sched-time">${esc(S.timeLabel(task.time, calLocale()))}</span>` : '<span class="sched-time"></span>';
  const kind = task.kind === 'exam' ? '<span class="sched-pill">Exam</span>' : `<span class="sched-kind">${esc(KIND_LABEL[task.kind] || 'Other')}</span>`;
  const from = task.note ? `<span class="sched-from">${esc(task.note.title || 'Untitled')}</span>` : '<span class="sched-from">Added in Leo</span>';
  const when = showDate && task.date ? ` · ${esc(S.fromKey(task.date).toLocaleDateString(calLocale(), { month: 'short', day: 'numeric' }))}` : '';
  const due = task.time && task.kind !== 'meeting' ? ` · Due ${esc(S.timeLabel(task.time, calLocale()))}` : '';
  const google = task.sent_at || (cal.data && (cal.data.events || []).some((e) => e.task === task.id)) ? '<span class="sched-badge">Google</span>' : '';
  const missing = task.found === false ? ' · <span class="sched-warn">not found in the note</span>' : '';
  const repeat = S.repeats(task) ? ` · ${esc(S.repeatLabel(task.repeat, calLocale()))}` : '';
  const on = task.on ? ` data-on="${esc(task.on)}"` : '';
  const set = !task.date && !task.done ? `<button type="button" class="btn plain sm" data-action="task-date" data-id="${esc(task.id)}">Set date</button>` : '';
  return `<div class="sched-row${task.done ? ' done' : ''}" data-task="${esc(task.id)}" style="--tone: var(${toneOfTask(task)})">
    ${showDate ? '' : time}
    <input type="checkbox" class="sched-check" data-action="task-done" data-id="${esc(task.id)}"${on} aria-label="Done: ${esc(task.title)}"${task.done ? ' checked' : ''}>
    <button type="button" class="sched-main" data-action="task-open" data-id="${esc(task.id)}"${on}><span class="sched-title">${esc(task.title)}</span><span class="sched-sub">${from}${when}${due}${repeat}${missing}</span></button>
    ${google}${kind}${set}
  </div>`;
}

function eventLine(event) {
  const time = event.all_day ? '' : S.timeLabel(new Date(event.start).toTimeString().slice(0, 5), calLocale());
  const end = event.all_day ? '' : S.timeLabel(new Date(event.end).toTimeString().slice(0, 5), calLocale());
  const span = event.all_day ? '' : `${time}–${end}`;
  const place = event.location ? ` · ${esc(event.location)}` : '';
  const zone = event.zone_unknown ? ' · <span class="sched-warn">time zone unknown</span>' : '';
  const sub = [span ? esc(span) : 'All day', event.location ? esc(event.location) : ''].filter(Boolean).join(' · ');
  return `<div class="sched-row sched-event" style="--tone: var(${toneOfEvent(event)})">
    <span class="sched-time">${esc(time)}</span>
    <span class="sched-bar" aria-hidden="true"></span>
    <button type="button" class="sched-main" data-action="event-open" data-id="${esc(event.id)}"><span class="sched-title">${esc(event.title)}</span><span class="sched-sub">${sub}${zone}</span></button>
  </div>`;
}

function calItem(item) {
  return item.type === 'task' ? taskLine(item.task) : eventLine(item.event);
}

function drawCalendar() {
  const box = $('#sched-page');
  if (!box) return;
  const today = S.dayKey(new Date());
  const firstDay = S.weekStart(calLocale());
  const tasks = (cal.data && cal.data.tasks) || [];
  const events = (cal.data && cal.data.events) || [];
  const { grid } = calWindow();
  const strip = S.weekOf(S.fromKey(cal.selected), firstDay);
  const ahead7 = S.dayKey(S.addDays(new Date(), 7));
  const plan = S.plan(tasks, events, today, {
    from: [grid[0].key, strip[0], today].sort()[0],
    to: [grid[grid.length - 1].key, strip[6], ahead7].sort()[2],
  });
  const monthName = cal.month.toLocaleDateString(calLocale(), { month: 'long', year: 'numeric' });
  const names = grid.slice(0, 7).map((c) => S.fromKey(c.key).toLocaleDateString(calLocale(), { weekday: 'short' }));
  const dots = (key) => {
    const day = plan.byDay.get(key);
    if (!day) return '';
    const items = [...day.allDay, ...day.timed].filter((i) => i.type === 'event' || !i.task.done);
    if (!items.length) return '';
    const tones = [];
    for (const i of items) {
      const tone = i.type === 'event' ? toneOfEvent(i.event) : i.task.date < today ? '--hue-red' : toneOfTask(i.task);
      if (!tones.includes(tone)) tones.push(tone);
    }
    return `<span class="sched-dots" aria-label="${items.length} ${items.length === 1 ? 'item' : 'items'}">${tones.slice(0, 4).map((t) => `<i style="background: var(${t})"></i>`).join('')}</span>`;
  };
  const cell = (c) => `<button type="button" class="sched-day${c.inMonth ? '' : ' out'}${c.key === today ? ' today' : ''}${c.key === cal.selected ? ' on' : ''}" data-action="cal-pick" data-day="${c.key}" aria-label="${esc(S.fromKey(c.key).toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' }))}"${c.key === cal.selected ? ' aria-pressed="true"' : ''}><span class="sched-n">${c.day}</span>${dots(c.key)}</button>`;
  const week = strip.map((key) => {
    const d = S.fromKey(key);
    return `<button type="button" class="sched-day strip${key === today ? ' today' : ''}${key === cal.selected ? ' on' : ''}" data-action="cal-pick" data-day="${key}"><span class="sched-wd">${esc(d.toLocaleDateString(calLocale(), { weekday: 'narrow' }))}</span><span class="sched-n">${d.getDate()}</span>${dots(key)}</button>`;
  }).join('');
  const day = plan.byDay.get(cal.selected) || { allDay: [], timed: [] };
  const shown = (list) => list.filter((i) => cal.showDone || i.type === 'event' || !i.task.done);
  const allDay = shown(day.allDay);
  const timed = shown(day.timed);
  const taskCount = [...allDay, ...timed].filter((i) => i.type === 'task').length;
  const eventCount = allDay.length + timed.length - taskCount;
  const summary = [taskCount ? plural(taskCount, 'task') : '', eventCount ? plural(eventCount, 'event') : ''].filter(Boolean).join(' · ');
  const now = new Date();
  const nowMs = now.getTime();
  let nowShown = cal.selected !== today;
  const timedRows = timed.map((item) => {
    const at = item.type === 'event' ? new Date(item.event.start).getTime() : S.fromKey(item.task.date).getTime() + (() => {
      const [h, m] = String(item.task.time || '00:00').split(':').map(Number);
      return ((h || 0) * 60 + (m || 0)) * 60000;
    })();
    let line = '';
    if (!nowShown && at > nowMs) {
      nowShown = true;
      line = `<div class="sched-now" aria-label="Now"><span>${esc(now.toLocaleTimeString(calLocale(), { hour: 'numeric', minute: '2-digit' }))}</span></div>`;
    }
    return line + calItem(item);
  }).join('') + (nowShown || !timed.length ? '' : `<div class="sched-now" aria-label="Now"><span>${esc(now.toLocaleTimeString(calLocale(), { hour: 'numeric', minute: '2-digit' }))}</span></div>`);
  const ahead = [];
  for (let k = 1; k <= 7 && ahead.length < 6; k++) {
    const key = S.dayKey(S.addDays(new Date(), k));
    const later = plan.byDay.get(key);
    if (!later) continue;
    for (const item of [...later.allDay, ...later.timed]) {
      if (item.type === 'task' && item.task.done) continue;
      if (ahead.length >= 6) break;
      ahead.push({ key, item });
    }
  }
  const coming = ahead.length
    ? `<section class="sched-coming"><h2>Coming up</h2>${ahead.map(({ key, item }) => {
      const title = item.type === 'event' ? item.event.title : item.task.title;
      const tone = item.type === 'event' ? toneOfEvent(item.event) : toneOfTask(item.task);
      const time = item.type === 'event' ? (item.event.all_day ? '' : S.timeLabel(new Date(item.event.start).toTimeString().slice(0, 5), calLocale())) : (item.task.time ? S.timeLabel(item.task.time, calLocale()) : '');
      const d = S.fromKey(key);
      return `<button type="button" class="sched-ahead" data-action="cal-pick" data-day="${key}" style="--tone: var(${tone})"><span class="sched-ahead-day"><b>${esc(d.toLocaleDateString(calLocale(), { weekday: 'short' }))}</b>${d.getDate()}</span><span class="sched-ahead-text"><span class="sched-ahead-title">${esc(title)}</span>${time ? `<span class="sched-sub">${esc(time)}</span>` : ''}</span></button>`;
    }).join('')}</section>`
    : '';
  const agenda = allDay.length || timed.length
    ? `${allDay.length ? `<div class="sched-group">All day</div>${allDay.map(calItem).join('')}` : ''}${timed.length ? `${allDay.length ? '<div class="sched-group">Schedule</div>' : ''}${timedRows}` : ''}`
    : `<p class="sched-empty">Nothing scheduled</p><button type="button" class="btn plain sm" data-action="task-new" data-day="${cal.selected}">+ Task for this day</button>`;
  const overdue = plan.overdue.length ? `<section class="sched-list"><h2 class="sched-late">Overdue · ${plan.overdue.length}</h2>${plan.overdue.map((t) => taskLine(t, { showDate: true })).join('')}</section>` : '';
  const undated = plan.undated.length ? `<section class="sched-list"><h2>No date · ${plan.undated.length}</h2>${plan.undated.map((t) => taskLine(t, { showDate: true })).join('')}</section>` : '';
  const connect = cal.data && !cal.data.connected ? '<p class="set-note sched-connect">Show your Google Calendar here · <a href="#/settings">Connect it in Settings</a></p>' : '';
  const heading = S.fromKey(cal.selected).toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' });
  box.innerHTML = `<header class="sched-head">
      <h1>Calendar</h1>
      <div class="sched-status" role="status">${calStatus()}</div>
      <div class="sched-nav">
        <button type="button" class="btn plain sm" data-action="cal-today">Today</button>
        <button type="button" class="icon-btn sched-arrow" data-action="cal-prev" aria-label="Previous ${cal.gridOpen || window.matchMedia('(min-width: 600px)').matches ? 'month' : 'week'}">‹</button>
        <button type="button" class="sched-month" data-action="cal-month">${esc(monthName)}</button>
        <button type="button" class="icon-btn sched-arrow" data-action="cal-next" aria-label="Next ${cal.gridOpen || window.matchMedia('(min-width: 600px)').matches ? 'month' : 'week'}">›</button>
        <button type="button" class="btn primary sm" data-action="task-new">+ Task</button>
      </div>
    </header>
    ${connect}
    <div class="sched-body">
      <div class="sched-side">
        <section class="set-card sched-card${cal.gridOpen ? ' open' : ''}" aria-label="${esc(monthName)}">
          <div class="sched-grid" role="grid">${names.map((n) => `<span class="sched-wd">${esc(n)}</span>`).join('')}${grid.map(cell).join('')}</div>
        </section>
        <div class="sched-strip">${week}</div>
        ${coming}
        <div class="sched-lists">${overdue}${undated}</div>
      </div>
      <section class="sched-agenda" aria-label="${esc(heading)}">
        <div class="sched-agenda-head">
          <h2 class="sched-day-head">${esc(heading)}</h2>
          ${tasks.some((t) => t.done || (t.done_on || []).length) ? `<button type="button" class="link-btn sched-done-toggle" data-action="cal-done">${cal.showDone ? 'Hide completed' : 'Show completed'}</button>` : ''}
        </div>
        ${summary ? `<p class="sched-summary">${summary}</p>` : ''}
        ${agenda}
      </section>
      <div class="sched-lists phone">${overdue}${undated}</div>
    </div>`;
}

function calFind(id) {
  return ((cal.data && cal.data.tasks) || []).find((t) => t.id === id) || null;
}

function calMove(days) {
  const next = S.addDays(S.fromKey(cal.selected), days);
  cal.selected = S.dayKey(next);
  const month = monthOf(cal.selected);
  if (month.getTime() !== cal.month.getTime()) {
    cal.month = month;
    drawCalendar();
    loadSchedule(seq);
  } else drawCalendar();
}

function calShiftMonth(step) {
  const wide = window.matchMedia('(min-width: 600px)').matches || cal.gridOpen;
  if (!wide) return calMove(step * 7);
  const m = new Date(cal.month.getFullYear(), cal.month.getMonth() + step, 1);
  const keep = Math.min(S.fromKey(cal.selected).getDate(), new Date(m.getFullYear(), m.getMonth() + 1, 0).getDate());
  cal.month = m;
  cal.selected = S.dayKey(new Date(m.getFullYear(), m.getMonth(), keep));
  drawCalendar();
  loadSchedule(seq);
}

app.addEventListener('keydown', (e) => {
  const day = e.target.closest && e.target.closest('.sched-day');
  if (!day || state.view !== 'calendar') return;
  const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[e.key];
  if (!step) return;
  e.preventDefault();
  calMove(step);
  const again = app.querySelector(`.sched-day[data-day="${cal.selected}"]:not(.strip)`) || app.querySelector(`.sched-day[data-day="${cal.selected}"]`);
  if (again) again.focus();
});

function taskDoneOn(task, on) {
  return on ? (task.done_on || []).includes(on) : Boolean(task.done);
}

async function setTaskDone(id, done, on = null) {
  const task = calFind(id);
  try {
    await api(`/api/tasks/${enc(id)}`, { method: 'PATCH', body: on ? { done, on } : { done } });
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
    return loadSchedule(seq);
  }
  if (task && on) {
    task.done_on = (task.done_on || []).filter((d) => d !== on);
    if (done) task.done_on = [...task.done_on, on].sort();
  } else if (task) task.done = done;
  setTimeout(drawTasksWindow, done ? 1200 : 0);
  const rowOf = () => app.querySelector(`.sched-row[data-task="${CSS.escape(id)}"]${on ? `:has(.sched-check[data-on="${CSS.escape(on)}"])` : ''}`);
  const row = rowOf();
  if (row) row.classList.toggle('done', done);
  if (done) {
    toast('Done', { action: 'Undo', run: () => setTaskDone(id, false, on) });
    setTimeout(() => {
      const still = rowOf();
      if (still && !cal.showDone && still.classList.contains('done')) slideHeight(still, still.getBoundingClientRect().height, 0, () => drawCalendar());
    }, 1200);
  } else drawCalendar();
  if (state.view === 'note' && task && task.note && state.session && state.session.note.id === task.note.id && !saving.unsaved()) showNote(task.note.id);
}

function pickerBody(box, value, { meeting = false, onChange = () => {} } = {}) {
  const pick = { date: value.date || null, time: value.time ? value.time.slice(0, 5) : '', end: value.end ? value.end.slice(0, 5) : '', month: monthOf(value.date || S.dayKey(new Date())) };
  const today = new Date();
  const draw = () => {
    const grid = S.monthGrid(pick.month.getFullYear(), pick.month.getMonth(), S.weekStart(calLocale()));
    const names = grid.slice(0, 7).map((c) => S.fromKey(c.key).toLocaleDateString(calLocale(), { weekday: 'narrow' }));
    const chip = (attrs, label, on) => `<button type="button" class="chip pick-chip${on ? ' on' : ''}" ${attrs} aria-pressed="${on}">${esc(label)}</button>`;
    const quick = [...S.quickDates(today).map((q) => chip(`data-pick-day="${q.key}"`, q.label, pick.date === q.key)), chip('data-pick-day=""', 'No date', !pick.date)].join('');
    const times = [['', 'All day'], ['09:00', '9:00'], ['12:00', '12:00'], ['15:00', '15:00'], ['18:00', '18:00'], ['23:59', 'End of day']];
    box.innerHTML = `<div class="pick-quick">${quick}</div>
      <div class="pick-month"><button type="button" class="icon-btn" data-pick-month="-1" aria-label="Previous month">‹</button><span>${esc(pick.month.toLocaleDateString(calLocale(), { month: 'long', year: 'numeric' }))}</span><button type="button" class="icon-btn" data-pick-month="1" aria-label="Next month">›</button></div>
      <div class="sched-grid small">${names.map((n) => `<span class="sched-wd">${esc(n)}</span>`).join('')}${grid.map((c) => `<button type="button" class="sched-day${c.inMonth ? '' : ' out'}${c.key === S.dayKey(today) ? ' today' : ''}${c.key === pick.date ? ' on' : ''}" data-pick-day="${c.key}"><span class="sched-n">${c.day}</span></button>`).join('')}</div>
      ${pick.date ? `<div class="pick-times"><span class="set-label">Time</span>${times.map(([t, label]) => chip(`data-pick-time="${t}"`, label, pick.time === t)).join('')}<input class="set-input pick-time" type="time" value="${esc(pick.time)}" aria-label="Time">${meeting && pick.time ? `<label class="pick-end">Ends <input class="set-input pick-end-time" type="time" value="${esc(pick.end)}"></label>` : ''}</div>` : ''}`;
  };
  box.addEventListener('click', (e) => {
    const day = e.target.closest('[data-pick-day]');
    const time = e.target.closest('[data-pick-time]');
    const month = e.target.closest('[data-pick-month]');
    if (day) {
      pick.date = day.dataset.pickDay || null;
      if (pick.date) pick.month = monthOf(pick.date);
      if (!pick.date) pick.time = '';
    } else if (time) {
      pick.time = time.dataset.pickTime;
      if (!pick.time) pick.end = '';
    } else if (month) {
      pick.month = new Date(pick.month.getFullYear(), pick.month.getMonth() + Number(month.dataset.pickMonth), 1);
    } else return;
    draw();
    onChange(pick);
  });
  box.addEventListener('change', (e) => {
    if (e.target.classList.contains('pick-time')) pick.time = e.target.value;
    else if (e.target.classList.contains('pick-end-time')) pick.end = e.target.value;
    else return;
    draw();
    onChange(pick);
  });
  draw();
  return pick;
}

function dateSheet(id) {
  const task = calFind(id);
  if (!task) return;
  const box = sheet(`<h3>${task.date ? 'Change the date' : 'Set a date'}</h3><p class="hint">${esc(task.title)}</p><div class="picker-box" id="pick-box"></div>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="pick-save">Save</button></div>`);
  const pick = pickerBody($('#pick-box', box), task, { meeting: task.kind === 'meeting' });
  $('#pick-save', box).addEventListener('click', () => saveDate(task, pick, false));
}

async function saveDate(task, pick, confirm) {
  const body = { date: pick.date || null, time: pick.time || null, end: pick.end || null };
  if (confirm) body.confirm = true;
  try {
    await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body });
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (e.status === 409 && !confirm) {
      const preview = e.data && e.data.preview;
      if (preview) {
        const box = sheet(`<h3>Check the change</h3><p>leo could not find the old date in this task, so it adds the new one after it.</p>
          <div class="sched-preview"><span class="set-label">Before</span><p>${esc(preview.before)}</p><span class="set-label">After</span><p>${esc(preview.after)}</p></div>
          <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="pick-confirm">Change it</button></div>`);
        $('#pick-confirm', box).addEventListener('click', () => saveDate(task, pick, true));
        return;
      }
    }
    toast(e.message, { bad: true });
    return;
  }
  closeSheet();
  toast(pick.date ? 'Date saved' : 'Date cleared');
  loadTasksWindow().catch(() => {});
  if (state.view === 'calendar') {
    if (pick.date) {
      cal.selected = pick.date;
      cal.month = monthOf(pick.date);
    }
    await loadSchedule(seq);
  }
}

function newTaskSheet(day = null) {
  const box = sheet(`<h3>New task</h3>
    <input class="set-input task-title" id="task-title" maxlength="300" placeholder="What needs doing? Like Submit lab report" autocomplete="off">
    <div class="picker-box" id="pick-box"></div>
    <div class="set-row column task-repeat" id="task-repeat"><span class="set-label">Repeat</span>${repeatBubbles([])}<span class="hint repeat-hint">${esc(repeatHint([]))}</span></div>
    <details class="task-more source-more"><summary>${ICON.chevron}<span>Details</span></summary><div class="chips task-kinds" id="task-kinds">${KIND_ORDER.map((k) => `<button type="button" class="chip pick-chip${k === 'other' ? ' on' : ''}" data-kind="${k}" aria-pressed="${k === 'other'}">${esc(KIND_LABEL[k])}</button>`).join('')}</div></details>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="task-add">Add task</button></div>`);
  let kind = 'other';
  let repeat = [];
  const pick = pickerBody($('#pick-box', box), { date: day }, { meeting: false });
  repeatPicker($('#task-repeat', box), [], (days) => { repeat = days; });
  $('#task-kinds', box).addEventListener('click', (e) => {
    const chip = e.target.closest('[data-kind]');
    if (!chip) return;
    kind = chip.dataset.kind;
    for (const c of box.querySelectorAll('[data-kind]')) {
      c.classList.toggle('on', c === chip);
      c.setAttribute('aria-pressed', String(c === chip));
    }
  });
  const title = $('#task-title', box);
  const add = $('#task-add', box);
  const save = async () => {
    const text = title.value.trim();
    if (!text) {
      title.focus();
      return toast('Give the task a title.', { bad: true });
    }
    if (add.disabled) return;
    add.disabled = true;
    try {
      const made = await api('/api/tasks', { method: 'POST', body: { title: text, date: pick.date, time: pick.time || null, kind, repeat } });
      closeSheet();
      toast(`Added “${made.title}”`);
      loadTasksWindow().catch(() => {});
      if (state.view === 'calendar') {
        if (made.date && !S.repeats(made)) {
          cal.selected = made.date;
          cal.month = monthOf(made.date);
        }
        await loadSchedule(seq);
      }
    } catch (e) {
      add.disabled = false;
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  };
  add.addEventListener('click', save);
  title.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      save();
    }
  });
  title.focus();
}

function taskSheet(id, on = null) {
  const task = calFind(id);
  if (!task) return;
  const repeating = S.repeats(task);
  const day = repeating ? on || (state.view === 'calendar' ? cal.selected : S.dayKey(new Date())) : null;
  const doneNow = taskDoneOn(task, day);
  const dayName = day ? S.fromKey(day).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' }) : '';
  const onGoogle = (cal.data && (cal.data.events || []).find((e) => e.task === task.id)) || null;
  const google = onGoogle ? 'On Google Calendar' : task.sent_at ? 'Opened in Google Calendar' : 'Not added';
  const when = task.date
    ? `${S.fromKey(task.date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' })}${task.time ? ` · ${S.timeLabel(task.time, calLocale())}` : ''}`
    : 'No date';
  const moved = onGoogle && task.date && !onGoogle.all_day && S.dayKey(new Date(onGoogle.start)) !== task.date && task.time;
  const box = sheet(`<h3 class="task-sheet-title">${esc(task.title)}</h3>
    ${task.found === false ? '<p class="set-note warn">leo could not find this checkbox in the note any more. Open the note to check it.</p>' : ''}
    <div class="set-row"><span class="set-label">${repeating ? 'Starts' : 'Date'}</span><span class="set-value">${esc(when)}<button type="button" class="btn plain sm" data-action="task-date" data-id="${esc(task.id)}">${task.date ? 'Change' : 'Set date'}</button></span></div>
    ${task.source === 'leo' ? `<div class="set-row column task-repeat" id="task-repeat"><span class="set-label">Repeat</span>${repeatBubbles(task.repeat || [])}<span class="hint repeat-hint">${esc(repeatHint(task.repeat || []))}</span></div>` : ''}
    <div class="set-row column"><span class="set-label">Type</span><div class="chips task-kinds" id="task-kinds">${KIND_ORDER.map((k) => `<button type="button" class="chip pick-chip${task.kind === k ? ' on' : ''}" data-kind="${k}" aria-pressed="${task.kind === k}">${esc(KIND_LABEL[k])}</button>`).join('')}</div></div>
    <div class="set-row"><span class="set-label">Note</span><span class="set-value">${task.note ? `<a href="${noteHash(task.note.id)}">${esc(task.note.title || 'Untitled')}</a>` : 'Added in Leo'}</span></div>
    <div class="set-row"><span class="set-label">Google</span><span class="set-value">${esc(google)}</span></div>
    ${moved ? `<p class="set-note">The Google event is on another day. <a href="${esc(S.googleDay(S.dayKey(new Date(onGoogle.start))))}" target="_blank" rel="noopener">Open that day in Google Calendar</a></p>` : ''}
    <div class="buttons task-acts">
      ${task.date && !onGoogle ? `<button type="button" class="btn primary" data-action="task-google" data-id="${esc(task.id)}">${ICON.calendar}<span>Add to Google Calendar</span></button>` : ''}
      ${!task.date ? `<button type="button" class="btn primary" data-action="task-date" data-id="${esc(task.id)}" data-then="google">Set a date to add it to Google Calendar</button>` : ''}
      ${task.date ? `<a class="btn plain" href="/api/tasks/${enc(task.id)}/ics" download>Download .ics</a>` : ''}
      <button type="button" class="btn plain" data-action="task-done" data-id="${esc(task.id)}"${day ? ` data-on="${esc(day)}"` : ''} data-set="${doneNow ? '0' : '1'}">${doneNow ? (day ? `Not done on ${esc(dayName)}` : 'Not done') : (day ? `Done for ${esc(dayName)}` : 'Done')}</button>
      ${task.source === 'leo' ? `<button type="button" class="btn plain danger" data-action="task-delete" data-id="${esc(task.id)}">Delete</button>` : task.promoted ? `<button type="button" class="btn plain" data-action="task-delete" data-id="${esc(task.id)}">Stop tracking</button>` : ''}
    </div>`);
  const repeatBox = $('#task-repeat', box);
  if (repeatBox) {
    repeatPicker(repeatBox, task.repeat || [], async (days) => {
      try {
        const saved = await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body: { repeat: days } });
        Object.assign(task, saved && saved.id ? saved : { repeat: days });
        if (state.view === 'calendar') drawCalendar();
        drawTasksWindow();
      } catch (err) {
        if (err instanceof Locked) throw err;
        toast(err.message, { bad: true });
      }
    });
  }
  $('#task-kinds', box).addEventListener('click', async (e) => {
    const chip = e.target.closest('[data-kind]');
    if (!chip || chip.classList.contains('on')) return;
    try {
      await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body: { kind: chip.dataset.kind } });
      task.kind = chip.dataset.kind;
      for (const c of box.querySelectorAll('[data-kind]')) {
        c.classList.toggle('on', c === chip);
        c.setAttribute('aria-pressed', String(c === chip));
      }
      if (state.view === 'calendar') drawCalendar();
    } catch (err) {
      if (err instanceof Locked) throw err;
      toast(err.message, { bad: true });
    }
  });
}

async function addToGoogle(id) {
  const task = calFind(id);
  if (!task) return;
  if (!task.date) return dateSheet(id);
  const link = S.googleLink(task, calZone());
  window.open(link, '_blank', 'noopener');
  try {
    await api(`/api/tasks/${enc(id)}`, { method: 'PATCH', body: { sent: true } });
    task.sent_at = new Date().toISOString();
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  closeSheet();
  toast('Opened in Google Calendar. Press Save there; it shows here as one item once Google updates the calendar link.');
  if (state.view === 'calendar') drawCalendar();
}

async function deleteTask(id) {
  const task = calFind(id);
  try {
    const gone = await api(`/api/tasks/${enc(id)}`, { method: 'DELETE' });
    closeSheet();
    if (gone && task && task.source === 'leo') {
      toast(`Deleted “${gone.title}”`, {
        action: 'Undo',
        run: async () => {
          await api('/api/tasks/import', { method: 'POST', body: { tasks: [gone] } });
          await loadSchedule(seq);
        },
      });
    } else toast('No longer tracked as a task. The checkbox stays in the note.');
    await loadSchedule(seq);
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
}

function eventSheet(id) {
  const event = ((cal.data && cal.data.events) || []).find((e) => e.id === id);
  if (!event) return;
  const when = event.all_day
    ? S.fromKey(event.date).toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' })
    : `${new Date(event.start).toLocaleString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })} – ${new Date(event.end).toLocaleTimeString(calLocale(), { hour: 'numeric', minute: '2-digit' })}`;
  const soon = !event.all_day && new Date(event.start) - Date.now() < 15 * 60000 && new Date(event.end) > Date.now();
  const about = String(event.context || '').split('\n').slice(1).join('\n').trim();
  sheet(`<h3>${esc(event.title)}</h3>
    <div class="set-row"><span class="set-label">When</span><span class="set-value">${esc(when)}</span></div>
    ${event.location ? `<div class="set-row"><span class="set-label">Where</span><span class="set-value">${esc(event.location)}</span></div>` : ''}
    <div class="set-row"><span class="set-label">Calendar</span><span class="set-value">${esc(event.calendar || 'Google')}</span></div>
    ${about ? `<p class="sched-about">${esc(about.slice(0, 600))}</p>` : ''}
    ${event.zone_unknown ? '<p class="set-note warn">This event names a time zone leo does not know, so its time may be off.</p>' : ''}
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Close</button>${soon ? '<button type="button" class="btn primary" data-action="record">Record this meeting</button>' : ''}</div>`);
}

async function noteTasksSheet() {
  const s = state.view === 'note' ? state.session : null;
  if (!s || !s.note.id) return;
  const ready = await ready_();
  if (!ready) return;
  let listed = [];
  try {
    listed = (await api('/api/tasks')).tasks.filter((t) => t.note && t.note.id === s.note.id);
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  const body = s.doc ? s.doc.source() : '';
  const lines = body.split('\n');
  const boxes = lines
    .map((line, n) => ({ n, m: line.match(/^\s*(?:[-*+]|\d+[.)])\s{1,4}\[([ xX])\]\s(.+)$/) }))
    .filter((b) => b.m && b.m[2].trim());
  const rows = boxes.slice(0, 200).map((b) => {
    const tracked = listed.find((t) => t.line === b.n);
    return `<div class="set-row"><span class="grow">${b.m[1] !== ' ' ? '<s>' : ''}${esc(b.m[2].trim())}${b.m[1] !== ' ' ? '</s>' : ''}</span>${tracked ? `<button type="button" class="btn plain sm" data-action="task-open-here" data-id="${esc(tracked.id)}">In your tasks</button>` : `<button type="button" class="btn sm" data-action="task-promote" data-line="${b.n}">Add to tasks</button>`}</div>`;
  }).join('');
  sheet(`<h3>Tasks in this note</h3>
    <p class="hint">Checkboxes under an Action items heading are tasks already. Add any other checkbox to show it on the Calendar.</p>
    ${rows || '<p class="hint">This note has no checkboxes yet.</p>'}
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Close</button><button type="button" class="btn primary" data-action="calendar">Open the Calendar</button></div>`);
}

async function ready_() {
  try {
    return await ready();
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
    return null;
  }
}

async function promoteTask(line) {
  const s = state.view === 'note' ? state.session : null;
  if (!s || !s.note.id) return;
  try {
    await api('/api/tasks/promote', { method: 'POST', body: { note: s.note.id, line: Number(line) } });
    toast('Added to your tasks', { action: 'Calendar', run: () => go('#/calendar') });
    await noteTasksSheet();
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.status === 400 ? 'That line is not a checkbox any more.' : e.message, { bad: true });
  }
}

async function openTaskAnywhere(id, on = null) {
  if (!cal.data || !calFind(id)) {
    try {
      cal.data = { ...(cal.data || {}), tasks: (await api('/api/tasks')).tasks, events: (cal.data && cal.data.events) || [] };
    } catch (e) {
      if (e instanceof Locked) throw e;
      return;
    }
  }
  taskSheet(id, on);
}

var tasksWin = { folded: false, practice: '', practiceFolded: true, timer: 0, tasks: null };
const TASKS_FOLDED = 'leo-tasks-folded';
const TASKS_AHEAD_DAYS = 7;

function dock() {
  let box = document.getElementById('dock');
  if (!box) {
    box = document.createElement('div');
    box.id = 'dock';
    box.className = 'dock';
    document.body.appendChild(box);
  }
  return box;
}

const tasksBox = document.createElement('aside');
tasksBox.className = 'tasks-window';
tasksBox.id = 'tasks-window';
tasksBox.hidden = true;
tasksBox.setAttribute('aria-label', 'Tasks');
dock().prepend(tasksBox);

try {
  tasksWin.folded = localStorage.getItem(TASKS_FOLDED) === '1';
} catch (e) {
  tasksWin.folded = false;
}

function tasksGroups(tasks) {
  const today = S.dayKey(new Date());
  const ahead = S.dayKey(S.addDays(new Date(), TASKS_AHEAD_DAYS));
  const groups = [['Overdue', []], ['Today', []], ['Coming up', []], ['No date', []]];
  for (const task of tasks) {
    const t = S.repeats(task) ? S.nextOpen(task, today, ahead) : task;
    if (!t || t.done) continue;
    if (!t.date) groups[3][1].push(t);
    else if (t.date < today) groups[0][1].push(t);
    else if (t.date === today) groups[1][1].push(t);
    else if (t.date <= ahead) groups[2][1].push(t);
  }
  return groups.filter(([, list]) => list.length);
}

function tasksDue() {
  const tasks = tasksWin.tasks || (cal.data && cal.data.tasks) || [];
  const groups = new Map(tasksGroups(tasks));
  const late = (groups.get('Overdue') || []).length;
  return { late, due: late + (groups.get('Today') || []).length };
}

function tasksDueBadge() {
  const { late, due } = tasksDue();
  if (!due) return '';
  return `<span class="due-badge${late ? ' late' : ''}" aria-label="${esc(late ? `${due} due, ${late} overdue` : `${due} due today`)}">${due} due</span>`;
}

function drawTasksWindow() {
  const practice = /^#\/interviews\/([a-zA-Z0-9-]{8,64})$/.exec(location.hash)?.[1] || '';
  if (practice !== tasksWin.practice) { tasksWin.practice = practice; tasksWin.practiceFolded = true; }
  const folded = practice ? tasksWin.practiceFolded : tasksWin.folded;
  const tasks = tasksWin.tasks || (cal.data && cal.data.tasks) || [];
  const groups = tasksGroups(tasks);
  const open = groups.reduce((n, [, list]) => n + list.length, 0);
  const here = /^#\/calendar/.test(location.hash);
  tasksBox.hidden = !open || here || state.view === 'locked';
  if (tasksBox.hidden) return;
  const late = groups.find(([name]) => name === 'Overdue');
  const row = (t) => `<div class="tasks-row" data-task="${esc(t.id)}" style="--tone: var(${toneOfTask(t)})">
    <input type="checkbox" class="sched-check" data-action="task-done" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''} aria-label="Done: ${esc(t.title)}">
    <button type="button" class="tasks-title" data-action="task-open-here" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''}>${esc(t.title)}</button>
    ${t.date ? `<span class="tasks-when">${esc(t.date === S.dayKey(new Date()) ? (t.time ? S.timeLabel(t.time, calLocale()) : 'Today') : S.fromKey(t.date).toLocaleDateString(calLocale(), { month: 'short', day: 'numeric' }))}</span>` : `<button type="button" class="btn plain sm" data-action="task-date" data-id="${esc(t.id)}">Set date</button>`}
  </div>`;
  const label = late ? `${late[1].length} overdue · ${open} to do` : `${open} to do`;
  tasksBox.classList.toggle('folded', folded);
  tasksBox.innerHTML = `<div class="tasks-head">
      <button type="button" class="tasks-toggle" data-action="tasks-fold" aria-expanded="${!folded}"><span class="tasks-icon${late ? ' late' : ''}">${ICON.check}</span><span class="grow">Tasks <span class="tasks-count">${esc(label)}</span></span><span class="activity-fold">${folded ? 'Show' : 'Hide'}</span></button>
      <button type="button" class="icon-btn tasks-new" data-action="task-new" aria-label="New task" title="New task">${ICON.plus}</button>
    </div>
    ${folded ? '' : `<div class="tasks-body">${groups.map(([name, list]) => `<div class="tasks-group${name === 'Overdue' ? ' late' : ''}">${name}</div>${list.slice(0, name === 'No date' ? 5 : 20).map(row).join('')}`).join('')}</div>
    <div class="tasks-foot"><button type="button" class="link-btn" data-action="calendar">Open calendar</button></div>`}`;
}

async function loadTasksWindow() {
  clearTimeout(tasksWin.timer);
  if (state.view === 'locked') return;
  try {
    tasksWin.tasks = (await api('/api/tasks')).tasks;
    if (cal.data) cal.data = { ...cal.data, tasks: tasksWin.tasks };
    else cal.data = { events: [], tasks: tasksWin.tasks };
  } catch (e) {
    if (e instanceof Locked) return;
  }
  drawTasksWindow();
  tasksWin.timer = setTimeout(() => {
    if (document.visibilityState === 'visible') loadTasksWindow().catch(() => {});
    else tasksWin.timer = setTimeout(() => loadTasksWindow().catch(() => {}), CAL_POLL_MS);
  }, CAL_POLL_MS);
}

setTimeout(() => loadTasksWindow().catch(() => {}), 0);
window.addEventListener('hashchange', () => drawTasksWindow());

const source = { note: null, name: null };
const SOURCE_KEY = 'leo-source-width';

const sourcePane = document.createElement('aside');
sourcePane.className = 'source-pane';
sourcePane.id = 'source-pane';
sourcePane.hidden = true;
sourcePane.setAttribute('aria-label', 'Original file');
document.body.appendChild(sourcePane);

function sourceWidth() {
  let kept = 0;
  try {
    kept = Number(localStorage.getItem(SOURCE_KEY)) || 0;
  } catch (e) {
    kept = 0;
  }
  return kept || Math.round(Math.min(640, Math.max(420, window.innerWidth * 0.38)));
}

function sourceFits() {
  if (source.note) return true;
  const style = getComputedStyle(document.body);
  const side = parseFloat(style.getPropertyValue('--side')) || 0;
  const right = document.body.classList.contains('chat-open') ? parseFloat(style.getPropertyValue('--right')) || 0 : 0;
  return window.innerWidth - side - right >= sourceWidth() + 640;
}

function closeSource() {
  if (!source.note) return;
  source.note = null;
  source.name = null;
  sourcePane.hidden = true;
  sourcePane.innerHTML = '';
  document.body.classList.remove('with-source');
}

async function openSource(id, name, page = null) {
  const kind = viewKind(name);
  if (!kind) return;
  source.note = id;
  source.name = name;
  document.body.style.setProperty('--source-w', `${sourceWidth()}px`);
  document.body.classList.add('with-source');
  const url = `/api/notes/${enc(id)}/originals/${enc(name)}`;
  let files = [];
  try {
    files = (await api(`/api/notes/${enc(id)}/originals`)).filter((f) => viewKind(f.name));
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  if (source.note !== id || source.name !== name) return;
  const picker = files.length > 1
    ? `<span class="source-files">${files.slice(0, 12).map((f) => `<button type="button" class="chip${f.name === name ? ' accent' : ''}" data-action="source-file" data-id="${esc(id)}" data-name="${esc(f.name)}">${esc(f.name)}</button>`).join('')}</span>`
    : `<b class="source-name" title="${esc(name)}">${esc(name)}</b>`;
  const body = kind === 'pdf'
    ? `<iframe class="source-frame" src="${url}?view=1${page ? `#page=${Number(page)}` : ''}" title="${esc(name)}"></iframe>`
    : kind === 'image'
      ? `<div class="source-scroll"><img class="source-img" src="${url}?view=1" alt="${esc(name)}"></div>`
      : '<div class="source-scroll prose source-text">Loading…</div>';
  sourcePane.hidden = false;
  sourcePane.innerHTML = `<div class="source-grip" role="separator" aria-orientation="vertical" aria-label="Drag to resize the original" tabindex="0"></div>
    <header class="source-head">${picker}<span class="source-tools"><a class="btn sm plain" href="${url}" download="${esc(name)}">Download</a><button type="button" class="icon-btn" data-action="source-close" aria-label="Close the original">${ICON.close}</button></span></header>
    <div class="source-body">${body}</div>`;
  if (kind === 'text') {
    const box = sourcePane.querySelector('.source-text');
    try {
      const response = await fetch(url, { credentials: 'same-origin' });
      if (!response.ok) throw new Error();
      const text = await response.text();
      if (!box.isConnected) return;
      box.innerHTML = /\.md$/i.test(name) ? md.render(text) : `<pre>${esc(text)}</pre>`;
    } catch (e) {
      box.textContent = 'This file could not be shown. Download it instead.';
    }
  }
}

sourcePane.addEventListener('pointerdown', (e) => {
  const grip = e.target.closest('.source-grip');
  if (!grip) return;
  e.preventDefault();
  const startX = e.clientX;
  const start = sourceWidth();
  document.body.classList.add('source-resizing');
  const move = (ev) => {
    const width = Math.round(Math.min(window.innerWidth - 600, Math.max(360, start + startX - ev.clientX)));
    document.body.style.setProperty('--source-w', `${width}px`);
    try {
      localStorage.setItem(SOURCE_KEY, String(width));
    } catch (err) {
      return;
    }
  };
  const up = () => {
    document.body.classList.remove('source-resizing');
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
});
