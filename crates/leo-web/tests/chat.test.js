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

test('citations become note chips, and unknown ones are left alone', () => {
  const sources = [{ n: 1, id: 'a', title: 'Heaps' }, { n: 2, id: 'b', title: 'A <long> title that keeps going on and on' }];
  const html = C.cite('<p>Min on top [n1]. Both [n1, n2]. Nope [n9].</p>', sources, esc);
  assert.equal((html.match(/class="cite"/g) || []).length, 3);
  assert.ok(html.includes('data-id="a"'));
  assert.ok(html.includes('&lt;long&gt;'), 'titles are escaped');
  assert.ok(html.includes('[n9]'));
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
