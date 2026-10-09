const { defineConfig, devices } = require('@playwright/test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const home = process.env.LEO_BROWSER_HOME || fs.mkdtempSync(path.join(os.tmpdir(), 'leo-browser-'));
process.env.LEO_BROWSER_HOME = home;
const port = '31832';
module.exports = defineConfig({
  testMatch: 'visual.spec.js',
  globalTeardown: './teardown.js',
  workers: 1,
  timeout: 60000,
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.01, animations: 'disabled', caret: 'hide' } },
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    launchOptions: { args: ['--no-proxy-server', '--font-render-hinting=none'] },
  },
  projects: [
    { name: 'desktop-light', use: { browserName: 'chromium', viewport: { width: 1280, height: 800 }, colorScheme: 'light' } },
    { name: 'desktop-dark', use: { browserName: 'chromium', viewport: { width: 1280, height: 800 }, colorScheme: 'dark' } },
    { name: 'phone-light', use: { ...devices['Pixel 7'], colorScheme: 'light' } },
    { name: 'phone-dark', use: { ...devices['Pixel 7'], colorScheme: 'dark' } },
  ],
  webServer: {
    command: 'node server.js',
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    env: { LEO_HOME: home, LEO_TEST_PORT: port, LEO_INSTALL_NO_MODEL: '1', LEO_NO_UPDATE_CHECK: '1', LEO_NO_MICROPHONE: '1', LEO_NO_OPEN: '1' },
  },
});
