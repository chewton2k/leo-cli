const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
function context() {
  const timers = [];
  const listeners = {};
  const listen = (name, fn) => { listeners[name] = fn; };
  const value = { svg: (paths) => `<svg>${paths}</svg>`, app: { addEventListener() {}, querySelectorAll: () => [] }, esc: text => String(text).replace(/[<>&"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c])), state: { view: 'folder' }, $: () => null, window: { addEventListener: listen }, document: { activeElement: null, visibilityState: 'visible', addEventListener: listen }, updateOnboardingPermissions() {}, maybeOnboarding() {}, onboardingHere: () => false, setTimeout: (fn, ms) => { timers.push({ fn, ms }); return timers.length; }, clearTimeout: id => { if (id) timers[id - 1] = null; }, api: async () => ({ preferences: {}, updates: {} }), speechBytes: (b) => Math.round(b / 1048576) + ' MB', skeleton: (n) => '<div class="skeleton"></div>'.repeat(n), md: { render: text => String(text).replace(/</g, '&lt;') } };
  vm.createContext(value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/refresh.js'), 'utf8'), value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/app-settings.js'), 'utf8'), value, { filename: path.join(__dirname, '../src/web/app/app-settings.js') });
  return { value, timers, listeners };
}
test('hidden and native-hidden windows make no automatic app-status requests', async () => {
  for (const native of [false, true]) {
    const { value, timers } = context();
    let calls = 0;
    value.api = async () => { calls++; return {}; };
    if (native) value.window.__LEO_VISIBLE__ = false;
    else value.document.visibilityState = 'hidden';
    await value.pollAppStatus(0);
    assert.equal(calls, 0);
    assert.equal(timers.filter(Boolean).length, 0);
  }
});
test('failed status reads retry and ordinary note views use the slower status cadence', async () => {
  const { value, timers } = context();
  value.api = async () => { throw new Error('temporary disconnect'); };
  await value.pollAppStatus(0);
  assert.equal(timers.filter(Boolean).length, 1);
  assert.equal(timers.filter(Boolean)[0].ms, 30000);
});
test('settings, active updates and onboarding retain their responsive cadence', async () => {
  for (const mode of ['settings', 'checking', 'downloading', 'installing', 'onboarding']) {
    const { value, timers } = context();
    if (mode === 'settings') value.state = {view:'settings',section:'general'};
    value.api = async () => ({updates:{state:mode}});
    if (mode === 'onboarding') { value.onboardingHere = () => true; value.ONBOARDING_POLL_MS = 1000; }
    await value.pollAppStatus(0);
    assert.equal(timers.filter(Boolean).length, 1);
    assert.equal(timers.filter(Boolean)[0].ms, mode === 'onboarding' ? 1000 : 2500);
  }
});
test('explicit native actions still run while hidden without restarting automatic work', async () => {
  const { value, timers } = context();
  let calls = 0;
  value.document.visibilityState = 'hidden';
  value.api = async () => { calls++; return {}; };
  await value.appChange({action:'preference'});
  assert.equal(calls, 1);
  assert.equal(timers.filter(Boolean).length, 0);
});
test('hide and resume invalidate late responses and leave only one polling chain', async () => {
  const { value, timers, listeners } = context();
  let finish, calls = 0;
  value.api = () => { calls++; return new Promise(resolve => { finish = resolve; }); };
  const pending = value.pollAppStatus(0);
  value.document.visibilityState = 'hidden';
  listeners.visibilitychange();
  finish({ version: 'stale' });
  await pending;
  assert.equal(timers.filter(Boolean).length, 0);
  assert.equal(vm.runInContext('desktopApp', value), null);
  value.api = async () => { calls++; return {version:'fresh'}; };
  value.document.visibilityState = 'visible';
  listeners.visibilitychange();
  await new Promise(resolve => setImmediate(resolve));
  listeners['leo-visibility']();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(calls, 2);
  assert.equal(timers.filter(Boolean).length, 1);
  assert.equal(vm.runInContext('desktopApp.version', value), 'fresh');
});
test('app polling resumes after successful and failed native actions', async () => {
  const { value, timers } = context();
  await value.appChange({ action: 'microphone' });
  assert.ok(timers.length > 0);
  timers.length = 0;
  value.api = async () => { throw new Error('permission request failed'); };
  await assert.rejects(value.appChange({ action: 'microphone' }));
  assert.ok(timers.length > 0);
});
test('Updates renders ready, busy and release notes safely with independent defaults', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', unsigned: true, preferences: { automatic_updates: true, install_on_quit: true }, updates: { state: 'ready', version: '2.0.0', busy: true, notes: '<script>bad()</script>' }, paths: { notes: '/notes', models: '/models' } });
  assert.ok(html.includes('Restart to update'));
  assert.ok(html.includes('Updates wait for recording and dictation to finish, then leo reopens.'));
  assert.ok(html.includes('Finish recording or dictation'));
  assert.ok(html.includes('checked'));
  assert.equal((html.match(/role="switch"/g) || []).length, 2);
  assert.ok(!html.includes('<script>'));
  assert.ok(html.includes('Accessibility'));
  assert.ok(html.includes('leo update') && html.includes('leo uninstall'));
  assert.ok(html.includes('your notes stay') && html.includes('title="/notes"'));
});

test('a native quit confirmation explains both ways to quit without navigating', async () => {
  const messages = [];
  const window = {};
  const events = fs.readFileSync(path.join(__dirname, '../src/web/app/events.js'), 'utf8');
  vm.runInNewContext(events.slice(events.indexOf('const APP_SETTINGS =')), { window, toast: text => messages.push(text), go: () => assert.fail('Quit must keep the current page') });
  await window.leoFromApp('quit', '');
  assert.equal(messages.length, 1);
  assert.ok(messages[0].includes('⌘Q again') && messages[0].includes('Quit again') && messages[0].includes('3 seconds'));
});

test('the downloading update shows a percent and a bar', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'downloading', version: '2.0.0', bytes: 41200000, total: 96500000 }, paths: {} });
  assert.ok(html.includes('42% · ') && html.includes('<progress value="42"'));
});
test('an available update has one primary Download and no Check now', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'available', version: '2.0.0' }, paths: {} });
  assert.ok(html.includes('btn primary sm') && html.includes('Download 2.0.0'));
  assert.ok(!html.includes('check_updates'));
});
test('idle Updates shows only the last-checked line', () => {
  const { value } = context();
  const html = value.updatesCard({ version: '1.2.3', platform: 'macos', preferences: {}, updates: { state: 'idle' }, paths: {} });
  assert.ok(html.includes('Not checked yet') && !html.includes('Check for a newer version.'));
});
test('Health loads in shape', () => {
  const { value } = context();
  const html = value.healthCards(null);
  assert.ok(html.includes('Checking…') && html.includes('activity-spin'));
  assert.equal((html.match(/class="skeleton"/g) || []).length, 6);
});
