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
