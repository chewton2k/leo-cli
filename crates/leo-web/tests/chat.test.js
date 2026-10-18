'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/chat.js');

const C = globalThis.leoChat;
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

test('a streamed reply is read line by line, keeping a half line for later', () => {
  const first = C.splitLines('{"sources":[]}\n{"t":"Hel');
  assert.deepEqual(first.events, [{ sources: [] }]);
  assert.equal(first.rest, '{"t":"Hel');
  const second = C.splitLines(first.rest + 'lo"}\n{"done":true}\n');
  assert.deepEqual(second.events, [{ t: 'Hello' }, { done: true }]);
  assert.equal(second.rest, '');
  assert.ok(C.splitLines('not json\n').events[0].error);
});

test('a graded answer is marked, and the marker never shows', () => {
  assert.deepEqual(C.grade('[[correct]] Nice, the heap keeps the minimum on top.'), { verdict: 'correct', text: 'Nice, the heap keeps the minimum on top.' });
  assert.deepEqual(C.grade('  [[Incorrect]]\nNot quite.'), { verdict: 'incorrect', text: 'Not quite.' });
  assert.deepEqual(C.grade('[[corr'), { verdict: null, text: '' }, 'a marker still arriving stays hidden');
  assert.deepEqual(C.grade('A [[link]] in the middle'), { verdict: null, text: 'A [[link]] in the middle' });
});

test('citations become note chips, and ones Felix was never given are dropped', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps' }, { n: 2, id: 'b', title: 'A <long> title that keeps going on and on' }];
  const html = C.cite('<p>Min on top [n1]. Both [n1, n2]. Nope [n9].</p>', sources, esc);
  assert.equal((html.match(/class="cite"/g) || []).length, 3);
  assert.ok(html.includes('data-id="a"'));
  assert.ok(html.includes('&lt;long&gt;'), 'titles are escaped');
  assert.ok(!html.includes('[n9]'), 'a citation of nothing is not shown');
  assert.ok(html.includes('Nope.</p>'), 'and leaves no gap');
  assert.ok(html.includes('Min on top <button'), 'a kept citation keeps its space');
  assert.deepEqual(C.cited('one [n2] and [n1, n2]', sources).map((s) => s.id), ['a', 'b']);
  assert.deepEqual(C.cited('no citations here n1', sources), []);
});

test('the conversation is kept per browser and survives broken storage', () => {
  const values = new Map();
  const storage = { getItem: (k) => values.get(k) || null, setItem: (k, v) => values.set(k, v) };
  C.save(storage, 'study', Array.from({ length: 60 }, (_, i) => ({ role: 'user', text: `m${i}` })));
  const back = C.load(storage);
  assert.equal(back.mode, 'study');
  assert.equal(back.messages.length, 40);
  assert.equal(back.messages[0].text, 'm20');
  const broken = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
  assert.deepEqual(C.load(broken), { id: null, mode: 'chat', messages: [], refs: [] });
  C.save(broken, 'ask', []);
});

test('Felix keeps his colour and shape', () => {
  const svg = C.felix(40, 'idle');
  assert.ok(svg.includes('class="felix idle"'));
  const body = /class="felix-skin" x="\d+" y="\d+" width="(\d+)" height="(\d+)"/.exec(svg);
  const ratio = Number(body[1]) / Number(body[2]);
  assert.ok(ratio > 1 && ratio < 1.4, `a box a little wider than tall, not ${ratio}`);
  assert.equal((svg.match(/width="4" height="4"/g) || []).length, 2, 'two eyes');
  assert.match(svg, /class="felix-eyes" shape-rendering="geometricPrecision"/, 'eyes are drawn exactly, so both come out the same size');
  assert.equal((svg.match(/class="felix-arm felix-(left|right)"/g) || []).length, 2, 'two arms');
  assert.match(svg, /<g class="felix-arm felix-left"><rect class="felix-arm-skin"[^>]*\/><g class="felix-claws">/, 'the monster’s claws move with his arms');
  assert.match(svg, /<g class="felix-rawr">/);
  assert.deepEqual(C.MODES.map((m) => m.id), ['chat', 'study', 'interview']);
});

test('Demon Felix has his own taps, flames, slashes and words, and cute Felix can still surprise', () => {
  const svg = C.felix(40, 'idle');
  for (const part of ['felix-flames', 'felix-slash', 'felix-hehe', 'felix-hi', 'felix-rawr', 'felix-monster']) assert.match(svg, new RegExp(`<g class="${part}">`), part);
  assert.equal(C.DEMON_TAPS.length, C.TAPS.length);
  assert.ok(C.DEMON_TAPS.every((tap) => !C.TAPS.includes(tap) && tap !== 'monster' && tap !== 'sweet'));
  assert.equal(C.MONSTER_ONE_IN, 10);
  assert.ok(C.TICKLED_TOO_LONG > 3 && C.TICKLED_TOO_LONG <= 10);
});

test('typing @ starts a note search, but an email address does not', () => {
  assert.deepEqual(C.mentionAt('compare @heap', 13), { start: 8, query: 'heap' });
  assert.deepEqual(C.mentionAt('@', 1), { start: 0, query: '' });
  assert.deepEqual(C.mentionAt('see @graph trav', 15), { start: 4, query: 'graph trav' });
  assert.equal(C.mentionAt('mail me at a@b.com', 18), null);
  assert.equal(C.mentionAt('@ nothing', 9), null);
  assert.equal(C.mentionAt('@line\nnext', 10), null);
  assert.equal(C.mentionAt('no mention here', 15), null);
});

test('a note is added once, and only up to the limit', () => {
  let refs = C.addRef([], { id: 'a', title: 'Heaps' });
  refs = C.addRef(refs, { id: 'a', title: 'Heaps' });
  assert.deepEqual(refs, [{ id: 'a', title: 'Heaps' }]);
  for (let i = 0; i < 20; i++) refs = C.addRef(refs, { id: `n${i}`, title: '' });
  assert.equal(refs.length, C.MOST_REFS);
  assert.equal(refs[1].title, 'Untitled');
});

test('attached notes are kept with the conversation', () => {
  const store = new Map();
  const storage = { getItem: (k) => store.get(k) || null, setItem: (k, v) => store.set(k, v) };
  C.save(storage, 'study', [{ role: 'user', text: 'hi' }], [{ id: 'a', title: 'Heaps' }], 'chat-1234');
  assert.deepEqual(C.load(storage), { id: 'chat-1234', mode: 'study', messages: [{ role: 'user', text: 'hi' }], refs: [{ id: 'a', title: 'Heaps' }] });
});

