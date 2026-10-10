'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/recorder.js');
require('../src/web/recording.js');

const R = globalThis.leoRecorder;
const L = globalThis.leoRecording;

test('48 kHz audio becomes 16 kHz: three samples in, one out, averaged', () => {
  const down = new R.Downsampler(48000);
  const out = down.push([Float32Array.from([0.3, 0.3, 0.3, -0.6, -0.6, -0.6])]);
  assert.deepEqual(out, [Math.round(0.3 * 32767), Math.round(-0.6 * 32768)]);
});

test('44.1 kHz audio keeps the right length over a second, across uneven blocks', () => {
  const down = new R.Downsampler(44100);
  let made = 0;
  for (let i = 0; i < 44100; i += 128) made += down.push([new Float32Array(Math.min(128, 44100 - i))]).length;
  assert.ok(Math.abs(made - 16000) <= 1, `made ${made}`);
});

test('stereo is mixed to mono and loud input is clipped, not wrapped', () => {
  const down = new R.Downsampler(16000);
  assert.deepEqual(down.push([Float32Array.from([1, 2]), Float32Array.from([0, 2])]), [16384, 32767]);
  assert.deepEqual(new R.Downsampler(16000).push([Float32Array.from([-3])]), [-32768]);
});

test('samples go to leo as little-endian 16-bit, in order', () => {
  const bytes = L.bytesOf(L.join([Int16Array.from([1, -2]), Int16Array.from([258])]));
  assert.deepEqual([...bytes], [1, 0, 0xfe, 0xff, 2, 1]);
});

test('audio waiting for a connection is capped by dropping the oldest, never the newest', () => {
  const held = [new Int16Array(10), new Int16Array(10), Int16Array.from([7])];
  assert.equal(L.trimHeld(held, 15), 10);
  assert.equal(held.length, 2);
  assert.equal(held[1][0], 7);
  const one = [new Int16Array(50)];
  assert.equal(L.trimHeld(one, 10), 0);
});

test('the clock reads like a timer', () => {
  assert.equal(L.clock(0), '0:00');
  assert.equal(L.clock(65.9), '1:05');
  assert.equal(L.clock(3723), '1:02:03');
});

test('the microphone needs a secure page, and each refusal says what to do', () => {
  const ok = { isSecureContext: true, mediaDevices: { getUserMedia() {} }, AudioWorkletNode: function () {} };
  assert.equal(L.micReady(ok), 'ok');
  assert.equal(L.micReady({ ...ok, isSecureContext: false }), 'insecure');
  assert.equal(L.micReady({ ...ok, mediaDevices: undefined }), 'unsupported');
  assert.match(L.micProblem('insecure'), /https link/);
  assert.match(L.micProblem({ name: 'NotAllowedError' }), /Allow it/);
  assert.match(L.micProblem({ name: 'NotFoundError' }), /No microphone/);
  assert.match(L.micProblem({ name: 'Weird', message: 'boom' }), /boom/);
});

test('the state line says what is happening, with steps while writing', () => {
  assert.equal(L.stateWord({ state: 'recording' }), 'Recording');
  assert.equal(L.stateWord({ state: 'writing', step: 'Writing the notes', steps: [1, 3] }), 'Writing the notes 1/3');
  assert.equal(L.stateWord({ state: 'writing', step: 'Transcribing the recording', steps: null }), 'Transcribing the recording');
});

test('a tab or screen can be shared only where the browser offers it, and refusals say what to do', () => {
  const full = { isSecureContext: true, mediaDevices: { getUserMedia() {}, getDisplayMedia() {} }, AudioWorkletNode: function () {} };
  assert.equal(L.canShareSound(full), true);
  assert.equal(L.canShareSound({ ...full, mediaDevices: { getUserMedia() {} } }), false, 'phones have no getDisplayMedia');
  assert.equal(L.canShareSound({ ...full, isSecureContext: false }), false);
  assert.match(L.sharingProblem('no-audio'), /Share tab audio/);
  assert.match(L.sharingProblem('unsupported'), /Chrome or Edge/);
  assert.match(L.sharingProblem({ name: 'NotAllowedError' }), /Nothing was shared/);
  assert.equal(L.fedByBrowser('tab'), true);
  assert.equal(L.fedByBrowser('screen'), false);
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
  assert.match(L.hearingWords('silent', 'tab', false), /Share tab audio/);
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
  assert.equal(L.sourceFor('microphone', { local: true, canShare: true }), 'browser');
  assert.equal(L.sourceFor('microphone', { local: false, canShare: false }), 'browser');
  assert.equal(L.sourceFor('screen', { local: true, canShare: false }), 'screen', 'on the computer: everything it plays');
  assert.equal(L.sourceFor('screen', { local: false, canShare: true }), 'tab', 'elsewhere: share a tab or screen');
  assert.equal(L.sourceFor('screen', { local: false, canShare: false }), null, 'a phone cannot record its screen');
});

test('call worklet keeps microphone and system tracks separate and flushes a short tail', () => {
  const vm=require('node:vm'); const fs=require('node:fs'); let Recorder;
  const sent=[];
  class Processor { constructor() { this.port={postMessage:(message) => sent.push(message)}; } }
  const sandbox={sampleRate:16000,AudioWorkletProcessor:Processor,registerProcessor:(_,cls) => { Recorder=cls; }};
  vm.runInNewContext(fs.readFileSync(require.resolve('../src/web/recorder.js'),'utf8'),sandbox);
  const recorder=new Recorder({processorOptions:{dual:true}});
  recorder.process([[Float32Array.from([0.5,0]),Float32Array.from([0,-0.5])]]);
  assert.equal(sent.length,0);
  recorder.port.onmessage({data:{flush:true}});
  assert.deepEqual([...new Int16Array(sent[0].samples)],[16384,0,0,-16384]); assert.equal(sent[1].flushed,true);
});
