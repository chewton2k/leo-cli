const { defineConfig, devices } = require('@playwright/test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const home = process.env.LEO_BROWSER_HOME || fs.mkdtempSync(path.join(os.tmpdir(), 'leo-browser-'));
process.env.LEO_BROWSER_HOME = home;
module.exports = defineConfig({
  testMatch: 'flows.spec.js',
  globalTeardown: './teardown.js',
  workers: 1,
  timeout: 30000,
  use: {
    baseURL: 'http://127.0.0.1:31831',
    trace: 'retain-on-failure',
    launchOptions: { args: ['--host-resolver-rules=MAP leo-http.test 127.0.0.1', '--no-proxy-server', '--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] },
  },
  projects: [
    { name: 'desktop', use: { browserName: 'chromium' } },
    { name: 'phone', use: { ...devices['Pixel 7'] } },
  ],
  webServer: {
    command: 'node server.js',
    url: 'http://127.0.0.1:31831',
    reuseExistingServer: false,
    env: { LEO_HOME: home, LEO_NO_UPDATE_CHECK: '1', LEO_NO_MICROPHONE: '1', LEO_NO_OPEN: '1' },
  },
});
