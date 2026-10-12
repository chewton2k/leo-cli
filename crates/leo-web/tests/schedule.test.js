const assert = require('node:assert/strict');
const S = require('../src/web/schedule.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

test('a month grid starts on the chosen weekday and always has six weeks', () => {
  const grid = S.monthGrid(2026, 9, 0);
  assert.equal(grid.length, 42);
  assert.equal(grid[0].key, '2026-09-27');
  assert.equal(grid.find((c) => c.inMonth).key, '2026-10-01');
  assert.equal(S.monthGrid(2026, 9, 1)[0].key, '2026-09-28');
  assert.deepEqual(S.weekOf(new Date(2026, 9, 15), 1), ['2026-10-12', '2026-10-13', '2026-10-14', '2026-10-15', '2026-10-16', '2026-10-17', '2026-10-18']);
});

test('tasks and events land on their days; overdue and undated are grouped; a linked event shows once', () => {
  const tasks = [
    { id: 'a', title: 'Submit lab', date: '2026-10-13', time: null, done: false },
    { id: 'b', title: 'Exam', date: '2026-10-15', time: '09:00:00', done: false },
    { id: 'c', title: 'Ask tutor', date: null, done: false },
    { id: 'd', title: 'Old', date: '2026-10-01', done: true },
    { id: 'e', title: 'Sent', date: '2026-10-16', time: null, done: false },
  ];
  const events = [
    { id: 'x', title: 'Trip', all_day: true, date: '2026-10-15', end_date: '2026-10-17', start: '2026-10-15T07:00:00Z' },
    { id: 'y', title: 'Meeting', start: '2026-10-15T17:00:00Z', end: '2026-10-15T18:00:00Z' },
    { id: 'z', title: 'Due: Sent', task: 'e', start: '2026-10-16T06:30:00Z' },
  ];
  const p = S.plan(tasks, events, '2026-10-14');
  assert.deepEqual(p.overdue.map((t) => t.id), ['a']);
  assert.deepEqual(p.undated.map((t) => t.id), ['c']);
  const day15 = p.byDay.get('2026-10-15');
  assert.deepEqual(day15.allDay.map((i) => i.event.title), ['Trip']);
  assert.equal(day15.timed.length, 2);
  assert.deepEqual(p.byDay.get('2026-10-16').allDay.map((i) => i.event ? i.event.title : i.task.title), ['Sent', 'Trip']);
  assert.ok(![...p.byDay.values()].some((d) => d.timed.some((i) => i.event && i.event.id === 'z')), 'the Google copy of a task is not shown twice');
  assert.equal(p.byDay.get('2026-10-17'), undefined, 'an all-day end date is exclusive');
});

test('quick dates are today, tomorrow, the coming Friday and next Monday', () => {
  const thursday = new Date(2026, 9, 15);
  assert.deepEqual(S.quickDates(thursday).map((q) => q.key), ['2026-10-15', '2026-10-16', '2026-10-16', '2026-10-19']);
  const friday = new Date(2026, 9, 16);
  assert.deepEqual(S.quickDates(friday).slice(2).map((q) => [q.label, q.key]), [['Next Fri', '2026-10-23'], ['Next week', '2026-10-19']]);
});

test('a Google Calendar link is prefilled for all-day, deadline and meeting tasks', () => {
  const read = (url) => Object.fromEntries(new URL(url).searchParams);
  const allDay = read(S.googleLink({ id: 'abcdef1234567890', title: 'Biology final', date: '2026-12-16', kind: 'exam' }, 'America/Los_Angeles'));
  assert.equal(allDay.dates, '20261216/20261217');
  assert.equal(allDay.ctz, 'America/Los_Angeles');
  assert.equal(allDay.details, 'From leo · leo-task:abcdef123456');
  const due = read(S.googleLink({ id: 't1', title: 'Submit lab', date: '2026-10-16', time: '00:10:00', kind: 'assignment', note: { title: 'Chem' } }));
  assert.equal(due.text, 'Due: Submit lab');
  assert.equal(due.dates, '20261015T234000/20261016T001000', 'a deadline is the half hour before it, even across midnight');
  assert.equal(due.details, 'From leo · Chem · leo-task:t1');
  const meet = read(S.googleLink({ id: 't2', title: 'Project meeting', date: '2026-10-16', time: '10:00:00', end: '11:30:00', kind: 'meeting' }));
  assert.deepEqual([meet.text, meet.dates], ['Project meeting', '20261016T100000/20261016T113000']);
  assert.equal(S.googleLink({ id: 't3', title: 'x', date: null }), null);
  assert.equal(S.googleDay('2026-10-08'), 'https://calendar.google.com/calendar/r/day/2026/10/8');
});

let failed = 0;
for (const [name, fn] of cases) {
  try {
    fn();
    console.log(`ok   ${name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`);
  }
}
console.log(`${cases.length - failed} passed, ${failed} failed`);
if (failed) process.exit(1);
