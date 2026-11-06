const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../src/web/chat.js'), 'utf8');
test('confirming a chat deletion retains its button and both animated labels', async () => {
  const classes = new Set(), attrs = new Map([['aria-label', 'Delete A long chat']]);
  const button = { focus() {}, dataset: { id: 'one' }, isConnected: true, classList: { toggle: (key, on) => on ? classes.add(key) : classes.delete(key), remove: key => classes.delete(key) }, setAttribute: (key, value) => attrs.set(key, value), getAttribute: key => attrs.get(key) };
  let draws = 0;
  const context = { state: { doomed: null, chats: [{ id: 'one', title: 'A long chat' }] }, panel: { querySelectorAll: () => [button], addEventListener() {} }, document: { addEventListener() {} }, drawChats: () => draws++, fetch: async () => ({ ok: true }), begin() {}, notify() {} };
  const start = source.includes('    function syncChatDelete(') ? source.indexOf('    function syncChatDelete(') : source.indexOf('    async function forget(');
  vm.createContext(context); vm.runInContext(source.slice(start, source.indexOf("    document.addEventListener('pointerdown', spare", start)), context);
  await context.forget('one');
  assert.equal(draws, 0, 'confirmation must not rebuild history');
  assert.equal(classes.has('sure'), true);
  assert.equal(attrs.get('aria-label'), 'Confirm delete A long chat');
  context.spare({ target: { closest: () => null } });
  assert.equal(classes.has('sure'), false);
  assert.equal(attrs.get('aria-label'), 'Delete A long chat');
});
test('history heading has no duplicate new-chat button and labels are escaped', () => {
  const heading = source.slice(source.indexOf('<div class="chat-history-head">'), source.indexOf('<input', source.indexOf('<div class="chat-history-head">')));
  assert.doesNotMatch(heading, /data-chat="new"/);
  const row = source.slice(source.indexOf('    function chatRow(c)'), source.indexOf('    let resumeSeq'));
  const context = { state: { id: 'one', doomed: null }, escape: text => text.replaceAll('<', '&lt;').replaceAll('>', '&gt;') };
  vm.createContext(context); vm.runInContext(row, context);
  const html = context.chatRow({ id: 'one', title: '<title>' });
  assert.match(html, /class="chat-delete-x"/); assert.match(html, /class="chat-delete-label"/);
  assert.match(html, /&lt;title&gt;/); assert.doesNotMatch(html, /<title>/);
});

test('Escape cancels a pending delete before another panel listener can stop an answer', () => {
  const panel = new EventTarget();
  let stopped = 0;
  const context = { panel, input: {}, state: { doomed: 'one', busy: { abort: () => stopped++ } }, syncChatDelete() {} };
  const start = source.indexOf("    panel.addEventListener('keydown', (e) => {", source.indexOf('    function spare(e)'));
  const end = source.indexOf("    document.addEventListener('pointerdown', spare", start);
  const busy = source.lastIndexOf("    panel.addEventListener('keydown', (e) => {");
  vm.createContext(context);
  vm.runInContext(source.slice(start, end) + source.slice(busy, source.indexOf('    return {', busy)), context);
  const escape = () => { const event = new Event('keydown', { cancelable: true }); Object.defineProperty(event, 'key', { value: 'Escape' }); panel.dispatchEvent(event); return event; };
  assert.equal(escape().defaultPrevented, true);
  assert.equal(context.state.doomed, null);
  assert.equal(stopped, 0);
  escape();
  assert.equal(stopped, 1);
});
