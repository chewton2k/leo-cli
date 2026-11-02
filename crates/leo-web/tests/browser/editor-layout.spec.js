const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

async function editorFixture(browser, run, { remember = false, body = null } = {}) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const settings = read('app/settings.js');
  const pickWidth = settings.slice(settings.indexOf('function pickWidth('), settings.indexOf('function pickFelix('));
  const passages = read('app/passages.js');
  const source = body ?? (remember ? Array.from({ length: 160 }, (_, i) => `Paragraph number ${i + 1} about reading.`).join('\n\n') : 'An ordinary paragraph.');
  const fixture = `window.doc = leoDoc.mount(document.querySelector('#doc'), {
    source: ${JSON.stringify(source)}, onChange() {}, outline: document.querySelector('.note-outline')
  });
  ${pickWidth}
  window.audit = { pick: width => pickWidth({dataset:{width}}), anchor: () => JSON.parse(localStorage.getItem('fixture-reading') || 'null') };
  ${remember ? `const $ = selector => document.querySelector(selector), enc = encodeURIComponent;
    const seq = 1, citedLines = null, state = {view:'note',session:{doc:window.doc}};
    async function api(url, options = {}) {
      if (options.method === 'PUT') localStorage.setItem('fixture-reading', JSON.stringify(options.body.anchor));
      return {anchor:audit.anchor()};
    }
    ${passages.slice(passages.indexOf('const reading ='))}
    restoreReading('fixture-note', seq);` : ''}`;
  const server = http.createServer((req, res) => {
    if (req.url === '/away') { res.setHeader('Content-Type', 'text/html'); return res.end('<p>Another page</p>'); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css'); return res.end(css); }
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript'); return res.end(fixture); }
    if (['/markdown.js', '/editing.js', '/doc.js', '/theme.js'].includes(req.url)) {
      res.setHeader('Content-Type', 'text/javascript'); return res.end(read(req.url.slice(1)));
    }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    const sidebar = remember ? '<aside class="side"><div class="side-head">Folders</div><div class="side-explorer"><nav class="side-tree">' + Array.from({length:100},(_,i)=>'<button class="side-row">Folder '+i+'</button>').join('') + '</nav></div></aside>' : '';
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><script src="/theme.js"></script><link rel="stylesheet" href="/style.css"></head><body class="on-note">'+sidebar+'<header id="bar" class="bar"><div class="bar-row">Leo</div></header><main><div class="note-layout"><div class="note-columns"><aside class="note-outline" hidden><details class="source-more"><summary>On this page</summary><nav aria-label="Headings"></nav></details></aside><article class="note"><h1>My note</h1><div id="doc" class="doc prose"></div></article></div></div></main><script src="/markdown.js"></script><script src="/editing.js"></script><script src="/doc.js"></script><script src="/fixture.js"></script></body></html>');
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

test('reading width choices cap the note column independently of its outline wrapper', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width:2000,height:900});
    await page.goto(url);
    expect((await page.locator('.note').boundingBox()).width).toBeCloseTo(980,-1);
    for (const [choice, width] of [['comfortable',760],['wide',980],['full',null]]) {
      await page.evaluate(choice=>audit.pick(choice),choice);
      await page.reload();
      expect(await page.evaluate(()=>leoTheme.noteWidth())).toBe(choice);
      const measured=(await page.locator('.note').boundingBox()).width;
      if (width) expect(measured).toBeCloseTo(width,-1);
      else expect(measured).toBeGreaterThan(1200);
    }
  });
});

test('scrolling the note saves and restores reading while the explorer scrolls independently', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width:1280,height:900});
    await page.goto(url);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    const tree=page.locator('.side-tree');
    await tree.hover();
    await page.mouse.wheel(0,600);
    await expect.poll(()=>tree.evaluate(el=>el.scrollTop)).toBeGreaterThan(0);
    expect(await page.evaluate(()=>scrollY)).toBe(0);
    expect(await page.evaluate(()=>audit.anchor())).toBeNull();
    await page.locator('#doc .blk').first().hover();
    await page.mouse.wheel(0,2400);
    await expect.poll(()=>page.evaluate(()=>scrollY)).toBeGreaterThan(1500);
    await expect.poll(()=>page.evaluate(()=>audit.anchor()),{timeout:5000}).not.toBeNull();
    await page.goto(url+'/away');
    await page.goto(url);
    await expect.poll(()=>page.evaluate(()=>scrollY)).toBeGreaterThan(1500);
  },{remember:true});
});