test('a style saved before there were two lands in the one that took it over', () => {
  assert.equal(C.modeOf('quiz'), 'study');
  assert.equal(C.modeOf('coach'), 'study');
  assert.equal(C.modeOf('meeting'), 'chat');
  assert.equal(C.modeOf('explain'), 'chat');
  assert.equal(C.modeOf('ask'), 'chat');
  assert.equal(C.modeOf(undefined), 'chat');
  const storage = { getItem: () => JSON.stringify({ mode: 'quiz', messages: [] }), setItem() {} };
  assert.equal(C.load(storage).mode, 'study');
});

test('past chats are grouped the way chat apps group them', () => {
  const now = new Date(2026, 9, 7, 15, 0);
  const at = (days, hour = 10) => ({ id: `c${days}`, title: 't', updated_at: new Date(2026, 9, 7 - days, hour).toISOString() });
  const g = C.groups([at(0), at(1), at(3), at(20), at(90)], now);
  assert.deepEqual(g.map((x) => x.name), ['Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older']);
  assert.deepEqual(C.groups([at(0, 1), at(0, 9)], now).map((x) => x.chats.length), [2]);
  assert.deepEqual(C.groups([], now), []);
});

test('a chat id is safe in a file name even without randomUUID', () => {
  const saved = globalThis.crypto;
  Object.defineProperty(globalThis, 'crypto', { value: {}, configurable: true });
  const id = C.newId();
  Object.defineProperty(globalThis, 'crypto', { value: saved, configurable: true });
  assert.match(id, /^[a-z0-9-]{8,64}$/);
  assert.match(C.newId(), /^[a-zA-Z0-9-]{8,64}$/);
});

test('a starter that needs classes names the ones picked', () => {
  const plan = C.MODES[1].starters[2];
  const across = C.MODES[1].starters[1];
  assert.equal(C.starterWords(plan, ['cs130']), 'Make me a 3-day review plan for cs130');
  assert.equal(C.starterWords(plan, ['cs130', 'math61', 'phys1b']), 'Make me a 3-day review plan for cs130, math61 and phys1b');
  assert.equal(C.starterWords(across, ['cs130', 'math61']), 'Quiz me across cs130 and math61');
  for (const m of C.MODES) for (const s of m.starters) assert.ok(s.ask, `${s.text} asks when nothing is open`);
});

test('splitFiles keeps files already sent out of the box for the next message', () => {
  const docs = [{ id: 'a', name: 'week3.txt' }, { id: 'b', name: 'week4.txt' }, { id: 'c', name: 'old.pdf' }];
  const messages = [
    { role: 'user', text: 'what is in it?', docs: ['week3.txt'], files: ['a'] },
    { role: 'assistant', text: 'heaps', files: ['b'] },
    { role: 'user', text: 'before files had ids', docs: ['old.pdf'] },
  ];
  const { sent, waiting } = C.splitFiles(docs, messages);
  assert.deepEqual(sent.map((d) => d.id), ['a', 'c']);
  assert.deepEqual(waiting.map((d) => d.id), ['b']);
  assert.deepEqual(C.splitFiles(docs, []).waiting.length, 3);
  assert.deepEqual(C.splitFiles(undefined, undefined), { sent: [], waiting: [] });
});

test('asNote keeps the question, links cited notes and drops the grade', () => {
  const sources = [{ n: 1, id: 'a', title: 'Graph traversals' }, { n: 2, id: 'b', title: 'Queues [old]' }];
  const note = C.asNote('  how does   BFS work? ', '[[correct]] It takes vertices from a queue [n1, n2] in order [n9].', sources);
  assert.equal(note.title, 'how does BFS work?');
  assert.equal(note.body, '**Q:** how does BFS work?\n\nIt takes vertices from a queue [[Graph traversals]] [[Queues old]] in order [n9].');
  const long = C.asNote('Explain why breadth first search finds the shortest path in an unweighted graph every time', 'Because.', []);
  assert.ok(long.title.endsWith('…'));
  assert.ok(long.title.length <= 71);
  assert.ok(!long.title.includes('  '));
  assert.deepEqual(C.asNote('', 'Just text', []), { title: 'From Felix', body: 'Just text' });
});

test('reviewPrompt asks each missed question again with the old answer', () => {
  const text = C.reviewPrompt([
    { question: 'What does BFS use?', answer: 'a stack' },
    { question: 'Define a heap.', answer: '' },
  ]);
  assert.match(text, /one at a time/);
  assert.match(text, /\n1\. What does BFS use\? \(last time I said: a stack\)\n2\. Define a heap\.$/);
});

test('each tool gives Felix its own prop, and anything else none', () => {
  assert.equal(C.poseOf('search_notes'), 'tool-search');
  assert.equal(C.poseOf('open_note'), 'tool-open');
  assert.equal(C.poseOf('connected_notes'), 'tool-map');
  assert.equal(C.poseOf('edit_note'), 'tool-edit');
  assert.equal(C.poseOf('create_note'), 'tool-create');
  assert.equal(C.poseOf('delete_everything'), null);
  for (const prop of ['search', 'open', 'map', 'edit', 'create']) assert.ok(C.felix(40).includes(`felix-tool-${prop}`), prop);
});

test('what an answer cost reads as money for keys, model and effort for plans, free for this computer', () => {
  const paid = C.spentLabel({ by: 'Anthropic', model: 'claude-sonnet-5-5', input: 12000, output: 800, cost: 0.032, steps: 2 });
  assert.equal(paid.text, '$0.032 · Anthropic · claude-sonnet-5-5');
  assert.equal(paid.title, '12,000 tokens in, 800 out over 2 steps');
  assert.equal(C.spentLabel({ by: 'OpenAI', model: 'gpt-5-nano', cost: 0.00042, estimated: true }).text, '≈ $0.0004 · OpenAI · gpt-5-nano');
  assert.equal(C.spentLabel({ by: 'OpenAI', model: 'gpt-5.5', cost: 1.234 }).text, '$1.23 · OpenAI · gpt-5.5');
  assert.equal(C.spentLabel({ by: 'Codex', model: 'gpt-6.1-sol', effort: 'high', plan: true }).text, 'Codex · gpt-6.1-sol · high effort · on your plan');
  assert.equal(C.spentLabel({ by: 'Claude Code', model: 'claude-opus-5-5', plan: true }).text, 'Claude Code · claude-opus-5-5 · default effort · on your plan');
  assert.equal(C.spentLabel({ by: 'Ollama', model: 'qwen3:8b', local: true, cost: 0 }).text, 'Ollama · qwen3:8b · free on this computer');
  assert.equal(C.spentLabel({ by: 'OpenRouter', model: 'x/y' }).text, 'OpenRouter · x/y · price unknown');
  assert.equal(C.spentLabel(null), null);
  assert.equal(C.spentLabel({ cost: 1 }), null);
});

