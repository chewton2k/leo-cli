const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

let seeded = null;

async function signIn(page) {
  const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
  await page.goto(`/?token=${token}`);
  await expect(page.locator('#app')).not.toBeEmpty();
}

async function seed(page) {
  const existing = await (await page.request.get('/api/notes?dir=cs130')).json();
  const found = existing.find((n) => n.title === 'Graph traversals');
  if (found) return { graphs: found };
  for (const dir of ['cs130', 'cs162', 'math61']) await page.request.post('/api/dirs', { data: { path: dir } });
  const data = await page.evaluate(() => {
    const canvas = document.createElement('canvas');
    canvas.width = 480;
    canvas.height = 240;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#eef2ff';
    ctx.fillRect(0, 0, 480, 240);
    ctx.fillStyle = '#4f46e5';
    for (const [x, y] of [[240, 50], [140, 120], [340, 120], [90, 190], [190, 190], [290, 190], [390, 190]]) ctx.fillRect(x - 22, y - 22, 44, 44);
    return canvas.toDataURL('image/png').split(',')[1];
  });
  const picture = await (await page.request.post('/api/images', { data: { name: 'heap.png', data } })).json();
  const graphs = await (await page.request.post('/api/notes', {
    data: {
      title: 'Graph traversals',
      directory: 'cs130',
      body: `Breadth-first search visits vertices level by level using a **queue**.\n\n## Steps\n- Start at the source\n- [x] Read chapter 3\n- [ ] Practice problems\n\n![A small heap](${picture.path})\n\n> Depth-first search uses a stack instead.\n\n| Search | Structure |\n|---|---|\n| BFS | queue |\n| DFS | stack |`,
    },
  })).json();
  await page.request.post('/api/notes', { data: { title: 'Scheduling', directory: 'cs162', body: 'Round robin takes the next process from a ready queue.' } });
  await page.request.post('/api/notes', { data: { title: 'Induction', directory: 'math61', body: 'Prove the base case, then the step.' } });
  const gone = await (await page.request.post('/api/notes', { data: { title: 'Old draft', body: 'Not needed any more.' } })).json();
  await page.request.delete(`/api/notes/${gone.id}`);
  return { graphs };
}

const settle = (page) => page.evaluate(() => document.fonts.ready);

test.beforeEach(async ({ page }) => {
  await signIn(page);
  seeded = await seed(page);
});

for (const [name, hash, mask] of [
  ['folders', '#/', []],
  ['folder', '#/f/cs130', []],
  ['search', '#/search/queue', []],
  ['trash', '#/trash', []],
  ['settings', '#/settings', []],
  ['storage', '#/settings/storage', ['.store-big', '.store-size', '.store-key-size', '.store-bar', '.store-count']],
  ['record', '#/record', []],
]) {
  test(`${name} looks the same`, async ({ page }) => {
    await page.request.post('/api/sessions/end', { data: { others: true } });
    await page.goto(`/${hash}`);
    await page.waitForLoadState('networkidle');
    await settle(page);
    await expect(page).toHaveScreenshot(`${name}.png`, { fullPage: true, mask: mask.map((m) => page.locator(m)) });
  });
}

test('a note with a picture looks the same', async ({ page }) => {
  await page.goto(`/#/n/${seeded.graphs.id}`);
  await expect(page.locator('#doc img.note-img')).toBeVisible();
  await expect.poll(() => page.locator('#doc img.note-img').evaluate((el) => el.complete && el.naturalWidth)).toBeGreaterThan(0);
  await settle(page);
  await expect(page).toHaveScreenshot('note.png', { fullPage: true });
});

test('the upload sheet and the menu look the same', async ({ page }) => {
  await page.goto('/');
  await page.locator('.fab[data-action="upload"]:visible, #side [data-action="upload"]:visible').first().click();
  await expect(page.locator('.sheet')).toBeVisible();
  await settle(page);
  await expect(page).toHaveScreenshot('upload.png');
  await page.keyboard.press('Escape');
  if (!(await page.locator('#menu').isVisible())) return;
  await page.locator('#menu').click();
  await expect(page.locator('.sheet')).toBeVisible();
  await expect(page).toHaveScreenshot('menu.png');
});

test('Felix looks the same', async ({ page }) => {
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat')).toBeVisible();
  await settle(page);
  await expect(page).toHaveScreenshot('felix.png', { mask: [page.locator('.felix')] });
});
