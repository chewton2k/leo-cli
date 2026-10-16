const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/web/app/events.js'), 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('const unsaved =');
const end = source.indexOf("window.addEventListener(\n  'scroll',", start);
assert.ok(start > 0 && end > start);

function environment({ beacon = true, draft = false } = {}) {
  const windows = {}, documents = {}, sent = [], notices = [], saved = [];
  const context = {
    window: { addEventListener: (name, fn) => { windows[name] = fn; } },
    document: { visibilityState: 'visible', activeElement: null, body: {}, addEventListener: (name, fn) => { documents[name] = fn; } },
    state: { view: 'interviews', session: null },
    app: { contains: () => false },
    saving: { unsaved: () => false, retry: () => { saved.push('notes'); return Promise.resolve([]); } },
    interviewPage: draft ? { generation: 1, saved: 0 } : null,
    interviewHere: () => true,
    saveInterviewWorkspace: () => { saved.push('interview'); return Promise.resolve(); },
    navigator: { sendBeacon: (url, body) => { if (beacon === 'throw') throw new Error('Unavailable'); sent.push({ url, body, beacon: true }); return beacon; } },
    fetch: (url, options) => { sent.push({ url, ...options }); return Promise.resolve({ status: 204 }); },
    api: async () => context.backup,
    backup: { state: 'idle', at: 0, message: '' },
    toast: (message) => notices.push(message),
    route: () => {}, render: async () => {}, fail: () => {},
    Blob,
  };
  vm.createContext(context);
  vm.runInContext(source.slice(start, end), context);
  return { context, windows, documents, sent, notices, saved };
}

const tick = () => new Promise((resolve) => setImmediate(resolve));

test('leaving flushes note and interview work and sends one tiny beacon per departure', async () => {
  const e = environment({ draft: true });
  e.context.document.visibilityState = 'hidden';
  e.documents.visibilitychange();
  e.windows.pagehide();
  assert.deepEqual(e.saved, ['notes', 'interview', 'notes', 'interview']);
  assert.equal(e.sent.length, 1);
  assert.equal(e.sent[0].url, '/api/backup');
  assert.equal(e.sent[0].body.type, 'application/json');
  assert.equal(await e.sent[0].body.text(), '{}');
  e.context.document.visibilityState = 'visible';
  e.documents.visibilitychange();
  e.windows.pagehide();
  assert.equal(e.sent.length, 2);
});

test('a refused or unsupported beacon falls back to a same-origin keepalive request', () => {
  for (const beacon of [false, 'throw']) {
    const e = environment({ beacon });
    e.windows.pagehide();
    const request = e.sent.at(-1);
    assert.equal(request.url, '/api/backup');
    assert.equal(request.method, 'POST');
    assert.equal(request.keepalive, true);
    assert.equal(request.credentials, 'same-origin');
  }
});

test('pending interview work warns before closing but a cancelled close does not request a push', () => {
  for (const draft of [false, true]) {
    const e = environment({ draft });
    let prevented = false;
    e.windows.beforeunload({ preventDefault: () => { prevented = true; } });
    assert.equal(prevented, draft);
    assert.equal(e.saved.includes('interview'), draft);
    assert.equal(e.sent.length, 0);
  }
});

test('internal navigation never requests a backup and a restored page can leave again', () => {
  const e = environment();
  e.windows.hashchange();
  assert.equal(e.sent.length, 0);
  e.windows.pagehide();
  e.windows.pageshow();
  e.windows.pagehide();
  assert.equal(e.sent.length, 2);
});

test('backup failures appear once when returning and failed status reads stay quiet', async () => {
  const e = environment();
  e.context.backup = { state: 'failed', at: 10, message: 'Run leo backup to retry.' };
  e.documents.visibilitychange();
  await tick();
  e.documents.visibilitychange();
  await tick();
  assert.deepEqual(e.notices, ['Run leo backup to retry.']);
  e.context.backup.at = 11;
  e.documents.visibilitychange();
  await tick();
  assert.equal(e.notices.length, 2);
  e.context.api = async () => { throw new Error('Offline'); };
  e.documents.visibilitychange();
  await tick();
  assert.equal(e.notices.length, 2);
});