test('a file card names its type and shows a small example of what is inside', () => {
  assert.deepEqual(C.fileKind('Week 3.pdf'), { label: 'PDF', tone: 'pdf' });
  assert.deepEqual(C.fileKind('essay.DOCX'), { label: 'DOCX', tone: 'doc' });
  assert.deepEqual(C.fileKind('slides.pptx'), { label: 'PPTX', tone: 'slides' });
  assert.deepEqual(C.fileKind('grades.csv'), { label: 'CSV', tone: 'sheet' });
  assert.deepEqual(C.fileKind('board.jpeg'), { label: 'JPG', tone: 'image' });
  assert.deepEqual(C.fileKind('notes.md'), { label: 'MD', tone: 'text' });
  assert.deepEqual(C.fileKind('README'), { label: 'FILE', tone: 'other' });
  const doc = C.fileCard({ name: 'a<b>.pdf', excerpt: 'Week 3\nHeaps' }, esc, { remove: 'd1' });
  assert.match(doc, /class="file-card tone-pdf"/);
  assert.match(doc, /<span class="file-page">Week 3\nHeaps<\/span>/);
  assert.match(doc, /a&lt;b&gt;\.pdf/);
  assert.match(doc, /data-chat="unfile" data-id="d1"/);
  const pic = C.fileCard({ name: 'p.png', thumb: 'data:image/png;base64,AAAA' }, esc);
  assert.match(pic, /<img src="data:image\/png;base64,AAAA"/);
  assert.doesNotMatch(pic, /file-x/);
  const odd = C.fileCard({ name: 'p.png', thumb: 'javascript:alert(1)' }, esc, { reading: true });
  assert.doesNotMatch(odd, /<img/);
  assert.match(odd, /Reading…/);
});

test('Felix has three ways of handling notes and Shift+Tab goes round them', () => {
  assert.deepEqual(C.ACCESS.map((a) => a.id), ['ask', 'auto', 'read']);
  assert.equal(C.accessOf('auto'), 'auto');
  assert.equal(C.accessOf('everything'), 'ask');
  assert.equal(C.accessOf(null), 'ask');
  assert.equal(C.nextAccess('ask'), 'auto');
  assert.equal(C.nextAccess('auto'), 'read');
  assert.equal(C.nextAccess('read'), 'ask');
});

test('notes attached earlier in a chat come along with later questions, newest first', () => {
  const said = (refs) => ({ role: 'user', text: 'q', refs });
  const messages = [said([{ id: 'a', title: 'A' }, { id: 'b', title: 'B' }]), { role: 'assistant', text: 'x', refs: [{ id: 'z' }] }, said([]), said([{ id: 'c', title: 'C' }, { id: 'a', title: 'A' }])];
  assert.deepEqual(C.threadRefs(messages).map((r) => r.id), ['c', 'a', 'b']);
  assert.deepEqual(C.threadRefs([]), []);
  const many = Array.from({ length: 12 }, (_, i) => said([{ id: `n${i}`, title: 'N' }]));
  assert.equal(C.threadRefs(many).length, C.MOST_REFS);
});

test('notes cited in the last three answers are sent along, newest first', () => {
  const said = (text, sources) => ({ role: 'assistant', text, sources });
  const s = (n, id) => ({ n, id, title: id });
  const messages = [
    said('old [n1]', [s(1, 'oldest')]),
    { role: 'user', text: 'q' },
    said('uses [n1] and [n2]', [s(1, 'a'), s(2, 'b')]),
    said('nothing cited', [s(1, 'c')]),
    said('only [n2]', [s(1, 'x'), s(2, 'd')]),
  ];
  assert.deepEqual(C.recentNotes(messages), ['d', 'a', 'b']);
  assert.deepEqual(C.recentNotes([]), []);
});

test('a practice answer is checked kindly: case, accents, spaces and punctuation do not count', () => {
  const mc = { kind: 'multiple_choice', question: 'What does BFS use?', options: ['a stack', 'a queue'], answer: 'a queue' };
  assert.equal(C.checkQuiz(mc, 'A Queue'), true);
  assert.equal(C.checkQuiz(mc, 'a stack'), false);
  const blank = { kind: 'fill_blank', question: "f'(x) = 2x - 4 ___", answer: '\\sin(x)' };
  assert.equal(C.checkQuiz(blank, 'sin x'), null, 'a blank is marked by Felix, who reads what it means');
  assert.equal(C.checkQuiz(blank, 'newest'), null);
  const asked = C.quizSay({ kind: 'fill_blank', question: 'q', answer: 'oldest | earliest' }, 'the earliest one', null);
  assert.ok(asked.includes('Accepted answers: oldest or earliest.'));
  assert.ok(asked.includes('sin x and sin(x)'), 'notation does not decide the mark');
  assert.equal(C.checkQuiz({ kind: 'free_response', answer: 'Because…' }, 'anything'), null, 'free answers are marked by Felix');
});

test('what Felix is told about a practice answer says how it went and how to reply', () => {
  const mc = { kind: 'multiple_choice', question: 'What does BFS use?', answer: 'a queue' };
  const wrong = C.quizSay(mc, 'a stack', false);
  assert.ok(wrong.includes('Question: What does BFS use?'));
  assert.ok(wrong.includes('My answer: a stack'));
  assert.ok(wrong.includes('the right answer is a queue'));
  assert.ok(wrong.includes('[[incorrect]]'));
  assert.ok(C.quizSay(mc, 'a queue', true).includes('[[correct]]'));
  const free = C.quizSay({ kind: 'free_response', question: 'Why a queue?', answer: 'Oldest first.' }, 'it is fair', null);
  assert.ok(free.includes('[[correct]] or [[incorrect]]'));
  assert.ok(free.includes('Oldest first.'));
  assert.ok(C.quizSay({ kind: 'fill_blank', question: 'q', answer: 'oldest | earliest' }, 'x', false).includes('the right answer is oldest.'));
});

test('tokens read from the cache are named in the cost line tooltip', () => {
  const label = C.spentLabel({ by: 'Anthropic', model: 'claude-sonnet-5-5', input: 12000, cached: 9000, output: 800, cost: 0.01, steps: 3 });
  assert.equal(label.title, '12,000 tokens in (9,000 from the cache), 800 out over 3 steps');
  assert.ok(!C.spentLabel({ by: 'Anthropic', model: 'm', input: 5, output: 1, cost: 0 }).title.includes('cache'));
});

