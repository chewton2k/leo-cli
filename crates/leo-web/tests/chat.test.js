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
  C.save(storage, 'quiz', Array.from({ length: 60 }, (_, i) => ({ role: 'user', text: `m${i}` })));
  const back = C.load(storage);
  assert.equal(back.mode, 'quiz');
  assert.equal(back.messages.length, 40);
  assert.equal(back.messages[0].text, 'm20');
  const broken = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
  assert.deepEqual(C.load(broken), { mode: 'ask', messages: [] });
  C.save(broken, 'ask', []);
});

test('Felix keeps his colour and shape', () => {
  const svg = C.felix(40, 'idle');
  assert.ok(svg.includes('class="felix idle"'));
  assert.ok(svg.includes('x="5" y="2" width="50" height="26"'), 'his body');
  assert.equal((svg.match(/width="4" height="4"/g) || []).length, 2, 'two eyes');
  assert.equal((svg.match(/class="felix-arm/g) || []).length, 2, 'two arms');
  assert.deepEqual(C.MODES.map((m) => m.id), ['ask', 'coach', 'quiz', 'explain', 'meeting']);
});
