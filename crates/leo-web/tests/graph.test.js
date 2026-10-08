'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/graph.js');

const G = globalThis.leoGraph;

const data = {
  nodes: [
    { id: 'n:a', kind: 'note', label: 'Graph traversals', folder: 'cs130/week2', summary: 'BFS and DFS', concepts: ['breadth-first search', 'queue'] },
    { id: 'n:b', kind: 'note', label: 'Stacks', folder: 'cs130', summary: 'LIFO', concepts: ['stack'] },
    { id: 'n:c', kind: 'note', label: 'Scheduling', folder: 'cs162', summary: 'Round robin', concepts: ['queue', 'round robin'] },
    { id: 'n:d', kind: 'note', label: 'Induction', folder: 'math61', summary: 'Proofs', concepts: ['recursion'] },
    { id: 'n:e', kind: 'note', label: 'Inbox', folder: '', summary: '', concepts: [] },
    { id: 'c:queue', kind: 'concept', label: 'queue', folder: '', summary: '', concepts: [] },
  ],
  edges: [
    { a: 'n:a', b: 'n:c', kind: 'related', relation: 'same idea', strength: 3, why: 'Both keep waiting work in a queue' },
    { a: 'n:b', b: 'n:d', kind: 'related', relation: 'builds on', strength: 2, why: 'Recursion needs the call stack' },
    { a: 'n:a', b: 'n:b', kind: 'related', relation: 'contrasts', strength: 1, why: 'Queue versus stack order' },
    { a: 'n:a', b: 'n:b', kind: 'link', strength: 2 },
    { a: 'n:a', b: 'c:queue', kind: 'covers', strength: 1 },
    { a: 'n:c', b: 'c:queue', kind: 'covers', strength: 1 },
    { a: 'n:a', b: 'n:missing', kind: 'related', strength: 2 },
  ],
};

test('classes are top-level folders, each with its own colour', () => {
  const g = G.prepare(data);
  assert.deepEqual(g.folders, ['cs130', 'cs162', 'math61', '']);
  assert.equal(g.byId.get('n:a').top, 'cs130');
  assert.notEqual(G.colorOf(g, 'cs130', false), G.colorOf(g, 'cs162', false));
  assert.equal(G.colorOf(g, '', false), null);
  assert.equal(g.edges.length, 6, 'an edge to a missing note is dropped');
  assert.deepEqual(G.counts(g), { notes: 5, ideas: 1, connections: 3, across: 2 });
});

test("a note's connections put other classes first, then the strongest", () => {
  const g = G.prepare(data);
  const list = G.connections(g, 'n:a');
  assert.deepEqual(list.map((c) => [c.node.id, c.label, c.cross, c.linked]), [
    ['n:c', 'Same idea', true, false],
    ['n:b', 'Contrasts', false, true],
  ]);
});

test('a direction is read from the side you are on', () => {
  const g = G.prepare(data);
  assert.equal(G.connections(g, 'n:b').find((c) => c.node.id === 'n:d').label, 'Builds on');
  assert.equal(G.connections(g, 'n:d')[0].label, 'Built on by');
});

test('the strongest list leads with connections across classes', () => {
  const g = G.prepare(data);
  const top = G.strongest(g);
  assert.equal(top.length, 3);
  assert.deepEqual(top.map((x) => [x.a.id, x.b.id, x.cross]), [
    ['n:a', 'n:c', true],
    ['n:b', 'n:d', true],
    ['n:a', 'n:b', false],
  ]);
});

test('ideas are hidden until asked for, and only connections across classes can be shown', () => {
  const g = G.prepare(data);
  const plain = G.visible(g);
  assert.equal(plain.nodes.has('c:queue'), false);
  assert.equal(plain.edges.some((e) => e.kind === 'covers'), false);
  const ideas = G.visible(g, { ideas: true });
  assert.equal(ideas.nodes.has('c:queue'), true);
  const across = G.visible(g, { crossOnly: true });
  assert.deepEqual(across.edges.filter((e) => e.kind === 'related').map((e) => e.relation).sort(), ['builds on', 'same idea']);
  assert.ok(across.edges.some((e) => e.kind === 'link'), 'links you wrote stay');
});

