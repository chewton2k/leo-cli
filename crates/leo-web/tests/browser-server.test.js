const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, 'browser/server.js'), 'utf8');

test('browser server disables real coding CLIs before starting and refuses existing config', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'leo-browser-config-'));
  let started = 0;
  const context = {
    __dirname: path.join(__dirname, 'browser'),
    process: { env: { LEO_HOME: home }, on() {} },
    require: name => name === 'node:child_process' ? { spawn: () => { started++; return { on() {}, kill() {} }; } } : require(name),
  };
  try {
    vm.runInNewContext(source, context);
    assert.equal(started, 1);
    const config = fs.readFileSync(path.join(home, 'config.toml'), 'utf8');
    for (const name of ['codex', 'claude_code']) assert.ok(config.includes(`[providers.${name}]`));
    for (const name of ['no-codex', 'no-claude']) {
      assert.ok(config.includes(JSON.stringify(path.join(home, name))));
      assert.ok(!fs.existsSync(path.join(home, name)));
    }
    assert.throws(() => vm.runInNewContext(source, { ...context }), /EEXIST/);
    assert.equal(started, 1);
    assert.equal(fs.readFileSync(path.join(home, 'config.toml'), 'utf8'), config);
    assert.throws(() => vm.runInNewContext(source, { ...context, process: { env: {} } }), /disposable LEO_HOME/);
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});
