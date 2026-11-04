const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

async function editorFixture(browser, run, { remember = false, body = null, touch = false } = {}) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const settings = read('app/settings.js');
  const pickWidth = settings.slice(settings.indexOf('function pickWidth('), settings.indexOf('function pickFelix('));
  const passages = read('app/passages.js');
  const source = body ?? (remember ? Array.from({ length: 160 }, (_, i) => `Paragraph number ${i + 1} about reading.`).join('\n\n') : 'An ordinary paragraph.');
  const fixture = `window.doc = leoDoc.mount(document.querySelector('#doc'), {
    source: new URLSearchParams(location.search).get('body') ?? ${JSON.stringify(source)}, onChange() {}, outline: document.querySelector('.note-outline')
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
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><script src="/theme.js"></script><link rel="stylesheet" href="/style.css"></head><body class="on-note">'+sidebar+'<header id="bar" class="bar"><div class="bar-row">Leo</div></header><main><div class="note-layout"><div class="note-columns"><aside class="note-outline" hidden><details class="source-more"><summary aria-label="Headings in this note"></summary><nav aria-label="Headings"></nav></details></aside><article class="note"><h1>My note</h1><div id="doc" class="doc prose"></div></article></div></div></main><script src="/markdown.js"></script><script src="/editing.js"></script><script src="/doc.js"></script><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ hasTouch: touch });
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

test('switching editable blocks keeps unrelated rendered content and the viewport stable', async ({ browser }) => {
  const samples = ['Ordinary text', '## Heading', '- A list item', '- [ ] A task', '```js\nconst answer = 42;\n```', '| Name | Value |\n| --- | --- |\n| One | Two |', '> A quote\n> Another line', '> [!note] A callout\n> Kept open', '$$\nx = 2\n$$'];
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1400, 390]) {
      await page.setViewportSize({ width, height: 900 });
      for (const sample of samples) {
        await page.goto(url + '/?body=' + encodeURIComponent('First paragraph\n\n' + sample + '\n\nFollowing paragraph'));
        const target = page.locator('#doc .blk').nth(2);
        await target.scrollIntoViewIfNeeded();
        await page.evaluate(() => { window.keptBlock = Array.from(document.querySelectorAll('#doc .blk')).at(-1); });
        const before = await page.locator('#doc .blk').last().boundingBox();
        await target.click({ position: { x: 30, y: 10 } });
        await expect(page.locator('#doc textarea')).toBeFocused();
        expect(Math.abs((await page.locator('#doc .blk').last().boundingBox()).y - before.y), sample).toBeLessThanOrEqual(1);
        await page.locator('#doc .blk').first().click();
        expect(await page.evaluate(() => window.keptBlock.isConnected), sample).toBe(true);
        expect(await page.evaluate(() => doc.source())).toBe('First paragraph\n\n' + sample + '\n\nFollowing paragraph');
      }
    }
  });
});

test('focusing and dragging from a formatted line preserves document geometry in both directions', async ({ browser }) => {
  const samples = ['## A heading', '- A list item', '  - A nested item', '- [ ] A task', '**Bold** text ' + 'wrapped words '.repeat(35), '```js\nconst answer = 42;\n```', '| Name | Value |\n| --- | --- |\n| One | Two |', '> A quote\n> Another line'];
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1400, 390]) {
      await page.setViewportSize({ width, height: 900 });
      for (const sample of samples) {
        for (const direction of ['up', 'down']) {
          const body = 'Before this line\n' + sample + '\nAfter this line\nLast line';
          await page.goto(url + '/?body=' + encodeURIComponent(body));
          const block = page.locator('#doc .blk').nth(1);
          const geometry = () => page.locator('#doc .blk').evaluateAll(els => els.map(el => ({ y: el.getBoundingClientRect().top + scrollY, height: el.getBoundingClientRect().height })));
          const before = await geometry();
          await block.click({ position: { x: 35, y: 10 } });
          const area = page.locator('#doc textarea');
          await expect(area).toBeFocused();
          const focused = await geometry();
          for (let i = 0; i < before.length; i++) {
            expect(Math.abs(focused[i].y - before[i].y), `${width} ${direction} ${sample}: focus block ${i}`).toBeLessThanOrEqual(1);
            expect(Math.abs(focused[i].height - before[i].height), `${width} ${sample}: focus height ${i}`).toBeLessThanOrEqual(1);
          }
          const box = await area.boundingBox();
          const target = page.locator('#doc .blk').nth(direction === 'up' ? 0 : 2);
          const end = await target.boundingBox();
          await page.mouse.move(box.x + 35, box.y + Math.min(10, box.height / 2));
          await page.mouse.down();
          await page.mouse.move(end.x + 35, end.y + end.height / 2, { steps: 8 });
          await page.mouse.up();
          const after = await geometry();
          for (let i = 0; i < before.length; i++) expect(Math.abs(after[i].y - before[i].y), `${width} ${direction} ${sample}: selection block ${i}`).toBeLessThanOrEqual(1);
          expect(await page.evaluate(() => getSelection().toString())).toContain(direction === 'up' ? 'this line' : 'After');
          expect(await page.evaluate(() => doc.source())).toBe(body);
        }
      }
    }
  });
});

test('focusing a partially visible Markdown line does not scroll the page', async ({ browser }) => {
  const sample = '**Bold words** and ordinary words. '.repeat(25);
  const body = 'Before\n\n'.repeat(30) + sample + '\n\n' + 'After\n\n'.repeat(30);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    await page.goto(url);
    const block = page.locator('#doc .blk').filter({ hasText: 'Bold words' });
    await block.evaluate(el => scrollTo(0, scrollY + el.getBoundingClientRect().top + 80));
    const box = await block.boundingBox();
    const before = await page.evaluate(() => scrollY);
    await page.mouse.click(box.x + 70, 100);
    await expect(page.locator('#doc textarea')).toBeFocused();
    expect(await page.evaluate(() => scrollY)).toBe(before);
  }, { body });
});

test('dragging after editing wrapped Markdown keeps the target under the pointer', async ({ browser }) => {
  const words = '**word 😀** '.repeat(25);
  const samples = [words, '## ' + words, '- ' + words, '- [ ] ' + words, '  - [x] ' + words, '```txt\n' + words + '\n```', '> ' + words, '| Name | Value |\n| --- | --- |\n| One | Two |', '', '- [ ] '];
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    for (const sample of samples) for (const direction of ['up', 'down']) {
      await page.goto(url + '/?body=' + encodeURIComponent('Before this line\n' + sample + '\nAfter this line\nLast line'));
      await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
      const area = page.locator('#doc textarea');
      const value = await area.inputValue();
      const base = 'Before this line\n'.length + sample.length - value.length;
      await area.fill(sample === '' || sample === '- [ ] ' ? '' : value + (sample === words ? 'x\nPasted continuation' : 'x'));
      const source = await page.evaluate(() => doc.source());
      const box = await area.boundingBox();
      const target = page.locator('#doc .blk').nth(direction === 'up' ? 0 : 2);
      const before = await target.boundingBox();
      const end = { x: before.x + 35, y: before.y + before.height / 2 };
      const offset = await target.evaluate((el, point) => {
        const pos = document.caretPositionFromPoint?.(point.x, point.y);
        const hit = pos ? null : document.caretRangeFromPoint(point.x, point.y);
        const range = document.createRange();
        range.setStart(el, 0);
        range.setEnd(pos ? pos.offsetNode : hit.startContainer, pos ? pos.offset : hit.startOffset);
        return range.toString().length;
      }, end);
      await page.mouse.move(Math.max(2, box.x + 35), box.y + 10);
      await page.mouse.down();
      const anchor = base + await area.evaluate(el => el.selectionStart);
      await page.mouse.move(end.x, end.y, { steps: 8 });
      await page.mouse.up();
      expect(Math.abs((await target.boundingBox()).y - before.y), direction + sample).toBeLessThanOrEqual(1);
      expect(await page.evaluate(() => getSelection().focusOffset)).toBe(offset);
      const to = (direction === 'up' ? 0 : source.indexOf('After this line')) + offset;
      const expected = source.slice(Math.min(anchor, to), Math.max(anchor, to));
      const copied = await page.evaluate(() => {
        const data = new DataTransfer();
        document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
        return data.getData('text/plain');
      });
      expect(copied, direction + sample).toBe(expected);
      await page.keyboard.press('Backspace');
      expect(await page.evaluate(() => doc.source())).toBe(source.slice(0, Math.min(anchor, to)) + source.slice(Math.max(anchor, to)));
    }
  });
});

test('a dragged source line can be refocused without replacing its editor and returns to preview when cleared', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    for (const sample of ['**word** '.repeat(25), '- [ ] ' + '**word** '.repeat(25), '```txt\nword\n```']) {
      await page.goto(url + '/?body=' + encodeURIComponent('Before\n' + sample + '\nAfter this line\nLast'));
      await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
      const area = page.locator('#doc textarea');
      await area.fill((await area.inputValue()) + 'x');
      await area.evaluate(el => { window.keptEditor = el; });
      const from = await area.boundingBox(), to = await page.locator('#doc .blk').nth(2).boundingBox();
      await page.mouse.move(from.x + 35, from.y + 10);
      await page.mouse.down();
      await page.mouse.move(to.x + 35, to.y + to.height / 2, { steps: 8 });
      await page.mouse.up();
      await expect(area).toHaveCount(0);
      const selected = page.locator('#doc .blk.editing .line-edit');
      const before = await selected.boundingBox();
      await page.mouse.move(before.x + 35, before.y + 10);
      await page.mouse.down();
      await page.mouse.up();
      await expect(area).toBeFocused();
      expect(await area.evaluate(el => el === window.keptEditor)).toBe(true);
      expect(await area.boundingBox()).toEqual(before);
      const value = await area.inputValue();
      const caret = await area.evaluate(el => el.selectionStart);
      await page.keyboard.type('Z');
      await expect(area).toHaveValue(value.slice(0, caret) + 'Z' + value.slice(caret));
      await page.locator('article.note > h1').click();
      await expect(page.locator('#doc .line-edit')).toHaveCount(0);
      expect(await page.evaluate(() => doc.source())).toContain('Z');
    }
  });
});

test('inserting a picture after a source-selection drag preserves the note and uses a live editor state', async ({ browser }) => {
  const body = 'Before\n' + '**word** '.repeat(25) + '\nAfter this line\nLast';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
    const area = page.locator('#doc textarea');
    await area.fill((await area.inputValue()) + 'x');
    const source = await page.evaluate(() => doc.source());
    const from = await area.boundingBox(), to = await page.locator('#doc .blk').nth(2).boundingBox();
    await page.mouse.move(from.x + 35, from.y + 10);
    await page.mouse.down();
    await page.mouse.move(to.x + 35, to.y + to.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect(area).toHaveCount(0);
    await page.evaluate(() => doc.insert('![picture](attachment.png)'));
    expect(await page.evaluate(() => doc.source())).toBe(source + '\n\n![picture](attachment.png)');
  }, { body });
});

test('returning a drag to a list marker selects only the edited item text', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const item of ['- [ ] Task words', '- Item words', '12. Item words', '  - Nested words']) {
      const body = 'Before this line\n' + item + '\nAfter this line\nLast line';
      await page.goto(url + '/?body=' + encodeURIComponent(body));
      await page.locator('#doc .blk').nth(1).locator('li').click({ position: { x: 60, y: 10 } });
      const area = page.locator('#doc textarea');
      const value = await area.inputValue();
      const box = await area.boundingBox(), below = await page.locator('#doc .blk').nth(2).boundingBox();
      await page.mouse.move(box.x + 20, box.y + 10);
      await page.mouse.down();
      const anchor = await area.evaluate(el => el.selectionStart);
      await page.mouse.move(below.x + 20, below.y + 10, { steps: 5 });
      await page.mouse.move(box.x - 8, box.y + 10, { steps: 5 });
      await page.mouse.up();
      expect(await page.evaluate(() => doc.selection().text), item).toBe(value.slice(0, anchor));
      const copied = await page.evaluate(() => {
        const data = new DataTransfer();
        document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
        return data.getData('text/plain');
      });
      expect(copied, item).toBe(value.slice(0, anchor));
      await page.keyboard.press('Backspace');
      expect(await page.evaluate(() => doc.source()), item).toBe('Before this line\n' + item.slice(0, item.length - value.length) + value.slice(anchor) + '\nAfter this line\nLast line');
    }
  });
});

test('clicks and shift selections after emoji use the same raw caret offsets', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const sample of ['😀 target words', '**😀** target words', '- [ ] 😀 target words', '## 😀 target words']) {
      const body = sample + '\nAfter this line';
      await page.goto(url + '/?body=' + encodeURIComponent(body));
      const point = await page.locator('#doc .blk').first().evaluate(el => {
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
        let node;
        while ((node = walker.nextNode())) {
          const at = node.data.indexOf('target');
          if (at < 0) continue;
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + 1);
          const rect = range.getBoundingClientRect();
          return { x: rect.left + .2, y: rect.top + rect.height / 2 };
        }
      });
      await page.mouse.click(point.x, point.y);
      const area = page.locator('#doc textarea');
      await expect(area).toBeFocused();
      expect(await area.evaluate(el => el.selectionStart), sample).toBe((await area.inputValue()).indexOf('target'));
      await page.keyboard.down('Shift');
      await page.locator('#doc .blk').nth(1).click({ position: { x: 35, y: 10 } });
      await page.keyboard.up('Shift');
      const copied = await page.evaluate(() => {
        const data = new DataTransfer();
        document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
        return data.getData('text/plain');
      });
      expect(copied, sample).toBe('target words\nAfter');
    }
  });
});

test('heading navigation scrolls smoothly and respects reduced motion', async ({ browser }) => {
  const body = '## First\n\n' + 'A paragraph.\n\n'.repeat(80) + '## Last\n\n' + 'More text.\n\n'.repeat(20);
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const reducedMotion of ['no-preference', 'reduce']) {
      await page.emulateMedia({ reducedMotion });
      await page.goto(url);
      const behavior = await page.evaluate(() => {
        const scroll = window.scrollTo.bind(window);
        let behavior;
        window.scrollTo = options => { behavior = options.behavior; scroll(options); };
        document.querySelector('.note-outline button[data-line="162"]').click();
        return behavior;
      });
      expect(behavior).toBe(reducedMotion === 'reduce' ? 'instant' : 'smooth');
      await expect.poll(() => page.locator('#doc .blk:focus').textContent()).toBe('Last');
      await expect.poll(() => page.locator('#doc .blk:focus').evaluate(el => el.getBoundingClientRect().top - document.querySelector('#bar').getBoundingClientRect().bottom)).toBeLessThanOrEqual(80);
      expect(await page.locator('#doc .blk:focus').evaluate(el => el.getBoundingClientRect().top >= document.querySelector('#bar').getBoundingClientRect().bottom)).toBe(true);
    }
  }, { body });
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
        return { left: el.getBoundingClientRect().left + ctx.measureText(el.closest('li') ? '' : marker).width, size: style.fontSize };
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

test('the outline sits at the right edge as heading ticks, keeping the note centred, and opens its full outline on demand', async ({ browser }) => {
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
    const nav = page.locator('.note-outline nav');
    expect(await nav.evaluate((el) => getComputedStyle(el).opacity)).toBe('0');
    await outline.locator('summary').click();
    await expect.poll(() => nav.evaluate((el) => getComputedStyle(el).opacity)).toBe('1');
    await nav.getByRole('button', { name: 'Three', exact: true }).click();
    await expect(page.locator('#doc .blk:focus')).toContainText('Three');
    await outline.locator('.outline-ticks span').first().click();
    await expect(page.locator('#doc .blk:focus')).toContainText('One');
  }, { body });
});

test('list editing keeps rendered markers, indentation and wrapping while preserving Markdown operations', async ({ browser }) => {
  const items = ['- # Literal heading marker', '- Bullet 文字', '* Star item', '+ Plus item', '12. Number item', '3) Parenthesis item', '- [ ] Pending item', '* [x] Done item', '2. [ ] Ordered task', '  - Nested item', '    - [ ] Nested task', '- [ ]', '-   [ ] Wide spaces', '- ' + 'Wrapped text '.repeat(30)];
  const body = items.concat('Following paragraph').join('\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const width of [1400, 390]) {
      await page.setViewportSize({width, height:900});
      await page.goto(url);
      for (let i = 0; i < items.length; i++) {
        const block = page.locator('#doc .blk').nth(i);
        await block.scrollIntoViewIfNeeded();
        const before = await block.boundingBox();
        const below = await page.locator('#doc .blk').last().boundingBox();
        const boxCount = await block.locator('input[data-box]').count();
        const textLeft = await block.evaluate(el => {
          const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
          const node = walker.nextNode();
          if (!node) return null;
          const range = document.createRange();
          range.selectNodeContents(node);
          return range.getBoundingClientRect().left;
        });
        await block.locator('li').click({position:{x:Math.min(80, before.width / 2),y:10}});
        const area = block.locator('textarea');
        await expect(area).toBeFocused();
        if (textLeft !== null) expect(Math.abs((await area.boundingBox()).x - textLeft)).toBeLessThanOrEqual(1);
        await expect(block.locator('li')).toHaveCount(1);
        expect(await area.getAttribute('data-head')).toBeNull();
        await expect(block.locator('input[data-box]')).toHaveCount(boxCount);
        await expect(area).toHaveValue(items[i].replace(/^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\](?:\s+|$))?/, ''));
        expect(Math.abs((await block.boundingBox()).height - before.height), `${width}: ${items[i]}`).toBeLessThanOrEqual(1);
        expect(Math.abs((await page.locator('#doc .blk').last().boundingBox()).y - below.y)).toBeLessThanOrEqual(1);
        await page.keyboard.press('Escape');
        await expect(page.locator('#doc textarea')).toHaveCount(0);
        expect(await page.evaluate(() => doc.source())).toBe(body);
      }
    }
    await page.goto(url);
    await page.locator('#doc .task > span').first().click();
    await page.locator('#doc textarea').fill('Changed task');
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await expect(page.locator('#doc textarea')).toHaveValue('');
    expect(await page.evaluate(() => doc.source())).toContain('- [ ] Changed task\n- [ ] ');
    await page.keyboard.insertText('Next task');
    await page.keyboard.press('Tab');
    expect(await page.evaluate(() => doc.source())).toContain('  - [ ] Next task');
    await page.keyboard.press('Shift+Tab');
    expect(await page.evaluate(() => doc.source())).toContain('\n- [ ] Next task');
  }, {body});
});

test('application controls resist highlighting while note content and text fields remain selectable', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.evaluate(() => {
      const controls = document.createElement('div');
      controls.innerHTML = '<button>Action label</button><label>Setting label <input value="Editable text"></label><details open><summary>Disclosure label</summary><p class="hint">Helpful explanation</p></details><a href="#test">Readable link</a><img alt="Picture"><div draggable="true">Movable note</div>';
      document.body.appendChild(controls);
      window.controls = controls;
    });
    for (const selector of ['button', 'label', 'summary', '[draggable="true"]']) {
      expect(await page.evaluate(selector => getComputedStyle(controls.querySelector(selector)).userSelect, selector)).toBe('none');
    }
    for (const selector of ['input', '.hint']) {
      expect(await page.evaluate(selector => getComputedStyle(controls.querySelector(selector)).userSelect, selector)).toBe('text');
    }
    expect(await page.locator('#doc p').evaluate(el => getComputedStyle(el).userSelect)).not.toBe('none');
    for (const selector of ['a', 'img']) {
      expect(await page.evaluate(selector => getComputedStyle(controls.querySelector(selector)).webkitUserDrag, selector)).toBe('none');
    }
    const paragraph = page.locator('#doc p');
    const box = await paragraph.boundingBox();
    await page.mouse.move(box.x + 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 140, box.y + box.height / 2, {steps:10});
    await page.mouse.up();
    expect(await page.evaluate(() => getSelection().toString())).not.toBe('');
  });
});

test('select all and undo keep checklists rendered and copy their complete Markdown', async ({ browser }) => {
  const body = '- [ ] First task\n2. Second item\n- [x] Last task';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    await page.locator('#doc .task > span').first().click();
    const area = page.locator('#doc textarea');
    await area.evaluate(el => el.setSelectionRange(2, 7));
    expect(await page.evaluate(() => doc.selection())).toMatchObject({text:'rst t',from:1,to:1});
    await page.keyboard.press('ControlOrMeta+a');
    await expect(area).toHaveCount(0);
    await expect(page.locator('#doc input[data-box]')).toHaveCount(2);
    const copy = () => page.evaluate(() => {
      const data = new DataTransfer();
      document.dispatchEvent(new ClipboardEvent('copy', {clipboardData:data,bubbles:true,cancelable:true}));
      return data.getData('text/plain');
    });
    expect(await copy()).toBe(body);
    await page.keyboard.press('Backspace');
    expect(await page.evaluate(() => doc.source())).toBe('');
    await page.keyboard.press('ControlOrMeta+z');
    await expect(area).toHaveCount(0);
    await expect(page.locator('#doc input[data-box]')).toHaveCount(2);
    expect(await copy()).toBe(body);
    await page.keyboard.press('ControlOrMeta+Shift+z');
    expect(await page.evaluate(() => doc.source())).toBe('');
  }, {body});
});

test('list text supports caret clicks, removing formatting, undo and navigation in both directions', async ({ browser }) => {
  const body = 'Before\n  - [x] Task text\n3. Number text\nAfter';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.goto(url);
    const task = page.locator('#doc .blk').nth(1);
    await task.locator('label > span').click();
    const area = page.locator('#doc textarea');
    await area.evaluate(el => el.setSelectionRange(el.value.length, el.value.length));
    await area.click({position:{x:1,y:10}});
    expect(await area.evaluate(el => el.selectionStart)).toBe(0);
    await page.keyboard.insertText('New ');
    expect(await page.evaluate(() => doc.source())).toContain('  - [x] New Task text');
    await area.evaluate(el => el.setSelectionRange(0, 0));
    await page.keyboard.press('Backspace');
    await expect(area).toHaveValue('New Task text');
    expect(await page.evaluate(() => doc.source())).toContain('\nNew Task text\n');
    await page.keyboard.press('ControlOrMeta+z');
    expect(await page.evaluate(() => doc.source())).toContain('  - [x] New Task text');
    await page.keyboard.press('Escape');
    await task.locator('label > span').click();
    await page.keyboard.press('ArrowDown');
    await expect(area).toHaveValue('Number text');
    await page.keyboard.press('ArrowUp');
    await expect(area).toHaveValue('New Task text');
    await page.keyboard.press('End');
    await page.keyboard.insertText('\nPasted continuation');
    expect(await page.evaluate(() => doc.source())).toContain('  - [x] New Task text\nPasted continuation');
  }, {body});
});

test('empty lists exit and a whole single list item can be deleted, cut or replaced', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    for (const source of ['- [ ] ', '2. [ ] ', '-   [ ] ', '  - ', '3) ']) {
      await page.goto(url + '/?body=' + encodeURIComponent(source));
      await page.locator('#doc .blk').click({position:{x:80,y:10}});
      await expect(page.locator('#doc textarea')).toBeFocused();
      expect((await page.locator('#doc textarea').boundingBox()).height).toBeGreaterThan(20);
      await page.keyboard.press('Enter');
      expect(await page.evaluate(() => doc.source()), source).toBe('');
    }
    for (const source of ['- [ ] Task', '12. Item', '* Bullet']) {
      for (const action of ['delete', 'cut', 'replace']) {
        await page.goto(url + '/?body=' + encodeURIComponent(source));
        await page.locator('#doc .blk').click({position:{x:80,y:10}});
        await page.keyboard.press('ControlOrMeta+a');
        if (action === 'cut') {
          const cut = await page.evaluate(() => {
            const data = new DataTransfer();
            document.dispatchEvent(new ClipboardEvent('cut', {clipboardData:data,bubbles:true,cancelable:true}));
            return data.getData('text/plain');
          });
          expect(cut).toBe(source);
        } else await page.keyboard.press(action === 'delete' ? 'Backspace' : 'X');
        expect(await page.evaluate(() => doc.source()), `${source}: ${action}`).toBe(action === 'replace' ? 'X' : '');
      }
    }
  });
});

test('formatted wrapped text and heading prefixes keep their position when focused', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({width:390,height:900});
    for (const line of ['- ' + '**word** '.repeat(25), '- [ ] Task ' + '[word](https://example.com) '.repeat(15), '**Bold text** '.repeat(30), ...Array.from({length:6}, (_, i) => '#'.repeat(i + 1) + ' A heading')]) {
      await page.goto(url + '/?body=' + encodeURIComponent(line + '\nFollowing paragraph'));
      const block = page.locator('#doc .blk').first();
      const before = await block.boundingBox();
      const below = await page.locator('#doc .blk').last().boundingBox();
      await block.click({position:{x:Math.min(35,before.width / 2),y:10}});
      const area = page.locator('#doc textarea');
      await expect(area).toBeFocused();
      expect(Math.abs((await block.boundingBox()).height - before.height), line).toBeLessThanOrEqual(1);
      expect(Math.abs((await page.locator('#doc .blk').last().boundingBox()).y - below.y), line).toBeLessThanOrEqual(1);
      if (line.startsWith('#')) {
        const rawTextLeft = await area.evaluate((el, prefix) => {
          const style = getComputedStyle(el);
          const canvas = document.createElement('canvas').getContext('2d');
          canvas.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
          return el.getBoundingClientRect().left + canvas.measureText(prefix).width;
        }, line.match(/^#+ /)[0]);
        expect(Math.abs(rawTextLeft - before.x)).toBeLessThanOrEqual(2);
      }
    }
  });
});

test('heading rail previews magnify nearby bars without opening the whole outline', async ({ browser }) => {
  const body = '# Overview\nA **clear** introduction.\n## Details 😀\nA <script>literal</script> preview.\n### Empty\n# Last\nFinal section.';
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    const rail = page.getByRole('button', { name: /Jump to heading:/ });
    const preview = page.getByRole('tooltip');
    await rail.locator('[data-tick]').nth(1).hover();
    await expect(preview).toContainText('Details 😀');
    await expect(preview).toContainText('A <script>literal</script> preview.');
    await expect(preview.locator('script')).toHaveCount(0);
    await expect(rail.locator('[data-tick]').nth(1)).toHaveAttribute('data-distance', '0');
    await expect(rail.locator('[data-tick]').first()).toHaveAttribute('data-distance', '1');
    await expect(page.getByRole('navigation', { name: 'Headings' })).toBeHidden();
    await rail.locator('[data-tick]').nth(1).click();
    await expect(page.locator('#doc .blk:focus')).toContainText('Details 😀');
    await page.mouse.move(600, 500);
    await expect(preview).toBeHidden();
    await rail.focus();
    await rail.press('End');
    await expect(preview).toContainText('Last');
    await rail.press('ArrowUp');
    await expect(preview).toHaveText('Empty');
    await rail.press('Home');
    await expect(preview).toContainText('Overview');
    await rail.press('Enter');
    await expect(page.locator('#doc .blk:focus')).toContainText('Overview');
    await expect(preview).toBeHidden();
    await rail.focus();
    await rail.press('Escape');
    await expect(preview).toBeHidden();
    await page.emulateMedia({ reducedMotion: 'reduce' });
    expect(await rail.locator('[data-tick]').first().evaluate(el => getComputedStyle(el).transitionDuration)).toBe('0s');
    await page.setViewportSize({ width: 390, height: 900 });
    await expect(rail).toBeHidden();
    await expect(preview).toBeHidden();
  }, { body });
});

test('heading previews preserve edits, refresh body text, and release their elements on destroy', async ({ browser }) => {
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.goto(url);
    await page.locator('#doc .blk').nth(1).click();
    const editor = page.locator('#doc textarea');
    await editor.fill('An updated **draft** 😀');
    await editor.evaluate(el => el.setSelectionRange(4, 9));
    const rail = page.locator('.outline-ticks');
    await rail.locator('[data-tick]').first().hover();
    await expect(page.getByRole('tooltip')).toContainText('An updated draft 😀');
    await expect(editor).toBeFocused();
    expect(await editor.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([4, 9]);
    await editor.fill('Another draft');
    await expect(page.getByRole('tooltip')).toContainText('Another draft');
    await page.evaluate(() => window.doc.destroy());
    await expect(rail).toHaveCount(0);
    await expect(page.locator('.outline-preview')).toHaveCount(0);
  }, { body: '# Start\nOriginal paragraph.\n## End\nLast paragraph.' });
});

test('heading rail keeps every marker reachable in a short viewport and tracks reading separately from previews', async ({ browser }) => {
  const body = Array.from({ length: 300 }, (_, i) => `## Section ${i + 1}\nSome reading material.`).join('\n\n');
  await editorFixture(browser, async (context, url) => {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1600, height: 500 });
    await page.goto(url);
    const rail = page.locator('.outline-ticks');
    await expect(rail.locator('[data-tick]')).toHaveCount(300);
    const box = await rail.boundingBox();
    expect(box.y + box.height).toBeLessThan(500);
    await rail.focus();
    await rail.press('End');
    await expect(page.getByRole('tooltip')).toContainText('Section 300');
    await expect(rail.locator('.on')).toHaveAttribute('data-tick', '0');
    await rail.press('Space');
    await expect(page.locator('#doc .blk:focus')).toContainText('Section 300');
    await expect(rail.locator('.on')).toHaveAttribute('data-tick', '897');
    await rail.focus();
    await rail.press('Home');
    await expect(page.getByRole('tooltip')).toContainText('Section 1');
    const tip = await page.getByRole('tooltip').boundingBox();
    expect(tip.y).toBeGreaterThanOrEqual(0);
    expect(tip.y + tip.height).toBeLessThan(500);
    await rail.press('ArrowUp');
    await expect(page.getByRole('tooltip')).toContainText('Section 1');
  }, { body });
});

