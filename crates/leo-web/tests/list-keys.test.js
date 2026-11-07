const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../src/web/app/select-menu.js'), 'utf8');
const from = source.indexOf('let listTyped');
const end = source.indexOf('\nfunction ', source.indexOf('function listKeyTarget') + 1);

function load() {
  const context = { Date: { now: () => context.now }, now: 1000 };
  vm.createContext(context);
  vm.runInContext(source.slice(from, end), context);
  return context;
}

const items = ['Alpha', 'Beta', 'Bravo', 'Gamma'].map((textContent) => ({ textContent }));
const key = (k, extra = {}) => ({ key: k, ...extra });

test('arrows clamp, Home and End jump, unknown current starts at the top', () => {
  const c = load();
  assert.equal(c.listKeyTarget(items, items[0], key('ArrowDown')), items[1]);
  assert.equal(c.listKeyTarget(items, items[0], key('ArrowUp')), items[0]);
  assert.equal(c.listKeyTarget(items, items[3], key('ArrowDown')), items[3]);
  assert.equal(c.listKeyTarget(items, items[1], key('End')), items[3]);
  assert.equal(c.listKeyTarget(items, items[2], key('Home')), items[0]);
  assert.equal(c.listKeyTarget(items, null, key('ArrowDown')), items[0]);
});

test('type-ahead cycles on one letter and narrows on more', () => {
  const c = load();
  assert.equal(c.listKeyTarget(items, items[0], key('b')), items[1]);
  c.now += 100;
  assert.equal(c.listKeyTarget(items, items[1], key('b')), items[2]);
  c.now += 1000;
  assert.equal(c.listKeyTarget(items, items[0], key('b')), items[1]);
  c.now += 100;
  assert.equal(c.listKeyTarget(items, items[1], key('r')), items[2]);
});

test('non-list keys are left alone', () => {
  const c = load();
  assert.equal(c.listKeyTarget(items, items[0], key('Enter')), null);
  assert.equal(c.listKeyTarget(items, items[0], key(' ')), null);
  assert.equal(c.listKeyTarget(items, items[0], key('k', { metaKey: true })), null);
});
