const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

test('documents split notes and keep Felix at the right without losing drafts', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const html = fs.readFileSync(path.join(web, 'index.html'), 'utf8');
  const style = html.match(/<style>([\s\S]*?)<\/style>/)[1];
  const calendar = fs.readFileSync(path.join(web, 'app/calendar.js'), 'utf8');
  const uploads = fs.readFileSync(path.join(web, 'app/uploads.js'), 'utf8');
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const scrollbar = core.slice(core.indexOf('function pageBarRight()'), core.indexOf('function drawPageBar()'));
  const source = calendar.slice(calendar.indexOf('const source ='));
  const viewer = uploads.slice(uploads.indexOf('function viewKind'));
  const fixture = `const state = {view: 'note', session: {note: {id: 'sample'}}};
    const enc = encodeURIComponent;
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const ICON = {close: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M4 4l8 8M4 12l8-8" stroke="currentColor"/></svg>'};
    const md = {render: text => '<p>' + esc(text) + '</p>'};
    class Locked extends Error {}
    let pending = null;
    async function api() { if (pending) await pending; return [{name:'paper.pdf'}, {name:'notes.txt'}, {name:'board.png'}]; }
    function sheet(contents) { const el = document.createElement('div'); el.className = 'scrim'; el.innerHTML = '<section class="sheet viewer">' + contents + '</section>'; document.body.append(el); return el; }
    const $ = selector => document.querySelector(selector);
    const chat = leoChat.create({render: md.render, escape: esc, icons: ICON});
    ${source}
    ${viewer}
    ${scrollbar}
    document.addEventListener('click', event => {
      const el = event.target.closest('[data-action]'); if (!el) return;
      event.preventDefault();
      if (el.dataset.action === 'view-original') viewOriginal('sample', 'paper.pdf');
      if (el.dataset.action === 'source-close') closeSource();
      if (el.dataset.action === 'source-file') openSource('sample', el.dataset.name);
      if (el.dataset.action === 'close') el.closest('.scrim').remove();
    });
    window.audit = {scrollbarRight: pageBarRight, open: name => viewOriginal('sample', name), close: closeSource, source,
      chat(open) { chat.toggle(open); },
      wait() { pending = new Promise(resolve => window.release = () => { pending = null; resolve(); }); }};`;
  const server = http.createServer((request, response) => {
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript'); return response.end(fixture); }
    if (request.url === '/chat.js') { response.setHeader('Content-Type', 'text/javascript'); return response.end(fs.readFileSync(path.join(web, 'chat.js'))); }
    if (request.url.startsWith('/api/') && !request.url.includes('/originals/')) { response.setHeader('Content-Type', 'application/json'); return response.end('[]'); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css'); return response.end(style); }
    if (request.url.includes('/originals/notes.txt')) return response.end('Original text <must stay escaped>.');
    if (request.url.includes('/originals/')) return response.end('Document fixture');
    response.setHeader('Content-Type', 'text/html');
    response.end('<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="/style.css"></head><body><aside class="side">Notes</aside><header class="bar"><div class="bar-row">Lecture</div></header><main id="app"><h1>Lecture notes</h1><a href="/paper.pdf" data-action="view-original">paper.pdf</a><textarea id="note-draft">My note draft</textarea><p>' + 'Notes stay readable. '.repeat(300) + '</p></main><script src="/chat.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.setViewportSize({width: 1280, height: 900});
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(() => audit.chat(true));
    await page.locator('#chat-input').fill('Help me understand this');
    await page.locator('#chat-input').focus();
    await page.locator('#chat-input').evaluate(el => el.setSelectionRange(5, 9));
    await page.evaluate(() => audit.open('paper.pdf'));
    await expect(page.locator('#source-pane')).toBeVisible();
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('.source-frame')).toHaveAttribute('src', /paper\.pdf\?view=1$/);
    await expect(page.locator('#chat-input')).toBeFocused();
    expect(await page.locator('#chat-input').evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([5, 9]);
    for (const theme of ['light', 'dark']) {
      for (const width of [1000, 1280, 1440, 1920]) {
        await page.setViewportSize({width, height: 900});
        await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
        await expect.poll(async () => (await page.locator('#chat-input').boundingBox()).width).toBeGreaterThan(150);
        const boxes = await page.evaluate(() => {
          const rect = selector => { const r = document.querySelector(selector).getBoundingClientRect(); return {left:r.left, right:r.right, width:r.width}; };
          return {note:rect('main'), source:rect('#source-pane'), chat:rect('.chat'), input:rect('#chat-input'), viewport:document.documentElement.clientWidth, scroll:document.documentElement.scrollWidth};
        });
        expect(boxes.note.right).toBeLessThanOrEqual(boxes.source.left + 2);
        expect(boxes.source.right).toBeLessThanOrEqual(boxes.chat.left + 2);
        expect(boxes.chat.right).toBeLessThanOrEqual(boxes.viewport + 1);
        expect(boxes.note.width).toBeGreaterThan(175);
        expect(boxes.source.width).toBeGreaterThanOrEqual(250);
        expect(boxes.chat.width).toBeGreaterThanOrEqual(290);
        expect(boxes.input.width).toBeGreaterThan(150);
        expect(boxes.scroll).toBeLessThanOrEqual(boxes.viewport + 1);
      }
    }
    await page.setViewportSize({width: 1280, height: 900});
    await expect(page.locator('.chat-history')).toBeHidden();
    await page.locator('.chat-history-toggle').click();
    await expect(page.locator('.chat-history')).toBeVisible();
    await expect(page.locator('.chat-history-back')).toBeVisible();
    await page.locator('.chat-history-back').click();
    await expect(page.locator('#chat-input')).toHaveValue('Help me understand this');
    await page.setViewportSize({width: 1920, height: 900});
    const grip = page.locator('.source-grip');
    const before = (await page.locator('#source-pane').boundingBox()).width;
    await grip.focus();
    await page.keyboard.press('ArrowRight');
    await expect.poll(async () => (await page.locator('#source-pane').boundingBox()).width).toBeLessThan(before - 20);
    await page.evaluate(() => audit.chat(false));
    expect(await page.evaluate(() => audit.scrollbarRight())).toBeGreaterThan(250);
    await page.evaluate(() => audit.chat(true));
    await expect(page.locator('.source-frame')).toHaveCount(1);
    await page.evaluate(() => audit.open('notes.txt'));
    await expect(page.locator('.source-text')).toHaveText('Original text <must stay escaped>.');
    await expect(page.locator('.source-text must')).toHaveCount(0);
    const divider = await page.locator('.source-grip').boundingBox();
    const startWidth = (await page.locator('#source-pane').boundingBox()).width;
    await page.mouse.move(divider.x + 4, divider.y + 200);
    await page.mouse.down();
    await page.mouse.move(divider.x + 36, divider.y + 200);
    await expect.poll(async () => (await page.locator('#source-pane').boundingBox()).width).toBeLessThan(startWidth - 20);
    await page.evaluate(() => audit.close());
    await expect(page.locator('body')).not.toHaveClass(/source-resizing/);
    await page.mouse.up();
    await page.evaluate(() => { audit.wait(); audit.open('paper.pdf'); audit.close(); release(); });
    await expect(page.locator('#source-pane')).toBeHidden();
    await expect(page.locator('#source-pane iframe')).toHaveCount(0);
    await page.locator('#app > a').click();
    await expect(page.locator('#source-pane')).toBeVisible();
    await page.locator('[data-action="source-close"]').click();
    await expect(page.locator('#source-pane')).toBeHidden();
    await expect(page.locator('#app > a')).toBeFocused();
    await page.setViewportSize({width: 390, height: 844});
    await page.evaluate(() => audit.open('notes.txt'));
    await expect(page.locator('.sheet.viewer')).toBeVisible();
    await expect(page.locator('.viewer-text')).toHaveText('Original text <must stay escaped>.');
    expect(await page.locator('.sheet.viewer').evaluate(el => document.elementFromPoint(el.getBoundingClientRect().left + 20, el.getBoundingClientRect().top + 20)?.closest('.viewer') !== null)).toBe(true);
    await page.locator('.viewer [data-action="close"]').click();
    await expect(page.locator('#chat-input')).toHaveValue('Help me understand this');
    await expect(page.locator('#note-draft')).toHaveValue('My note draft');
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});
