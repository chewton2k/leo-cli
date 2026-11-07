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
  const dashes = await stroke.evaluateAll(lines => lines.map(el => {
    const length = el.getTotalLength();
    const units = el.hasAttribute('pathLength') ? el.pathLength.baseVal.value : length;
    return parseFloat(getComputedStyle(el).strokeDasharray) * length / units;
  }));
  expect(dashes.length).toBeGreaterThan(0);
  for (const dash of dashes) expect(dash).toBeLessThanOrEqual(2);
  await mark.click();
  await expect(page.locator('.margin-note .margin-felix')).toContainText(thread.messages.at(-1).text);
  await expect(page.locator('.margin-reply textarea')).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const animations = await stroke.evaluateAll(lines => lines.map(el => getComputedStyle(el).animationName));
  for (const animation of animations) expect(animation).toBe('none');
});

test('a reading mode picked on a new note is saved with the note', async ({ page }) => {
  await page.goto('/#/new');
  const button = page.locator('#margin-mode');
  await expect(button).toBeEnabled();
  await button.click();
  await page.waitForTimeout(300);
  await page.getByRole('menuitemradio', { name: 'Reads along' }).click();
  await expect(button).toHaveText('Reads along');
  await page.locator('#title').click();
  await page.keyboard.type('Picked early');
  await expect(page).toHaveURL(/#\/n\//);
  const id = page.url().match(/#\/n\/([^/?]+)/)[1];
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${id}/margin`)).json()).mode).toBe('along');
  await expect(button).toHaveText('Reads along');
});

test('a drag across page chrome paints nothing, fields stay selectable once focused, and the mode menu clears a selection', async ({ page }) => {
  await page.goto('/#/record');
  await expect(page.locator('#rec-title')).toBeVisible();
  await page.mouse.move(60, 20);
  await page.mouse.down();
  await page.mouse.move(640, 400, { steps: 6 });
  await page.mouse.move(1270, 700, { steps: 6 });
  await page.mouse.up();
  expect(await page.evaluate(() => getSelection().toString())).toBe('');
  const field = page.locator('#rec-title');
  await field.click();
  await page.keyboard.type('hello world');
  await page.mouse.click(5, 5);
  const box = await field.boundingBox();
  await page.mouse.move(box.x + 10, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 200, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  expect(await field.evaluate(i => i.value.slice(i.selectionStart, i.selectionEnd))).toContain('world');
  const note = await (await page.request.post('/api/notes', { data: { title: 'Selection chrome', body: 'Alpha beta gamma.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  const word = page.locator('#doc .blk').first();
  const at = await word.boundingBox();
  await page.mouse.move(at.x + 1, at.y + at.height / 2);
  await page.mouse.down();
  await page.mouse.move(at.x + 40, at.y + at.height / 2, { steps: 4 });
  await page.mouse.up();
  expect(await page.evaluate(() => getSelection().toString())).not.toBe('');
  await expect(page.locator('#margin-mode')).toBeEnabled();
  await page.locator('#margin-mode').click();
  expect(await page.evaluate(() => getSelection().toString())).toBe('');
});

test('the Tasks window leaves every outline tick reachable and the preview sits above it', async ({ page }) => {
  await page.setViewportSize({ width: 1300, height: 640 });
  const today = new Date().toLocaleDateString('en-CA');
  for (const title of ['Submit lab report', 'Email prof', 'Pay rent', 'Buy lab coat', 'Book flights']) {
    await page.request.post('/api/tasks', { data: { title, date: today } });
  }
  const body = Array.from({ length: 12 }, (_, i) => `## Heading number ${i + 1}\n\nText under heading ${i + 1}.\n`).join('\n');
  const note = await (await page.request.post('/api/notes', { data: { title: 'Outline over tasks', body } })).json();
  await page.goto(`/#/n/${note.id}`);
  await page.reload();
  await expect(page.locator('#tasks-window')).toBeVisible();
  await expect(page.locator('.outline-ticks span')).toHaveCount(12);
  await page.waitForTimeout(400);
  const hits = await page.evaluate(() => [...document.querySelectorAll('.outline-ticks span')].map((tick) => {
    const r = tick.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + 4, r.top + r.height / 2);
    return hit && !!hit.closest('.dock');
  }));
  expect(hits).not.toContain(true);
  const last = await page.locator('.outline-ticks span').last().boundingBox();
  await page.mouse.move(last.x + 3, last.y + last.height / 2);
  await expect(page.locator('.outline-preview')).toBeVisible();
  const z = await page.evaluate(() => ({ outline: +getComputedStyle(document.querySelector('.note-outline')).zIndex, dock: +getComputedStyle(document.querySelector('.dock')).zIndex }));
  expect(z.outline).toBeGreaterThan(z.dock);
});

test('lit passages join across the blank line between them without a seam', async ({ page }) => {
  const note = await (await page.request.post('/api/notes', { data: { title: 'Glow seams', body: 'First paragraph.\n\nSecond paragraph.\n\nThird paragraph.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('#doc .blk')).toHaveCount(5);
  await page.evaluate(() => [0, 2, 4].forEach(i => document.querySelectorAll('#doc .blk')[i].classList.add('blk-glow')));
  await page.waitForTimeout(800);
  const paint = await page.evaluate(() => [...document.querySelectorAll('#doc .blk')].map(el => {
    const style = getComputedStyle(el);
    return { fill: style.backgroundColor !== 'rgba(0, 0, 0, 0)', top: style.borderTopLeftRadius, bottom: style.borderBottomLeftRadius };
  }));
  expect(paint.map(p => p.fill)).toEqual([true, true, true, true, true]);
  expect(paint.map(p => p.top + p.bottom)).toEqual(Array(5).fill('0px0px'));
});

test('the New task title placeholder fits its field', async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 760 });
  await page.goto('/#/calendar');
  await page.locator('[data-action="task-new"]').first().click();
  const title = page.locator('#task-title');
  await expect(title).toBeVisible();
  const fit = await title.evaluate(el => {
    const style = getComputedStyle(el);
    const ctx = document.createElement('canvas').getContext('2d');
    ctx.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    return { text: ctx.measureText(el.placeholder).width, room: el.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) };
  });
  expect(fit.text).toBeLessThanOrEqual(fit.room);
});

test('a narrow Felix panel keeps the placeholder on one line, the note chip readable and the send button inside', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Word study notes', body: 'Hello.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('.chat-chip:not([hidden])')).toBeVisible();
  await page.evaluate(() => document.documentElement.style.setProperty('--chat-w', '330px'));
  await expect.poll(() => page.locator('#chat').evaluate(el => el.getBoundingClientRect().width)).toBeLessThan(400);
  const fit = await page.evaluate(() => {
    const input = document.querySelector('#chat-input');
    const form = document.querySelector('#chat-form').getBoundingClientRect();
    const chip = document.querySelector('.chat-chip:not([hidden])').getBoundingClientRect();
    const send = document.querySelector('#chat-send').getBoundingClientRect();
    return { lines: input.scrollHeight <= input.clientHeight, chip: chip.width, inside: send.right <= form.right };
  });
  expect(fit.lines).toBe(true);
  expect(fit.chip).toBeGreaterThanOrEqual(76);
  expect(fit.inside).toBe(true);
});

test('the note toolbar moves clear of an overlaying Felix panel instead of hiding behind it', async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 700 });
  const note = await (await page.request.post('/api/notes', { data: { title: 'Overlay toolbar', body: 'Hello.' } })).json();
  await page.goto(`/#/n/${note.id}`);
  await expect(page.locator('nav.actions')).toBeVisible();
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat')).toBeVisible();
  await page.waitForTimeout(500);
  const gap = await page.evaluate(() => ({ pill: document.querySelector('nav.actions').getBoundingClientRect(), chat: document.querySelector('#chat').getBoundingClientRect() }));
  expect(gap.pill.left).toBeGreaterThanOrEqual(0);
  expect(gap.pill.right).toBeLessThanOrEqual(gap.chat.left + 1);
});