test('what any model is told about an earlier answer includes what Felix did, asked and suggested, and who wrote it', () => {
  const answer = {
    role: 'assistant',
    text: '[[incorrect]] It uses a queue [n1].',
    mode: 'study',
    spent: { by: 'Codex', model: 'gpt-6.1-sol' },
    steps: [{ text: 'Searched your notes for “queue”', found: ['Graph traversals', 'Heaps'] }, 'Opened “Graph traversals”'],
    asks: [{ question: 'Which week?', options: ['1', '2'], answered: '2' }],
    quizzes: [{ kind: 'multiple_choice', question: 'What does BFS use?', options: ['a stack', 'a queue'], answer: 'a queue', given: 'a stack', correct: false }],
    proposals: [{ kind: 'edit', title: 'Graph traversals', find: 'stack', replace: 'queue', state: 'applied' }],
  };
  const text = C.historyText(answer);
  assert.ok(text.startsWith('[Written by Codex · gpt-6.1-sol, in study style]\n[What Felix did'), 'notes come first, so clipping a long answer keeps them');
  assert.ok(text.endsWith('\nIt uses a queue [n1].'), 'the verdict marker is not repeated');
  assert.ok(text.includes('[What Felix did for this answer: Searched your notes for “queue” (found: Graph traversals, Heaps); Opened “Graph traversals”]'));
  assert.ok(text.includes('[Felix asked: Which week? (choices: 1 | 2); the user answered: 2]'));
  assert.ok(text.includes('[Practice question (multiple choice): What does BFS use?; choices: a stack | a queue; answer: a queue; answered “a stack”, wrong]'));
  assert.ok(text.includes('Suggested a change to “Graph traversals”: “stack” → “queue”; applied]'));
  assert.equal(C.historyText({ role: 'assistant', text: 'Plain.' }), 'Plain.');
  assert.equal(C.historyText({ role: 'user', text: 'a stack', say: 'Quiz answer. …' }), 'Quiz answer. …');
});

test('notes Felix opened, not only the ones he cited, come along after a switch', () => {
  const messages = [{ role: 'assistant', text: 'No citation here.', sources: [{ n: 1, id: 'opened-1', title: 'A', why: 'opened by Felix' }, { n: 2, id: 'found-1', title: 'B', why: 'found by searching for "x"' }] }];
  assert.deepEqual(C.recentNotes(messages), ['opened-1']);
});

test('a verdict after a short lead-in still counts, and the lead-in is dropped', () => {
  assert.deepEqual(C.grade("I'll mark this answer and give you the next card. [[incorrect]] It uses a queue."), { verdict: 'incorrect', text: 'It uses a queue.' });
  assert.deepEqual(C.grade("I'll mark this [[inc"), { verdict: null, text: "I'll mark this " }, 'a marker still arriving after a lead-in stays hidden');
  const late = `${'word '.repeat(80)}[[correct]] later`;
  assert.equal(C.grade(late).verdict, null, 'a marker deep inside an answer is not a verdict');
});

test('a hint asks for a nudge without the answer, and every mark explains itself', () => {
  const mc = { kind: 'multiple_choice', question: 'Which equation?', options: ['x = 2 sin x', 'x = 0'], answer: 'x = 2 sin x' };
  const hint = C.hintSay(mc);
  assert.ok(hint.includes('Choices: x = 2 sin x | x = 0'));
  assert.ok(hint.includes('Do not give the answer'));
  const wrong = C.quizSay(mc, 'x = 0', false);
  assert.ok(wrong.includes('do not give a hint') && wrong.includes('why each of the other choices is wrong'));
  const right = C.quizSay(mc, 'x = 2 sin x', true);
  assert.ok(right.includes('praise me briefly in your own words') && right.includes('explain why it is right'));
  const free = C.quizSay({ kind: 'free_response', question: 'Why?', answer: 'Because.' }, 'dunno', null);
  assert.ok(free.includes('intuitive shortcut or tip') && free.includes('praise me briefly'));
  for (const said of [wrong, right, free]) assert.ok(said.includes('give it once, at the very end'));
});

test('each further hint knows the hints already given and goes one step further', () => {
  const q = { kind: 'free_response', question: 'Why does BFS find shortest paths?', hints: [{ text: 'Think about the order it visits vertices.' }] };
  const said = C.hintSay(q);
  assert.ok(said.includes('Hints you already gave me:\n1. Think about the order it visits vertices.'));
  assert.ok(said.includes('one step further'));
  assert.ok(said.includes('Do not give the answer'));
  assert.ok(C.hintSay({ kind: 'free_response', question: 'Why?' }).includes('Give me one short hint'));
});


test('only explicitly generated interview snapshots are replaced after a chat reload', () => {
  const docs = [{ id: 'board-1', name: 'board.png' }, { id: 'notes-1', name: 'notes.pdf' }, { id: 'upload-1', name: 'my-board.png' }];
  const messages = [{ role: 'user', files: ['board-1', 'notes-1'], cards: [{ name: 'board.png', interviewBoard: true }, { name: 'notes.pdf' }] }];
  const { sent, waiting } = C.splitFiles(docs, messages);
  assert.deepEqual(sent, [{ ...docs[0], interviewBoard: true }, docs[1]]);
  assert.deepEqual(waiting, [docs[2]]);
  assert.equal(docs[0].interviewBoard, undefined);
  assert.deepEqual(C.splitFiles(docs, [{ role: 'assistant', cards: [{ name: 'my-board.png', interviewBoard: true }] }]).waiting, docs);
});

test('a selection in an answer maps back to its whole Markdown lines, fences and math kept', () => {
  const source = 'Intro line.\n\nA **heap** keeps the *minimum* at the root [n1].\n\n```js\nconst a = 1;\nconst b = 2;\n```\n\n$$\nx^2\n$$\nEnd.';
  assert.equal(C.answerLines(source, 'heap keeps the minimum'), 'A **heap** keeps the *minimum* at the root [n1].');
  assert.equal(C.answerLines(source, 'const b = 2;'), '```js\nconst a = 1;\nconst b = 2;\n```');
  assert.equal(C.answerLines(source, 'x2'), '$$\nx^2\n$$');
  assert.equal(C.answerLines(source, 'Intro line.\n\nA heap keeps'), 'Intro line.\n\nA **heap** keeps the *minimum* at the root [n1].');
  assert.equal(C.answerLines(source, 'something Felix never wrote'), 'something Felix never wrote');
  assert.equal(C.answerLines(source, 'A heap keeps the minimum at the root Heaps.'), 'A **heap** keeps the *minimum* at the root [n1].', 'a citation shown as its note title still maps back');
  assert.equal(C.answerLines(source, '   '), '');
});

