const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('native commands have explicit permissions scoped to their own window and origin', () => {
  const desktop = path.resolve(__dirname, '../../leo-desktop');
  const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
  const files = folder => fs.existsSync(folder) ? fs.readdirSync(folder).filter(name => name.endsWith('.json')).map(name => read(path.join(folder, name))) : [];
  const permissions = files(path.join(desktop, 'permissions')).flatMap(file => file.permission || []);
  const capabilities = files(path.join(desktop, 'capabilities'));
  for (const [command, window, remote] of [['print_page', 'main', true], ['dictation_popup', 'dictation', false]]) {
    const grants = permissions.filter(permission => permission.commands?.allow?.includes(command));
    assert.equal(grants.length, 1, `${command} needs an explicit permission`);
    assert.deepEqual(grants[0].commands.allow, [command]);
    const callers = capabilities.filter(capability => capability.permissions.includes(grants[0].identifier));
    assert.equal(callers.length, 1, `${command} needs one scoped capability`);
    assert.deepEqual(callers[0].windows, [window]);
    assert.deepEqual(callers[0].webviews || [], []);
    assert.equal(callers[0].local !== false, !remote);
    assert.deepEqual(callers[0].remote?.urls || [], remote ? ['http://127.0.0.1:*', 'http://localhost:*'] : []);
  }
});

test('desktop printing keeps the Windows native print function and bridges only macOS', async () => {
  const desktop = path.resolve(__dirname, '../../leo-desktop/src');
  const rust = fs.readFileSync(path.join(desktop, 'lib.rs'), 'utf8');
  const script = rust.match(/const PRINT: &str = "(.*)";/)?.[1] || fs.readFileSync(path.join(desktop, 'print.js'), 'utf8');
  for (const platform of ['mac', 'windows']) {
    let native = 0, bridge = 0;
    const original = () => { native++; };
    const window = { __LEO_APP__: platform, print: original, __TAURI_INTERNALS__: { invoke: async command => { assert.equal(command, 'print_page'); bridge++; } } };
    vm.runInNewContext(script, { window, Promise, setTimeout });
    const printed = window.print();
    assert.equal(bridge, 0, 'the note title reaches the page before the native print job reads it');
    await printed;
    assert.equal(native, platform === 'windows' ? 1 : 0);
    assert.equal(bridge, platform === 'mac' ? 1 : 0);
  }
});

test('PDF export reports print errors and restores its title', async () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');
  const share = source.slice(source.indexOf('async function share()'), source.indexOf('async function openTitle'));
  for (const rejects of [false, true]) {
    const listeners = new Map(), errors = [];
    const context = { seq: 1, document: { title: 'Leo' }, ready: async () => ({ note: { title: 'My note' } }), toast: (text, options) => errors.push([text, options]), window: { addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name), print: async () => { if (rejects) throw Error('raw programmer error'); listeners.get('afterprint')(); } } };
    vm.createContext(context);
    vm.runInContext(share, context);
    await context.share();
    assert.equal(context.document.title, 'Leo');
    assert.equal(listeners.size, 0);
    assert.equal(errors.length, rejects ? 1 : 0);
    if (rejects) assert.match(errors[0][0], /print|PDF/i);
  }
});

test('finishing a save or print never prints or renames a newer page', async () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');
  const share = source.slice(source.indexOf('async function share()'), source.indexOf('async function openTitle'));
  for (const navigate of ['save', 'print']) {
    const listeners = new Map();
    let printed = 0;
    const value = { seq: 1, document: { title: 'Leo' }, toast() {}, window: { addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name), print: async () => { printed++; value.seq++; value.document.title = 'New page'; listeners.get('afterprint')(); } } };
    value.ready = async () => { if (navigate === 'save') { value.seq++; value.document.title = 'New page'; } return { note: { title: 'My note' } }; };
    vm.createContext(value);
    vm.runInContext(share, value);
    await value.share();
    assert.equal(printed, navigate === 'save' ? 0 : 1);
    assert.equal(value.document.title, 'New page');
    assert.equal(listeners.size, 0);
  }
  let message;
  const empty = { seq: 1, ready: async () => null, toast: text => { message = text; } };
  vm.createContext(empty);
  vm.runInContext(share, empty);
  await empty.share();
  assert.match(message, /Write something first/);
});
