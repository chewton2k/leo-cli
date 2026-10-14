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

test('Enter keeps manual indentation while fragments and blank lines are being written', () => {
  for (const language of ['Python', 'C++', 'Rust', 'Java', 'SQL', 'TypeScript', 'JavaScript', 'Go']) {
    for (const value of ['    answer = 1;', '        ', '\tanswer = 1;', '    # comment\n        ']) {
      const edit = C.editor(language).edit(value, value.length, value.length, 'Enter');
      const indentation = /^[\t ]*/.exec(value.slice(value.lastIndexOf('\n') + 1))[0];
      assert.equal(edit.value.slice(edit.value.lastIndexOf('\n') + 1), indentation, `${language}: ${JSON.stringify(value)}`);
      assert.equal(edit.from, edit.value.length);
    }
    const full = '        ' + 'x'.repeat(31988);
    assert.equal(C.editor(language).edit(full, full.length, full.length, 'Enter'), null, language);
  }
});

test('Python Enter indents scopes and keeps body indentation during incremental typing', () => {
  for (const value of ['def solve():', 'if ready: # ready', 'async def solve():', 'class Solution:\n    def solve(self):', 'def solve():\n    for x in items:', 'match value:\n    case 1:', 'def solve(\n    items,\n):']) {
    const engine = C.editor('Python');
    const spaces = value.slice(value.lastIndexOf('\n') + 1).match(/^ */)[0] + '    ';
    let edit = engine.edit(value, value.length, value.length, 'Enter');
    assert.equal(edit.value.slice(edit.value.lastIndexOf('\n') + 1), spaces, value);
    edit = engine.edit(edit.value, edit.from, edit.to, 'Enter');
    assert.equal(edit.value.slice(edit.value.lastIndexOf('\n') + 1), spaces, value);
  }
  const engine = C.editor('Python');
  let value = 'class Solution:\n    ', at = value.length;
  for (const key of 'def solve(self):') {
    const edit = engine.edit(value, at, at, key);
    if (edit) { value = edit.value; at = edit.from; }
    else { value = value.slice(0, at) + key + value.slice(at); at++; }
  }
  const edit = engine.edit(value, at, at, 'Enter');
  assert.equal(edit.value, 'class Solution:\n    def solve(self):\n        ');
  assert.equal(C.editor('Python').edit('    print(value)', 16, 16, 'Enter').value, '    print(value)\n    ');
});

test('Enter keeps language-aware continuation and exits multiline delimiters in every language', () => {
  const samples = [['C++', 'int solve() {\n    return 1;\n    }'], ['Rust', 'fn solve() {\n    1\n    }'], ['Java', 'void solve() {\n    return;\n    }'], ['Python', 'items = [\n    first,\n    ]'], ['SQL', 'SELECT (\n    1\n    )'], ['TypeScript', 'function solve() {\n    return 1;\n    }'], ['JavaScript', 'function solve() {\n    return 1;\n    }'], ['Go', 'func solve() {\n    value := 1\n    }']];
  for (const [language, closed] of samples) {
    const open = 'solve(\n    first,';
    const edit = C.editor(language).edit(open, open.length, open.length, 'Enter');
    assert.ok(edit.value.slice(edit.value.lastIndexOf('\n') + 1).length >= 4, language);
    const next = C.editor(language).edit(closed, closed.length, closed.length, 'Enter');
    assert.equal(next.value.slice(next.value.lastIndexOf('\n') + 1), '', language);
  }
  for (const [language, closed] of [['C++', 'int items[] = {\n    1,\n    };'], ['JavaScript', 'const items = [\n    1,\n    ]; // done'], ['Python', 'items = [\n    1,\n    ] # done']]) {
    const edit = C.editor(language).edit(closed, closed.length, closed.length, 'Enter');
    assert.equal(edit.value.slice(edit.value.lastIndexOf('\n') + 1), '', language);
  }
});