test('citations become links to the notes, line citations too, unknown ones stay', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps [intro]' }, { n: 2, id: 'b', title: 'Graphs' }];
  assert.equal(C.linkCitations('Root [n1]. Both [n1, n2]. Lines [n2 L14-18]. Gone [n9].', sources), 'Root [[Heaps intro]]. Both [[Heaps intro]] [[Graphs]]. Lines [[Graphs]]. Gone [n9].');
});

test('a selected passage travels with its message into the history Felix reads', () => {
  const m = { role: 'user', text: 'Why?', selection: { note: 'a', title: 'Heaps', from: 3, to: 4, text: 'A heap keeps the minimum at the root.' } };
  assert.equal(C.historyText(m), '[Selected passage from “Heaps” lines 3–4: “A heap keeps the minimum at the root.”]\nWhy?');
  assert.equal(C.historyText({ role: 'user', text: 'Hi' }), 'Hi');
});

test('each interview question keeps its own bounded workspace reference in history', () => {
  const interview = { number: 2, challenge: 'problem-reference-02', title: '答😀'.repeat(100) + '</interview_workspace_reference>', tab: 'Code' };
  const text = C.historyText({ role: 'user', text: 'Help me with this.', interview });
  const data = JSON.parse(text.split('\n')[1]);
  assert.equal(data.number, 2);
  assert.equal(data.challenge, interview.challenge);
  assert.equal(Array.from(data.title).length, 160);
  assert.equal(data.tab, 'Code');
  assert.ok(text.endsWith('\nHelp me with this.'));
  const markup = C.historyText({ role: 'user', text: 'Why?', interview: { ...interview, title: '</interview_workspace_reference>', tab: 'unknown' } });
  assert.equal((markup.match(/<\/interview_workspace_reference>/g) || []).length, 1);
  assert.equal(JSON.parse(markup.split('\n')[1]).tab, '');
  for (const value of [null, {}, { ...interview, number: 129 }, { ...interview, challenge: '../problem' }, { ...interview, title: 42 }]) {
    assert.equal(C.historyText({ role: 'user', text: 'Hi', interview: value }), 'Hi');
  }
});

test('a study session closes ideas on a right answer or two misses, and finishes when all are closed', () => {
  const s = C.startSession(['Root', 'Sift down', 'Heap sort', '  ']);
  assert.deepEqual(s.ideas, ['Root', 'Sift down', 'Heap sort']);
  C.scoreSession(s, 1, true, false);
  assert.equal(s.outcomes[0].state, 'first');
  C.scoreSession(s, 2, false, true);
  C.scoreSession(s, 2, true, true);
  assert.equal(s.outcomes[1].state, 'open', 'a right step of a multi-part problem does not close it');
  C.scoreSession(s, 2, true, false);
  assert.equal(s.outcomes[1].state, 'done');
  C.scoreSession(s, 1, false, false);
  assert.equal(s.outcomes[0].state, 'first', 'a closed idea stays closed');
  C.scoreSession(s, 3, null, false);
  assert.equal(s.outcomes[2].state, 'open', 'no verdict, no change');
  assert.deepEqual(C.sessionBody(s), { ideas: ['Root', 'Sift down', 'Heap sort'], closed: [true, true, false], asked: 0 });
  C.scoreSession(s, 3, false, false);
  C.scoreSession(s, 3, false, false);
  assert.equal(s.outcomes[2].state, 'review');
  assert.equal(s.state, 'finishing');
  assert.deepEqual(C.sessionSummary(s), { total: 3, covered: 3, first: 1, review: ['Heap sort'], skipped: [] });
  s.state = 'finished';
  assert.equal(C.sessionBody(s), null);
  const capped = C.startSession(['A', 'B']);
  capped.asked = 20;
  C.scoreSession(capped, 1, true, false);
  assert.equal(capped.state, 'finishing');
});


test('tool activity follows streamed text without dropping text or splitting Unicode', () => {
  const text = 'Read first.\n\nReview next.\n\nDone.';
  const parts = C.timeline(text, [{ text: 'Read attempt', at: 13 }, { text: 'Read progress', at: 27 }, { text: 'Saved review', at: 27 }]);
  assert.equal(parts.map((p) => p.text || '').join(''), text);
  assert.deepEqual(parts.map((p) => p.text || p.steps.map((s) => s.text)), ['Read first.\n\n', ['Read attempt'], 'Review next.\n\n', ['Read progress', 'Saved review'], 'Done.']);
  const emoji = C.timeline('答😀え', [{ text: 'Looked', at: 2 }]);
  assert.equal(emoji[0].text, '答😀');
  assert.equal(emoji[2].text, 'え');
  const malformed = C.timeline('answer', [null, {}, { text: 'Read', at: -4, found: 'bad' }, { text: 'Done', at: 999 }, { text: 'Later', at: 1 }]);
  assert.equal(malformed.map((p) => p.text || '').join(''), 'answer');
  assert.equal(malformed[0].steps[0].found.length, 0);
  assert.equal(C.timeline('answer', Array(100).fill('Read')).flatMap((p) => p.steps || []).length, 64);
  assert.deepEqual(C.timeline('old answer', ['Legacy step']).map((p) => p.text || p.steps[0].text), ['Legacy step', 'old answer']);
});

test('conversation rows display only escaped titles, including search results', () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8');
  const row = source.slice(source.indexOf('    function chatRow(c) {'), source.indexOf('\n    let resumeSeq', source.indexOf('    function chatRow(c) {')));
  const context = vm.createContext({ state: { id: 'chat-one', doomed: null }, escape: esc });
  vm.runInContext(row, context);
  for (const about of ['', 'A response preview that should stay hidden', '<script>bad()</script>']) {
    const html = context.chatRow({ id: 'chat-one', title: 'Graphs <review>', about });
    assert.ok(html.includes('Graphs &lt;review&gt;'));
    assert.ok(!html.includes('chat-history-about'));
    if (about) assert.ok(!html.includes(about));
    assert.ok(html.includes('data-chat="resume"') && html.includes('data-chat="forget"'));
  }
});

