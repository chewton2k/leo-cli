'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/recording.js');

const L = globalThis.leoRecording;

test('background recording levels stay bounded without a mounted waveform', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
  const start = source.indexOf('    function feedFromServer(view)');
  const end = source.indexOf('    function advance(now)', start);
  const state = { queue: [], lastFed: 0, fedUpTo: 0 };
  const feed = require('node:vm').runInNewContext(`(function(){${source.slice(start, end)} return feedFromServer;})()`, {
    s: state, freshLevels: L.freshLevels, loudness: L.loudness, spread: L.spread, SERVER_STEP_MS: 250, WAVE_STEP_MS: 60, MOST_LEVEL_QUEUE: 40,
  });
  for (let poll = 0; poll < 1000; poll++) {
    feed({ levels: [0.1, 0.2, 0.3, 0.4], levels_start: poll * 4 });
    assert.ok(state.queue.length <= 40);
  }
  assert.equal(state.queue.length, 40);
  assert.equal(state.fedUpTo, 4000);
});

test('the clock reads like a timer', () => {
  assert.equal(L.clock(0), '0:00');
  assert.equal(L.clock(65.9), '1:05');
  assert.equal(L.clock(3723), '1:02:03');
});

test('the state line says what is happening, with steps while writing', () => {
  assert.equal(L.stateWord({ state: 'recording' }), 'Recording');
  assert.equal(L.stateWord({ state: 'writing', step: 'Writing the notes', steps: [1, 3] }), 'Writing the notes 1/3');
  assert.equal(L.stateWord({ state: 'writing', step: 'Transcribing the recording', steps: null }), 'Transcribing the recording');
});

test('loudness follows how loud it sounds, not raw amplitude', () => {
  assert.equal(L.loudness(0), 0);
  assert.equal(L.loudness(-1), 0);
  assert.equal(L.loudness(Number.NaN), 0);
  assert.equal(L.loudness(1), 1);
  const whisper = L.loudness(0.005);
  const speech = L.loudness(0.1);
  assert.ok(whisper > 0 && whisper < L.QUIET, `a whisper of noise reads as quiet: ${whisper}`);
  assert.ok(speech > 0.5, `normal speech fills most of the wave: ${speech}`);
});

test('the wave says when sound is heard, and what to check when it is not', () => {
  const loud = Array(10).fill(0.6);
  assert.equal(L.hearing([], 80), 'waiting');
  assert.equal(L.hearing(loud, 80), 'sound');
  assert.equal(L.hearing([...loud, ...Array(10).fill(0)], 80), 'sound', 'a short pause between words still counts');
  assert.equal(L.hearing([...loud, ...Array(30).fill(0)], 80), 'quiet');
  assert.equal(L.hearing(Array(30).fill(0), 250), 'silent');
  assert.equal(L.hearingWords('sound', 'browser', false), 'Hearing sound');
  assert.match(L.hearingWords('silent', 'browser', false), /microphone/);
  assert.match(L.hearingWords('silent', 'screen', false), /playing on the computer/);
  assert.match(L.hearingWords('sound', 'browser', true), /Paused/);
});

test('the wave rises quickly with sound and falls back gently', () => {
  let level = 0;
  level = L.ease(level, 1);
  assert.ok(level > 0.5, `a sound shows at once: ${level}`);
  const peak = level;
  level = L.ease(level, 0);
  assert.ok(level > peak * 0.8, `and fades slowly: ${level}`);
  let settled = 1;
  for (let i = 0; i < 60; i++) settled = L.ease(settled, 0);
  assert.ok(settled < 0.01);
});

test('levels from the computer are spread into smooth steps and never repeated', () => {
  assert.deepEqual(L.spread(0, 1, 4), [0.25, 0.5, 0.75, 1]);
  assert.deepEqual(L.freshLevels([0.1, 0.2, 0.3], 10, 0), { fresh: [0.1, 0.2, 0.3], fedUpTo: 13 });
  assert.deepEqual(L.freshLevels([0.2, 0.3, 0.4], 11, 13), { fresh: [0.4], fedUpTo: 14 });
  assert.deepEqual(L.freshLevels([0.2, 0.3, 0.4], 11, 14), { fresh: [], fedUpTo: 14 });
  assert.deepEqual(L.freshLevels([0.5, 0.6], 50, 14), { fresh: [0.5, 0.6], fedUpTo: 52 }, 'after a gap, only what is still there');
});

test('two choices: the microphone here, or the screen, wherever that can work', () => {
  assert.equal(L.sourceFor('microphone', { local: true, canShare: true }), 'microphone');
  assert.equal(L.sourceFor('microphone', { local: false, canShare: false }), 'microphone');
  assert.equal(L.sourceFor('screen', { local: true, canShare: false }), 'screen', 'on the computer: everything it plays');
  assert.equal(L.sourceFor('screen', { local: false, canShare: true }), 'screen', 'elsewhere: share a tab or screen');
  assert.equal(L.sourceFor('screen', { local: false, canShare: false }), 'screen', 'a phone cannot record its screen');
});

