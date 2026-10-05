const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

test.beforeEach(async ({ page }) => {
  const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
  await page.goto(`/?token=${token}`);
  await expect(page.locator('#app')).not.toBeEmpty();
});

async function openNote(page, title) {
  const response = await page.request.post('/api/notes', { data: { title, body: 'Original sentence' } });
  expect(response.status()).toBe(201);
  const note = await response.json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#title')).toHaveText(title);
  await page.locator('.blk').first().click();
  return note;
}

test('a failed save survives navigation and reload, then saves on reconnection', async ({ page }) => {
  const note = await openNote(page, 'Draft recovery');
  await page.route('**/api/notes/*', (route) => route.request().method() === 'PATCH'
    ? route.fulfill({ status: 500 }) : route.continue());
  await page.locator('.line-edit').fill('Keep this phone draft');
  await expect(page.locator('#save-state')).toContainText('draft kept');
  await page.locator('#back').click();
  await page.reload();
  await page.locator('#menu').click();
  await page.locator('[data-action="drafts"]').click();
  await page.getByRole('button', { name: /Draft recovery/ }).click();
  await expect(page.locator('#doc')).toContainText('Keep this phone draft');
  const original = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(original.body).toBe('Original sentence');
  await page.unroute('**/api/notes/*');
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await expect(page.locator('#save-state')).toHaveText('Saved');
  const saved = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(saved.body).toBe('Keep this phone draft');
  expect(await page.evaluate(() => Object.keys(localStorage).filter((key) => key.startsWith('leo-draft-v1:')))).toEqual([]);
});

test('typing during an in-flight save keeps the latest edit', async ({ page }) => {
  const note = await openNote(page, 'Slow save');
  let release;
  const hold = new Promise((resolve) => { release = resolve; });
  let requests = 0;
  await page.route(`**/api/notes/${note.id}`, async (route) => {
    if (route.request().method() === 'PATCH' && ++requests === 1) await hold;
    await route.continue();
  });
  await page.locator('.line-edit').fill('First edit');
  await expect(page.locator('#save-state')).toHaveText('Saving…');
  await page.locator('.line-edit').fill('Latest edit');
  release();
  await expect(page.locator('#save-state')).toHaveText('Saved');
  const saved = await (await page.request.get(`/api/notes/${note.id}`)).json();
  expect(saved.body).toBe('Latest edit');
  expect(requests).toBe(2);
});

test('a phone edit preserves an external edit as a separate conflict copy', async ({ page }) => {
  const note = await openNote(page, 'Conflict example');
  const updated = await page.request.patch(`/api/notes/${note.id}`, { data: { body: 'Computer edit' } });
  expect(updated.status()).toBe(200);
  await page.locator('.line-edit').fill('Phone edit');
  await expect(page.locator('#save-state')).toHaveText('Saved');
  await expect(page.locator('#title')).toContainText('conflict from phone');
  const notes = await (await page.request.get('/api/notes')).json();
  expect(notes.find((n) => n.id === note.id).body).toBe('Computer edit');
  expect(notes.some((n) => n.title === 'Conflict example (conflict from phone)' && n.body === 'Phone edit')).toBe(true);
});

test('authenticated requests cannot create notes outside the notes folder', async ({ page }) => {
  for (const directory of ['../outside', '/tmp/outside', 'nested/../../outside', '..\\outside', 'C:\\outside', '.trash']) {
    const response = await page.request.post('/api/notes', { data: { title: 'Escape', directory } });
    expect(response.status()).toBe(400);
  }
  expect(fs.existsSync(path.join(process.env.LEO_BROWSER_HOME, 'outside'))).toBe(false);
});

test('search finds a note named after a command', async ({ page }) => {
  await page.request.post('/api/notes', { data: { title: 'backup checklist', body: 'Check my backups' } });
  await page.locator('#search-toggle').click();
  await page.locator('#search-input').fill('backup');
  await expect(page.locator('.card').filter({ hasText: 'backup checklist' })).toBeVisible();
});


test.describe('plain HTTP access', () => {
  test('a new note can be created without secure-context browser APIs', async ({ page }) => {
    const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
    await page.goto(`http://leo-http.test:31831/?token=${token}`);
    expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
    expect(await page.evaluate(() => typeof crypto.randomUUID)).toBe('undefined');
    await page.locator('.fab[data-action="new"]').click();
    await expect(page.locator('#title')).toBeVisible();
    await page.locator('#title').fill('Created over plain HTTP');
    await expect(page.locator('#save-state')).toHaveText('Saved');
    const saved = await page.evaluate(async () => (await fetch('/api/notes')).json());
    expect(saved.some((note) => note.title === 'Created over plain HTTP')).toBe(true);
  });
});
