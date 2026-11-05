const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');
test.beforeEach(async ({ page }) => {
  const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
  await page.goto('/?token=' + token);
  await expect(page.locator('#app')).not.toBeEmpty();
});
async function note(page, title) {
  return (await page.request.post('/api/notes', { data: { title, body: 'Original paragraph.' } })).json();
}
test('a deleted note keeps its draft and offers an explicit path to Settings and Trash', async ({ page }) => {
  const item = await note(page, 'Deleted while editing');
  await page.goto('/#/n/' + item.id);
  await page.locator('#doc .blk').first().click();
  await page.request.delete('/api/notes/' + item.id);
  await page.locator('#doc textarea.line-edit').fill('Locally retained edits after deletion.');
  await expect(page.locator('#save-state')).toContainText('draft kept');
  await page.locator('#side [data-action="settings"]').click();
  await expect(page.locator('#title')).toHaveText(item.title);
  await page.getByRole('button', { name: 'Leave, draft kept', exact: true }).click();
  await expect(page.locator('.settings-layout')).toBeVisible();
  expect(await page.evaluate(() => Object.keys(localStorage).some(key => key.startsWith('leo-draft-v1:')))).toBe(true);
  await page.goto('/#/settings/trash');
  await expect(page.locator('#app')).toContainText(item.title);
  await page.request.post('/api/trash/' + item.id + '/restore', { data: {} });
  await page.goto('/#/n/' + item.id);
  await expect(page.locator('#doc')).toContainText('Locally retained edits after deletion.');
  await expect(page.locator('#save-state')).toHaveText('Saved');
});
test('a draft that could not be persisted cannot use the leave escape', async ({ page }) => {
  const refuseDrafts = () => {
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key.startsWith('leo-draft-v1:')) throw new DOMException('Full', 'QuotaExceededError');
      return set.call(this, key, value);
    };
  };
  await page.addInitScript(refuseDrafts);
  await page.evaluate(refuseDrafts);
  const item = await note(page, 'Memory-only draft');
  await page.goto('/#/n/' + item.id);
  await page.route('**/api/notes/' + item.id, route => route.request().method() === 'PATCH' ? route.fulfill({ status: 503, json: { error: 'Save refused. Try again.' } }) : route.continue());
  await page.locator('#doc .blk').first().click();
  await page.locator('#doc textarea.line-edit').fill('Keep this editor open.');
  await expect(page.locator('#save-state')).toContainText('keep this page open');
  await page.locator('#side [data-action="settings"]').click();
  await expect(page.locator('#title')).toHaveText(item.title);
  await expect(page.getByRole('button', { name: 'Leave, draft kept', exact: true })).toHaveCount(0);
});
test('rapid note choices during an in-flight save honor the latest destination', async ({ page }) => {
  const current = await note(page, 'A current draft'), first = await note(page, 'B first target'), latest = await note(page, 'C latest target');
  await page.goto('/#/n/' + current.id);
  let release; const waiting = new Promise(resolve => { release = resolve; });
  await page.route('**/api/notes/' + current.id, async route => { if (route.request().method() === 'PATCH') await waiting; await route.continue(); });
  await page.locator('#doc .blk').first().click();
  await page.locator('#doc textarea.line-edit').fill('The latest saved body.');
  await page.locator('#side .side-note[data-id="' + first.id + '"]').click();
  await page.locator('#side .side-note[data-id="' + latest.id + '"]').click();
  release();
  await expect(page.locator('#title')).toHaveText(latest.title);
  expect((await (await page.request.get('/api/notes/' + current.id)).json()).body).toBe('The latest saved body.');
});
test('chat history retains its delete control through confirmation and handles failure', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  const title = 'A longer conversation title with useful distinguishing words';
  await page.request.put('/api/chats/review-history', { data: { title, mode: 'chat', messages: [{ role: 'user', text: title }, { role: 'assistant', text: 'A saved answer.' }] } });
  await page.reload();
  await page.locator('#chat-toggle').click();
  const history = page.locator('#chat-history');
  if (!(await history.isVisible())) await page.locator('.chat-head [data-chat="history"]').click();
  await expect(history).toBeVisible();
  await expect(history.locator('[data-chat="new"]')).toHaveCount(0);
  await expect(page.locator('.chat-head [data-chat="new"]')).toBeVisible();
  const row = history.locator('.chat-history-row').filter({ has: page.locator('[data-id="review-history"]') });
  const button = row.locator('.chat-history-x');
  await button.evaluate(el => {
    window.retainedDelete = el;
    el.addEventListener('click', () => {
      window.deleteWidths = [];
      const started = performance.now();
      const sample = () => {
        window.deleteWidths.push(el.getBoundingClientRect().width);
        if (performance.now() - started < 400) requestAnimationFrame(sample);
      };
      sample();
    }, { once: true });
  });
  await button.click();
  expect(await button.evaluate(el => el === window.retainedDelete)).toBe(true);
  await expect(button).toHaveClass(/sure/);
  await expect(button).toBeFocused();
  await page.waitForTimeout(450);
  const widths = await page.evaluate(() => window.deleteWidths);
  expect(new Set(widths.map(value => Math.round(value))).size).toBeGreaterThan(2);
  expect(Math.max(...widths) - Math.min(...widths)).toBeGreaterThan(8);
  const font = await row.locator('.chat-history-title').evaluate(el => parseFloat(getComputedStyle(el).fontSize));
  expect(font).toBeLessThanOrEqual(13);
  await page.keyboard.press('Escape');
  await expect(button).not.toHaveClass(/sure/);
  await page.route('**/api/chats/review-history', route => route.request().method() === 'DELETE' ? route.fulfill({ status: 503 }) : route.continue());
  await button.click(); await button.click();
  await expect(row).toBeVisible();
  await expect(page.locator('.toast')).toContainText('could not delete');
  await expect(button).toBeEnabled();
  await page.unroute('**/api/chats/review-history');
  await button.click();
  await expect(row).toHaveCount(0);
});
