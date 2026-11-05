'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
require('../src/web/chat.js');
const C = globalThis.leoChat;

test('remembered interview style falls back to Chat without losing saved messages', () => {
  const messages = [{ role: 'assistant', text: 'Saved answer', proposals: [{ kind: 'interview_test' }] }];
  const cached = JSON.stringify({ id: 'retired-chat', mode: 'interview', messages, refs: [] });
  const storage = { getItem: () => cached, setItem: () => assert.fail('reading old state must not rewrite it') };
  assert.equal(C.modeOf('interview'), 'chat');
  assert.equal(C.load(storage).mode, 'chat');
  assert.deepEqual(C.load(storage).messages, messages);
  assert.deepEqual(C.MODES.map(m => m.id), ['chat', 'study']);
});

test('retired interview cards render a short notice without controls and keep Study cards separate', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
  const start = source.indexOf('    function proposalCard('), end = source.indexOf('    function askCard(', start);
  const context = { retiredPractice: C.retiredPractice };
  vm.createContext(context);
  vm.runInContext(source.slice(start, end), context);
  for (const kind of ['interview_code_edit', 'interview_problem_edit', 'delete_interview_problem', 'restore_interview_problem', 'interview_topics', 'interview_test', 'interview_schedule']) {
    const html = context.proposalCard({ kind, state: 'applied' }, 0, 0);
    assert.match(html, /Interview practice is no longer part of leo\./);
    assert.doesNotMatch(html, /button|data-chat|href=/);
  }
  for (const kind of ['edit', 'create', 'quiz', 'add_task', 'update_board', 'margin_note']) assert.equal(C.retiredPractice({ kind }), false);
});

test('retired bookmarks and saved last places land in the library without touching stale practice keys', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  const start = source.indexOf("const LAST_PLACE_KEY ="), end = source.indexOf("window.addEventListener('hashchange'", start);
  for (const hash of ['#/interviews', '#/interviews/old-practice']) {
    const saved = new Map([['leo-last-place', hash], ['leo-interview-drafts', 'kept']]);
    const location = { hash: '#/' };
    const context = { location, localStorage: { getItem: k => saved.get(k), setItem: (k,v) => saved.set(k,v) }, sessionStorage: { getItem: () => null, setItem() {} }, history: { state: null, replaceState(_state,_name,hash) { location.hash = hash; } } };
    vm.createContext(context);
    vm.runInContext(source.slice(start,end),context);
    context.reopenLastPlace();
    assert.equal(location.hash, '#/');
    assert.equal(saved.get('leo-last-place'), '#/');
    assert.equal(saved.get('leo-interview-drafts'),'kept');
    const routeStart = source.indexOf('  if (/^#\\/interviews', source.indexOf('async function render()'));
    const routeEnd = source.indexOf('  const hash =', routeStart);
    assert.ok(routeStart > 0 && routeEnd > routeStart);
    location.hash = hash;
    vm.runInContext(source.slice(routeStart,routeEnd),context);
    assert.equal(location.hash, '#/');
  }
});
