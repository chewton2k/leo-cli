const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
function helpers() {
  const context = { esc: value => String(value).replace(/[<>&"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c])), localStorage: { getItem: () => 'dictation', setItem() {} }, document: { addEventListener() {} }, app: { querySelector: () => null }, svg: paths => `<svg>${paths}</svg>`, ICON: {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), context);
  return context;
}
test('all grouped Settings routes resolve, with Storage and Trash under Data', () => {
  const h = helpers();
  for (const section of ['general', 'dictation', 'models', 'writing', 'calendar', 'backup', 'storage', 'trash', 'health', 'updates']) assert.equal(h.settingsSection(section), section);
  assert.equal(h.settingsSection(''), 'dictation');
  assert.equal(h.settingsSection('../bad'), 'general');
  const html = h.settingsFrame('storage', '<section>Storage content</section>');
  for (const label of ['Leo', 'Your notes', 'Data', 'Trash', 'This app', 'Speech models', 'Updates &amp; about']) assert.ok(html.includes(label), label);
  assert.ok(html.includes('href="#/settings/storage" aria-current="page"'));
  assert.ok(!html.includes('Phone'));
  assert.ok(html.includes('Storage content'));
  assert.ok(html.includes('<h1>Storage</h1>'));
  assert.ok(html.includes('<span class="settings-nav-pill" aria-hidden="true"></span>'));
  assert.ok(html.includes('class="settings-section arrive"'));
  assert.equal((html.match(/class="settings-link/g) || []).length, 10);
  assert.equal((html.match(/<svg>/g) || []).length, 7, 'every section without a shared icon draws its own');
});
test('switches stay checkboxes and segmented choices carry their value', () => {
  const h = helpers();
  const on = h.settingsSwitch('data-x="1"', true, 'Thing');
  assert.ok(on.includes('type="checkbox" class="switch" role="switch" aria-label="Thing" data-x="1" checked'));
  assert.ok(!h.settingsSwitch('data-x="1"', false).includes('checked'));
  const field = h.segmented({ label: 'Mode', choices: [['a', 'A'], ['b', 'B<'], ['c', 'C']], current: 'b', field: 'data-mode="m"' });
  assert.ok(field.includes('<input type="hidden" data-mode="m" value="b">'));
  assert.ok(field.includes('style="--count:3;--at:1"'));
  assert.ok(field.includes('role="radiogroup" aria-label="Mode"'));
  assert.ok(field.includes('data-value="b" role="radio" aria-checked="true" tabindex="0"'));
  assert.ok(field.includes('B&lt;') && !field.includes('B<'));
  const unknown = h.segmented({ label: 'Mode', choices: [['a', 'A'], ['b', 'B']], current: 'zzz', field: 'data-mode="m"' });
  assert.ok(unknown.includes('--at:0'));
  const buttons = h.segmented({ label: 'Theme', choices: [['light', 'Light'], ['dark', 'Dark']], current: 'dark', action: 'theme-pick', key: 'theme' });
  assert.ok(buttons.includes('data-action="theme-pick" data-theme="dark" aria-pressed="true"'));
  assert.ok(!buttons.includes('type="hidden"') && buttons.includes('role="group"'));
});
test('every Settings section names its current page in the same breadcrumb', () => {
  const h = helpers();
  for (const [id, title] of [['general', 'General'], ['dictation', 'Dictation'], ['models', 'Speech models'], ['writing', 'Writing AI'], ['calendar', 'Calendar'], ['backup', 'Backup'], ['storage', 'Storage'], ['trash', 'Trash'], ['health', 'Health'], ['updates', 'Updates &amp; about']]) {
    assert.equal(h.settingsCrumbs(id), `<span class="sep">/</span><button data-action="settings">Settings</button><span class="sep">/</span><button aria-current="page">${title}</button>`);
  }
});