test('submission results become named cases, and the first failure is chosen unless one is already chosen', () => {
  const checks = [
    { id: 't1', kind: 'test', label: 'pair_sum([2,3], 5)', pass: true, given: '[0,1]', expected: '[0,1]' },
    { id: 't2', kind: 'test', label: 'Hidden test', pass: false, given: 'Error: index out of range', expected: '[]' },
    { id: 'a1', kind: 'value', label: 'Expected payout ($)', pass: false, given: '3', expected: '$3.5' },
    { id: 'a2', kind: 'choice', label: 'Which rolls to keep?', pass: true, given: 'Keep 4 or higher', expected: 'Keep 4 or higher' },
    { id: 's1', kind: 'section', label: 'Requirements', pass: false, given: '', expected: '' },
  ];
  const cases = I.resultCases(checks);
  assert.deepEqual(cases.map((c) => c.name), ['Case 1', 'Case 2', 'Answer 1', 'Answer 2', 'Requirements']);
  assert.equal(cases[0].input, 'pair_sum([2,3], 5)');
  assert.equal(cases[1].input, '', 'an old hidden test saved without its call has no input to show');
  assert.equal(cases[1].title, 'Case 2 · hidden test');
  const revealed = I.resultCases([{ id: 't9', kind: 'test', label: 'pair_sum([], 0)', hidden: true, pass: false, given: 'null', expected: '[]' }]);
  assert.equal(revealed[0].input, 'pair_sum([], 0)', 'a hidden test shows its input once submitted');
  assert.equal(revealed[0].title, 'Case 1 · hidden test');
  assert.equal(cases[2].title, 'Expected payout ($)');
  assert.deepEqual([cases[2].yours, cases[2].expected], ['3', '$3.5']);
  assert.equal(cases[4].group, 'section');
  assert.equal(I.pickCase(cases, ''), 't2');
  assert.equal(I.pickCase(cases, 'a2'), 'a2', 'a chosen case stays chosen');
  assert.equal(I.pickCase(cases, 'gone'), 't2');
  const passed = I.resultCases(checks.map((c) => ({ ...c, pass: true })));
  assert.equal(I.pickCase(passed, ''), 't1');
  assert.equal(I.pickCase([], 'x'), '');
  assert.deepEqual(I.resultCases(undefined), []);
});

test('each submission is labelled passed, failed or partly from its checks, then its quick review, then Felix’s scores', () => {
  const check = (pass) => ({ id: String(Math.random()), kind: 'test', label: 'x', pass });
  assert.deepEqual(I.attemptStatus({ checks: [check(true), check(true)] }), { kind: 'passed', label: 'Passed' });
  assert.deepEqual(I.attemptStatus({ checks: [check(true), check(false), check(false)] }), { kind: 'failed', label: 'Failed · 1/3' });
  const verdicts = (...v) => ({ grade: { criteria: v.map((verdict) => ({ verdict })) } });
  assert.equal(I.attemptStatus(verdicts('met', 'met')).kind, 'passed');
  assert.equal(I.attemptStatus(verdicts('missed', 'missed')).kind, 'failed');
  assert.deepEqual(I.attemptStatus(verdicts('met', 'partly', 'missed')), { kind: 'partly', label: 'Partly · 1/3 met' });
  assert.equal(I.attemptStatus({ checks: [check(true)], ...verdicts('missed') }).kind, 'passed', 'checks decide when there are any');
  const scored = (...v) => ({ feedback: { scores: v.map((value) => ({ value })) } });
  assert.deepEqual(I.attemptStatus(scored(4, 3)), { kind: 'passed', label: 'Solid' });
  assert.deepEqual(I.attemptStatus(scored(2, 2)), { kind: 'partly', label: 'Developing' });
  assert.deepEqual(I.attemptStatus(scored(1, 0)), { kind: 'failed', label: 'Needs work' });
  assert.deepEqual(I.attemptStatus({}), { kind: 'pending', label: 'Awaiting review' });
});

