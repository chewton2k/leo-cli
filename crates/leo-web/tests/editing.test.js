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

test('outline groups follow heading levels, including skipped levels and repeated titles', () => {
  const rows = ed.headings(['# First', '### Nested', '##### Deep', '## Sibling', '# Next', '## Nested', '# Last']);
  const tree = ed.headingTree(rows);
  assert.deepEqual(tree.map(row => row.text), ['First', 'Next', 'Last']);
  assert.deepEqual(tree[0].children.map(row => row.text), ['Nested', 'Sibling']);
  assert.equal(tree[0].children[0].children[0].text, 'Deep');
  assert.equal(tree[1].children[0].line, 5);
  assert.deepEqual(tree[2].children, []);
  assert.deepEqual(ed.headingTree([]), []);
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

test('list prefixes keep every rendered marker out of the text field', () => {
  for (const prefix of ['- ', '* ', '+ ', '12. ', '3) ', '- [ ] ', '* [x] ', '2. [X] ', '  - ', '    - [ ] ', '-   [ ] ']) {
    assert.equal(ed.listPrefix(prefix + '文字 😀'), prefix);
  }
  assert.equal(ed.listPrefix('- [ ]'), '- [ ]');
  for (const plain of ['Plain text', '# Heading', '> Quote', '![alt](a.png)', '-word', '1.word', 'A. Smith', 'ok. fine', 'Hi. there']) assert.equal(ed.listPrefix(plain), '');
});

test('Enter continues every kind of list with the next marker', () => {
  const next = (line) => ed.continueLine(line).prefix;
  assert.deepEqual(ed.continueLine('- milk'), { prefix: '- ', exit: false });
  assert.equal(next('  * nested'), '  * ');
  assert.equal(next('+ plus'), '+ ');
  assert.equal(next('- [x] done'), '- [ ] ');
  assert.equal(next('2. [x] done'), '3. [ ] ');
  assert.equal(next('3) [ ] pending'), '4) [ ] ');
  assert.equal(next('9. nine'), '10. ');
  assert.equal(next('a. one'), 'b. ');
  assert.equal(next('b) two'), 'c) ');
  assert.equal(next('C.  three'), 'D.  ');
  assert.equal(next('  a) nested'), '  b) ');
  assert.equal(next('z) last'), 'aa) ');
  assert.equal(next('aa) doubled'), 'bb) ');
  assert.equal(next('Z.  last'), 'AA.  ');
  assert.equal(next('Z) last'), 'AA) ');
  assert.equal(next('a. [ ] box'), 'b. [ ] ');
  assert.equal(next('> quoted'), '> ');
  assert.deepEqual(ed.continueLine('plain'), { prefix: '', exit: false });
});

test('words that only look like markers stay prose', () => {
  for (const line of ['Hello. world', 'ok. sure', 'hi) there', 'a)b', 'e.g. this', '-word', '1.word', 'a dog']) {
    assert.deepEqual(ed.continueLine(line), { prefix: '', exit: false }, line);
    assert.equal(ed.itemHead(line), '', line);
  }
});

test('Enter on an empty item steps out one level, then leaves the list', () => {
  const back = (line) => ed.continueLine(line).back;
  for (const line of ['- ', '2. [ ] ', '- [ ] ', 'a) ', '* ', '+ ', '-   [ ] ']) {
    assert.equal(ed.continueLine(line).exit, true, line);
    assert.equal(back(line), '', line);
  }
  assert.equal(back('  - '), '- ');
  assert.equal(back('    - [ ] '), '  - [ ] ');
  assert.equal(back('  2. '), '2. ');
  assert.equal(back('\ta) '), 'a) ');
});

test('Backspace at the start of an item outdents, then keeps only the text', () => {
  assert.equal(ed.stepBack('    - deep'), '  - deep');
  assert.equal(ed.stepBack('  - sub'), '- sub');
  assert.equal(ed.stepBack('- sub'), 'sub');
  assert.equal(ed.stepBack('- [x] done 😀'), 'done 😀');
  assert.equal(ed.stepBack('12. twelve'), 'twelve');
  assert.equal(ed.stepBack('c) three'), 'three');
  assert.equal(ed.stepBack('- '), '');
});

test('numbers and letters follow their place after a change', () => {
  const after = (lines, at) => {
    const out = [...lines];
    ed.renumber(out, at);
    return out;
  };
  assert.deepEqual(after(['1. a', '2. ', '2. b', '3. c'], 1), ['1. a', '2. ', '3. b', '4. c']);
  assert.deepEqual(after(['a) a', 'b) ', 'b) b'], 1), ['a) a', 'b) ', 'c) b']);
  assert.deepEqual(after(['1. a', '  2. b', '3. c'], 1), ['1. a', '  1. b', '2. c'], 'a new sub-list starts over and the outer one closes up');
  assert.deepEqual(after(['1. a', '2. b', '  2. c'], 1), ['1. a', '2. b', '  1. c']);
  assert.deepEqual(after(['1. a', '  c) b', '  a) c'], 1), ['1. a', '  a) b', '  b) c']);
  assert.deepEqual(after(['5. a', '6. b', '  - x', '6. c'], 1), ['5. a', '6. b', '  - x', '7. c'], 'the first number stays and sub-items are skipped');
  assert.deepEqual(after(['9. [ ] a', '9. [x] b'], 1), ['9. [ ] a', '10. [x] b']);
  assert.deepEqual(after(['- a', '1. b', 'plain', '1. c'], 1), ['- a', '1. b', 'plain', '1. c'], 'a plain line ends the list');
  assert.deepEqual(after(['```', '3. in code', '```', '1. a', '1. b'], 3), ['```', '3. in code', '```', '1. a', '2. b']);
  assert.deepEqual(after(['- a', 'plain'], 1), ['- a', 'plain'], 'a plain line changes nothing');
});

test('a line leaving a list closes the numbers around it', () => {
  const left = (lines, at) => {
    const out = [...lines];
    ed.closeList(out, at);
    return out;
  };
  assert.deepEqual(left(['1. a', 'b', '3. c', '4. d'], 1), ['1. a', 'b', '2. c', '3. d']);
  assert.deepEqual(left(['1. a', '', '3. c'], 1), ['1. a', '', '2. c']);
  assert.deepEqual(left(['b', '3. c'], 0), ['b', '3. c'], 'nothing above, so the number stays');
  assert.deepEqual(left(['1. a', '  1. b', '  3. c'], 1), ['1. a', '  1. b', '  2. c'], 'an item is renumbered in place');
  assert.deepEqual(left(['1. a', 'b'], 1), ['1. a', 'b']);
});

test('a list item knows its level', () => {
  assert.deepEqual(['- a', '  - b', '    1. c', '  a) d', '\t- e', 'plain', '  plain'].map((_, at, all) => ed.listLevel(all, at)), [0, 1, 2, 1, 2, 0, 0]);
  const wide = ['1. a', '   - b', '     1) c', '   - d', '10. e', '    - f'];
  assert.deepEqual(wide.map((_, at) => ed.listLevel(wide, at)), [0, 1, 2, 1, 0, 1], 'markers of any width nest by their depth');
  assert.equal(ed.listLevel(['text', '  - lone'], 1), 1, 'an old indent with nothing above keeps its level');
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
  assert.equal(ed.outdent('\t- item'), '- item');
  assert.equal(ed.outdent('   - item'), ' - item');
});

test('Tab nests under the marker above and Shift-Tab returns to the parent', () => {
  assert.equal(ed.indent('- b', ['- a']), '  - b');
  assert.equal(ed.indent('2. b', ['1. a']), '   2. b');
  assert.equal(ed.indent('b) b', ['a) a']), '   b) b');
  assert.equal(ed.indent('11. b', ['10. a']), '    11. b');
  assert.equal(ed.indent('B.  b', ['A.  a']), '    B.  b');
  assert.equal(ed.indent('   - b', ['1. a', '   - sibling']), '     - b', 'under the sibling at its own depth');
  assert.equal(ed.indent('  - old', ['1. a']), '    - old', 'an old two-space child moves a full level');
  assert.equal(ed.indent('plain'), '  plain');
  assert.equal(ed.outdent('   - b', ['1. a']), '- b');
  assert.equal(ed.outdent('      x) c', ['1. a', '   - b']), '   x) c');
  assert.equal(ed.stepBack('   - ', ['1. a']), '- ');
  assert.equal(ed.continueLine('   - ', ['1. a']).back, '- ');
});

test('a heading marker comes off in one Backspace and an empty heading ends on Enter', () => {
  assert.equal(ed.itemHead('## Title'), '## ');
  assert.equal(ed.stepBack('## Title'), 'Title');
  assert.equal(ed.stepBack('# '), '');
  assert.deepEqual(ed.continueLine('# '), { prefix: '', exit: true, back: '' });
  assert.deepEqual(ed.continueLine('# Title'), { prefix: '', exit: false });
  assert.equal(ed.itemHead('#tag'), '');
  assert.equal(ed.itemHead('####### seven'), '');
});

test('lettered tasks tick like the others', () => {
  assert.equal(ed.toggleBox('a) [ ] one'), 'a) [x] one');
  assert.equal(ed.toggleBox('  bb. [x] two'), '  bb. [ ] two');
  assert.equal(ed.boxesBefore(['a) [ ] one', 'A.  [ ] two', 'A. [ ] prose', ''], 3), 2);
});

test('a style wraps a selection and the same key takes it off again', () => {
  const on = (text, picked, style) => {
    const from = text.indexOf(picked);
    return ed.restyle(text, from, from + picked.length, style);
  };
  const back = (r, style) => ed.restyle(r.text, r.from, r.to, style).text;
  for (const [text, picked, style, styled] of [
    ['a word here', 'word', 'bold', 'a **word** here'],
    ['a word here', 'word', 'italic', 'a *word* here'],
    ['a word here', 'word', 'underline', 'a <u>word</u> here'],
    ['**word**', 'word', 'italic', '***word***'],
    ['*word*', 'word', 'bold', '***word***'],
    ['**word**', 'word', 'underline', '**<u>word</u>**'],
    ['- item', 'item', 'bold', '- **item**'],
    ['a) item', 'item', 'underline', 'a) <u>item</u>'],
  ]) {
    const made = on(text, picked, style);
    assert.equal(made.text, styled);
    assert.equal(made.text.slice(made.from, made.to), picked);
    assert.equal(back(made, style), text, styled);
  }
  assert.equal(on('***word***', 'word', 'bold').text, '*word*');
  assert.equal(on('***word***', 'word', 'italic').text, '**word**');
  assert.equal(on('_word_', 'word', 'italic').text, 'word');
  assert.equal(on('__word__', 'word', 'bold').text, 'word');
  assert.equal(on('**word**', '**word**', 'bold').text, 'word', 'a selection that took the marks in');
  assert.equal(on('<u>word</u>', '<u>word</u>', 'underline').text, 'word');
  assert.equal(on('<u>**word**</u>', 'word', 'underline').text, '**word**');
  assert.equal(on('**<u>word</u>**', '**<u>word</u>**', 'underline').text, '**word**');
  assert.equal(on('<u>word</u>', 'word</u>', 'underline').text, 'word', 'the page can hand back only one side of the marks');
  assert.equal(on('**word**', 'word**', 'bold').text, 'word');
  assert.equal(on('snake_case', 'snake_case', 'bold').text, '**snake_case**');
});

test('a style across lines skips markers and blank lines', () => {
  const text = '- one\n\n1. two\n# three';
  const made = ed.restyle(text, 0, text.length, 'bold');
  assert.equal(made.text, '- **one**\n\n1. **two**\n# **three**');
  assert.equal(ed.restyle(made.text, made.from, made.to, 'bold').text, text);
  assert.equal(ed.restyle('- one', 0, 2, 'bold').text, '- one', 'only the marker was picked');
  const mixed = 'one\n---\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncode\n```\ntwo';
  assert.equal(ed.restyle(mixed, 0, mixed.length, 'bold').text, mixed.replace('one', '**one**').replace('two', '**two**'), 'select-all leaves tables, code and rules alone');
  const inCell = '| a | word |\n|---|---|';
  assert.equal(ed.restyle(inCell, 6, 10, 'bold').text, '| a | **word** |\n|---|---|', 'one cell of a table still can');
  const under = ed.restyle('- one\n- two', 0, 11, 'underline');
  assert.equal(under.text, '- <u>one</u>\n- <u>two</u>');
  assert.equal(ed.restyle(under.text, 5, under.text.length, 'underline').text, '- one\n- two', 'starting inside the first tag');
});

test('Tab walks table cells and a new row stays a table', () => {
  const text = '| A | B |\n|---|:-:|\n| 1 |  |';
  const here = (at, back) => {
    const cell = ed.nextCell(text, at, back);
    return cell && text.slice(cell.from, cell.to);
  };
  assert.equal(here(2), 'B');
  assert.equal(here(6), '1', 'the rule row is skipped');
  assert.equal(here(22, true), 'B');
  assert.equal(ed.nextCell(text, text.length - 1), null);
  assert.equal(ed.cellEnd(text, 0, 1), 7);
  assert.equal(ed.cellEnd(text, 1, 0), 23);
  assert.equal(ed.cellEnd(text, 4, 4), 'end');
  const row = ed.addRow(text);
  assert.equal(row.text, `${text}\n|  |  |`);
  assert.equal(row.at, text.length + 3);
  assert.equal(ed.splitBlocks(row.text.split('\n')).length, 1);
  assert.equal(ed.nextCell('a | b \\| c | d\n--|--|--', 0).from, 4);
});

test('a tap lands on the same spot in the raw text', () => {
  assert.equal(ed.rawOffset('see [link](https://x.y)', 'see link'), 'see [link](https://x.y)'.length, 'the end of a link is past its address');
  assert.equal(ed.rawOffset('**bold**', 'bold'), 8);
  assert.equal(ed.rawOffset('<u>under</u> word', 'under w'), 14);
  assert.equal(ed.rawOffset('_it_ word', 'it w'), 6);
  assert.equal(ed.rawOffset('snake_case word', 'snake_c'), 7);
  assert.equal(ed.rawOffset('a) item', 'it'), 5);
  assert.equal(ed.rawOffset('**bold** word', 'bold w'), 10);
  assert.equal(ed.rawOffset('- [ ] buy milk', 'buy'), 9);
  assert.equal(ed.rawOffset('## Heading', 'Head'), 7);
  assert.equal(ed.rawOffset('plain text', ''), 0);
  assert.equal(ed.rawOffset('[link](https://x.y) after', 'link af'), 22);
  assert.equal(ed.rawOffset('short', 'much longer than the line'), 5);
  assert.equal(ed.rawOffset('😀 after', '😀'), 2);
  assert.equal(ed.rawOffset('## 😀 **bold** after', '😀 bold af'), '## 😀 **bold** af'.length);
  assert.equal(ed.rawOffset('[😀](https://x.y) after', '😀 af'), '[😀](https://x.y) af'.length);
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
