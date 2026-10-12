(function (root) {
  'use strict';

  const pad = (n) => String(n).padStart(2, '0');

  function dayKey(date) {
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  function fromKey(key) {
    const [y, m, d] = String(key).split('-').map(Number);
    return new Date(y, (m || 1) - 1, d || 1);
  }

  function addDays(date, n) {
    const next = new Date(date.getFullYear(), date.getMonth(), date.getDate() + n);
    return next;
  }

  function weekStart(locale) {
    try {
      const info = new Intl.Locale(locale || 'en-US');
      const week = info.getWeekInfo ? info.getWeekInfo() : info.weekInfo;
      if (week && Number.isInteger(week.firstDay)) return week.firstDay % 7;
    } catch (e) {
      return 0;
    }
    return 0;
  }

  function monthGrid(year, month, firstDay = 0) {
    const first = new Date(year, month, 1);
    const lead = (first.getDay() - firstDay + 7) % 7;
    const start = addDays(first, -lead);
    return Array.from({ length: 42 }, (_, i) => {
      const date = addDays(start, i);
      return { key: dayKey(date), day: date.getDate(), inMonth: date.getMonth() === month };
    });
  }

  function weekOf(date, firstDay = 0) {
    const lead = (date.getDay() - firstDay + 7) % 7;
    const start = addDays(date, -lead);
    return Array.from({ length: 7 }, (_, i) => dayKey(addDays(start, i)));
  }

  function eventDays(event) {
    if (event.all_day && event.date) {
      const out = [];
      const last = event.end_date ? fromKey(event.end_date) : addDays(fromKey(event.date), 1);
      for (let d = fromKey(event.date); d < last && out.length < 366; d = addDays(d, 1)) out.push(dayKey(d));
      return out;
    }
    const start = new Date(event.start);
    return Number.isNaN(start.getTime()) ? [] : [dayKey(start)];
  }

  const BYDAY = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];

  function weekdayOf(key) {
    return (fromKey(key).getDay() + 6) % 7;
  }

  function repeats(task) {
    return Array.isArray(task.repeat) && task.repeat.length > 0;
  }

  function repeatsOn(task, key) {
    return repeats(task) && task.repeat.includes(weekdayOf(key)) && (!task.date || key >= task.date);
  }

  function occurrence(task, key) {
    return { ...task, date: key, done: (task.done_on || []).includes(key), on: key };
  }

  function occurrences(task, from, to) {
    const out = [];
    if (!repeats(task)) return out;
    for (let d = fromKey(from); dayKey(d) <= to && out.length < 400; d = addDays(d, 1)) {
      const key = dayKey(d);
      if (repeatsOn(task, key)) out.push(occurrence(task, key));
    }
    return out;
  }

  function nextOpen(task, from, to) {
    return occurrences(task, from, to).find((o) => !o.done) || null;
  }

  function repeatLabel(repeat, locale) {
    const days = [...new Set(repeat || [])].filter((d) => d >= 0 && d < 7).sort((a, b) => a - b);
    if (!days.length) return '';
    if (days.length === 7) return 'Every day';
    if (days.join() === '0,1,2,3,4') return 'Every weekday';
    if (days.join() === '5,6') return 'Every weekend';
    const name = (d) => new Date(2024, 0, 1 + d).toLocaleDateString(locale || undefined, { weekday: 'short' });
    return `Every ${days.map(name).join(', ')}`;
  }

  function plan(tasks, events, today, range) {
    const todayKey = typeof today === 'string' ? today : dayKey(today);
    const from = (range && range.from) || dayKey(addDays(fromKey(todayKey), -42));
    const to = (range && range.to) || dayKey(addDays(fromKey(todayKey), 42));
    const byDay = new Map();
    const slot = (key) => {
      if (!byDay.has(key)) byDay.set(key, { allDay: [], timed: [] });
      return byDay.get(key);
    };
    const overdue = [];
    const undated = [];
    const linked = new Set(events.map((e) => e.task).filter(Boolean));
    for (const task of tasks) {
      if (repeats(task)) {
        for (const one of occurrences(task, from, to)) {
          const item = { type: 'task', task: one, time: one.time || null };
          if (one.time) slot(one.date).timed.push(item);
          else slot(one.date).allDay.push(item);
        }
        continue;
      }
      if (!task.date) {
        if (!task.done) undated.push(task);
        continue;
      }
      if (!task.done && task.date < todayKey) overdue.push(task);
      const item = { type: 'task', task, time: task.time || null };
      if (task.time) slot(task.date).timed.push(item);
      else slot(task.date).allDay.push(item);
    }
    for (const event of events) {
      if (event.task && tasks.some((t) => t.id === event.task)) continue;
      for (const key of eventDays(event)) {
        const item = { type: 'event', event, time: event.all_day ? null : event.start };
        if (event.all_day) slot(key).allDay.push(item);
        else slot(key).timed.push(item);
      }
    }
    for (const day of byDay.values()) {
      day.timed.sort((a, b) => sortTime(a) - sortTime(b));
    }
    return { byDay, overdue, undated, linked };
  }

  function sortTime(item) {
    if (item.type === 'event') return new Date(item.event.start).getTime();
    const [h, m] = String(item.task.time || '00:00').split(':').map(Number);
    return fromKey(item.task.date).getTime() + ((h || 0) * 60 + (m || 0)) * 60000;
  }

  function nextWeekday(date, weekday) {
    const ahead = (weekday - date.getDay() + 7) % 7 || 7;
    return addDays(date, ahead);
  }

  function quickDates(today) {
    return [
      { label: 'Today', key: dayKey(today) },
      { label: 'Tomorrow', key: dayKey(addDays(today, 1)) },
      { label: today.getDay() === 5 ? 'Next Fri' : 'Fri', key: dayKey(nextWeekday(today, 5)) },
      { label: 'Next week', key: dayKey(nextWeekday(today, 1)) },
    ];
  }

  function stamp(key, time) {
    const [y, m, d] = String(key).split('-');
    if (!time) return `${y}${m}${d}`;
    const [h, min] = String(time).split(':');
    return `${y}${m}${d}T${pad(Number(h))}${pad(Number(min))}00`;
  }

  function shifted(key, time, minutes) {
    const [h, m] = String(time).split(':').map(Number);
    const at = fromKey(key);
    at.setHours(h || 0, (m || 0) + minutes, 0, 0);
    return { key: dayKey(at), time: `${pad(at.getHours())}:${pad(at.getMinutes())}` };
  }

  function firstDay(task) {
    if (!repeats(task)) return task.date || null;
    const start = task.date || dayKey(new Date());
    const found = occurrences({ ...task, date: start }, start, dayKey(addDays(fromKey(start), 7)));
    return found.length ? found[0].date : start;
  }

  function rrule(repeat) {
    const days = [...new Set(repeat || [])].filter((d) => d >= 0 && d < 7).sort((a, b) => a - b);
    return days.length ? `RRULE:FREQ=WEEKLY;BYDAY=${days.map((d) => BYDAY[d]).join(',')}` : '';
  }

  function googleLink(task, zone) {
    if (task && repeats(task)) task = { ...task, date: firstDay(task) };
    if (!task || !task.date) return null;
    let dates;
    let title = task.title;
    if (!task.time) {
      dates = `${stamp(task.date)}/${stamp(dayKey(addDays(fromKey(task.date), 1)))}`;
    } else if (task.end) {
      dates = `${stamp(task.date, task.time)}/${stamp(task.date, task.end)}`;
    } else {
      const start = shifted(task.date, task.time, -30);
      dates = `${stamp(start.key, start.time)}/${stamp(task.date, task.time)}`;
      if (task.kind !== 'meeting') title = `Due: ${task.title}`;
    }
    const from = task.note && task.note.title ? ` · ${task.note.title}` : '';
    const params = new URLSearchParams({ action: 'TEMPLATE', text: title, dates, details: `From leo${from} · leo-task:${String(task.id).slice(0, 12)}` });
    if (zone) params.set('ctz', zone);
    if (repeats(task)) params.set('recur', rrule(task.repeat));
    return `https://calendar.google.com/calendar/render?${params.toString()}`;
  }

  function googleDay(key) {
    const [y, m, d] = String(key).split('-').map(Number);
    return `https://calendar.google.com/calendar/r/day/${y}/${m}/${d}`;
  }

  function timeLabel(time, locale) {
    if (!time) return '';
    const [h, m] = String(time).split(':').map(Number);
    const at = new Date(2000, 0, 1, h || 0, m || 0);
    return at.toLocaleTimeString(locale || undefined, { hour: 'numeric', minute: '2-digit' });
  }

  const api = { dayKey, fromKey, addDays, weekStart, monthGrid, weekOf, eventDays, plan, quickDates, weekdayOf, repeats, repeatsOn, occurrences, nextOpen, repeatLabel, rrule, googleLink, googleDay, timeLabel, nextWeekday };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoSchedule = api;
})(typeof window !== 'undefined' ? window : globalThis);
