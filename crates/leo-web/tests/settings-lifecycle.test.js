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
    const pending = context.changeSetting({set:'effort',value:'high'}).catch(() => {});
    context.seq++;
    context.state = {view:'folder',settings:{}};
    finish();
    await pending;
    assert.equal(draws, 0);
  }
});
test('the writing card says Ready only when the writing check is ready', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text), choiceCards: () => '' };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const task = { task: 'writing', provider: 'ollama', ready: true, choices: [], models: [] };
  for (const [writing, label] of [['ready', 'Ready'], ['no_model', 'Not set up'], [undefined, 'Not set up']]) {
    context.state = { settings: { writing: writing && { state: writing } } };
    assert.ok(context.taskCard(task).includes(`>${label}</span>`), writing);
  }
});
test('the writing card says where notes go for each provider and stays quiet for custom ones', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} }, window: {}, ICON: new Proxy({}, {get: () => ''}), esc: text => String(text), choiceCards: () => '', state: { settings: {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const line = (provider, task = 'writing') => (context.taskCard({ task, provider, choices: [], models: [] }).match(/<p class="set-note">([^<]*)<\/p>/) || [])[1];
  assert.equal(line('claude_code'), 'Notes and transcripts you send go to Anthropic through Claude Code.');
  assert.equal(line('codex'), 'Notes and transcripts you send go to OpenAI through Codex.');
  assert.equal(line('ollama'), 'Ollama runs on this computer; nothing leaves it.');
  assert.equal(line('my-custom'), undefined);
});
test('late onboarding completion cannot close a newer sheet', async () => {
  let finish, closed = 0;
  const context = { desktopApp: null, appChange: () => new Promise(resolve => { finish = resolve; }), closeSheet() { closed++; }, document: { querySelector: () => null } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/onboarding.js'), 'utf8'), context);
  context.maybeOnboarding = () => {};
  const pending = context.finishOnboarding({dataset:{step:'0'}});
  finish();
  await pending;
  assert.equal(closed, 0);
});

test('the note footer keeps its note tools without the bottom Ask button', () => {
  const core = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const script = core.slice(core.indexOf('function noteActions('), core.indexOf('async function showNote('));
  const context = { ON_MAC: true, ICON: new Proxy({}, {get: () => '<svg></svg>'}), esc: text => String(text) };
  vm.createContext(context);
  vm.runInContext(script, context);
  const html = context.noteActions('note-id', true);
  assert.doesNotMatch(html, /data-action="(?:ask-selection|note-map)"/);
  for (const name of ['Show transcript', 'Show tasks', 'Add picture', 'Move to folder', 'Export as PDF', 'Move to trash']) assert.ok(html.includes('aria-label="' + name + '"'));
});

test('Codex and Claude Code models list newest release first, then by power', () => {
  const context = { app: { addEventListener() {} }, document: { addEventListener() {} } };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const order = (ids) => JSON.stringify(context.modelsByRelease(ids.map((id) => ({ id }))).map((m) => m.id));
  assert.equal(order(['gpt-6-luna', 'gpt-6-sol', 'gpt-6.1-sol', 'gpt-6-astra']), JSON.stringify(['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna']));
  assert.equal(order(['claude-haiku-5-5', 'claude-sonnet-5-5', 'claude-opus-5-5', 'claude-fable-5-1']), JSON.stringify(['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5', 'claude-fable-5-1']));
  assert.equal(order(['plain', 'gpt-6.9-a', 'gpt-6.10-a']), JSON.stringify(['gpt-6.10-a', 'gpt-6.9-a', 'plain']));
});
test('the calendar card guides each provider and checks the address before saving', () => {
  const listeners = {};
  const nodes = {};
  const field = (extra = {}) => ({ value: '', attrs: {}, setAttribute(k, v) { this.attrs[k] = v; }, ...extra });
  const context = { app: { addEventListener() {} }, document: { addEventListener: (type, fn) => (listeners[type] = [...(listeners[type] || []), fn]) }, window: {}, ICON: new Proxy({}, { get: () => '' }), esc: text => String(text), $: sel => nodes[sel] || null, segmented: ({ current, choices, field: f }) => `<seg ${f} ${current}>${choices.map(c => c[1]).join('|')}</seg>`, state: {} };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8'), context);
  const firstLink = { google: 'https://calendar.google.com/calendar/r/settings', outlook: 'https://outlook.live.com/calendar/0/options/calendar/SharedCalendars', apple: 'https://www.icloud.com/calendar' };
  for (const [provider, url] of Object.entries(firstLink)) {
    context.state.calendarProvider = provider;
    const html = context.calendarCard({ calendars: [] });
    assert.equal((html.match(/<li>/g) || []).length, 3, provider);
    assert.ok(html.includes(`<li>Open <a href="${url}" target="_blank" rel="noopener noreferrer">`), provider);
    assert.ok(html.includes('<seg data-cal-provider ' + provider + '>Google|Outlook|Apple</seg>'));
  }
  assert.ok(context.calendarSteps('apple').includes('webcal://'));
  assert.ok(context.calendarSteps('outlook').includes('Publish a calendar'));
  const steps = { outerHTML: '' };
  nodes['.cal-steps'] = steps;
  nodes['#calendar-link'] = field();
  nodes['#calendar-error'] = { textContent: 'old' };
  for (const fn of listeners.change) fn({ target: { closest: () => ({ value: 'outlook' }) } });
  assert.equal(context.state.calendarProvider, 'outlook');
  assert.ok(steps.outerHTML.includes('outlook.live.com') && !steps.outerHTML.includes('google.com'));
  assert.equal(nodes['#calendar-error'].textContent, '');
  const share = 'That is the share link. Paste the secret address that ends in .ics.';
  const generic = 'Paste the calendar address that starts with https:// or webcal://.';
  assert.equal(context.calendarAddressError('https://calendar.google.com/calendar/u/0?cid=abc', 'google'), share);
  assert.equal(context.calendarAddressError('https://calendar.google.com/calendar?cid=abc', 'google'), share);
  assert.equal(context.calendarAddressError('hello', 'google'), generic);
  assert.equal(context.calendarAddressError('webcal://p01-caldav.icloud.com/published/2/abc', 'apple'), '');
  assert.equal(context.calendarAddressError('https://calendar.google.com/calendar/ical/me/private-abc/basic.ics', 'google'), '');
  context.state.calendarProvider = 'google';
  nodes['#calendar-link'].value = 'https://calendar.google.com/calendar/u/0?cid=abc';
  return context.addCalendar().then(() => {
    assert.equal(nodes['#calendar-error'].textContent, share);
    assert.equal(nodes['#calendar-link'].attrs['aria-invalid'], 'true');
  });
});
