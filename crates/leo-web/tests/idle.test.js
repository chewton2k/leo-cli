const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const refresh = fs.readFileSync(path.join(__dirname, '../src/web/app/refresh.js'), 'utf8');

function page(visibility, shown) {
  const context = { document: { visibilityState: visibility }, window: { __LEO_VISIBLE__: shown } };
  vm.createContext(context);
  vm.runInContext(refresh, context);
  return context;
}

test('the page counts as visible only when neither the webview nor the app says it is hidden', () => {
  assert.equal(page('visible', undefined).appVisible(), true);
  assert.equal(page('visible', true).appVisible(), true);
  assert.equal(page('hidden', true).appVisible(), false);
  assert.equal(page('visible', false).appVisible(), false);
});
