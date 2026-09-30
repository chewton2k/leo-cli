const assert = require('node:assert/strict');
const ed = require('../src/web/editing.js');
const md = require('../src/web/markdown.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

test('every ordinary line is its own block, blank lines included', () => {
  const lines = ['# Title', '', 'para one', 'para two', '- item'];
  assert.deepEqual(
    ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]),
    [[0, 1, 'line'], [1, 2, 'line'], [2, 3, 'line'], [3, 4, 'line'], [4, 5, 'line']]
  );
});

test('code fences, tables and quotes stay whole', () => {
  const lines = ['```', 'a', '- [ ] b', '```', '| x | y |', '|---|---|', '| 1 | 2 |', '> one', '> two', 'after'];
  assert.deepEqual(
    ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]),
    [[0, 4, 'code'], [4, 7, 'table'], [7, 9, 'quote'], [9, 10, 'line']]
  );
});

test('an unclosed fence runs to the end', () => {
  assert.deepEqual(ed.splitBlocks(['```', 'x']).map((b) => [b.start, b.end, b.kind]), [[0, 2, 'code']]);
});

test('star, plus, numbered and empty boxes count and toggle like leo', () => {
  const lines = ['* [ ] star', '+ [x] plus', '1. [ ] one', '2) [X] two', '- [ ]', '-[ ] not', '- [y] not'];
  assert.deepEqual(lines.map(ed.isBox), [true, true, true, true, true, false, false]);
  assert.equal(ed.toggleBox('* [ ] star'), '* [x] star');
  assert.equal(ed.toggleBox('3. [x] three'), '3. [ ] three');
  assert.equal(ed.toggleBox('- [ ]'), '- [x]');
  assert.equal(ed.boxLine(lines, 5), 4);
  const html = md.render(lines.slice(0, 5).join('\n'));
  assert.equal((html.match(/data-box="/g) || []).length, 5, html);
  assert.ok(!html.includes('disabled'), html);
});

test('an empty note is one empty line', () => {
  assert.deepEqual(ed.splitBlocks(['']).map((b) => [b.start, b.end]), [[0, 1]]);
});

test('box numbers count the same lines leo counts', () => {
  const lines = ['- [ ] a', 'text', '  - [x] b', '```', '- [ ] in code', '```', '> - [ ] quoted', '- [X] c', '- [ ]'];
  assert.equal(ed.boxesBefore(lines, 0), 0);
  assert.equal(ed.boxesBefore(lines, 3), 2);
  assert.equal(ed.boxesBefore(lines, 7), 3);
  assert.equal(ed.boxesBefore(lines, 9), 5);
});

test('rendering a line block numbers its box globally', () => {
  const lines = ['- [ ] first', '- [x] second'];
  const html = md.render(lines[1], { boxOffset: ed.boxesBefore(lines, 1) });
  assert.ok(html.includes('data-box="2" checked'), html);
});

test('a numbered item keeps its number when rendered alone', () => {
  assert.ok(md.render('3. third').includes('<ol start="3">'), md.render('3. third'));
});

test('Enter continues a list, a checklist and a numbered list', () => {
  assert.deepEqual(ed.continueLine('- milk'), { prefix: '- ', exit: false });
  assert.deepEqual(ed.continueLine('  * nested'), { prefix: '  * ', exit: false });
  assert.deepEqual(ed.continueLine('- [x] done'), { prefix: '- [ ] ', exit: false });
  assert.deepEqual(ed.continueLine('9. nine'), { prefix: '10. ', exit: false });
  assert.deepEqual(ed.continueLine('> quoted'), { prefix: '> ', exit: false });
  assert.deepEqual(ed.continueLine('plain'), { prefix: '', exit: false });
});

test('Enter on an empty list item leaves the list', () => {
  assert.deepEqual(ed.continueLine('- '), { prefix: '', exit: true });
  assert.deepEqual(ed.continueLine('- [ ] '), { prefix: '', exit: true });
  assert.deepEqual(ed.continueLine('  2. '), { prefix: '', exit: true });
});

test('ticking a box flips only that box', () => {
  assert.equal(ed.toggleBox('- [ ] buy milk'), '- [x] buy milk');
  assert.equal(ed.toggleBox('  - [x] done'), '  - [ ] done');
  assert.equal(ed.toggleBox('  - [X] done'), '  - [ ] done');
  assert.equal(ed.toggleBox('not a box [ ]'), 'not a box [ ]');
});

test('Tab indents a list item and Shift-Tab brings it back', () => {
  assert.equal(ed.indent('- item'), '  - item');
  assert.equal(ed.outdent('  - item'), '- item');
  assert.equal(ed.outdent('- item'), '- item');
  assert.equal(ed.outdent(' - item'), '- item');
});

test('a tap lands on the same spot in the raw text', () => {
  assert.equal(ed.rawOffset('**bold** word', 'bold w'), 10);
  assert.equal(ed.rawOffset('- [ ] buy milk', 'buy'), 9);
  assert.equal(ed.rawOffset('## Heading', 'Head'), 7);
  assert.equal(ed.rawOffset('plain text', ''), 0);
  assert.equal(ed.rawOffset('[link](https://x.y) after', 'link af'), 22);
  assert.equal(ed.rawOffset('short', 'much longer than the line'), 5);
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
process.exit(failed ? 1 : 0);
