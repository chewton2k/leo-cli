const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

async function editorFixture(browser, run) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const fixture = `window.doc = leoDoc.mount(document.querySelector('#doc'), {
    source: 'An ordinary paragraph.', onChange() {}, outline: document.querySelector('.note-outline')
  });`;
  const server = http.createServer((req, res) => {
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css'); return res.end(css); }
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript'); return res.end(fixture); }
    if (['/markdown.js', '/editing.js', '/doc.js'].includes(req.url)) {
      res.setHeader('Content-Type', 'text/javascript'); return res.end(read(req.url.slice(1)));
    }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body class="on-note"><header id="bar" class="bar"><div class="bar-row">Leo</div></header><main><div class="note-layout"><div class="note-columns"><aside class="note-outline" hidden><details class="source-more"><summary>On this page</summary><nav aria-label="Headings"></nav></details></aside><article class="note"><h1>My note</h1><div id="doc" class="doc prose"></div></article></div></div></main><script src="/markdown.js"></script><script src="/editing.js"></script><script src="/doc.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext();
  try { await run(context, 'http://127.0.0.1:' + server.address().port); }
  finally { await context.close(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}

for (const theme of ['light', 'dark']) {
  test('typing and removing every heading keeps the note, text and caret in place in ' + theme, async ({ browser }) => {
    await editorFixture(browser, async (context, url) => {
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      for (const width of [1600, 1200, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(url);
        await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
        await page.locator('#doc .blk').first().click();
        const editor = page.locator('#doc textarea');
        const before = await editor.boundingBox();
        for (let level = 1; level <= 6; level++) {
          const text = '#'.repeat(level) + ' Heading 文字 ' + level;
          if (level === 1) {
            await editor.fill('');
            await page.keyboard.type('#');
            await expect(editor).toHaveValue('#');
            await page.keyboard.insertText(text.slice(1));
          } else await editor.fill(text);
          await expect(page.locator('.note-outline button[data-line]')).toHaveText('Heading 文字 ' + level);
          const after = await editor.boundingBox();
          expect(Math.abs(after.x - before.x)).toBeLessThanOrEqual(1);
          expect(Math.abs(after.y - before.y)).toBeLessThanOrEqual(1);
          expect(Math.abs(after.width - before.width)).toBeLessThanOrEqual(1);
          await expect(editor).toBeFocused();
          expect(await editor.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([text.length, text.length]);
          await page.keyboard.type(' next');
          await expect(editor).toHaveValue(text + ' next');
        }
        await editor.fill('An ordinary paragraph.');
        await expect(page.locator('.note-outline')).toBeHidden();
        expect(await editor.boundingBox()).toEqual(before);
        await page.keyboard.press('Escape');
        await expect(page.locator('#doc .blk')).toHaveText('An ordinary paragraph.');
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      }
      expect(errors).toEqual([]);
      await page.close();
    });
  });
}

test('an active heading refits after pane resizing without clipping text or losing selection', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').first().click();
    const editor = page.locator('#doc textarea');
    const text = '### ' + 'Long heading 文字 😀 '.repeat(35);
    await editor.fill(text);
    await expect(page.locator('.note-outline')).toBeVisible();
    await editor.evaluate(el => el.setSelectionRange(8, 12));
    for (const width of [1200, 900, 390, 1600]) {
      await page.setViewportSize({ width, height: 900 });
      await expect.poll(() => editor.evaluate(el => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(1);
      await expect(editor).toBeFocused();
      await expect(editor).toHaveValue(text);
      expect(await editor.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([8, 12]);
      expect(await editor.evaluate(el => getComputedStyle(el).color === getComputedStyle(document.body).color)).toBe(true);
    }
    await page.keyboard.insertText('replacement');
    await expect(editor).toHaveValue(text.slice(0, 8) + 'replacement' + text.slice(12));
  });
});