test('hiding a class hides its notes and their connections', () => {
  const g = G.prepare(data);
  const shown = G.visible(g, { hidden: new Set(['cs162']) });
  assert.equal(shown.nodes.has('n:c'), false);
  assert.ok(shown.edges.every((e) => e.a !== 'n:c' && e.b !== 'n:c'));
});

test('focus shows a note and its neighbourhood to the chosen depth', () => {
  const g = G.prepare(data);
  assert.deepEqual([...G.visible(g, { focus: 'n:c', depth: 1 }).nodes].sort(), ['n:a', 'n:c']);
  assert.deepEqual([...G.visible(g, { focus: 'n:c', depth: 2 }).nodes].sort(), ['n:a', 'n:b', 'n:c']);
  assert.deepEqual([...G.visible(g, { focus: 'n:c', depth: 2, crossOnly: true }).nodes].sort(), ['n:a', 'n:b', 'n:c']);
  assert.deepEqual([...G.visible(g, { focus: 'n:e' }).nodes], ['n:e']);
});

test('an idea lists the notes that share it', () => {
  const g = G.prepare(data);
  assert.deepEqual(G.conceptNotes(g, 'c:queue').map((n) => n.label), ['Graph traversals', 'Scheduling']);
});

test('find puts names that start with the words first, notes before ideas', () => {
  const g = G.prepare(data);
  assert.deepEqual(G.find(g, 'que').map((n) => n.id), ['c:queue']);
  assert.deepEqual(G.find(g, 'st').map((n) => n.id), ['n:b']);
  assert.deepEqual(G.find(g, '  '), []);
});

test('the layout settles into finite, repeatable positions with classes apart', () => {
  const g = G.prepare(data);
  const run = () => {
    const sim = G.createSim(g);
    const vis = G.visible(g, { ideas: true });
    for (let i = 0; i < 400; i++) G.tick(sim, vis);
    return sim;
  };
  const sim = run();
  for (const p of sim.pos.values()) assert.ok(Number.isFinite(p.x) && Number.isFinite(p.y));
  assert.ok(sim.alpha < 0.01);
  assert.deepEqual(run().pos.get('n:a'), sim.pos.get('n:a'), 'the same notes give the same map');
  const d = (a, b) => Math.hypot(sim.pos.get(a).x - sim.pos.get(b).x, sim.pos.get(a).y - sim.pos.get(b).y);
  assert.ok(d('n:a', 'n:b') > 10, 'nodes do not overlap');
});

test('a large graph lays out quickly', () => {
  const nodes = Array.from({ length: 400 }, (_, i) => ({ id: `n:${i}`, kind: 'note', label: `Note ${i}`, folder: `c${i % 6}`, summary: '', concepts: [] }));
  const edges = Array.from({ length: 900 }, (_, i) => ({ a: `n:${i % 400}`, b: `n:${(i * 37 + 11) % 400}`, kind: 'related', relation: 'same idea', strength: 1 + (i % 3) }));
  const g = G.prepare({ nodes, edges });
  const sim = G.createSim(g);
  const vis = G.visible(g);
  const started = Date.now();
  for (let i = 0; i < 60; i++) G.tick(sim, vis);
  assert.ok(Date.now() - started < 3000, `60 ticks took ${Date.now() - started} ms`);
  for (const p of sim.pos.values()) assert.ok(Number.isFinite(p.x) && Number.isFinite(p.y));
});

test('a large map shows the most connected notes, and a focus still reaches any note', () => {
  const g = G.prepare(data);
  const top = G.visible(g, { most: 2 });
  assert.deepEqual([...top.nodes].sort(), ['n:a', 'n:b']);
  assert.deepEqual(top.limited, { shown: 2, of: 5 });
  assert.ok(top.edges.every((e) => top.nodes.has(e.a) && top.nodes.has(e.b)));
  const focused = G.visible(g, { most: 2, focus: 'n:e' });
  assert.ok(focused.nodes.has('n:e'));
  assert.equal(focused.limited, null);
  assert.equal(G.visible(g).limited, null, 'small maps are whole');
  assert.equal(G.MAP_MOST, 400);
});
