const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
test('permission steps share one page while existing completion flags stay compatible', () => {
  const context = {};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, false] }), 2);
  assert.equal(context.nextOnboarding({ onboarding: [true, true, true] }), -1);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, false] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [false, false, true] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [false, true, true] }), 0);
  assert.equal(context.nextOnboarding({ onboarding: [true, false, true] }), 0);
  assert.equal(context.nextOnboarding({}), 0);
});
