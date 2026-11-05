const assert = require('node:assert/strict');
const M = require('../src/web/margin.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

const source = 'Save the password in the users table,\nthen login just checks if it matches, I think?\n\nSessions last a day.';
const quote = 'just checks if it matches';

test('anchor: one exact match', () => {
  const at = M.find(source, { quote });
  assert.equal(source.slice(at.from, at.to), quote);
  assert.equal(at.how, 'exact');
});

test('anchor: several matches are told apart by prefix and suffix', () => {
  const text = 'one same words two\n\nthree same words four';
  const second = M.find(text, { quote: 'same words', prefix: 'three ', suffix: ' four' });
  assert.equal(second.from, text.lastIndexOf('same words'));
  const first = M.find(text, { quote: 'same words', prefix: 'one ', suffix: ' two' });
  assert.equal(first.from, text.indexOf('same words'));
  assert.equal(M.find(text, { quote: 'same words' }).from, text.indexOf('same words'));
});

test('anchor: an edited quote is found between its prefix and suffix', () => {
  const edited = source.replace('just checks if it matches', 'only compares the two');
  const at = M.find(edited, { quote, prefix: 'table,\nthen login ', suffix: ', I think?\n\nSessions last' });
  assert.equal(edited.slice(at.from, at.to), 'only compares the two');
  assert.equal(at.how, 'context');
});

test('anchor: at the very start or very end of a note', () => {
  const edited = 'Rewritten opening\n\nrest of the note';
  const start = M.find(edited, { quote: 'Old opening', prefix: '', suffix: '\n\nrest of the note' });
  assert.equal(edited.slice(start.from, start.to), 'Rewritten opening');
  const end = M.find(edited, { quote: 'Old tail', prefix: 'Rewritten opening\n\n', suffix: '' });
  assert.equal(edited.slice(end.from, end.to), 'rest of the note');
});

test('anchor: lost when the quote and its surroundings are gone', () => {
  assert.equal(M.find('Nothing like it.', { quote, prefix: 'table,\nthen login ', suffix: ', I think?' }), null);
  assert.equal(M.find(source, { quote: 'gone words', prefix: 'then login ', suffix: 'no such tail here' }), null);
  assert.equal(M.find(source, { quote: 'gone words' }), null);
  assert.equal(M.find(source, { quote: '' }), null);
});

test('anchor: lost when the passage between prefix and suffix was emptied or is too far apart', () => {
  assert.equal(M.find('table,\nthen login , I think?', { quote, prefix: 'then login ', suffix: ', I think?' }), null);
  assert.equal(M.find('then login   , I think?', { quote, prefix: 'then login ', suffix: '  , I think?' }), null);
  const far = `then login ${'word '.repeat(100)}, I think?`;
  assert.equal(M.find(far, { quote: 'short', prefix: 'then login ', suffix: ', I think?' }), null);
});

test('anchor: a short or missing side never attaches to unrelated text', () => {
  const text = 'ab some new words and a long enough tail';
  assert.equal(M.find(text, { quote: 'old words', prefix: 'ab ', suffix: ' and a long enough tail' }), null);
  assert.equal(M.find(text, { quote: 'old words', prefix: 'ab some ', suffix: ' t' }), null);
  assert.equal(M.find('anything at all here', { quote: 'old words', prefix: '', suffix: '' }), null);
});

test('around gives the words beside a selection on its own line', () => {
  const text = 'same words\n\nsame words';
  assert.deepEqual(M.around(text, 'same words', 3), { prefix: 'same words\n\n', suffix: '' });
  assert.deepEqual(M.around(text, 'same words', 1), { prefix: '', suffix: '\n\nsame words' });
  assert.equal(M.around(text, 'missing', 1), null);
  assert.deepEqual(M.around('only here', 'only', 9), { prefix: '', suffix: ' here' });
});

test('around never splits a surrogate pair', () => {
  for (const pad of ['', 'x']) {
    const text = `${pad}${'😀'.repeat(40)}words${'😀'.repeat(40)}${pad}`;
    const beside = M.around(text, 'words', 1);
    assert.ok(beside.prefix.isWellFormed() && beside.suffix.isWellFormed());
    assert.ok(JSON.stringify(beside).length > 0);
    assert.ok(text.includes(`${beside.prefix}words${beside.suffix}`));
  }
});

test('lineAfter counts the lines up to an offset', () => {
  assert.equal(M.lineAfter('one\ntwo\nthree', 5), 2);
  assert.equal(M.lineAfter('one', 0), 1);
});

test('stacking: notes stay where they want when there is room', () => {
  assert.deepEqual(M.stack([{ id: 'a', want: 10, height: 40 }, { id: 'b', want: 200, height: 40 }]), { a: 10, b: 200 });
});

test('stacking: a collision pushes the later note down, in order', () => {
  const tops = M.stack([{ id: 'b', want: 20, height: 40 }, { id: 'a', want: 10, height: 40 }, { id: 'c', want: 30, height: 40 }], { gap: 10 });
  assert.deepEqual(tops, { a: 10, b: 60, c: 110 });
});

test('stacking: an open note holds its place and others make way above and below', () => {
  const tops = M.stack([{ id: 'a', want: 100, height: 40 }, { id: 'b', want: 110, height: 120 }, { id: 'c', want: 120, height: 40 }], { gap: 10, pinned: 'b' });
  assert.equal(tops.b, 110);
  assert.equal(tops.a, 60);
  assert.equal(tops.c, 240);
});

test('stacking: nothing goes above the floor, even with an open note', () => {
  const tops = M.stack([{ id: 'a', want: 5, height: 40 }, { id: 'b', want: 10, height: 40 }], { gap: 10, pinned: 'b', floor: 0 });
  assert.equal(tops.a, 0);
  assert.equal(tops.b, 50);
  assert.deepEqual(M.stack([], {}), {});
  assert.deepEqual(M.stack([{ id: 'a', want: -30, height: 10 }], { floor: 0 }), { a: 0 });
});

test('both margins: a crowded right side spills left', () => {
  const on = M.sides([{ id: 'a', want: 0, height: 100 }, { id: 'b', want: 10, height: 100 }, { id: 'c', want: 20, height: 100 }, { id: 'd', want: 400, height: 10 }]);
  assert.deepEqual(on, { a: 'right', b: 'left', c: 'right', d: 'right' });
});

test('layout: nothing is reserved without threads or read-along', () => {
  assert.equal(M.arrangement({ width: 1600 }), 'none');
});

test('layout: threads or read-along take the right margin when it fits', () => {
  assert.equal(M.arrangement({ width: 1076, threads: 1 }), 'right');
  assert.equal(M.arrangement({ width: 1076, along: true }), 'right');
  assert.equal(M.arrangement({ width: M.ROOM_RIGHT, threads: 1 }), 'right');
  assert.equal(M.arrangement({ width: M.ROOM_RIGHT - 1, threads: 1 }), 'marks');
});

test('layout: both margins only with room for both', () => {
  assert.equal(M.arrangement({ width: 1400, threads: 1, setting: 'both' }), 'both');
  assert.equal(M.arrangement({ width: 1076, threads: 1, setting: 'both' }), 'right');
});

test('layout: marks only by choice, full width or with Felix open', () => {
  assert.equal(M.arrangement({ width: 1600, threads: 1, setting: 'marks' }), 'marks');
  assert.equal(M.arrangement({ width: 1600, threads: 1, noteWidth: 'full' }), 'marks');
  assert.equal(M.arrangement({ width: 1600, threads: 1, panel: true }), 'marks');
});

test('changed blocks: only new or edited blocks, each counted once', () => {
  const before = M.blocksOf('First paragraph long enough.\n\nSame paragraph long enough.');
  assert.deepEqual(M.changedBlocks(before, 'First paragraph long enough.\n\nSame paragraph long enough.'), []);
  assert.deepEqual(M.changedBlocks(before, 'First paragraph long enough.\n\nSame paragraph long enough!'), ['Same paragraph long enough!']);
  assert.deepEqual(M.changedBlocks(['Repeated block long enough.'], 'Repeated block long enough.\n\nRepeated block long enough.'), ['Repeated block long enough.']);
});

test('changed blocks: short scraps wait and the amount sent is bounded', () => {
  assert.deepEqual(M.changedBlocks([], 'ok\n\nfine'), []);
  const many = Array.from({ length: 9 }, (_, i) => `Block number ${i} long enough to read.`).join('\n\n');
  const picked = M.changedBlocks([], many);
  assert.equal(picked.length, 6);
  assert.ok(picked[5].includes('number 8'));
  const huge = M.changedBlocks([], `${'é'.repeat(9000)}\n\n${'b'.repeat(3000)}`);
  assert.ok([...huge.join('')].length <= 8000);
  assert.ok([...huge[0]].length <= 4000);
  assert.ok(`${'é'.repeat(9000)}`.endsWith(huge[0]));
});

test('drawing: a wave spans its width and a tether joins its ends', () => {
  assert.ok(M.wave(10, 100, 20).startsWith('M10.0 20.0 q'));
  const line = M.tether({ x: 10, y: 20 }, { x: 110, y: 40 });
  assert.ok(line.startsWith('M10.0 20.0 C') && line.endsWith('110.0 40.0'));
  assert.ok(M.tether({ x: 10, y: 20 }, { x: 110, y: 40 }, 80).startsWith('M10.0 20.0 L80.0 20.0 C'), 'a tether clears the rest of the line');
});

for (const [name, fn] of cases) {
  try {
    fn();
  } catch (e) {
    console.error(`FAILED: ${name}`);
    throw e;
  }
}
console.log(`ok ${cases.length}`);
