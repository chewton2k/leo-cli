const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
function context() {
  const timers = [];
  const value = { app: { addEventListener() {}, querySelectorAll: () => [] }, esc: text => String(text).replace(/[<>&"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c])), state: { view: 'folder' }, $: () => null, window: {}, document: { activeElement: null, addEventListener() {} }, updateOnboardingPermissions() {}, maybeOnboarding() {}, setTimeout: fn => timers.push(fn), api: async () => ({ preferences: {}, updates: {} }), md: { render: text => String(text).replace(/</g, '&lt;') } };
  vm.createContext(value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), value);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/app-settings.js'), 'utf8'), value);
  return { value, timers };
}
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
  assert.ok(html.includes('Finish recording or dictation'));
  assert.ok(html.includes('checked'));
  assert.equal((html.match(/role="switch"/g) || []).length, 2);
  assert.ok(!html.includes('<script>'));
  assert.ok(html.includes('Accessibility'));
  assert.ok(html.includes('leo update') && html.includes('leo uninstall'));
});
