const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const read = (file) => fs.readFileSync(path.join(__dirname, '../src', file), 'utf8');

test('graph page has no navigation, actions, route or bundled renderer', () => {
  for (const file of ['web/app/side.js', 'web/app/sheets.js', 'web/app/actions.js', 'web/app/events.js', 'web/app/core.js']) {
    assert.doesNotMatch(read(file), /sideRow\('map'|#\/map|showMap|leaveMap|['"]map-[a-z-]+['"]|note-map/, file);
  }
  assert.doesNotMatch(read('routes/assets.rs'), /web\/graph\.js|app\/map\.js/);
  assert.doesNotMatch(read('lib.rs'), /\.route\("\/(?:api\/graph|graph\.js)/);
  assert.doesNotMatch(read('web/index.html'), /src="\/graph\.js"|on-map|\.map-(?:tools|panel|zoom)/);
});

test('shared count wording works without the removed graph script', () => {
  const vm = require('node:vm');
  const core = read('web/app/core.js');
  const declaration = core.match(/^const plural = .*;$/m);
  assert.ok(declaration, 'count wording belongs to the shared app code');
  const count = vm.runInNewContext(`${declaration[0]} plural;`);
  assert.equal(count(0, 'note'), '0 notes');
  assert.equal(count(1, 'note'), '1 note');
  assert.equal(count(2, 'note'), '2 notes');
  assert.equal(count(2, 'class', 'classes'), '2 classes');
});

test('creation chooses explicit target, selected folder, open-note folder, then root', () => {
  const vm = require('node:vm');
  const declaration = read('web/app/side.js').match(/^const creationDir = .*;$/m)[0];
  const context = { side: { target: null }, state: { view: 'workspace', dir: '' } };
  vm.createContext(context);
  const choose = vm.runInContext(`${declaration} creationDir;`, context);
  assert.equal(choose(), '');
  context.state = { view: 'note', dir: 'Open' };
  assert.equal(choose(), 'Open');
  context.side.target = 'Selected';
  assert.equal(choose(), 'Selected');
  assert.equal(choose('Explicit'), 'Explicit');
  assert.equal(choose(''), '');
});

test('context targets capture ids and folders without changing the current note', () => {
  const vm = require('node:vm');
  const side = read('web/app/side.js');
  const source = side.slice(side.indexOf('function explorerTarget('), side.indexOf('\nfunction explorerMenu('));
  const target = vm.runInNewContext(`${source} explorerTarget;`);
  const el = (note, folder) => ({ closest: selector => selector === '.side-note-wrap' && note ? { querySelector: () => ({ dataset: note }) } : selector === '.side-folder' && folder ? { dataset: { dropDir: folder } } : null });
  assert.deepEqual(JSON.parse(JSON.stringify(target(el({ id: 'note-a', from: 'Folder A' })))), { kind: 'note', id: 'note-a', dir: 'Folder A' });
  assert.deepEqual(JSON.parse(JSON.stringify(target(el(null, 'Folder B')))), { kind: 'folder', dir: 'Folder B' });
  assert.deepEqual(JSON.parse(JSON.stringify(target(el()))), { kind: 'root', dir: '' });
});