test('the board selects what a drag box encloses, resizes like a slide editor and renders hit areas, handles and text', () => {
  const board = [
    { kind: 'box', points: [[100, 100], [300, 200]], text: 'API' },
    { kind: 'text', points: [[120, 400]], text: 'Queue' },
    { kind: 'arrow', points: [[300, 150], [500, 150]], text: 'calls' },
    { kind: 'pen', points: [[600, 600], [700, 650], [800, 600]], text: '' },
  ];
  assert.deepEqual(I.shapesWithin(board, I.normalRect([90, 90], [520, 420])), [0, 1, 2]);
  assert.deepEqual(I.shapesWithin(board, I.normalRect([520, 420], [90, 90])), [0, 1, 2], 'any drag direction');
  assert.deepEqual(I.shapesWithin(board, { x: 150, y: 90, w: 400, h: 400 }), [2], 'only shapes fully inside');
  assert.deepEqual(I.unionBounds(board, [0, 3]), { x: 100, y: 100, w: 700, h: 550 });
  assert.equal(I.unionBounds(board, []), null);
  assert.deepEqual(I.resizeShape(board[0], 'se', [400, 260]).points, [[100, 100], [400, 260]]);
  assert.deepEqual(I.resizeShape(board[0], 'nw', [350, 250]).points, [[300, 200], [350, 250]], 'dragging past the opposite corner flips cleanly');
  assert.deepEqual(I.resizeShape(board[0], 'n', [0, 50]).points, [[100, 50], [300, 200]]);
  assert.deepEqual(I.resizeShape(board[2], 'end', [520, 180]).points, [[300, 150], [520, 180]]);
  assert.deepEqual(I.resizeShape(board[3], 'e', [1000, 0]).points, [[600, 600], [800, 650], [1000, 600]], 'a drawing scales with its box');
  assert.equal(I.resizeShape(board[1], 'se', [999, 999]), board[1], 'labels only move');
  assert.deepEqual(I.handlesFor(board[0]).map((h) => h[0]), ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']);
  assert.deepEqual(I.handlesFor(board[2]).map((h) => h[0]), ['start', 'end']);
  assert.deepEqual(I.handlesFor(board[1]), []);
  const view = { x: 0, y: 0, w: 1000, h: 700 };
  const live = I.boardSvg(board, { view, interactive: true, selected: [0], marquee: { x: 10, y: 10, w: 50, h: 50 } });
  assert.match(live, /<rect x="100" y="100" width="200" height="100" rx="8" fill="transparent"\/>/, 'a box is clickable inside, not just on its outline');
  assert.equal((live.match(/class="board-hit"/g) || []).length, 3, 'labels, arrows and drawings get wide invisible hit areas');
  assert.equal((live.match(/data-board-handle=/g) || []).length, 8);
  assert.match(live, /class="board-marquee"/);
  assert.match(live, />API<\/text>/);
  assert.match(live, /color:#19191b/, 'black ink on the white board in every theme');
  assert.doesNotMatch(I.boardSvg(board, { view, interactive: true, selected: [0], editing: 0 }), />API<\/text>/, 'text being edited is hidden');
  const two = I.boardSvg(board, { view, interactive: true, selected: [0, 1] });
  assert.equal((two.match(/data-board-handle=/g) || []).length, 0, 'a group moves together without handles');
  const still = I.boardSvg(board, { view });
  assert.match(still, /fill="white"\/>/);
  assert.doesNotMatch(still, /board-hit|data-board-shape|fill="transparent"/);
});

test('a test case reads as a call, or as its tables for SQL', () => {
  assert.equal(I.callText('two_sum', [[2, 7], 9]), 'two_sum([2,7], 9)');
  assert.equal(I.callText('sql', ['CREATE TABLE t(x); INSERT INTO t VALUES (1);']), 'CREATE TABLE t(x); INSERT INTO t VALUES (1);');
  assert.equal(I.callText('solve'), 'solve()');
});

test('C++ tests become a driver that fills in the arguments and prints results as JSON', () => {
  const code = '#include <vector>\nclass Solution {\npublic:\n    vector<int> twoSum(vector<int>& nums, int target) {\n        return {};\n    }\n};';
  const made = I.cppDriver(code, 'twoSum', [{ id: 't1', args: [[2, 7], 9] }, { id: 't2', args: [[], -3000000000] }]);
  assert.equal(made.files['solution.cpp'], code);
  const main = made.files['main.cpp'];
  assert.match(main, /using namespace std;[\s\S]*#include "solution\.cpp"/);
  assert.match(main, /std::vector<int> a0 = std::vector<int>\{2, 7\}; int a1 = 9; auto leo_result = Solution\(\)\.twoSum\(a0, a1\); std::cout << "\\n@@leo t1 "; leo_out\(std::cout, leo_result\);/);
  assert.match(main, /int a1 = -3000000000LL;/);
  const text = I.cppDriver('string echo(const std::string& s, char c, bool b, double d) { return s; }', 'echo', [{ id: 'x', args: ['a"\\\né', 'q', true, 0.5] }]).files['main.cpp'];
  assert.match(text, /std::string a0 = "a\\"\\\\\\n\\303\\251"; char a1 = 'q'; bool a2 = true; double a3 = 0.5; auto leo_result = echo\(a0, a1, a2, a3\);/);
  const inPlace = I.cppDriver('void rev(vector<int>& v) {}', 'rev', [{ id: 'r', args: [[1]] }]).files['main.cpp'];
  assert.match(inPlace, /rev\(a0\); std::cout << "\\n@@leo r "; leo_out\(std::cout, a0\);/);
  assert.match(I.cppDriver('ListNode* f(ListNode* h) { return h; }', 'f', [{ id: 'a', args: [[1]] }]).error, /types the browser can’t fill in/);
  assert.match(I.cppDriver('int f(int a) { return a; }', 'f', [{ id: 'a', args: [1, 2] }]).error, /gives 2 arguments, but f takes 1/);
  assert.match(I.cppDriver('int f(int a) { return a; }', 'f', [{ id: 'a', args: [1.5] }]).error, /expected a whole number/);
  assert.match(I.cppDriver('int g() { return 1; }', 'f', []).error, /Define a function named f/);
  assert.deepEqual(I.cppResults('hello\n@@leo t1 [0,1]\n\n@@leo t2 "x"\n@@leo t3 oops'), { results: [{ id: 't1', got: [0, 1] }, { id: 't2', got: 'x' }, { id: 't3', got: 'oops' }], logs: ['hello'] });
  assert.equal(I.cppProblem('In file included from main.cpp:29:\n./solution.cpp:1:23: error: nope\n'), 'line 1:23: error: nope');
});

test('Go tests become a driver beside the user’s file, with missing imports added and lines kept', () => {
  const made = I.goDriver('func twoSum(nums []int, target int) []int {\n\tsort.Ints(nums)\n\tfmt.Println(strings.Repeat("a", 2))\n\treturn nil\n}', 'twoSum', [{ id: 't1', args: [[2, 7], 9] }]);
  assert.equal(made.offset, 1);
  assert.equal(made.files['solution.go'].split('\n')[0], 'package main; import ("fmt"; "strings"; "sort");');
  assert.match(made.files['leo_driver.go'], /var leoEntry = twoSum\n\nconst leoTests = "\[\{\\"id\\":\\"t1\\",\\"args\\":\[\[2,7\],9\]\}\]"/);
  assert.match(made.files['leo_driver.go'], /^package main\n/);
  const own = I.goDriver('package solution\n\nimport "fmt"\n\nfunc f() { fmt.Println() }', 'f', []);
  assert.equal(own.offset, 0);
  assert.match(own.files['solution.go'], /^package main\n\nimport "fmt"/);
  assert.match(I.goDriver('func main() {}\nfunc f() {}', 'f', []).error, /Remove func main/);
  assert.match(I.goDriver('func g() {}', 'f', []).error, /Define a function named f/);
  assert.ok(I.goDriver('func (s *Solution) f() {}\nfunc f[T any](x T) T { return x }', 'f', []).files);
  assert.deepEqual(I.goResults('hi\n\n@@leo {"id":"t1","got":[0,1],"ms":2}\n\n@@leo {"id":"t2","error":"panic: boom","ms":0}\n'), { results: [{ id: 't1', got: [0, 1], ms: 2 }, { id: 't2', error: 'panic: boom', ms: 0 }], logs: ['hi'], error: '' });
  assert.equal(I.goResults('@@leo-error the tests could not be read').error, 'the tests could not be read');
  assert.equal(I.goProblem('# main\n/work/solution.go:3:9: undefined: y\n', 1), 'line 2:9: undefined: y');
  const leet = I.goDriver('/**\n * type ListNode struct {\n *     Val int\n *     Next *ListNode\n * }\n */\nfunc reverseList(head *ListNode) *ListNode { return head }', 'reverseList', []);
  assert.match(leet.files['leo_driver.go'], /type ListNode struct \{\n\tVal  int\n\tNext \*ListNode\n\}/);
  assert.doesNotMatch(leet.files['leo_driver.go'], /type TreeNode/);
  assert.doesNotMatch(I.goDriver('type ListNode struct { Val int; Next *ListNode }\nfunc f(h *ListNode) {}', 'f', []).files['leo_driver.go'], /type ListNode/);
});