test('bottom checkboxes keep their position and focus when checked and unchecked', async ({ browser }) => {
  const body = '# A long note\n\n' + 'A paragraph above the checklist.\n\n'.repeat(140) + '- [ ] Pending task\n- [x] Completed task\n  - [ ] Nested task\n* [ ] Star task\n+ [ ] Plus task\n1. [ ] Numbered task\n- [ ]\n-   [ ] Wide spaces\n2) [ ] Parenthesis task';
  await editorFixture(browser, async (context,url)=>{
    const page=await context.newPage();
    await page.setViewportSize({width:1280,height:900});
    await page.goto(url);
    const boxes=page.locator('#doc input[data-box]');
    await expect(boxes).toHaveCount(9);
    for (let index=0; index<9; index++) {
      const box=boxes.nth(index);
      await box.scrollIntoViewIfNeeded();
      const before=await page.evaluate(()=>scrollY);
      expect(before).toBeGreaterThan(2000);
      const checked=await box.isChecked();
      await box.click();
      await expect(box).toBeChecked({checked:!checked});
      expect(Math.abs(await page.evaluate(()=>scrollY)-before)).toBeLessThanOrEqual(1);
      await expect(box).toBeFocused();
      await page.keyboard.press('Space');
      await expect(box).toBeChecked({checked});
      expect(Math.abs(await page.evaluate(()=>scrollY)-before)).toBeLessThanOrEqual(1);
      await expect(box).toBeFocused();
    }
    const box=boxes.nth(1), item=box.locator('..');
    const inputRect=await box.boundingBox(), labelRect=await item.boundingBox();
    await item.click({position:{x:inputRect.x+inputRect.width-labelRect.x+2,y:labelRect.height/2}});
    await expect(box).not.toBeChecked();
    await expect(box).toBeFocused();
    await box.click();
    await expect(box).toBeChecked();
    expect(await page.evaluate(()=>window.doc.source())).toBe(body);
  },{body});
});

test('toggling a bottom task after editing above it changes that task and keeps its position', async ({ browser }) => {
  const body = 'Earlier paragraph.\n\n'.repeat(120) + 'Editable line\n\n- [ ] Earlier task\n- [ ] Final task';
  await editorFixture(browser, async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    const editable=page.locator('#doc .blk').filter({hasText:'Editable line'});
    await editable.scrollIntoViewIfNeeded();
    await editable.click();
    await page.locator('#doc textarea').fill('Editable line\n- [ ] Newly typed task');
    const final=page.locator('#doc .task-item').filter({hasText:'Final task'}).locator('input');
    const before=(await final.boundingBox()).y;
    await final.click();
    await expect(final).toBeChecked();
    expect(Math.abs((await final.boundingBox()).y-before)).toBeLessThanOrEqual(1);
    await expect(final).toBeFocused();
    expect(await page.evaluate(()=>window.doc.source())).toContain('- [ ] Newly typed task');
    expect(await page.evaluate(()=>window.doc.source())).toContain('- [ ] Earlier task');
    expect(await page.evaluate(()=>window.doc.source())).toContain('- [x] Final task');
  },{body});
});

