const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/interview.js');
const I = globalThis.leoInterview;
require('../src/web/vendor/code-tools.min.js');
const C = globalThis.leoCode;

test('CodeMirror typing handles indentation and paired delimiters in every supported language', () => {
  for (const language of ['C++', 'Rust', 'Java', 'Python', 'SQL', 'TypeScript', 'JavaScript', 'Go']) {
    const engine = C.editor(language);
    for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}']]) {
      const inserted = engine.edit('', 0, 0, open);
      assert.equal(inserted.value, open + close, language);
      assert.equal(inserted.from, 1);
      const skipped = engine.edit(inserted.value, 1, 1, close);
      assert.equal(skipped.value, open + close, language);
      assert.equal(skipped.from, 2);
      const removed = engine.edit(open + close, 1, 1, 'Backspace');
      assert.equal(removed.value, '', language);
    }
    const block = engine.edit('{}', 1, 1, 'Enter');
    assert.equal(block.value, '{\n    \n}', language);
    assert.equal(block.from, 6);
    const tab = engine.edit('  ', 2, 2, 'Tab');
    assert.equal(tab.value, '    ', language);
    assert.equal(tab.change.insert, '  ');
    assert.equal(engine.edit('    ', 4, 4, 'Tab', true).value, '', language);
    assert.equal(engine.edit('x'.repeat(32000), 32000, 32000, '('), null);
  }
  for (const value of ['&', 'fn solve<', '', 'break ']) assert.equal(C.editor('Rust').edit(value, value.length, value.length, "'"), null);
  assert.equal(C.editor('Rust').edit('', 0, 0, '"').value, '""');
  const python = C.editor('Python').edit('def solve():', 12, 12, 'Enter');
  assert.equal(python.value, 'def solve():\n    ');
  const cpp = C.editor('C++');
  const selected = cpp.edit('答😀', 0, 3, '[');
  assert.equal(selected.value, '[答😀]');
  assert.equal(selected.value.slice(selected.from, selected.to), '答😀');
  const multiline = cpp.edit('a\nb', 0, 3, 'Tab');
  assert.equal(multiline.value, '    a\n    b');
  assert.equal(cpp.edit(multiline.value, multiline.from, multiline.to, 'Tab', true).value, 'a\nb');
});

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
  assert.deepEqual(I.point(-5, 2000), [-5, 2000]);
  const shape = { kind: 'text', text: '<script>alert(1)</script>', points: [[30, 40]] };
  assert.ok(I.boardSvg([shape]).includes('&lt;script&gt;'));
  for (const bad of [[null], [3], Array(101).fill(shape), [{ ...shape, kind: 'script' }], [{ ...shape, points: [[Infinity, 2]] }], [{ ...shape, points: [[-I.BOARD_LIMIT - 1, 2]] }], [{ ...shape, points: Array(129).fill([1, 2]) }], Array(40).fill({ ...shape, points: Array(128).fill([1, 2]) })]) assert.equal(I.validBoard(bad), false);
});

