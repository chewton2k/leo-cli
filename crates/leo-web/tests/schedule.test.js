const assert = require('node:assert/strict');
const S = require('../src/web/schedule.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

test('Quick Leo reads a reminder, a question or a note from one line', () => {
  const now = new Date(2026, 9, 19, 10, 0);
  assert.deepEqual(S.quickParse('remind me to email Bob tomorrow at 3pm', now), { kind: 'task', title: 'email Bob', date: '2026-10-20', time: '15:00' });
  assert.deepEqual(S.quickParse('Pay rent on Friday', now), { kind: 'task', title: 'Pay rent', date: '2026-10-23', time: null });
  assert.deepEqual(S.quickParse('todo: submit form next week', now), { kind: 'task', title: 'submit form', date: '2026-10-26', time: null });
  assert.deepEqual(S.quickParse('call mom tonight', now), { kind: 'task', title: 'call mom', date: '2026-10-19', time: '20:00' });
  assert.deepEqual(S.quickParse('standup 9:30', now), { kind: 'task', title: 'standup', date: '2026-10-19', time: '09:30' });
  assert.deepEqual(S.quickParse('dentist in 3 days', now), { kind: 'task', title: 'dentist', date: '2026-10-22', time: null });
  assert.equal(S.quickParse('next monday review notes', now).date, '2026-10-26', 'next Monday is the week after the coming one');
  assert.equal(S.quickParse('gym monday', now).date, '2026-10-19', 'a weekday that is today means today');
  assert.equal(S.quickParse('buy sun cream', now).date, null, 'sun and sat mid-sentence are words, not days');
  assert.equal(S.quickParse('I sat the exam and want to review it', now).date, null);
  assert.equal(S.quickParse('gym sat', now).date, '2026-10-24');
  assert.equal(S.quickParse('brunch on sun with Ana', now).date, '2026-10-25');
  assert.equal(S.quickParse('call mom fri', now).date, '2026-10-23');
  assert.deepEqual(S.quickParse('for sun, remind me to go buy sunscreen', now), { kind: 'task', title: 'go buy sunscreen', date: '2026-10-25', time: null });
  assert.deepEqual(S.quickParse('can you remind me to call mom tomorrow?', now), { kind: 'task', title: 'call mom', date: '2026-10-20', time: null });
  assert.deepEqual(S.quickParse("don't forget to pay rent by fri", now), { kind: 'task', title: 'pay rent', date: '2026-10-23', time: null });
  assert.deepEqual(S.quickParse('sat. pick up the cake', now), { kind: 'task', title: 'pick up the cake', date: '2026-10-24', time: null });
  assert.deepEqual(S.quickParse('remind me on sun to buy sun cream', now), { kind: 'task', title: 'buy sun cream', date: '2026-10-25', time: null });
  assert.deepEqual(S.quickParse('What is a heap?', now), { kind: 'ask', title: 'What is a heap?', date: null, time: null });
  assert.equal(S.quickParse('explain dijkstra', now).kind, 'ask');
  assert.deepEqual(S.quickParse('note: idea for the essay', now), { kind: 'note', title: 'idea for the essay', date: null, time: null });
  assert.equal(S.quickParse('remind me to stretch', now).kind, 'task');
  assert.equal(S.quickParse('remind me to stretch', now).date, null);
});

test('Quick Leo puts what you most likely want first, like Spotlight', () => {
  const now = new Date(2026, 9, 19, 10, 0);
  const notes = [{ id: 'a', title: 'Graph algorithms' }, { id: 'b', title: 'Heaps' }, { id: 'c', title: 'Weekly plan' }];
  const places = [{ name: 'Calendar', hash: '#/calendar' }, { name: 'Settings', hash: '#/settings' }, { name: 'Interview practice', hash: '#/interviews' }, { name: 'Knowledge graph', hash: '#/map' }];
  const top = (text, found = notes) => S.quickRank(text, { notes: found, places }, now)[0];
  assert.deepEqual(top('email Bob tomorrow at 3pm'), { kind: 'task', title: 'email Bob', date: '2026-10-20', time: '15:00' });
  assert.deepEqual(top('What is a heap?'), { kind: 'ask', text: 'What is a heap?' });
  assert.deepEqual(top('heaps'), { kind: 'open', id: 'b', title: 'Heaps' });
  assert.deepEqual(top('graph alg'), { kind: 'open', id: 'a', title: 'Graph algorithms' });
  assert.deepEqual(top('cal', []), { kind: 'place', name: 'Calendar', hash: '#/calendar' });
  assert.deepEqual(top('graph', [{ id: 'a', title: 'Graph algorithms' }]), { kind: 'open', id: 'a', title: 'Graph algorithms' }, 'a note named like that beats a page whose later word matches');
  assert.deepEqual(top('graph', []), { kind: 'place', name: 'Knowledge graph', hash: '#/map' });
  assert.deepEqual(top('interview', []), { kind: 'place', name: 'Interview practice', hash: '#/interviews' });
  assert.deepEqual(top('note: idea for the essay'), { kind: 'note', title: 'idea for the essay' });
  assert.equal(top('dijkstra versus bellman ford for negative edges', []).kind, 'ask');
  assert.equal(top('how do graph algorithms work').kind, 'ask', 'a question asks even when a note matches');
  const rows = S.quickRank('Pay rent', { notes: [], places }, now);
  assert.deepEqual(rows.map((r) => r.kind), ['task', 'ask', 'note'], 'every other choice stays one arrow away');
  assert.deepEqual(top('buy milk', []), { kind: 'task', title: 'buy milk', date: null, time: null }, 'a short thing to do is a task');
  assert.equal(top('pick up John from the airport', []).kind, 'task');
  assert.equal(top('I keep mixing up the two proofs and want to get them straight before Friday somehow', []).kind, 'task', 'a date still means a reminder');
  assert.equal(top('so I was thinking about the project and how the parts fit together overall', []).kind, 'ask', 'Felix works out anything longer');
  const folders = ['Classes/CS 161', 'Work'];
  const chats = [{ id: 'k1', title: 'Heap questions' }];
  assert.deepEqual(S.quickRank('cs 161', { folders, places }, now)[0], { kind: 'folder', path: 'Classes/CS 161' });
  assert.ok(S.quickRank('cs', { folders, places }, now).some((r) => r.kind === 'folder' && r.path === 'Classes/CS 161'));
  assert.ok(S.quickRank('heap', { chats, places }, now).some((r) => r.kind === 'chat' && r.id === 'k1'), 'past chats that match are listed');
  assert.equal(new Set(S.quickRank('heaps', { notes, places }, now)).size, S.quickRank('heaps', { notes, places }, now).length);
  assert.deepEqual(S.quickRank('   ', { notes, places }, now), []);
});

test('Calendar descriptions keep their formatting and links but nothing that can run', () => {
  const google = 'Itinerary: Charlton S<br />Monday, Oct 19, 2026 PDT<br/><br/>1:45PM (PDT) - 2:45PM (PDT): Data/Algos Interview<br/><br/>Hi Charlton<br/><br/>You’re all set!<br/><br/><strong>How to connect:</strong><ul><li>Click "Join with Google Meet" then “Join Now”.</li><li>You may see “ask to join the call”.</';
  const html = S.richText(google);
  assert.match(html, /Itinerary: Charlton S<br>Monday, Oct 19, 2026 PDT<br><br>1:45PM/);
  assert.match(html, /<strong>How to connect:<\/strong><ul><li>Click &quot;Join with Google Meet&quot; then “Join Now”.<\/li><li>You may see/);
  assert.ok(html.endsWith('</li></ul>'), 'a cut-off description is closed properly');
  assert.ok(!html.includes('<br />') && !html.includes('</'.repeat(2)));
  const hostile = S.richText('<script>alert(1)</script><img src=x onerror=alert(1)><a href="javascript:alert(1)">bad</a> <a href="https://meet.google.com/abc-defg" onclick="x()">Join</a> <b onmouseover="x">bold</b>');
  assert.ok(!/<script|<img|onerror|onclick|onmouseover|javascript:/i.test(hostile), hostile);
  assert.ok(!hostile.startsWith('alert'), 'a script and its text are dropped');
  assert.match(hostile, /<a href="https:\/\/meet\.google\.com\/abc-defg" target="_blank" rel="noopener noreferrer">Join<\/a>/);
  assert.match(hostile, /<strong>bold<\/strong>/);
  assert.equal(S.richText('Join at https://zoom.us/j/123.\nBring notes & questions'), 'Join at <a href="https://zoom.us/j/123" target="_blank" rel="noopener noreferrer">https://zoom.us/j/123</a>.<br>Bring notes &amp; questions');
  assert.equal(S.richText('Tom &amp; Jerry &lt;3'), 'Tom &amp; Jerry &lt;3');
  assert.equal(S.richText('<p>One</p><p>Two</p>'), '<p>One</p><p>Two</p>');
  assert.equal(S.richText('<a href="mailto:a@b.co">mail</a>'), '<a href="mailto:a@b.co" target="_blank" rel="noopener noreferrer">mail</a>');
});

test('Calendar event details split into organiser, invited and description', () => {
  const d = S.eventDetails('Calendar event: Interview\nPlace: Google Meet\nOrganised by: c_c463e763f192@group.calendar.google.com\nInvited: GoodTime Sync, charlton@ucla.edu, room@resource.calendar.google.com\nDescription:\nLine one\nLine two');
  assert.deepEqual(d, { place: 'Google Meet', organiser: 'A shared calendar', invited: ['GoodTime Sync', 'charlton@ucla.edu'], description: 'Line one\nLine two' });
  assert.deepEqual(S.eventDetails('Calendar event: Lunch'), { place: '', organiser: '', invited: [], description: '' });
});

test('Calendar links preserve descriptions and safely encode plain text', () => {
  const base = { id: 'task-one', title: 'Study', date: '2026-10-16' };
  const details = 'Read chapter 3\nBring café notes; <b>not HTML</b>&x=y';
  const link = new URL(S.googleLink({ ...base, description: details }));
  assert.equal(link.searchParams.get('details'), `${details}\n\nFrom leo · leo-task:task-one`);
  assert.equal(link.searchParams.get('x'), null);
  assert.equal(new URL(S.googleLink(base)).searchParams.get('details'), 'From leo · leo-task:task-one');
});

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

test('a repeating task shows on its weekdays from its start, done day by day, never overdue', () => {
  const gym = { id: 'g', title: 'Gym', date: '2026-10-12', repeat: [0, 2], done_on: ['2026-10-12'] };
  const p = S.plan([gym], [], '2026-10-20', { from: '2026-10-05', to: '2026-10-25' });
  const days = [...p.byDay.keys()].sort();
  assert.deepEqual(days, ['2026-10-12', '2026-10-14', '2026-10-19', '2026-10-21']);
  assert.equal(p.byDay.get('2026-10-12').allDay[0].task.done, true);
  assert.equal(p.byDay.get('2026-10-14').allDay[0].task.done, false);
  assert.equal(p.byDay.get('2026-10-14').allDay[0].task.on, '2026-10-14');
  assert.deepEqual(p.overdue, []);
  assert.deepEqual(p.undated, []);
  assert.equal(S.weekdayOf('2026-10-12'), 0);
  assert.equal(S.weekdayOf('2026-10-18'), 6);
  assert.equal(S.nextOpen(gym, '2026-10-12', '2026-10-19').date, '2026-10-14');
  assert.equal(S.repeatLabel([0, 1, 2, 3, 4]), 'Every weekday');
  assert.equal(S.repeatLabel([6, 5, 0, 1, 2, 3, 4]), 'Every day');
  assert.equal(S.rrule([4, 0, 2]), 'RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR');
});

test('Google gets a weekly rule starting on the first repeat day', () => {
  const link = new URL(S.googleLink({ id: 'g', title: 'Gym', date: '2026-10-13', repeat: [2], kind: 'other' }));
  assert.equal(link.searchParams.get('recur'), 'RRULE:FREQ=WEEKLY;BYDAY=WE');
  assert.equal(link.searchParams.get('dates'), '20261014/20261015');
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
