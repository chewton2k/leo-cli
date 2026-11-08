const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function context() {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = {};
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('function faNiceMax'), source.indexOf('function faFold')), value);
  return value;
}

test('the chart maximum is the next 1, 2, 2.5 or 5 times a power of ten', () => {
  const { faNiceMax } = context();
  for (const [value, max] of [[0, 1], [0.7, 1], [1, 1], [1.5, 2], [2.2, 2.5], [3, 5], [5, 5], [6, 10], [180, 200], [232500, 250000], [250000, 250000], [250001, 500000], [1000, 1000]]) assert.equal(faNiceMax(value), max, String(value));
});

test('models belong to the maker that trains them and shade by rank, a fourth going to Other', () => {
  const { faFamily, faShades } = context();
  for (const [name, family] of [['gpt-6.1-sol', 'openai'], ['o3-mini', 'openai'], ['codex-mini', 'openai'], ['gpt-oss:20b', 'local'], ['claude-sonnet-5-5', 'claude'], ['Opus', 'claude'], ['llama3.1', 'local'], ['', 'local']]) assert.equal(faFamily(name), family, name);
  const shades = faShades({ models: ['gpt-a', 'claude-a', 'gpt-b', 'gpt-c', 'gpt-d', 'llama3.1'] });
  assert.equal(shades['gpt-a'], 'var(--fam-openai-1)');
  assert.equal(shades['gpt-c'], 'var(--fam-openai-3)');
  assert.equal(shades['gpt-d'], undefined);
  assert.equal(shades['claude-a'], 'var(--fam-claude-1)');
  assert.equal(shades['llama3.1'], 'var(--fam-local-1)');
});
