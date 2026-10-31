const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
test('all three setup steps can be independently skipped and stay done', () => {
  const context = {};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, false] }), 1);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, false] }), 2);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true] }), -1);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, false] }), 0);
});
