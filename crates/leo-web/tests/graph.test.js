'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/graph.js');

const G = globalThis.leoGraph;

const data = {
  nodes: [
    { id: 'n:a', kind: 'note', label: 'Graph traversals', folder: 'cs130/week2', count: 2, concepts: ['Breadth-First Search', 'queue'] },
    { id: 'n:b', kind: 'note', label: 'Stacks', folder: 'cs130', count: 1, concepts: ['stack'] },
    { id: 'n:c', kind: 'note', label: 'Scheduling', folder: 'cs162', count: 1, concepts: ['Queue', 'round robin'] },
    { id: 'n:d', kind: 'note', label: 'Inbox', folder: '', count: 0, concepts: [] },
    { id: 'c:queue', kind: 'concept', label: 'queue', folder: '', count: 2, concepts: [] },
    { id: 'c:round robin', kind: 'concept', label: 'round robin', folder: '', count: 1, concepts: [] },
    { id: 'c:breadth-first search', kind: 'concept', label: 'Breadth-First Search', folder: '', count: 1, concepts: [] },
  ],
  edges: [
    { a: 'n:a', b: 'c:queue', kind: 'covers' },
    { a: 'n:c', b: 'c:queue', kind: 'covers' },
    { a: 'n:c', b: 'c:round robin', kind: 'covers' },
    { a: 'n:a', b: 'c:breadth-first search', kind: 'covers' },
    { a: 'c:queue', b: 'c:round robin', kind: 'related', why: 'round robin takes the next process from a queue' },
    { a: 'c:breadth-first search', b: 'c:queue', kind: 'related', why: 'BFS keeps its frontier in a queue' },
    { a: 'n:a', b: 'n:b', kind: 'link' },
    { a: 'n:a', b: 'n:missing', kind: 'link' },
  ],
};

test('folders are grouped by their top level and each gets its own colour', () => {
  const g = G.prepare(data);
  assert.deepEqual(g.folders, ['cs130', 'cs162', '']);
  assert.equal(g.byId.get('n:a').top, 'cs130');
  assert.notEqual(G.colorOf(g, 'cs130', false), G.colorOf(g, 'cs162', false));
  assert.equal(G.colorOf(g, '', false), null);
  assert.equal(g.edges.length, 7, 'an edge to a missing note is dropped');
  assert.deepEqual(G.counts(g), { notes: 4, ideas: 3, links: 2 });
});

test('a note is connected to notes that share its ideas or that it links to', () => {
  const g = G.prepare(data);
  const linked = G.connectedNotes(g, 'n:a');
  assert.deepEqual(linked.map((x) => x.node.id), ['n:b', 'n:c']);
  assert.equal(linked[0].linked, true);
  assert.deepEqual(linked[1].via, ['queue']);
  assert.deepEqual([...G.neighbors(g, 'n:b')].sort(), ['n:a', 'n:b']);
});

test('an idea lists its notes and the ideas it relates to, with why', () => {
  const g = G.prepare(data);
  const detail = G.conceptDetail(g, 'c:queue');
  assert.deepEqual(detail.notes.map((n) => n.label), ['Graph traversals', 'Scheduling']);
  assert.deepEqual(detail.related.map((r) => [r.node.label, r.why]), [
    ['Breadth-First Search', 'BFS keeps its frontier in a queue'],
    ['round robin', 'round robin takes the next process from a queue'],
  ]);
});

test('links between subjects come before links inside one', () => {
  const g = G.prepare(data);
  const list = G.bridges(g);
  assert.equal(list.length, 2);
  assert.ok(list.every((b) => b.cross));
  assert.deepEqual(list.map((b) => b.a.label), ['Breadth-First Search', 'queue']);
  const inside = G.bridges(G.prepare({ ...data, edges: [...data.edges, { a: 'c:breadth-first search', b: 'c:round robin', kind: 'related' }].map((e) => (e.kind === 'covers' && e.a === 'n:a' && e.b === 'c:breadth-first search' ? { ...e, a: 'n:c' } : e)) }));
  assert.equal(inside[inside.length - 1].cross, false, 'two ideas from the same folder come last');
});

test('hiding a folder hides its notes and ideas only it uses', () => {
  const g = G.prepare(data);
  const shown = G.visible(g, new Set(['cs162']));
  assert.equal(shown.has('n:c'), false);
  assert.equal(shown.has('c:round robin'), false);
  assert.equal(shown.has('c:queue'), true, 'queue is still in a cs130 note');
  assert.equal(G.visible(g, new Set()).size, 7);
});

test('find puts names that start with the words first', () => {
  const g = G.prepare(data);
  assert.deepEqual(G.find(g, 'que').map((n) => n.id), ['c:queue']);
  assert.deepEqual(G.find(g, 'st').map((n) => n.id), ['n:b', 'c:breadth-first search']);
  assert.deepEqual(G.find(g, '  '), []);
});

test('the layout settles into finite positions and keeps folders apart', () => {
  const g = G.prepare(data);
  const sim = G.createLayout(g);
  const shown = G.visible(g, new Set());
  for (let i = 0; i < 400; i++) G.step(sim, shown);
  for (const p of sim.pos.values()) assert.ok(Number.isFinite(p.x) && Number.isFinite(p.y));
  assert.ok(sim.alpha < 0.01);
  const again = G.createLayout(g);
  for (let i = 0; i < 400; i++) G.step(again, shown);
  assert.deepEqual(again.pos.get('n:a'), sim.pos.get('n:a'), 'the same notes give the same map');
  const b = G.bounds(sim, shown);
  assert.ok(b.maxX > b.minX && b.maxY > b.minY);
});