test('problem deletion cards can be applied again after Undo with fresh revisions and tokens', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8');
  const organize = source.slice(source.indexOf('    async function organize('), source.indexOf('    async function undo('));
  for (const initial of ['delete_interview_problem', 'restore_interview_problem']) {
    let revision = 7;
    let token = 'first-token';
    let removed = initial === 'restore_interview_problem';
    const sent = [], errors = [];
    const deletion = () => ({ kind: 'delete_interview_problem', interview: 'practice', challenge: 'problem', revision, title: 'Problem', summary: 'Delete problem' });
    const restoration = () => ({ kind: 'restore_interview_problem', interview: 'practice', token, title: 'Problem', summary: 'Restore problem' });
    const context = vm.createContext({
      draw: () => {}, remember: () => {}, onOrganized: () => {}, notify: message => errors.push(message),
      fetch: async (_url, options) => {
        const action = JSON.parse(options.body);
        sent.push(action);
        const valid = action.kind === 'delete_interview_problem' ? !removed && action.revision === revision : removed && action.token === token;
        if (!valid) return { ok: false, json: async () => ({ error: 'This change is stale.' }) };
        removed = action.kind === 'delete_interview_problem';
        revision++;
        token = `token-${revision}`;
        return { ok: true, json: async () => ({ revision, undo: removed ? restoration() : deletion() }) };
      },
    });
    vm.runInContext(organize, context);
    const card = initial === 'delete_interview_problem' ? deletion() : restoration();
    await context.organize(card, card, true);
    assert.equal(card.state, 'applied');
    await context.organize(card, card.undo, false);
    assert.equal(card.state, 'new');
    await context.organize(card, card, true);
    assert.equal(card.state, 'applied');
    assert.deepEqual(errors, []);
    assert.equal(sent.length, 3);
    if (initial === 'delete_interview_problem') assert.deepEqual(sent.filter(v => v.kind === initial).map(v => v.revision), [7, 9]);
    else assert.deepEqual(sent.filter(v => v.kind === initial).map(v => v.token), ['first-token', 'token-9']);
  }
});

function chatSendFixture(mode = 'chat', id = 'practice', access = 'ask', before = async () => true, stream = {}) {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/chat.js'), 'utf8');
  const send = source.slice(source.indexOf('    async function send('), source.indexOf('    let leaving ='));
  const drawContext = source.slice(source.indexOf('    function drawContext('), source.indexOf('    const modeName ='));
  const state = { id, mode, access, interview: { id: 'practice', challenge: 'problem-two', number: 2, title: 'Graphs <review>', tab: 'Code' }, messages: [], refs: [], files: [], sent: [], context: null, selection: null };
  const requests = [], prepared = [], notices = [];
  const contextBox = { innerHTML: '' };
  const input = { value: 'Put the solution in my code area.' };
  const noop = () => {};
  const context = vm.createContext({
    state, input, AbortController, TextDecoder, Date: class extends Date { static now() { return stream.clock ? stream.clock() : Date.now(); } }, working: new Map(), MOST_REFS: C.MOST_REFS,
    historyText: C.historyText, modelHistory: C.modelHistory, historyRequest: C.historyRequest, threadRefs: C.threadRefs, recentNotes: C.recentNotes, sessionBody: C.sessionBody, splitLines: C.splitLines, grade: C.grade, scoreSession: C.scoreSession,
    cardOf: doc => doc, currentSession: () => null, closePick: noop, drawRefs: noop, fit: noop, awake: noop, mood: noop, thinking: noop, draw: noop, remember: noop, react: noop, talking: noop, onInterviewReply: noop,
    pose: noop, onSaved: noop, onChanged: noop, onOrganized: noop, queueMicrotask, organize: async card => { card.state = 'applied'; },
    $: () => contextBox, escape: esc,
    onBeforeSend: async args => { prepared.push(args); return before(args); }, notify: message => notices.push(message),
    fetch: async (_url, options) => {
      requests.push(JSON.parse(options.body));
      const reply = stream.replies ? stream.replies[requests.length - 1] || {} : stream;
      let read = false;
      return { ok: true, status: 200, body: { getReader: () => ({ read: async () => {
        if (read) {
          if (reply.finish) reply.finish(state);
          if (reply.error) throw reply.error;
          return { done: true };
        }
        read = true;
        const events = reply.events || [{ t: 'Ready.' }, { done: true }];
        return { done: false, value: new TextEncoder().encode(events.map(event => JSON.stringify(event)).join('\n') + '\n') };
      } }) } };
    },
  });
  vm.runInContext(send + drawContext, context);
  return { state, input, requests, prepared, notices, contextBox, send: context.send, drawContext: context.drawContext };
}

test('completed and stopped replies retain elapsed time without confusing completion with cancellation', async () => {
  for (const abort of [false, true]) {
    let now = 100;
    const fixture = chatSendFixture('interview', 'practice', 'ask', undefined, {
      clock: () => now,
      events: [{ proposal: { kind: 'interview_code_edit', state: 'applied' } }],
      finish: state => { now = 61250; if (abort) state.busy.abort(); },
    });
    await fixture.send('Fix my code.');
    const answer = fixture.state.messages.at(-1);
    assert.equal(answer.elapsed_ms, 61150);
    assert.equal(Boolean(answer.cancelled), abort);
    assert.equal(answer.error, abort ? 'Stopped.' : undefined);
  }
});

test('successful card-only replies finish without a stopped error for every proposal kind and state', async () => {
  const kinds = ['create', 'edit', 'add_task', 'update_task', 'delete_task', 'restore_task', 'move_note', 'make_folder', 'remove_folder', 'move_folder', 'delete_note', 'restore_note', 'rename_note', 'rename_folder', 'pin_note', 'delete_folder', 'restore_folder', 'interview_code_edit', 'delete_interview_problem', 'restore_interview_problem', 'interview_problem_edit', 'interview_topics', 'interview_test'];
  for (const kind of kinds) {
    for (const state of ['new', 'applied']) {
      const fixture = chatSendFixture('interview', 'practice', 'ask', undefined, { events: [{ proposal: { kind, state, made: 'note' } }, { done: true }] });
      await fixture.send('Make this change.');
      const answer = fixture.state.messages.at(-1);
      assert.equal(answer.error, undefined, `${kind}: ${state}`);
      assert.equal(answer.pending, false);
      assert.equal(answer.proposals[0].state, state);
    }
  }
  const events = [{ ask: { question: 'Which approach?', options: ['A', 'B'] } }, ...Object.keys(C.QUIZ_KINDS).map(kind => ({ quiz: { question: 'Why?', kind } })), { proposal: { kind: 'interview_code_edit', auto: true } }];
  for (const event of events) {
    for (const extra of [{}, { card: { question: 'Explain?', correct: null } }]) {
      const fixture = chatSendFixture('interview', 'practice', 'ask', undefined, { events: [event, { done: true }] });
      await fixture.send('Continue.', extra);
      assert.equal(fixture.state.messages.at(-1).error, undefined);
      if (extra.card) assert.equal(extra.card.reply.error, undefined);
      assert.equal(fixture.state.busy, null);
    }
  }
});

