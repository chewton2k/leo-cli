const fs = require('node:fs');
const path = require('node:path');
const { expect } = require('@playwright/test');

const token = () => fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();

async function signIn(page, hash = '') {
  await page.goto(`/?token=${token()}${hash}`);
  await expect(page.locator('#app')).not.toBeEmpty();
}

module.exports = { signIn, token };
