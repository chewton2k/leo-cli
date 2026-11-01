const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
function fixture() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const context = {matchMedia: () => ({matches:false})};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('const LEAVE_MS'), source.indexOf('function rel(')), context);
  const runs = [];
  const menu = {dataset:{},style:{},hidden:true,animate() { const run = {cancel() { queueMicrotask(() => run.oncancel?.()); }}; runs.push(run); return run; }};
  return {context,menu,runs};
}
test('reopening a closing menu clears its closing state immediately', () => {
  const {context,menu} = fixture();
  context.unfold(menu);
  context.fold(menu);
  assert.equal(menu.dataset.folding,'1');
  context.unfold(menu);
  assert.equal(menu.hidden,false);
  assert.equal(menu.dataset.folding,undefined);
});
test('cancelled menu animations cannot clear the state of a newer close', async () => {
  const {context,menu,runs} = fixture();
  context.unfold(menu);
  context.fold(menu);
  context.unfold(menu);
  context.fold(menu);
  await Promise.resolve();
  assert.equal(menu.dataset.folding,'1');
  runs.at(-1).onfinish();
  assert.equal(menu.hidden,true);
  assert.equal(menu.dataset.folding,undefined);
});
