const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
test('dictation controls escape history and preserve explicit text settings', () => {
  const context = { app: { addEventListener() {} }, settingsOption: (value, label) => `<option>${label}</option>`, ICON: { chevron: '' }, esc: value => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;'), state: {}, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8'), context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  const html = context.dictationCards({ settings: { shortcut: 'option+space', custom_words: ['東京'], mode: 'both', popup: 'live' }, recent: [{ id: 1, text: '<img src=x onerror=bad()>' }], shortcut: {} });
  assert.ok(html.includes('&lt;img'));
  assert.ok(!html.includes('<img'));
  assert.ok(html.includes('東京'));
  assert.ok(html.includes('data-dictation="custom_words"'));
  for (const retired of ['Lid closed', 'closed_microphone', 'trailing_space', 'data-dictation="enter"']) assert.ok(!html.includes(retired));
  assert.ok(html.includes('class="switch" role="switch"'));
  assert.ok(!/type="checkbox"(?![^>]*class="switch")/.test(html));
  assert.ok(html.includes('<input type="hidden" data-dictation="mode" value="both">'));
  assert.ok(html.includes('role="radiogroup" aria-label="Popup"'));
  assert.ok(/<input type="checkbox" class="switch" role="switch" data-dictation="writing" data-on="quick">/.test(html));
  assert.ok(html.includes('Adds a few seconds'));
  assert.ok(html.includes('Matches the tone of the app you dictate into (only the app’s name is used).'));
  assert.ok(!/id="dictation-position-row" hidden/.test(html));
  assert.ok(/id="dictation-position-row" hidden/.test(context.dictationCards({ settings: { popup: 'off' }, recent: [], shortcut: {} })));
  assert.ok(!html.includes('data-action="dictation-save"'));
  assert.ok(!html.includes('Save after editing'));
  assert.ok(!html.includes('No writing AI is set up'));
  const unready = context.dictationCards({ settings: { writing: 'quick' }, writing_ready: false, recent: [], shortcut: {} });
  assert.ok(unready.includes('No writing AI is set up'));
  assert.ok(unready.includes('>Set up writing AI</a>'));
  const chosen = context.dictationCards({ settings: { writing: 'selected' }, recent: [], shortcut: {} });
  assert.ok(chosen.includes('data-dictation="writing" data-on="selected" checked'));
  const fields = (checked, on) => [{ dataset: { dictation: 'writing', on }, type: 'checkbox', checked }];
  context.state = { dictation: { settings: { writing: 'selected' } } };
  context.app.querySelectorAll = () => fields(false, 'selected');
  assert.equal(context.dictationDraft().writing, 'off');
  context.app.querySelectorAll = () => fields(true, 'selected');
  assert.equal(context.dictationDraft().writing, 'selected');
  context.app.querySelectorAll = () => fields(true, 'quick');
  assert.equal(context.dictationDraft().writing, 'quick');
});
function editContext(words, recent) {
  const listeners = {}, calls = [], panel = { html: '' }, field = { value: words.join('\n') }, result = { textContent: '' };
  const context = {
    app: { addEventListener: (name, fn) => { listeners[name] = fn; }, querySelectorAll: () => [{ dataset: { dictation: 'custom_words' }, value: field.value }] },
    ICON: { plus: '' }, esc: (value) => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;'),
    state: { dictation: { settings: { custom_words: words }, recent } }, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {}, clearTimeout() {},
    refreshPanel: (box, html) => { panel.html = html; },
    $: (selector) => selector === '#dictation-recent' ? {} : selector === '#dictation-result' ? result : selector === '[data-dictation="custom_words"]' ? field : null,
    fail: (e) => { throw e; },
    api: async (url, options) => {
      calls.push(options.body);
      const edited = context.state.dictation.recent.map((item) => item.id === options.body.id ? { ...item, text: options.body.text, edited: true } : item);
      return { settings: { custom_words: words }, recent: edited };
    },
    seq: 1, updateDictationStatus() {}, appVisible: () => false, pulse() {},
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  vm.runInContext('state.view = "settings"', context);
  context.field = field;
  return { context, listeners, calls, panel, field, result };
}
const plain = (value) => JSON.parse(JSON.stringify(value));
const type = (context, text) => vm.runInContext(`dictationEdit.text = ${JSON.stringify(text)}`, context);
const chipWords = (html) => [...html.matchAll(/data-word="([^"]*)"/g)].map((m) => m[1]);
test('editing a recent dictation offers only its new words as custom words', async () => {
  const { context, calls, panel } = editContext(['Felix'], [{ id: 7, text: 'ship it to leo seal I users, Felix says' }]);
  context.startDictationEdit({ dataset: { id: '7' } });
  assert.ok(panel.html.includes('data-edit-text'));
  type(context, 'ship it to Leo CLI users, Felix says Acme Corp now Zed Labs today Extra Word');
  await context.saveDictationEdit();
  assert.deepEqual(plain(calls), [{ action: 'edit', id: 7, text: 'ship it to Leo CLI users, Felix says Acme Corp now Zed Labs today Extra Word' }]);
  assert.deepEqual(chipWords(panel.html), ['Leo CLI', 'Acme Corp now Zed Labs today Extra Word']);
  assert.ok(panel.html.includes('Edited'));
  assert.ok(!panel.html.includes('data-edit-text'));
  assert.ok(panel.html.includes('Add \u201cLeo CLI\u201d to custom words'));
});
test('edit offers skip known words, punctuation-only changes and cap at three', () => {
  const offers = (before, after, known) => plain(editContext(known, []).context.dictationWordOffers(before, after, known));
  assert.deepEqual(offers('a leo b', 'a Leo b', ['leo']), []);
  assert.deepEqual(offers('hello users', 'hello users.', []), []);
  assert.deepEqual(offers('x y', 'x A1 y Bb y Cc y Dd', []), ['A1', 'Bb', 'Cc']);
  assert.deepEqual(offers('x y', 'x Q y Zed', []), ['Zed']);
  assert.deepEqual(offers('x y', 'x Zed y zed', []), ['Zed']);
});
test('accepting an offer adds the word once, respects the limit, and Escape cancels', async () => {
  const { context, listeners, calls, panel, field, result } = editContext(['Felix'], [{ id: 7, text: 'leo seal I' }]);
  context.startDictationEdit({ dataset: { id: '7' } });
  type(context, 'Leo CLI');
  await context.saveDictationEdit();
  const saved = [];
  context.saveDictation = () => { saved.push(field.value); return Promise.resolve(); };
  const chip = { dataset: { id: '7', word: 'Leo CLI' } };
  await context.addDictationWord(chip);
  assert.equal(field.value, 'Felix\nLeo CLI');
  assert.deepEqual(chipWords(panel.html), []);
  assert.equal(saved.length, 1);
  vm.runInContext("dictationOffers[7] = ['Leo CLI']", context);
  await context.addDictationWord(chip);
  assert.equal(field.value, 'Felix\nLeo CLI');
  field.value = Array.from({ length: 200 }, (_, n) => `w${n}`).join('\n');
  vm.runInContext("dictationOffers[7] = ['Other']", context);
  await context.addDictationWord({ dataset: { id: '7', word: 'Other' } });
  assert.equal(field.value.split('\n').length, 200);
  assert.ok(!field.value.includes('Other'));
  assert.ok(result.textContent.includes('full'));
  assert.deepEqual(chipWords(panel.html), []);
  context.state.dictation.recent = [{ id: 7, text: 'leo seal I' }];
  const before = calls.length;
  context.startDictationEdit({ dataset: { id: '7' } });
  type(context, 'something else');
  let stopped = false;
  listeners.keydown({ target: { matches: () => true }, key: 'Escape', preventDefault() {}, stopPropagation() { stopped = true; } });
  assert.equal(vm.runInContext('dictationEdit', context), null);
  assert.ok(stopped);
  assert.equal(calls.length, before);
  assert.ok(!panel.html.includes('data-edit-text'));
});

test('clearing history asks first', () => {
  const calls = [];
  const context = { app: { addEventListener() {} }, confirmSheet: (options) => calls.push(options), state: {}, window: {}, document: { activeElement: null, addEventListener() {} }, setTimeout() {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
  context.clearDictationsAsk();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].action, 'dictation-clear-now');
  assert.equal(calls[0].yes, 'Clear history');
});
test('popup live and minimal states keep useful errors and timers', () => {
  const { display } = require('../../leo-desktop/dist/dictation.js');
  assert.equal(display({ state: 'listening', seconds: 62.9, text: 'hello' }).timer, '1:02');
  assert.equal(display({ state: 'listening', minimal: true, committed: 'hello', tentative: 'there' }).text, '');
  assert.equal(display({ state: 'error', minimal: true, message: 'Choose a microphone.' }).text, 'Choose a microphone.');
  assert.equal(display({ state: 'error', message: 'x' }).label, 'Dictation stopped');
  assert.equal(display({ state: 'loading' }).label, 'Loading model…');
  const starting = display({ state: 'starting' });
  assert.equal(starting.label, 'Starting microphone…');
  assert.equal(starting.recording, false);
  assert.equal(display({ state: 'writing', committed: 'hi' }).label, 'Polishing with AI…');
  assert.equal(display({ state: 'writing', committed: 'hi' }).caret, false);
  const notice = display({ state: 'notice', message: 'The writing AI took too long.', committed: 'hi' });
  assert.equal(notice.label, 'Pasted without AI writing');
  assert.equal(notice.text, 'The writing AI took too long.');
  assert.equal(notice.caret, false);
  assert.equal(display({ state: 'notice', title: 'Nothing heard', message: 'x' }).label, 'Nothing heard');
  assert.equal(display({ state: 'notice', title: '', message: 'x' }).label, 'Pasted without AI writing');
  assert.equal(display({ state: 'error', title: 'Nothing heard', message: 'x' }).label, 'Dictation stopped');
  assert.equal(display({ state: 'listening', top: true }).top, true);
});
function popupPage() {
  const dir = path.join(__dirname, '../../leo-desktop/dist');
  const nodes = {}, listeners = {};
  const node = (id) => nodes[id] || (nodes[id] = {
    id, textContent: '', hidden: false, dataset: {}, style: {}, children: [], scrollTop: 0, scrollHeight: 0, clientHeight: 0,
    classList: { toggle() {} }, appendChild(child) { this.children.push(child); }, addEventListener() {}, setAttribute() {}, removeAttribute() {},
  });
  const root = {
    document: { body: node('body'), documentElement: node('html'), getElementById: node, createElement: () => ({ style: {} }) },
    addEventListener: (name, fn) => { listeners[name] = fn; }, setInterval() {},
    __TAURI_INTERNALS__: { invoke: () => new Promise(() => {}), transformCallback: (fn) => { root.emit = fn; return 1; } },
  };
  vm.runInNewContext(fs.readFileSync(path.join(dir, 'dictation.js'), 'utf8'), root);
  return { node, listeners, emit: (payload) => root.emit({ payload }) };
}
test('popup blocks its context menu, shows the notice title and drops stale revisions', () => {
  const page = popupPage();
  let prevented = false;
  page.listeners.contextmenu({ preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
  page.emit({ state: 'listening', rev: 5, seconds: 3 });
  assert.equal(page.node('wave').hidden, false);
  assert.equal(page.node('timer').textContent, '0:03');
  page.emit({ state: 'notice', rev: 6, title: 'Check the paste', message: 'Leo could not confirm the paste.' });
  assert.equal(page.node('label').textContent, 'Check the paste');
  assert.equal(page.node('message').textContent, 'Leo could not confirm the paste.');
  assert.equal(page.node('wave').hidden, true);
  assert.equal(page.node('timer').textContent, '');
  page.emit({ state: 'listening', rev: 4 });
  assert.equal(page.node('label').textContent, 'Check the paste');
});
test('popup page hides live words from screen readers and the red bars outside listening', () => {
  const dir = path.join(__dirname, '../../leo-desktop/dist');
  const html = fs.readFileSync(path.join(dir, 'dictation.html'), 'utf8');
  for (const id of ['timer', 'committed', 'tentative', 'caret']) assert.match(html, new RegExp(`<span id="${id}" aria-hidden="true"`));
  for (const id of ['label', 'message']) assert.doesNotMatch(html, new RegExp(`<span id="${id}"[^>]*aria-hidden`));
  assert.match(html, /<div id="pill" role="status" aria-live="polite">/);
  assert.match(fs.readFileSync(path.join(dir, 'dictation.css'), 'utf8'), /#wave\[hidden\] \{ display: none; \}/);
});
test('popup shows committed and tentative words like Handy and keeps them while writing', () => {
  const { display, smooth, height } = require('../../leo-desktop/dist/dictation.js');
  const live = display({ state: 'listening', committed: 'Hello there.', tentative: 'how are' });
  assert.equal(live.text, 'Hello there. how are');
  assert.equal(live.committed, 'Hello there. ');
  assert.equal(live.caret, true);
  assert.equal(live.open, true);
  const writing = display({ state: 'transcribing', committed: 'Hello there.', tentative: '' });
  assert.equal(writing.text, 'Hello there. ');
  assert.equal(writing.caret, false);
  assert.equal(display({ state: 'listening', tentative: 'hi' }).text, 'hi');
  assert.equal(display({ state: 'listening', minimal: true, committed: 'hi' }).open, false);
  assert.equal(display({ state: 'listening' }).caret, false);
  assert.deepEqual(smooth([1, 0], [0, 1]), [0.7, 0.3]);
  assert.equal(height(0), 3);
  assert.equal(height(1), 18);
  assert.equal(height(5), 18);
});

for (const debounced of [false, true]) {
  test(`${debounced ? 'debounced' : 'queued'} dictation saves keep their draft when navigation replaces page state`, async () => {
    const listeners = {}, timers = [], calls = [];
    const field = { dataset: { dictation: 'custom_words' }, value: 'Charlton\n東京' };
    const context = {
      app: { addEventListener: (name, listener) => { listeners[name] = listener; }, querySelectorAll: () => [field] },
      state: { view: 'settings', dictation: { settings: { shortcut: 'fn' } } }, seq: 1,
      clearTimeout() {}, setTimeout: callback => { timers.push(callback); },
      api: async (url, options) => { calls.push({ url, ...options }); return { settings: options.body.settings }; },
      fail: error => { throw error; },
    };
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/dictation.js'), 'utf8'), context);
    let saving;
    if (debounced) listeners.input({ target: field });
    else saving = context.saveDictation();
    context.state = { view: 'settings', section: 'calendar' };
    context.seq++;
    if (debounced) timers.shift()();
    await (saving || vm.runInContext('dictationSaving', context));
    assert.equal(calls.length, 1);
    assert.deepEqual(JSON.parse(JSON.stringify(calls[0].body.settings)), { shortcut: 'fn', custom_words: ['Charlton', '東京'] });
    assert.equal(context.state.dictation, undefined);
  });
}