for (const interaction of ['touch', 'disclosure', 'escape']) {
  test('heading rail handles ' + interaction + ' without a prior keyboard selection', async ({ browser }) => {
    await editorFixture(browser, async (context, url) => {
      const page = await context.newPage();
      await page.setViewportSize({ width: 1600, height: 900 });
      await page.goto(url);
      if (interaction === 'touch') {
        await page.locator('.outline-ticks [data-tick]').nth(2).tap();
        await expect(page.locator('#doc .blk:focus')).toContainText('Three');
      } else if (interaction === 'disclosure') {
        await page.locator('.note-outline summary').tap();
        await expect(page.getByRole('navigation', { name: 'Headings' })).toBeVisible();
        await page.locator('.note-outline summary').tap();
        await expect(page.getByRole('navigation', { name: 'Headings' })).toBeHidden();
      } else {
        await page.locator('#doc .blk').nth(1).click();
        const editor = page.locator('#doc textarea');
        await page.locator('.outline-ticks [data-tick]').first().hover();
        await expect(page.getByRole('tooltip')).toBeVisible();
        await editor.press('Escape');
        await expect(page.getByRole('tooltip')).toBeHidden();
        await expect(editor).toBeFocused();
      }
    }, { body: '# One\nParagraph\n## Two\nMore\n# Three', touch: interaction !== 'escape' });
  });
}


