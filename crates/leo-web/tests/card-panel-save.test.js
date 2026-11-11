const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/web/app/boards.js'), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('  const running = {};');
const end = source.indexOf('  const names = [', start);
assert.ok(start > 0 && end > start);

test('card panel saves of one field run one after another, so the newest text lands last', async () => {
  const card = { id: 'c1', description: '' };
  const field = { text: 'first' };
  const writes = [];
  const releases = [];
  const context = {
    card, clearTimeout, timers: {}, saved: { isConnected: false }, problem: { isConnected: false }, title: {}, fit() {}, tellOver: () => false, tell() {}, toast() {}, enc: (s) => s,
    Locked: class extends Error {},
    valueOf: () => field.text,
    kanbanWrite: (url, { body }) => new Promise((resolve) => { writes.push(body.description); releases.push(() => resolve(body)); }),
    kanbanUpdate: (c, done) => Object.assign(c, done),
  };
  vm.createContext(context);
  vm.runInContext(`${source.slice(start, end)}\n;globalThis.__save = save;`, context);
  const first = context.__save('description');
  await new Promise((r) => setImmediate(r));
  field.text = 'second';
  const second = context.__save('description');
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(writes, ['first'], 'the second save waits for the first');
  releases[0]();
  await first;
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(writes, ['first', 'second']);
  releases[1]();
  await second;
  assert.equal(card.description, 'second');
});
