const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
function helpers() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const context = { state: { speechFilter: 'recommended' }, esc: value => String(value).replace(/</g, '&lt;'), ICON: { chevron: '' }, settingsOption: () => '', app: { addEventListener() {} } };
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('function speechCards(')), context);
  return context;
}
test('Downloaded lists every disk model even when filtered or damaged', () => {
  const h = helpers();
  const models = [{ id: 'one', name: 'English', languages: ['en'], size: 100, bytes: 100, downloaded: true, verified: true, in_use: true, recommended: true, speed: 8, accuracy: 96 }, { id: 'two', name: 'Damaged', languages: ['ja'], size: 100, bytes: 20, downloaded: true, verified: false, recommended: false, speed: 0, accuracy: 0 }];
  const html = h.speechCards({ selected: { id: 'one', language: 'en' }, ready: true, disk_bytes: 120, models, extra_files: [{ name: 'older.gguf', bytes: 10 }] });
  assert.ok(html.includes('<h3>Downloaded</h3>'));
  assert.ok(html.includes('Damaged'));
  assert.ok(html.includes('older.gguf'));
  assert.ok(html.includes('Check failed'));
  assert.ok(html.includes('Many languages'));
  assert.ok(html.includes('Accuracy'));
  assert.ok(html.includes('Speed'));
  assert.equal(h.speechFiltered(models, 'downloaded').length, 2);
  assert.equal(h.speechFiltered(models, 'english').length, 1);
  assert.equal(h.speechFiltered(models, 'many').length, 0);
});
