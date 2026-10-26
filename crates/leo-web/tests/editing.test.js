const assert = require('node:assert/strict');
const ed = require('../src/web/editing.js');
const md = require('../src/web/markdown.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

test('the outline finds headings at every level but excludes code, maths, quotes and tables', () => {
  const lines = ['# Intro', '## **Sounds**', '```md', '# Not a heading', '```', '> ## Quoted', '$$', '# Formula', '$$', '| Name |', '|---|', '# Table row |', '### Phrase [structure](https://example.com)', '###### Deep 😀', '## Again', '## Again', '####### Too deep', 'no # heading'];
  assert.deepEqual(ed.headings(lines), [
    { line: 0, level: 1, text: 'Intro' }, { line: 1, level: 2, text: '**Sounds**' },
    { line: 12, level: 3, text: 'Phrase [structure](https://example.com)' },
    { line: 13, level: 6, text: 'Deep 😀' }, { line: 14, level: 2, text: 'Again' }, { line: 15, level: 2, text: 'Again' },
  ]);
  assert.deepEqual(ed.headings(['##', '###    ', '  ## Indented ###']), [{ line: 2, level: 2, text: 'Indented' }]);
  assert.equal(ed.headings(Array(500).fill('# Section')).length, 300);
});

test('resizing counts only the pictures the page draws, so outside images and code never take the size', () => {
  const line = '`![code](a.png)` ![Logo](https://example.com/logo.png) ![[https://x.org/y.png]] ![Mine](attachments/me.png) ![Sketch](data:image/svg+xml,%3Csvg%2F%3E)';
  const drawn = md.render(line).match(/class="note-img/g).length;
  assert.equal(drawn, 2);
  assert.equal(ed.setPictureWidth(line, 0, 300), line.replace('![Mine](', '![Mine|300]('));
  assert.equal(ed.setPictureWidth(line, 1, 200), line.replace('![Sketch](', '![Sketch|200]('));
  assert.equal(ed.setPictureWidth(line, 2, 200), line, 'there is no third drawn picture');
});

test('a picture keeps a width in the Obsidian style and only the chosen one changes', () => {
  const line = 'Before ![Tree](attachments/tree.png) and ![[graph.png]] and ![Old|120](a.png "Title")';
  assert.equal(ed.setPictureWidth(line, 0, 400), 'Before ![Tree|400](attachments/tree.png) and ![[graph.png]] and ![Old|120](a.png "Title")');
  assert.equal(ed.setPictureWidth(line, 1, 250.4), 'Before ![Tree](attachments/tree.png) and ![[graph.png|250]] and ![Old|120](a.png "Title")');
  assert.equal(ed.setPictureWidth(line, 2, 300), 'Before ![Tree](attachments/tree.png) and ![[graph.png]] and ![Old|300](a.png "Title")');
  assert.equal(ed.setPictureWidth(line, 2, 0), 'Before ![Tree](attachments/tree.png) and ![[graph.png]] and ![Old](a.png "Title")', 'no width resets it');
  assert.equal(ed.setPictureWidth(line, 5, 300), line, 'a missing picture changes nothing');
  assert.equal(ed.setPictureWidth('![[a.png|Caption]]', 0, 200), '![[a.png|200]]');
  const shown = md.render('![Tree|400](attachments/tree.png)\n\n![[graph.png|250]]\n\n![Plain](x.png)');
  assert.match(shown, /alt="Tree" loading="lazy" width="400"/);
  assert.match(shown, /alt="graph.png" loading="lazy" width="250"/);
  assert.match(shown, /alt="Plain" loading="lazy">/);
  assert.doesNotMatch(md.render('![Huge|99999](x.png)'), /width=/);
});

test('a selection becomes its exact Markdown and 1-based lines, either direction', () => {
  const lines = ['# Heaps', '', 'A **min** heap', 'keeps 😀 the root', '$$x^2$$'];
  assert.deepEqual(ed.snapshotOf(lines, 9, 23), { from: 3, to: 3, text: 'A **min** heap' });
  assert.deepEqual(ed.snapshotOf(lines, 23, 9), { from: 3, to: 3, text: 'A **min** heap' });
  const whole = lines.join('\n');
  assert.deepEqual(ed.snapshotOf(lines, whole.indexOf('keeps'), whole.length), { from: 4, to: 5, text: 'keeps 😀 the root\n$$x^2$$' });
  assert.equal(ed.snapshotOf(lines, 0, 9).to, 2, 'ending at the start of a line does not take that line');
  assert.equal(ed.snapshotOf(lines, 8, 9), null, 'blank selections ask nothing');
  assert.equal(ed.snapshotOf(['x'.repeat(9000)], 0, 9000).text.length, ed.MOST_PASSAGE);
});

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
  assert.deepEqual(['-   [ ] wide', '*  [x] wide', '1.    [ ] wide', '-     [ ] code'].map(ed.isBox), [true, true, true, false]);
  assert.equal(ed.toggleBox('-   [ ] wide'), '-   [x] wide');
  assert.equal(ed.toggleBox('  *    [x] wide'), '  *    [ ] wide');
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
test('a \\[ block and a \\begin{aligned} block are each one block to edit', () => {
  const lines = ['Intro', '\\[', 'x^2', '\\]', '\\begin{aligned}', 'a &= b', '\\end{aligned}', 'After'];
  assert.deepEqual(ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]), [[0, 1, 'line'], [1, 4, 'math'], [4, 7, 'math'], [7, 8, 'line']]);
});

test('a $$ math block is one block to edit, and a one-line $$x$$ is a line', () => {
  const lines = ['intro', '$$', 'a^2 + b^2', '= c^2$$', 'after', '$$x$$'];
  assert.deepEqual(ed.splitBlocks(lines).map((b) => [b.start, b.end, b.kind]), [[0, 1, 'line'], [1, 4, 'math'], [4, 5, 'line'], [5, 6, 'line']]);
});

setImmediate(() => {
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
});
