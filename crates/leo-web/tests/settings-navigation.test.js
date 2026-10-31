const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
function helpers() {
  const context = { esc: value => String(value).replace(/[<>&"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c])), localStorage: { getItem: () => 'dictation', setItem() {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), context);
  return context;
}
test('all grouped Settings routes resolve and Storage keeps its route', () => {
  const h = helpers();
  for (const section of ['general', 'dictation', 'models', 'writing', 'calendar', 'backup', 'storage', 'health', 'updates']) assert.equal(h.settingsSection(section), section);
  assert.equal(h.settingsSection(''), 'dictation');
  assert.equal(h.settingsSection('../bad'), 'general');
  const html = h.settingsFrame('storage', '<section>Storage content</section>');
  for (const label of ['Leo', 'Your notes', 'This app', 'Speech models', 'Updates &amp; about']) assert.ok(html.includes(label), label);
  assert.ok(html.includes('href="#/settings/storage" aria-current="page"'));
  assert.ok(!html.includes('Phone'));
  assert.ok(html.includes('Storage content'));
});
