const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const binary = process.env.LEO_TEST_BIN || path.resolve(__dirname, '../../../../target/debug/leo');
const child = spawn(binary, ['serve', '--local', '--port', '31831'], { stdio: 'inherit', env: process.env });
function stop() {
  child.kill('SIGTERM');
}
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
child.on('exit', (code) => {
  fs.rmSync(process.env.LEO_HOME, { recursive: true, force: true });
  process.exit(code || 0);
});
