const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const baseline = process.argv[2] || 'b88b878';
assert.match(baseline, /^[a-f0-9]{7,40}$/);
const source = fs.readFileSync(path.join(__dirname, '../src/web/app/app-settings.js'), 'utf8');
const before = execFileSync('git', ['show', `${baseline}:crates/leo-web/src/web/app/app-settings.js`], { cwd: path.resolve(__dirname, '../../..'), encoding: 'utf8' });
const duration = 60000;

async function measure(script, hidden, settings) {
  let now = 0, next = 0, requests = 0;
  const timers = new Map();
  const context = vm.createContext({
    app: { addEventListener() {}, querySelectorAll: () => [] },
    state: { view: settings ? 'settings' : 'note', section: 'general' },
    window: { __LEO_VISIBLE__: !hidden, addEventListener() {} },
    document: { visibilityState: 'visible', addEventListener() {} },
    $: () => null, updateOnboardingPermissions() {}, maybeOnboarding() {}, onboardingHere: () => false,
    appVisible: () => !hidden,
    api: async () => { requests++; return { preferences: {}, updates: {} }; },
    setTimeout: (fn, delay) => { const id = ++next; timers.set(id, { fn, at: now + delay }); return id; },
    clearTimeout: id => timers.delete(id),
  });
  vm.runInContext(script, context);
  await context.pollAppStatus(0);
  for (let steps = 0; timers.size && steps < 1000; steps++) {
    const [id, timer] = [...timers].sort(([, a], [, b]) => a.at - b.at)[0];
    if (timer.at > duration) break;
    now = timer.at;
    timers.delete(id);
    await timer.fn();
  }
  assert.ok(requests < 1000);
  assert.ok(timers.size <= 1);
  return requests;
}

(async () => {
  for (const [name, hidden, settings] of [['hidden', true, false], ['visible notes', false, false], ['visible settings', false, true]]) {
    const trials = [];
    for (let i = 0; i < 3; i++) trials.push({ before: await measure(before, hidden, settings), after: await measure(source, hidden, settings) });
    console.log(JSON.stringify({ scenario: name, simulated_ms: duration, trials }));
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
