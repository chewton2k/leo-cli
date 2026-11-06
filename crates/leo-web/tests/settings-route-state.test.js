const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
test('the Settings entry stays selected for Storage and Trash, which live inside Settings', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/side.js'), 'utf8');
  const helper = source.slice(source.indexOf('function sidePlace()'), source.indexOf('function drawSide()'));
  const context = { state:{view:'storage'},location:{hash:'#/settings'} };
  vm.createContext(context);vm.runInContext(helper, context);
  assert.equal(context.sidePlace(), 'settings');
  context.location.hash = '#/settings/storage';
  assert.equal(context.sidePlace(), 'settings');
  context.state.view = 'trash';
  context.location.hash = '#/settings/trash';
  assert.equal(context.sidePlace(), 'settings');
});
test('Back from a remembered Storage section returns to the note instead of looping', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const helper = source.slice(source.indexOf('function back()'), source.indexOf('const PAGE_NAMES'));
  let target;
  const context = { state:{view:'storage'},location:{hash:'#/settings'},places:{from:'#/settings/storage',before:'#/n/note'},asideHash:()=>true,goBack:hash=>{target=hash;} };
  vm.createContext(context);vm.runInContext(helper,context);
  context.back();assert.equal(target,'#/n/note');
});

test('retired Backup normalization preserves the history entry', async () => {
  const settings = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const start = settings.indexOf('async function showSettings(');
  const script = settings.slice(start, settings.indexOf('  // Trash', start)) + 'return section; }';
  const entry = { leoAt: 7, kept: true };
  let result;
  const context = { settingsSection: () => 'general', location: { hash: '#/settings/backup' }, history: { state: entry, replaceState: (...args) => { result = args; } } };
  vm.createContext(context); vm.runInContext(script, context);
  await context.showSettings('backup');
  assert.equal(result[0], entry);
  assert.equal(result[2], '#/settings/general');
});
