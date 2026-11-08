const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

test.beforeEach(async ({ page }) => {
  const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
  await page.goto(`/?token=${token}#/new`);
  await expect(page.locator('#title')).toBeFocused();
});

for (const key of ['Enter', 'Tab']) {
  test(`${key} in the title moves to the body`, async ({ page }) => {
    await page.keyboard.type('Plain title');
    await page.keyboard.press(key);
    await expect(page.locator('#doc .line-edit')).toBeFocused();
    await expect(page.locator('#title')).toHaveText('Plain title');
  });
}

test('Shift+Enter never adds a line to the title, and Escape then Shift+Tab reaches the reading mode button', async ({ page }) => {
  await page.keyboard.type('One line');
  await page.keyboard.press('Shift+Enter');
  await expect(page.locator('#title')).toHaveText('One line');
  await expect(page.locator('#title br, #title div')).toHaveCount(0);
  await page.keyboard.press('Tab');
  await expect(page.locator('#doc .line-edit')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#doc .line-edit')).toHaveCount(0);
  await expect(page.locator('#margin-mode')).toBeEnabled();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#margin-mode')).toBeFocused();
});