test('heading rail stays vertically centred and its dropdown animates with reduced-motion support', async ({ browser }) => {
  const short='# One\n\nFirst paragraph.\n\n## Two\n\nSecond paragraph.\n\n### Three\n\nLast paragraph.';
  await editorFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    for(const body of [short,short+'\n\n'+'Long reading material.\n\n'.repeat(100)]) {
      await page.setViewportSize({width:1600,height:900});
      await page.goto(url+'?body='+encodeURIComponent(body));
      const outline=page.locator('.note-outline');
      await expect(outline).toBeVisible();
      for(const height of [900,640]) {
        await page.setViewportSize({width:1600,height});
        await expect.poll(async()=>{const box=await outline.boundingBox();return Math.abs(box.y+box.height/2-height/2);}).toBeLessThanOrEqual(2);
      }
      if(body!==short) {
        await page.evaluate(()=>scrollTo(0,document.documentElement.scrollHeight/2));
        await expect.poll(async()=>{const box=await outline.boundingBox();return Math.abs(box.y+box.height/2-320);}).toBeLessThanOrEqual(2);
      }
    }
    const nav=page.locator('.note-outline nav'), summary=page.locator('.note-outline summary'), details=page.locator('.note-outline details');
    await nav.evaluate(el=>{
      const animate=el.animate.bind(el);
      el.animate=(...args)=>{const run=animate(...args);run.pause();return run;};
    });
    await summary.click();
    expect(await nav.evaluate(el=>el.getAnimations().some(a=>a.playState==='paused'))).toBe(true);
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.currentTime=90;});
    expect(await nav.evaluate(el=>getComputedStyle(el).clipPath)).not.toBe('none');
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.finish();});
    await expect.poll(()=>nav.evaluate(el=>el.getAnimations().length)).toBe(0);
    await expect(details).toHaveAttribute('open','');
    const menu=await nav.boundingBox();
    expect(menu.y).toBeGreaterThanOrEqual(0);
    expect(menu.y+menu.height).toBeLessThanOrEqual(640);
    await summary.click();
    expect(await nav.evaluate(el=>el.getAnimations().some(a=>a.playState==='paused'))).toBe(true);
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.finish();});
    await expect(details).not.toHaveAttribute('open','');
    await summary.evaluate(el=>{el.click();el.click();el.click();});
    await nav.evaluate(el=>{for(const run of el.getAnimations())run.finish();});
    await expect.poll(()=>nav.evaluate(el=>el.getAnimations().length)).toBe(0);
    await expect(details).toHaveAttribute('open','');
    await page.emulateMedia({reducedMotion:'reduce'});
    await summary.click();
    await expect(details).not.toHaveAttribute('open','');
    expect(await nav.evaluate(el=>el.getAnimations().length)).toBe(0);
    await page.setViewportSize({width:390,height:844});
    await page.evaluate(()=>scrollTo(0,0));
    expect((await page.locator('.note-outline').boundingBox()).y).toBeLessThan(200);
  },{body:short});
});
