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
  assert.equal((svg.match(/class="felix-arm/g) || []).length, 2, 'two arms');
  assert.deepEqual(C.MODES.map((m) => m.id), ['chat', 'study']);
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
  const blank = { kind: 'fill_blank', question: 'BFS takes the ___ vertex first.', answer: 'oldest | earliest' };
  assert.equal(C.checkQuiz(blank, '  Oldest. '), true);
  assert.equal(C.checkQuiz(blank, 'earliest'), true);
  assert.equal(C.checkQuiz(blank, 'newest'), false);
  assert.equal(C.checkQuiz(blank, '   '), false);
  assert.equal(C.checkQuiz({ kind: 'fill_blank', answer: 'Schrödinger' }, 'schrodinger'), true);
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
