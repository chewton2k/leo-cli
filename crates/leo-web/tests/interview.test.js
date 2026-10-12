const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/interview.js');
const I = globalThis.leoInterview;

test('every interview track has distinct editable criteria and one shared framework', () => {
  assert.equal(I.PRESETS.length, 7);
  for (const p of I.PRESETS) {
    const f = I.framework(p.id);
    assert.equal(f.track, p.id);
    assert.ok(f.skills.length >= 3);
    f.skills.pop();
    assert.equal(I.preset(p.id).skills.length, f.skills.length + 1);
  }
});

test('progress counts assessed attempts only, uses recent observations and exposes sample counts', () => {
  const attempt = (value) => ({ feedback: { scores: [{ skill: 'Testing', value }] } });
  assert.deepEqual(I.progress([{ id: 'ungraded' }, ...[4, 0, 1, 2, 3, 4].map(attempt)]), [{ skill: 'Testing', n: 6, value: 2 }]);
  assert.equal(I.progress([attempt(5)]).length, 0);
});

test('the board bounds coordinates, shape and point counts and escapes text', () => {
  assert.deepEqual(I.point(-5, 2000), [0, 1000]);
  const shape = { kind: 'text', text: '<script>alert(1)</script>', points: [[30, 40]] };
  assert.ok(I.boardSvg([shape]).includes('&lt;script&gt;'));
  for (const bad of [[null], [3], Array(101).fill(shape), [{ ...shape, kind: 'script' }], [{ ...shape, points: [[Infinity, 2]] }], [{ ...shape, points: [[-1, 2]] }], [{ ...shape, points: Array(129).fill([1, 2]) }], Array(40).fill({ ...shape, points: Array(128).fill([1, 2]) })]) assert.equal(I.validBoard(bad), false);
});

test('practice and real calendar events share an agenda without losing completion or untimed dates', () => {
  const result = I.agenda([{ id: 'task-0001', date: '2026-10-12', time: '', done: true }], [{ id: 'gcal-0001', start: new Date(2026, 9, 12, 9).toISOString() }, { start: 'invalid' }]);
  assert.equal(result.length, 2);
  assert.equal(result[0].kind, 'event');
  assert.equal(result[1].done, true);
  assert.equal(result[1].day, '2026-10-12');
});

test('drafts survive failed saves, remain isolated by plan and challenge, and stay bounded', () => {
  let data = '{}';
  const storage = { getItem: () => data, setItem: (_, value) => { data = value; } };
  for (let n = 0; n < 10; n++) I.keepDraft(storage, `plan${n}`, 'challenge', { answer: `答え${n}`, board: [] });
  assert.equal(Object.keys(JSON.parse(data)).length, 8);
  assert.equal(I.readDraft(storage, 'plan9', 'challenge').answer, '答え9');
  assert.equal(I.readDraft(storage, 'plan9', 'other'), null);
  I.clearDraft(storage, 'plan9', 'challenge');
  assert.equal(I.readDraft(storage, 'plan9', 'challenge'), null);
  assert.equal(I.keepDraft({ getItem: () => { throw Error('offline'); } }, 'x', 'y', { answer: '', board: [] }), false);
});
