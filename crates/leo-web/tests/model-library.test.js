const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
function helpers() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const context = { state: { speechFilter: 'recommended' }, esc: require('../src/web/markdown.js').escape, ICON: { chevron: '' }, settingsOption: () => '', knobStretch() {}, plural: (n, word, many = `${word}s`) => `${n} ${n === 1 ? word : many}`, app: { addEventListener() {} } };
  vm.createContext(context);
  const nav = fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8');
  vm.runInContext(nav.slice(nav.indexOf('function segmented('), nav.indexOf('function pickSegment(')), context);
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
  assert.ok(html.includes('Multilingual'));
  assert.ok(html.includes('Accuracy'));
  assert.ok(html.includes('Speed'));
  assert.equal((html.match(/data-model-row="one"/g) || []).length, 1);
  assert.ok(!html.includes('data-filter="downloaded"'));
  assert.equal(h.speechFiltered(models, 'english').length, 1);
  assert.equal(h.speechFiltered(models, 'many').length, 0);
});
test('model rows show the catalog description capabilities size accuracy and speed', () => {
  const h = helpers();
  const html = h.speechModelRow({id:'parakeet',name:'Parakeet Unified EN 0.6B',description:'Fast, accurate <live> English transcription',languages:['en'],size:731357568,downloaded:true,verified:true,in_use:true,recommended:true,streaming:true,live:true,speed:13.955,accuracy:96.01,accuracy_measured:true,benchmark_language:'en'});
  for (const text of ['Fast, accurate &lt;live&gt; English transcription','English only','Streaming','697 MB','Accuracy','96.0%','Speed','14.0× real time','Recommended','In use','Delete']) assert.ok(html.includes(text),text);
  assert.ok(html.includes('<meter'));
  assert.ok(!html.includes('<live>'));
  const unknown = h.speechModelRow({id:'unknown',name:'Unknown',languages:['ja'],size:1,speed:0,accuracy:0,accuracy_measured:false});
  assert.equal((unknown.match(/Not measured/g)||[]).length,2);
});
test('the downloaded heading counts models in the singular', () => {
  const h = helpers();
  const models = [{ id: 'one', name: 'English', languages: ['en'], size: 100, bytes: 100, downloaded: true, verified: true, recommended: true, speed: 8, accuracy: 96 }];
  assert.ok(h.speechCards({ selected: { id: 'one', language: 'en' }, ready: true, disk_bytes: 100, models, extra_files: [] }).includes('1 model ·'));
});
