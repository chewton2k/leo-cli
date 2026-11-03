const { test } = require('node:test');
const assert = require('node:assert/strict');
const choose = require('./ci-base.cjs');

const sha = (digit) => digit.repeat(40);

async function fixture({ event = 'push', runs = [], ancestors = [], tag = '', releaseIsNewer = false, fail = false } = {}) {
  const calls = [];
  const outputs = {};
  const warnings = [];
  await choose({
    context: { eventName: event, ref: 'refs/heads/main', repo: { owner: 'example', repo: 'example' },
      payload: { pull_request: { base: { sha: sha('a') } } } },
    github: { rest: { actions: { listWorkflowRuns: async (args) => {
      calls.push(args);
      if (fail) throw new Error('unavailable');
      return { data: { workflow_runs: runs } };
    } } } },
    exec: { getExecOutput: async (command, args) => {
      assert.equal(command, 'git');
      if (args[0] === 'describe') return { exitCode: tag ? 0 : 1, stdout: tag };
      assert.deepEqual(args.slice(0, 2), ['merge-base', '--is-ancestor']);
      if (args[3] !== 'HEAD') return { exitCode: releaseIsNewer ? 0 : 1 };
      return { exitCode: ancestors.includes(args[2]) ? 0 : 1 };
    } },
    core: { setOutput: (name, value) => { outputs[name] = value; }, info: () => {},
      warning: (message) => { warnings.push(message); } },
  });
  return { base: outputs.base, calls, warnings };
}

test('only successful ancestors can advance the main baseline', async () => {
  const result = await fixture({ runs: [
    { conclusion: 'cancelled', head_sha: sha('1') },
    { conclusion: 'failure', head_sha: sha('2') },
    { conclusion: 'success', head_sha: sha('3') },
    { conclusion: 'success', head_sha: sha('4') },
  ], ancestors: [sha('1'), sha('2'), sha('4')], tag: 'v1.0.0' });
  assert.equal(result.base, sha('4'));
  assert.equal(result.calls.length, 1);
  assert.equal(result.calls[0].status, 'success');
  assert.equal(result.calls[0].event, 'push');
  assert.equal(result.calls[0].branch, 'main');
  assert.equal(result.calls[0].per_page, 100);
  assert.equal(result.calls[0].request.timeout, 10000);
});

test('the latest successful ancestor is preferred over a release tag', async () => {
  const result = await fixture({ runs: ['1', '2'].map((digit) => ({ conclusion: 'success', head_sha: sha(digit) })),
    ancestors: [sha('1'), sha('2')], tag: 'v1.0.0' });
  assert.equal(result.base, sha('1'));
});

test('missing history and API failures fall back to the last release', async () => {
  for (const fail of [false, true]) {
    const result = await fixture({ fail, tag: 'v1.0.0\n' });
    assert.equal(result.base, 'v1.0.0');
    assert.equal(result.warnings.length, fail ? 1 : 0);
  }
});

test('no trusted history means full coverage', async () => {
  assert.equal((await fixture()).base, '');
  assert.equal((await fixture({ fail: true })).base, '');
});

test('a newer release includes the bot version bump in the baseline', async () => {
  const result = await fixture({ runs: [{ conclusion: 'success', head_sha: sha('1') }],
    ancestors: [sha('1')], tag: 'v1.0.1', releaseIsNewer: true });
  assert.equal(result.base, 'v1.0.1');
});

test('manual and scheduled runs always request full coverage', async () => {
  for (const event of ['schedule', 'workflow_dispatch']) {
    const result = await fixture({ event, tag: 'v1.0.0' });
    assert.equal(result.base, '');
    assert.equal(result.calls.length, 0);
  }
});

test('pull requests compare with their base without reading workflow history', async () => {
  const result = await fixture({ event: 'pull_request' });
  assert.equal(result.base, sha('a'));
  assert.equal(result.calls.length, 0);
});

test('untrusted workflow SHAs are never passed to git', async () => {
  const result = await fixture({ runs: [{ conclusion: 'success', head_sha: '--help' }], ancestors: ['--help'] });
  assert.equal(result.base, '');
});
