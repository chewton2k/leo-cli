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