test('empty-call failures have a calm explanation and keep technical details separate', () => {
  const silent = 'No sound was recorded. Check that the browser may use the microphone.';
  const failure = L.failureOf({ error: `Others: ${silent}; You: ${silent}`, warnings: ['Recovered audio from the last interrupted recording.'] });
  assert.equal(failure.title, 'No audio was captured');
  assert.match(failure.message, /stopped before any sound/);
  assert.doesNotMatch(failure.message, /Others:|You:|Recovered/);
  assert.equal(failure.details.length, 2);
  assert.equal(L.failureOf({ error: 'Provider failed', warnings: [] }).title, 'Your note could not be saved');
});

test('failed recordings offer deletion and dismissal on their own page', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
  const start = source.indexOf('    function drawFailed()');
  const end = source.indexOf('    let folders', start);
  const render = (overview, error) => require('node:vm').runInNewContext(`(function(){${source.slice(start, end)} return drawFailed;})()`, {
    s: { overview, view: { id: 'failed-01', session: 'sess-1', source: 'nativecall', secs: 0, error, warnings: [] } },
    failureOf: L.failureOf, clock: L.clock, esc: (s) => String(s).replaceAll('<', '&lt;'),
    felix: { felix: () => '' }, icons: { mic: '', trash: '' },
  })();
  const html = render({ pending: [{ id: 'sess-1' }] }, '<unsafe>');
  assert.match(html, /data-action="rec-discard" data-id="failed-01"/);
  assert.match(html, /data-action="rec-dismiss"/);
  assert.match(html, /<details[^>]*.*Error details/);
  assert.ok(!html.includes('<unsafe>'));
  assert.match(render({ pending: [{ id: 'sess-1' }] }, 'Provider failed'), /data-action="rec-recover" data-id="sess-1"[^>]*>Recover recording<.*>Close</s);
  assert.doesNotMatch(render({ pending: [] }, 'Provider failed'), /rec-recover/);
  assert.doesNotMatch(render({ pending: [{ id: 'sess-1' }] }, 'No sound was recorded.'), /rec-recover/);
});

test('an interrupted recording says when it started, what it was and where it goes', () => {
  const started = new Date();
  started.setHours(9, 2, 0, 0);
  assert.match(L.pendingLine({ started: started.toISOString(), source: 'microphone', directory: 'cs162' }), /^Today .+ · Microphone · cs162$/);
  assert.equal(L.pendingLine({ source: 'microphone', directory: 'cs162' }), 'Microphone · cs162');
  assert.equal(L.pendingLine({ source: 'nativecall', directory: '' }), 'Call · Top level');
});

test('a recording saved without written notes says so where the user lands', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/recording.js'), 'utf8');
  const start = source.indexOf('    function accept(view)');
  const end = source.indexOf('    async function start(', start);
  const finish = (view) => {
    const went = [];
    const toasts = [];
    const accept = require('node:vm').runInNewContext(`(function(){${source.slice(start, end)} return accept;})()`, {
      s: { view: { id: 'r1', state: 'writing', warnings: [] }, container: { isConnected: true } },
      api: () => Promise.resolve(), feedFromServer: () => {}, draw: () => {}, pill: () => {}, noteReady: null,
      go: (hash) => went.push(hash), noteHash: (id) => `#/n/${id}`, toast: (text, opts) => toasts.push({ text, ...opts }), root: {},
    });
    accept({ id: 'r1', state: 'done', note: 'n1', warnings: [], ...view });
    return { went, toasts };
  };
  const failed = finish({ writer_failed: true });
  assert.deepEqual(failed.went, ['#/n/n1']);
  assert.equal(failed.toasts.length, 1);
  assert.equal(failed.toasts[0].text, 'Saved the transcript. The notes were not written.');
  assert.equal(failed.toasts[0].action, 'Set up writing AI');
  failed.toasts[0].run();
  assert.equal(failed.went.at(-1), '#/settings/writing');
  assert.equal(finish({}).toasts[0].text, 'Your recording is now a note.');
});

test('a note whose recording was not written up offers both ways forward', () => {
  const source = require('node:fs').readFileSync(require.resolve('../src/web/app/core.js'), 'utf8');
  const start = source.indexOf('${note.writeup_pending');
  const end = source.indexOf(" : ''}", start) + 6;
  const notice = (note) => require('node:vm').runInNewContext(`\`${source.slice(start, end)}\``, { note });
  const html = notice({ writeup_pending: true });
  assert.match(html, /<div class="set-card writeup-pending"><h3>These notes are not written up yet<\/h3><p class="set-note">No writing AI was ready, so leo kept the transcript as it is\.<\/p>/);
  assert.match(html, /<a class="btn primary sm" href="#\/settings\/writing">Set up writing AI<\/a>/);
  assert.match(html, /<button type="button" class="btn plain sm" data-action="note-sources">Write the notes<\/button>/);
  assert.equal(notice({}), '');
});
