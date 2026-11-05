const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

test.beforeEach(async ({ page }) => {
  const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
  await page.goto(`/?token=${token}`);
  await expect(page.locator('#app')).not.toBeEmpty();
});

test('note reading modes sit above the title, work in both orders and retain the confirmed mode on failure', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Reading modes', body: 'Words about a passage.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  const button = page.locator('#margin-mode');
  await expect(button).toBeEnabled();
  expect((await button.boundingBox()).y).toBeLessThan((await page.locator('#title').boundingBox()).y);
  for (const [key, label] of [['Home', 'Reads along'], ['End', 'When you ask'], ['Home', 'Reads along'], ['End', 'When you ask']]) {
    await button.click();
    await page.waitForTimeout(300);
    await expect(page.getByRole('menu', { name: 'Note reading mode' })).toBeVisible();
    await page.keyboard.press(key);
    await page.keyboard.press('Enter');
    await expect(button).toHaveText(label);
    await expect(button).toBeEnabled();
  }
  await page.reload();
  await expect(button).toHaveText('When you ask');
  await page.route(`**/api/notes/${note.id}/margin`, route => route.request().method() === 'POST'
    ? route.fulfill({ status: 503, json: { error: 'Try again.' } }) : route.continue());
  await button.click();
  await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
  await expect(button).toHaveText('When you ask');
  await expect(page.locator('#margin-status')).toHaveText('Could not save this mode. Try again.');
  await button.click();
  await page.keyboard.press('Escape');
  await expect(button).toBeFocused();
  await expect(page.getByRole('menu', { name: 'Note reading mode' })).toBeHidden();
});

test('compact margin anchors stay outside the passage and open real persisted replies without a mascot', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const quote = 'The password is stored as plain text.';
  const note = await (await page.request.post('/api/notes', { data: { title: 'Compact annotation', body: quote } })).json();
  const response = await page.request.post(`/api/notes/${note.id}/margin/threads`, { data: { version: note.version, quote, question: 'What should I change?' } });
  expect(response.ok()).toBe(true);
  const thread = await response.json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('.note-columns')).toHaveAttribute('data-margin', 'marks');
  const mark = page.locator('.margin-mark');
  await expect(mark).toBeVisible();
  await expect(mark.locator('.felix')).toHaveCount(0);
  const anchor = await mark.locator('.margin-anchor').boundingBox();
  const paragraph = await page.locator('#doc .blk').first().boundingBox();
  expect(anchor.x + anchor.width).toBeLessThanOrEqual(paragraph.x);
  const stroke = page.locator('.margin-ink .squiggle');
  const dash = await stroke.evaluate(el => {
    const length = el.getTotalLength();
    const units = el.hasAttribute('pathLength') ? el.pathLength.baseVal.value : length;
    return parseFloat(getComputedStyle(el).strokeDasharray) * length / units;
  });
  expect(dash).toBeLessThanOrEqual(2);
  await mark.click();
  await expect(page.locator('.margin-note .margin-felix')).toContainText(thread.messages.at(-1).text);
  await expect(page.locator('.margin-reply textarea')).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  expect(await page.locator('.margin-ink .squiggle').evaluate(el => getComputedStyle(el).animationName)).toBe('none');
});
