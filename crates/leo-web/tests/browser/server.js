const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const home = process.env.LEO_HOME;
if (!home) throw new Error('Browser tests need a disposable LEO_HOME. Run them through Playwright.');
fs.mkdirSync(home, { recursive: true });
fs.writeFileSync(path.join(home, 'config.toml'), `[providers.codex]\nbin = ${JSON.stringify(path.join(home, 'no-codex'))}\n[providers.claude_code]\nbin = ${JSON.stringify(path.join(home, 'no-claude'))}\n`, { flag: 'wx', mode: 0o600 });
const binary = process.env.LEO_TEST_BIN || path.resolve(__dirname, '../../../../target/debug/leo-test-server');
const port = process.env.LEO_TEST_PORT || '31831';
const child = spawn(binary, ['--port', port], { stdio: 'inherit', env: process.env });
function stop() {
  child.kill('SIGTERM');
}
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
child.on('exit', (code) => {
  fs.rmSync(process.env.LEO_HOME, { recursive: true, force: true });
  process.exit(code || 0);
});
