const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
test('permission steps share one page while existing completion flags stay compatible', () => {
  const context = {};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, false] }), 2);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true] }), -1);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, true] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, true] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, true] }), 0);
  assert.equal(context.nextOnboarding({}), 0);
});
function setup(candidates, speech) {
  const el = (extra = {}) => ({ innerHTML: '', textContent: '', disabled: false, ...extra });
  const nodes = { '#onboarding-writing': el(), '#onboarding-writer-status': el(), '#onboarding-use': el({ disabled: true }) };
  const calls = [];
  const settings = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const context = {
    ON_MAC: true, ICON: new Proxy({}, { get: () => '' }), desktopApp: null, state: {}, here: 3, calls,
    document: { querySelector: (sel) => sel === `[data-onboarding-step="${context.here}"]` ? {} : null },
    $: (sel) => nodes[sel] || null,
    choiceCards: (options) => `cards:${options.current}:${options.choices.map((c) => c[0]).join(',')}`,
    refreshPanel: (box, html) => { box.innerHTML = html; },
    ollamaSetup: () => '<ollama-setup>',
    appChange: async (change) => { calls.push(['app', change.step]); },
    api: async (url, options = {}) => {
      calls.push([url, options.body && JSON.parse(JSON.stringify(options.body))]);
      if (url === '/api/writing?probe=signin') return { writing: { state: 'not_chosen' }, candidates };
      if (url === '/api/speech') return speech(options.body);
      return {};
    },
    closeSheet() {}, setTimeout() {}, clearTimeout() {},
  };
  vm.createContext(context);
  vm.runInContext(settings.slice(settings.indexOf('function providerCard('), settings.indexOf('function taskCard(')), context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  context.maybeOnboarding = () => {};
  return { context, nodes, calls };
}
const settle = async () => { for (let i = 0; i < 10; i++) await new Promise(setImmediate); };
const claude = (signed_in) => ({ id: 'claude_code', installed: true, signed_in });
const codex = (signed_in) => ({ id: 'codex', installed: true, signed_in });
const ollama = (models) => ({ id: 'ollama', installed: true, running: true, models });

test('the writing step comes after the old steps and only for people who have not finished it', () => {
  const { context } = setup([]);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true], writing_done: false }), 3);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, false], writing_done: false }), 2);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true], writing_done: true }), -1);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true] }), -1);
});

test('the writing step preselects a signed-in Claude Code, then Codex, then Ollama with a model', async () => {
  for (const [candidates, chosen, use] of [
    [[codex(true), claude(true), ollama(['qwen3:4b'])], 'claude_code', 'Use Claude Code'],
    [[codex(true), claude(false), ollama(['qwen3:4b'])], 'codex', 'Use Codex'],
    [[codex(null), claude(false), ollama(['qwen3:4b'])], 'ollama', 'Use Ollama'],
    [[codex(null), claude(false), ollama([])], 'claude_code', 'Use Claude Code'],
  ]) {
    const { context, nodes, calls } = setup(candidates);
    await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
    assert.ok(nodes['#onboarding-writing'].innerHTML.startsWith(`cards:${chosen}:claude_code,codex,ollama`));
    assert.equal(nodes['#onboarding-use'].textContent, use);
    const ready = chosen !== 'claude_code' || candidates[1].signed_in;
    assert.equal(nodes['#onboarding-use'].disabled, !ready);
    assert.match(nodes['#onboarding-writer-status'].innerHTML, ready ? /set-status ok">Ready/ : /Sign in needed/);
    assert.equal(calls[0][0], '/api/writing?probe=signin');
  }
});

test('Use saves the writing provider like Settings and finishes the step; Skip only finishes it', async () => {
  const { context, calls } = setup([claude(true)]);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', context);
  await context.onboardingUse();
  assert.deepEqual(calls.slice(1), [['/api/settings', { set: 'provider', task: 'writing', value: 'claude_code' }], ['app', 3]]);
  const skipped = setup([claude(false)]);
  await vm.runInContext('loadOnboardingWriting(onboardingGeneration)', skipped.context);
  await skipped.context.onboardingUse();
  await skipped.context.finishOnboarding({ dataset: { step: '3' } });
  assert.deepEqual(skipped.calls.slice(1), [['app', 3]]);
});

test('Download and use finishes the speech step once the model is downloaded', async () => {
  let downloaded = false;
  const model = () => ({ id: 'base', name: 'Base', languages: ['en'], size: 1, verified: downloaded, in_use: false });
  const { context, calls } = setup([], (body) => {
    if (body?.action === 'download') return { models: [{ ...model(), progress: { state: 'downloading' } }] };
    downloaded = true;
    return { models: [model()] };
  });
  context.here = 2;
  await vm.runInContext('loadOnboardingModels(onboardingGeneration)', context);
  downloaded = false;
  await context.onboardingModel({ dataset: { verb: 'download' } });
  await settle();
  assert.deepEqual(calls.filter((c) => c[1]?.action).map((c) => c[1].action), ['download', 'use']);
  assert.deepEqual(calls.at(-1), ['app', 2]);
});

test('the dictation hint names the real shortcut and stays hidden without dictation', async () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  for (const [answer, text] of [[{ settings: { shortcut: '⌥Space' } }, 'Press ⌥Space anywhere to dictate.'], [null, '']]) {
    const context = { api: async () => { if (!answer) throw new Error('Dictation is not available.'); return answer; } };
    vm.createContext(context);
    vm.runInContext(core.slice(core.indexOf('async function dictateHint('), core.indexOf('function wireHomeAsk(')), context);
    const el = { hidden: true, isConnected: true, textContent: '' };
    await context.dictateHint(el);
    assert.equal(el.textContent, text);
    assert.equal(el.hidden, !answer);
  }
});
