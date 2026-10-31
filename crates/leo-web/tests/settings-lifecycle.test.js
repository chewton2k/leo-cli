const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
test('late Settings saves cannot redraw a different page on success or failure', async () => {
  for (const failed of [false, true]) {
    let finish;
    const context = { app: { addEventListener() {} }, seq: 1, state: { view: 'settings', settings: {} }, document: { addEventListener() {} }, api: () => new Promise((resolve, reject) => { finish = () => failed ? reject(new Error('failed')) : resolve({settings:{},message:'Saved'}); }), window: {}, toast() {} };
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
    let draws = 0;
    context.drawSettings = () => { draws++; };
    const pending = context.changeSetting({auto_push:'quit'}).catch(() => {});
    context.seq++;
    context.state = {view:'folder',settings:{}};
    finish();
    await pending;
    assert.equal(draws, 0);
  }
});
test('late onboarding completion cannot close a newer sheet', async () => {
  let finish, closed = 0;
  const context = { appChange: () => new Promise(resolve => { finish = resolve; }), closeSheet() { closed++; }, document: { querySelector: () => null } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  context.maybeOnboarding = () => {};
  const pending = context.finishOnboarding({dataset:{step:'0'}});
  finish();
  await pending;
  assert.equal(closed, 0);
});