test('board panning and moving preserve geometry, fit distant content and bound the virtual canvas', () => {
  const shape = { kind: 'box', text: '', points: [[-2000, 3000], [-1700, 3200]] };
  assert.deepEqual(I.moveShape(shape, 100, -200).points, [[-1900, 2800], [-1600, 3000]]);
  const moved = I.moveShape(shape, I.BOARD_LIMIT * 3, -I.BOARD_LIMIT * 3);
  assert.ok(I.validBoard([moved]));
  assert.equal(moved.points[1][0] - moved.points[0][0], 300);
  assert.equal(moved.points[1][1] - moved.points[0][1], 200);
  const board = [shape, { kind: 'text', text: 'Client → Server <script>', points: [[5000, -4000]] }];
  const view = I.fitBoard(board);
  for (const item of board) for (const [x, y] of item.points) assert.ok(x >= view.x && x <= view.x + view.w && y >= view.y && y <= view.y + view.h);
  const html = I.boardSvg(board);
  assert.ok(html.includes(`viewBox="${view.x} ${view.y} ${view.w} ${view.h}"`));
  assert.ok(html.includes('&lt;script&gt;'));
  assert.ok(!html.includes('tabindex'));
  const live = I.boardSvg(board, { view: { x: 2000, y: -5000, w: 1000, h: 700 }, interactive: true });
  assert.ok(live.includes('data-board-shape="1"'));
  assert.ok(live.includes('tabindex="0"'));
  assert.ok(live.includes('currentColor'));
  assert.deepEqual(I.point(Infinity, NaN), [0, 0]);
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

test('starting practice combines company research and challenge creation, while general practice skips research', () => {
  for (const track of ['coding', 'systems', 'design', 'marketing', 'business', 'quant']) {
    const f = { ...I.framework(track), company: 'Pinterest', role: 'Intern' };
    const company = I.preparation(f);
    assert.match(company.say, /Search the web before/);
    assert.match(company.say, /interview_challenge/);
    assert.match(company.say, /unverified candidate reports/);
    assert.ok(company.text.includes('Pinterest'));
    const general = I.preparation({ ...f, company: '' });
    assert.match(general.say, /Do not ask for a company/);
    assert.doesNotMatch(general.say, /Search the web before/);
    if (track === 'coding') assert.match(company.say, /function signature/);
  }
});

test('coding stubs are not answers and separate reasoning and drawings count as work', () => {
  const c = { coding: { starter: 'def solve(nums):\n    pass' } };
  assert.equal(I.hasWork({ answer: c.coding.starter, board: [] }, c), false);
  for (const w of [{ answer: 'return nums', board: [] }, { answer: c.coding.starter, reasoning: 'Use a map', board: [] }, { answer: c.coding.starter, board: [{ kind: 'text' }] }]) assert.equal(I.hasWork(w, c), true);
  assert.deepEqual(I.workspace({ answer: 'legacy', board: [] }), { answer: 'legacy', reasoning: '', board: [] });
});

test('coding indentation handles typing, selections, outdent and Unicode without losing the caret', () => {
  assert.deepEqual(I.codeEdit('def solve():', 12, 12, 'Enter'), { value: 'def solve():\n    ', from: 17, to: 17 });
  assert.deepEqual(I.codeEdit('答え', 2, 2, 'Tab'), { value: '答え    ', from: 6, to: 6 });
  const indented = I.codeEdit('a\nb', 0, 3, 'Tab');
  assert.equal(indented.value, '    a\n    b');
  assert.equal(I.codeEdit(indented.value, indented.from, indented.to, 'Tab', true).value, 'a\nb');
  assert.equal(I.codeEdit('x'.repeat(32000), 32000, 32000, 'Tab'), null);
});

test('coding colors cover common languages, aliases, incomplete code and untrusted text', () => {
  for (const [language, value] of [['C++', 'int solve() { return 2; }'], ['Rust', 'fn solve() -> i32 { 2 }'], ['Java', 'class Solution { int solve() { return 2; } }'], ['Python', 'def solve():\n    return 2'], ['SQL', 'SELECT name FROM users WHERE id = 2'], ['TypeScript', 'function solve(x: number): number { return x + 2; }'], ['JavaScript', 'function solve(x) { return x + 2; }'], ['Go', 'package main\nfunc solve() int { return 2 }']]) {
    const html = C.highlight(value, language);
    assert.match(html, /class="code-keyword"/, language);
    assert.match(html, /class="code-number"/, language);
    assert.ok(C.highlight(value.slice(0, -4), language), language);
  }
  for (const [alias, name] of [['cpp', 'cpp'], ['rs', 'rust'], ['python3', 'python'], ['ts', 'typescript'], ['jsx', 'javascript'], ['golang', 'go'], ['PostgreSQL', 'sql']]) assert.equal(C.language(alias), name);
  for (const name of ['custom language', '__proto__', 'constructor']) assert.equal(C.highlight('plain text', name), null);
  const value = 'const text = "<img src=x onerror=alert(1)>答え"; // <script>bad</script>';
  const html = C.highlight(value, 'JavaScript');
  assert.match(html, /&lt;img/);
  assert.match(html, /code-string/);
  assert.match(html, /code-comment/);
  assert.ok(!html.includes('<img') && !html.includes('<script>'));
  assert.equal(C.highlight('x'.repeat(32001), 'JavaScript'), null);
  assert.ok(C.highlight('('.repeat(16000) + ')'.repeat(16000), 'JavaScript'));
});

test('code indentation preserves literal contents and maps the selection without executing code', () => {
  const value = 'function solve() {\nreturn "答え";\n}';
  const from = value.indexOf('答え');
  const edit = C.indent(value, from, from + 2, 'JavaScript');
  assert.equal(edit.value, 'function solve() {\n    return "答え";\n}');
  assert.equal(edit.value.slice(edit.from, edit.to), '答え');
  assert.deepEqual(C.indent(edit.value, edit.from, edit.to, 'JavaScript'), edit);
  for (const [language, code, literal] of [['JavaScript', 'function solve() {\nconst x = `first\n  last`;\nreturn x;\n}', '`first\n  last`'], ['Python', 'def solve():\n  x = """first\n  last"""\n  return x', '"""first\n  last"""'], ['Rust', 'fn solve() {\nlet x = r#"first\n  last"#;\n}', 'r#"first\n  last"#'], ['SQL', "SELECT 'first\n  last' FROM users", "'first\n  last'"]]) {
    const changed = C.indent(code, 0, code.length, language);
    assert.ok(changed, language);
    assert.ok(changed.value.includes(literal), language);
  }
  for (const [text, from, to, language] of [['x'.repeat(32001), 0, 0, 'Java'], ['x', -1, 0, 'Go'], ['x', 1, 2, 'SQL'], ['x', 0, 0, 'unknown']]) assert.equal(C.indent(text, from, to, language), null);
});

test('Python deletes one indentation stop on blank lines while ordinary deletion stays native', () => {
  for (const [value, at, key, expected, caret] of [['    ', 4, 'Backspace', '', 0], ['        ', 8, 'Backspace', '    ', 4], ['      ', 6, 'Backspace', '    ', 4], ['  ', 2, 'Backspace', '', 0], ['\t', 1, 'Backspace', '', 0], ['    \n        ', 13, 'Backspace', '    \n    ', 9], ['        ', 0, 'Delete', '    ', 0], ['      ', 2, 'Delete', '    ', 2]]) {
    assert.deepEqual(I.codeEdit(value, at, at, key, false, 'Python'), { value: expected, from: caret, to: caret });
  }
  for (const [value, from, to, key, language] of [['', 0, 0, 'Backspace', 'Python'], ['    return 1', 4, 4, 'Backspace', 'Python'], ['    ', 0, 4, 'Backspace', 'Python'], ['    ', 4, 4, 'Backspace', 'Java'], ['    ', 0, 0, 'Delete', 'SQL']]) assert.equal(I.codeEdit(value, from, to, key, false, language), null);
});

test('answer fields are part of the work, normalised the way the server stores them', () => {
  const kept = I.workspace({ answer: '', reasoning: '', board: [], values: { b2: '50k', a1: ' 3.5 ', empty: '  ', long: 'x'.repeat(201) } });
  assert.deepEqual(Object.keys(kept.values), ['a1', 'b2']);
  assert.equal(I.workspace({ answer: 'x' }).values, undefined);
  assert.equal(JSON.stringify(I.workspace({ answer: 'x', values: {} })), JSON.stringify(I.workspace({ answer: 'x' })));
  assert.ok(I.hasWork({ answer: '', values: { a1: '3.5' } }, null));
  assert.ok(!I.hasWork({ answer: '', values: { a1: ' ' } }, null));
});

test('test results compare JSON values, floats and unordered lists like a judge would', () => {
  assert.ok(I.sameValue([0, 1], [0, 1]));
  assert.ok(!I.sameValue([0, 1], [1, 0]));
  assert.ok(I.sameValue([[1, 2], [3]], [[3], [1, 2]], true));
  assert.ok(I.sameValue(0.1 + 0.2, 0.3));
  assert.ok(I.sameValue({ a: [1, 2], b: null }, { b: null, a: [1, 2] }));
  assert.ok(!I.sameValue({ a: 1 }, { a: 1, b: 2 }));
  assert.ok(!I.sameValue('1', 1));
  assert.ok(!I.sameValue([1], [1, 1], true));
});

test('a summary of automatic checks goes to Felix so the review builds on it', () => {
  const checks = [
    { id: 't1', kind: 'test', label: 'pair_sum([2,3], 5)', pass: true, given: '[0,1]', expected: '[0,1]' },
    { id: 't2', kind: 'test', label: 'Hidden test', pass: false, given: 'None', expected: '[]' },
    { id: 's1', kind: 'section', label: 'Requirements', pass: true, given: '', expected: '' },
  ];
  assert.deepEqual(I.checkSummary(checks), [{ name: 'tests', pass: 1, total: 2 }, { name: 'parts covered', pass: 1, total: 1 }]);
  const note = I.reviewNote({ checks, grade: { criteria: [{ id: 'c1', text: 'Handles duplicates', verdict: 'partly', why: '' }] } });
  assert.match(note, /1\/2 tests/);
  assert.match(note, /Missed: Hidden test \(I wrote None\)/);
  assert.match(note, /Handles duplicates: partly/);
  assert.match(note, /not by you/);
  assert.equal(I.reviewNote({ checks: [] }), '');
});