test('opening a heading, list or task line keeps its text and the lines below where they were drawn', async ({ browser }) => {
  const body = ['# Big heading', '- Bullet item', '- [ ] Task item', 'Plain line', 'Last line'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(url);
    const firstText = (i) => page.evaluate((i) => {
      const blk = document.querySelectorAll('#doc .blk')[i];
      const walker = document.createTreeWalker(blk, NodeFilter.SHOW_TEXT);
      let node;
      while ((node = walker.nextNode())) {
        const at = node.data.search(/\S/);
        if (at < 0) continue;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + 1);
        return range.getBoundingClientRect().left;
      }
      return null;
    }, i);
    for (const [i, marker] of [[0, '# '], [1, '- '], [2, '- [ ] ']]) {
      const drawn = await firstText(i);
      const below = await page.locator('#doc .blk').nth(4).boundingBox();
      await page.locator('#doc .blk').nth(i).locator('text=/item|heading/').click();
      const area = page.locator('#doc textarea');
      await expect(area).toBeFocused();
      const raw = await area.evaluate((el, marker) => {
        const style = getComputedStyle(el);
        const ctx = document.createElement('canvas').getContext('2d');
        ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
        return { left: el.getBoundingClientRect().left + ctx.measureText(marker).width, size: style.fontSize };
      }, marker);
      if (i === 0) expect(raw.size).toBe('22px');
      else expect(Math.abs(raw.left - drawn)).toBeLessThanOrEqual(3);
      const after = await page.locator('#doc .blk').nth(4).boundingBox();
      expect(Math.abs(after.y - below.y)).toBeLessThanOrEqual(2);
      await page.keyboard.press('Escape');
      await expect(page.locator('#doc textarea')).toHaveCount(0);
    }
  }, { body });
});

test('a selection dragged out of the edited line keeps going across rendered lines, and copies as Markdown', async ({ browser }) => {
  const body = ['First line here', 'Second **bold** line', '- Third item', 'Fourth line'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(0).click();
    const area = page.locator('#doc textarea');
    await expect(area).toBeFocused();
    const box = await area.boundingBox();
    const third = await page.locator('#doc .blk').nth(2).locator('li').boundingBox();
    const fourth = await page.locator('#doc .blk').nth(3).boundingBox();
    await page.mouse.move(box.x + 1, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 30, box.y + box.height / 2, { steps: 3 });
    await page.mouse.move(third.x + third.width - 2, third.y + third.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect(page.locator('#doc textarea')).toHaveCount(0);
    const picked = await page.evaluate(() => getSelection().toString());
    expect(picked).toContain('First line here');
    expect(picked).toContain('Third item');
    expect(picked).not.toContain('Fourth');
    expect(Math.abs((await page.locator('#doc .blk').nth(3).boundingBox()).y - fourth.y)).toBeLessThanOrEqual(30);
    const copied = await page.evaluate(() => {
      const data = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return data.getData('text/plain');
    });
    expect(copied).toBe('First line here\nSecond **bold** line\n- Third item');
    await page.keyboard.press('Backspace');
    await expect.poll(() => page.evaluate(() => doc.source())).toBe('\nFourth line');
    await expect(page.locator('#doc textarea')).toBeFocused();
  }, { body });
});

test('the outline sits at the right edge as heading ticks, keeping the note centred, and opens on hover', async ({ browser }) => {
  const body = ['# One', 'Text', '## Two', 'More', '# Three'].join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    const outline = page.locator('.note-outline');
    await expect(outline).toBeVisible();
    await expect(outline.locator('.outline-ticks span')).toHaveCount(3);
    const note = await page.locator('.note').boundingBox();
    const main = await page.locator('.note-columns').boundingBox();
    expect(Math.abs((note.x - main.x) - (main.x + main.width - note.x - note.width))).toBeLessThanOrEqual(2);
    const ticks = await outline.locator('.outline-ticks').boundingBox();
    expect(ticks.x).toBeGreaterThanOrEqual(note.x + note.width - 1);
    const nav = page.getByRole('navigation', { name: 'Headings' });
    expect(await nav.evaluate((el) => getComputedStyle(el).opacity)).toBe('0');
    await outline.locator('.outline-ticks').hover();
    await expect.poll(() => nav.evaluate((el) => getComputedStyle(el).opacity)).toBe('1');
    await nav.getByRole('button', { name: 'Three', exact: true }).click();
    await expect(page.locator('#doc .blk:focus')).toContainText('Three');
    await outline.locator('.outline-ticks span').first().click();
    await expect(page.locator('#doc .blk:focus')).toContainText('One');
  }, { body });
});