test('a restarted reply counts retained applied cards and reports empty output when pending cards were discarded', async () => {
  for (const state of ['new', 'applied']) {
    const fixture = chatSendFixture('interview', 'practice', 'ask', undefined, { events: [{ proposal: { kind: 'interview_code_edit', state } }, { reset: true }, { done: true }] });
    await fixture.send('Continue.');
    const answer = fixture.state.messages.at(-1);
    if (state === 'applied') {
      assert.equal(answer.error, undefined);
      assert.equal(answer.proposals.length, 1);
    } else {
      assert.match(answer.error, /finished without an answer.*Try again/);
      assert.equal(answer.proposals.length, 0);
    }
  }
});

test('empty completion, cancellation and provider failures remain distinct even after partial content or cards', async () => {
  const cases = [
    { events: [{ done: true }], expected: /finished without an answer.*Try again/ },
    { events: [{ t: 'A partial answer.' }], finish: state => state.busy.abort(), expected: /^Stopped\.$/ },
    { events: [{ proposal: { kind: 'interview_code_edit', state: 'applied' } }], finish: state => state.busy.abort(), expected: /^Stopped\.$/ },
    { events: [], error: { name: 'AbortError' }, expected: /^Stopped\.$/ },
    { events: [{ t: 'A partial answer.' }], error: { name: 'AbortError' }, expected: /^Stopped\.$/ },
    { events: [{ error: 'Stopped.' }], expected: /^Stopped\.$/ },
    { events: [{ proposal: { kind: 'edit' } }], error: new Error('Connection lost. Try again.'), expected: /^Connection lost\. Try again\.$/ },
    { events: [{ error: 'The provider is unavailable. Try again.' }], expected: /^The provider is unavailable\. Try again\.$/ },
  ];
  for (const reply of cases) {
    const fixture = chatSendFixture('interview', 'practice', 'ask', undefined, reply);
    await fixture.send('Continue.');
    const answer = fixture.state.messages.at(-1);
    assert.match(answer.error, reply.expected);
    assert.equal(answer.pending, false);
    assert.equal(fixture.state.busy, null);
  }
});

test('proposal-only completion sends waiting follow-ups while cancellation restores them without sending', async () => {
  for (const abort of [false, true]) {
    const fixture = chatSendFixture('interview', 'practice', 'ask', undefined, { replies: [{
      events: [{ proposal: { kind: 'interview_code_edit', state: 'applied' } }],
      finish: state => {
        const message = { role: 'user', text: 'Explain its complexity.', steer: 'waiting' };
        state.messages.push(message);
        state.busy.waiting.push(message);
        if (abort) state.busy.abort();
      },
    }] });
    fixture.input.value = '';
    await fixture.send('Change my code.');
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(fixture.requests.length, abort ? 1 : 2);
    assert.equal(fixture.input.value, abort ? 'Explain its complexity.' : '');
    assert.equal(fixture.state.busy, null);
  }
});

test('the active practice context and draft save survive every chat style and access setting', async () => {
  for (const mode of ['chat', 'study', 'interview']) {
    for (const access of ['ask', 'auto', 'read']) {
      const fixture = chatSendFixture(mode, 'practice', access);
      fixture.drawContext();
      assert.match(fixture.contextBox.innerHTML, /Problem 2/);
      assert.match(fixture.contextBox.innerHTML, /Graphs &lt;review&gt;/);
      await fixture.send(fixture.input.value);
      assert.equal(fixture.prepared.length, 1, `${mode}: saves before sending`);
      assert.equal(fixture.prepared[0].challenge, 'problem-two');
      assert.equal(fixture.prepared[0].mode, mode);
      assert.equal(fixture.requests.length, 1);
      const request = fixture.requests[0];
      assert.equal(request.mode, mode);
      assert.equal(request.chat, 'practice');
      assert.equal(request.interview_problem, 'problem-two');
      assert.equal(request.access, access);
      assert.equal(request.practice, false);
      assert.equal(request.mark, false);
      assert.equal(fixture.state.messages[0].interview.challenge, 'problem-two');
      assert.deepEqual(fixture.notices, []);
    }
  }
  const quiz = chatSendFixture('study', 'practice', 'read');
  await quiz.send('My answer', { card: { question: 'Why?', correct: null } });
  assert.equal(quiz.requests[0].practice, true);
  assert.equal(quiz.requests[0].mark, true);
  assert.equal(quiz.requests[0].access, 'read');
  assert.equal(quiz.requests[0].interview_problem, 'problem-two');
});

test('a different conversation stays independent of the open practice', async () => {
  const fixture = chatSendFixture('chat', 'other-chat');
  fixture.drawContext();
  assert.doesNotMatch(fixture.contextBox.innerHTML, /Problem 2/);
  await fixture.send(fixture.input.value);
  assert.equal(fixture.prepared.length, 0);
  assert.equal(fixture.requests[0].chat, 'other-chat');
  assert.equal(fixture.requests[0].interview_problem, null);
});

test('switching chat style or conversation during a practice save keeps the unsent message', async () => {
  for (const [from, to] of [['chat', 'study'], ['study', 'chat'], ['interview', 'chat']]) {
    for (const change of ['mode', 'conversation']) {
      let finish;
      const fixture = chatSendFixture(from, 'practice', 'ask', () => new Promise(resolve => { finish = resolve; }));
      const text = fixture.input.value;
      const sending = fixture.send(text);
      assert.equal(typeof finish, 'function');
      if (change === 'mode') fixture.state.mode = to;
      else fixture.state.id = 'other-chat';
      finish(true);
      await sending;
      assert.equal(fixture.requests.length, 0);
      assert.equal(fixture.state.messages.length, 0);
      assert.equal(fixture.input.value, text);
      assert.equal(fixture.state.preparingSend, false);
    }
  }
});


test('the page saves the matching practice draft in every chat style before sending', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(require.resolve('../src/web/app/core.js'), 'utf8');
  const hook = source.slice(source.indexOf('onBeforeSend: async ') + 'onBeforeSend: '.length, source.indexOf('  onOpen:'));
  const body = hook.trim().replace(/,$/, '');
  for (const mode of ['chat', 'study', 'interview']) {
    let saves = 0;
    const session = { item: { id: 'practice', challenge: { id: 'problem-17' } } };
    const context = { interviewPage: session, interviewHere: s => s === session, saveInterviewWorkspace: async s => { assert.equal(s, session); saves += 1; } };
    const save = vm.runInNewContext(`(${body})`, context);
    assert.equal(await save({ id: 'practice', mode, challenge: 'problem-17' }), true);
    assert.equal(saves, 1);
    assert.equal(await save({ id: 'other', mode, challenge: 'problem-17' }), true);
    assert.equal(saves, 1);
    session.switching = true;
    await assert.rejects(save({ id: 'practice', mode, challenge: 'problem-17' }), /switching/);
    assert.equal(saves, 1);
  }
});

