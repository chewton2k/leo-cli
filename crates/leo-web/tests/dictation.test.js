const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
test('dictation controls escape history and preserve explicit text settings', () => {
  const context = { app: { addEventListener() {} }, settingsOption: (value, label) => `<option>${label}</option>`, ICON: { chevron: '' }, esc: value => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;'), state: {}, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  const html = context.dictationCards({ settings: { shortcut: 'option+space', custom_words: ['東京'], mode: 'both', popup: 'live' }, recent: [{ id: 1, text: '<img src=x onerror=bad()>' }], shortcut: {} });
  assert.ok(html.includes('&lt;img'));
  assert.ok(!html.includes('<img'));
  assert.ok(html.includes('東京'));
  assert.ok(html.includes('data-dictation="custom_words"'));
  assert.ok(html.includes('Lid closed'));
  assert.ok(html.includes('class="switch" role="switch"'));
  assert.ok(!/type="checkbox"(?![^>]*class="switch")/.test(html));
  assert.ok(html.includes('<input type="hidden" data-dictation="mode" value="both">'));
  assert.ok(html.includes('role="radiogroup" aria-label="Popup"'));
});
test('popup live and minimal states keep useful errors and timers', () => {
  const { display } = require('../../leo-desktop/dist/dictation.js');
  assert.equal(display({ state: 'listening', seconds: 62.9, text: 'hello' }).timer, '1:02');
  assert.equal(display({ state: 'listening', minimal: true, text: 'hello' }).text, '');
  assert.equal(display({ state: 'error', minimal: true, message: 'Choose a microphone.' }).text, 'Choose a microphone.');
  assert.equal(display({ state: 'loading' }).label, 'Loading model…');
  assert.equal(display({ state: 'listening', top: true }).top, true);
});
