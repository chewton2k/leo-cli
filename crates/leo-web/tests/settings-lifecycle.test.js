const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
test('late Settings saves cannot redraw a different page on success or failure', async () => {
  for (const failed of [false, true]) {
    let finish;
    const context = { app: { addEventListener() {} }, seq: 1, state: { view: 'settings', settings: {} }, document: { addEventListener() {} }, api: () => new Promise((resolve, reject) => { finish = () => failed ? reject(new Error('failed')) : resolve({settings:{},message:'Saved'}); }), window: {}, toast() {} };
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
    let draws = 0;
    context.drawSettings = () => { draws++; };
    const pending = context.changeSetting({set:'effort',value:'high'}).catch(() => {});
    context.seq++;
    context.state = {view:'folder',settings:{}};
    finish();
    await pending;
    assert.equal(draws, 0);
  }
});
test('the writing card says Ready only when the writing check is ready', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text), choiceCards: () => '' };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const task = { task: 'writing', provider: 'ollama', ready: true, choices: [], models: [] };
  for (const [writing, label] of [['ready', 'Ready'], ['no_model', 'Not set up'], [undefined, 'Not set up']]) {
    context.state = { settings: { writing: writing && { state: writing } } };
    assert.ok(context.taskCard(task).includes(`>${label}</span>`), writing);
  }
});
test('late onboarding completion cannot close a newer sheet', async () => {
  let finish, closed = 0;
  const context = { desktopApp: null, appChange: () => new Promise(resolve => { finish = resolve; }), closeSheet() { closed++; }, document: { querySelector: () => null } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  context.maybeOnboarding = () => {};
  const pending = context.finishOnboarding({dataset:{step:'0'}});
  finish();
  await pending;
  assert.equal(closed, 0);
});

test('the note footer keeps its note tools without the bottom Ask button', () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const script = core.slice(core.indexOf('function noteActions('), core.indexOf('async function showNote('));
  const context = { ON_MAC: true, ICON: new Proxy({}, {get: () => '<svg></svg>'}), esc: text => String(text) };
  vm.createContext(context);
  vm.runInContext(script, context);
  const html = context.noteActions('note-id', true);
  assert.doesNotMatch(html, /data-action="(?:ask-selection|note-map)"/);
  for (const name of ['Show transcript', 'Show tasks', 'Add picture', 'Move to folder', 'Export as PDF', 'Move to trash']) assert.ok(html.includes('aria-label="' + name + '"'));
});

test('Codex and Claude Code models list newest release first, then by power', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const order = (ids) => JSON.stringify(context.modelsByRelease(ids.map((id) => ({ id }))).map((m) => m.id));
  assert.equal(order(['gpt-6-luna', 'gpt-6-sol', 'gpt-6.1-sol', 'gpt-6-astra']), JSON.stringify(['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna']));
  assert.equal(order(['claude-haiku-5-5', 'claude-sonnet-5-5', 'claude-opus-5-5', 'claude-fable-5-1']), JSON.stringify(['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5', 'claude-fable-5-1']));
  assert.equal(order(['plain', 'gpt-6.9-a', 'gpt-6.10-a']), JSON.stringify(['gpt-6.10-a', 'gpt-6.9-a', 'plain']));
});