test('model history stays append only across quiz answers, hints, next cards and reloads', () => {
  for (const mode of ['chat', 'study', 'interview']) {
    const messages = [{ role: 'user', text: 'Practise this material.', mode }];
    let quiz = { kind: 'free_response', question: 'Why does the invariant hold?', answer: 'The state preserves the condition.', correct: null };
    messages.push({ role: 'assistant', text: 'Start here.', mode, quizzes: [quiz] });
    let prior = C.modelHistory(messages);
    for (const hint of [true, true, false, false, true, false]) {
      const request = hint ? C.hintSay(quiz) : C.quizSay(quiz, 'It preserves the state.', null);
      const pending = { role: 'assistant', text: '', pending: true, request };
      if (hint) quiz.hints = [...(quiz.hints || []), pending];
      else { quiz.given = 'It preserves the state.'; quiz.reply = pending; }
      const asking = [...C.modelHistory(messages), { role: 'user', text: request }];
      assert.deepEqual(asking.slice(0, prior.length), prior, `${mode}: the prior prefix stays unchanged`);
      pending.text = hint ? 'Trace the state before and after.' : '[[correct]] The invariant remains true.';
      pending.pending = false;
      if (!hint) {
        quiz.correct = true;
        quiz = { kind: 'free_response', question: 'Which case comes next?', answer: 'The boundary case.', correct: null };
        messages.push({ role: 'assistant', text: '', mode, quizzes: [quiz] });
      }
      const completed = C.modelHistory(messages);
      assert.deepEqual(completed.slice(0, asking.length), asking);
      assert.equal(completed.length, asking.length + 1);
      assert.equal(completed.at(-1).role, 'assistant');
      assert.deepEqual(C.modelHistory(JSON.parse(JSON.stringify(messages))), completed);
      prior = completed;
    }
  }
});

test('legacy quiz replies and hints reconstruct stable request turns without changing the cards', () => {
  for (const kind of Object.keys(C.QUIZ_KINDS)) {
    const quiz = { kind, question: 'Choose the invariant.', answer: 'A', options: ['A', 'B'], given: 'A', correct: true, hint: { role: 'assistant', text: 'Look at the boundary.' }, hints: [{ role: 'assistant', text: 'Check the first element.' }], reply: { role: 'assistant', text: '[[correct]] The invariant holds.' } };
    const messages = [{ role: 'assistant', text: '', quizzes: [quiz] }];
    const before = JSON.stringify(messages);
    const history = C.modelHistory(messages);
    assert.equal(history.length, 7);
    assert.deepEqual(history.map(m => m.role), ['assistant', 'user', 'assistant', 'user', 'assistant', 'user', 'assistant']);
    assert.match(history[1].text, /Hint, please/);
    assert.match(history[3].text, /Hints you already gave me/);
    assert.match(history[5].text, /Quiz answer/);
    assert.doesNotMatch(history[0].text, /answered|right|Felix replied/);
    assert.equal(JSON.stringify(messages), before);
    quiz.correct = false;
    assert.deepEqual(C.modelHistory(messages), history);
  }
});

test('canonical history preserves notes, actions and ordinary conversation changes while omitting failed or pending turns', () => {
  const messages = [{ role: 'user', text: 'Change the note.', selection: { title: 'Heaps', text: 'The minimum is first.' } }, { role: 'assistant', text: 'A focused change.', proposals: [{ kind: 'edit', title: 'Heaps', find: 'before', replace: 'after', state: 'new' }] }, { role: 'assistant', text: 'Not sent.', pending: true }, { role: 'assistant', text: 'Failed.', error: 'Stopped.' }, { role: 'user', text: 'Waiting.', steer: 'waiting' }];
  const before = C.modelHistory(messages);
  assert.equal(before.length, 2);
  assert.match(before[0].text, /Selected passage/);
  assert.match(before[1].text, /waiting for the user/);
  messages[1].proposals[0].state = 'applied';
  assert.notDeepEqual(C.modelHistory(messages), before);
  messages[1].proposals[0].state = 'new';
  assert.deepEqual(C.modelHistory(messages), before);
  const long = Array.from({ length: 900 }, (_, i) => ({ role: i % 2 ? 'assistant' : 'user', text: `Turn ${i}` }));
  const kept = C.modelHistory(long);
  assert.ok(kept.length <= 400);
  assert.equal(kept.at(-1).text, 'Turn 899');
});

test('actual quiz sends keep the prior model history stable through hints, marking and saved next questions', async () => {
  for (const mode of ['chat', 'study', 'interview']) {
    const hints = [true, true, false, false, true, false];
    const replies = hints.map(hint => ({ events: [{ t: hint ? 'Check the boundary.' : '[[correct]] Your invariant holds.' }, ...(!hint ? [{ quiz: { kind: 'free_response', question: 'What happens next?', answer: 'The next state.', explain: 'Follow the state.' } }] : []), { done: true }] }));
    const fixture = chatSendFixture(mode, 'practice', 'auto', undefined, { replies });
    let quiz = { kind: 'free_response', question: 'Why is it preserved?', answer: 'The transition preserves it.', correct: null };
    fixture.state.messages.push({ role: 'user', text: 'Practise this material.' }, { role: 'assistant', text: '', quizzes: [quiz] });
    let previous = null;
    for (const hint of hints) {
      const text = hint ? C.hintSay(quiz) : 'The transition preserves the state.';
      const extra = hint ? { card: quiz, hint: true } : { card: quiz, say: C.quizSay(quiz, text, null) };
      if (!hint) { quiz.given = text; quiz.correct = null; }
      await fixture.send(text, extra);
      const request = fixture.requests.at(-1);
      assert.equal(request.practice, true);
      assert.equal(request.mark, !hint);
      if (previous) {
        assert.deepEqual(request.messages.slice(0, previous.length), previous);
        assert.equal(request.messages.length, previous.length + 2);
      }
      const nested = hint ? quiz.hints.at(-1) : quiz.reply;
      assert.equal(nested.request, request.messages.at(-1).text);
      assert.match(nested.request, hint ? /Hint, please/ : /Quiz answer/);
      assert.deepEqual(C.modelHistory(JSON.parse(JSON.stringify(fixture.state.messages))), C.modelHistory(fixture.state.messages));
      previous = request.messages;
      if (!hint) quiz = fixture.state.messages.at(-1).quizzes[0];
    }
    assert.match(C.modelHistory(fixture.state.messages).map(m => m.text).join('\n'), /\[\[correct\]\]/);
  }
});
