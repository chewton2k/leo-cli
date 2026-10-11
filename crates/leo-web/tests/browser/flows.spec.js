const base = require('@playwright/test');
const { expect } = base;

const TECHNICAL = /Unexpected end of JSON|is not valid JSON|Failed to execute|Cannot read propert|is not a function|is not defined|\bundefined\b|\[object |\bNaN\b|SyntaxError|TypeError|ReferenceError/;
const test = base.test.extend({
  guard: [async ({ page }, use) => {
    const crashes = [];
    const shown = [];
    page.on('pageerror', (e) => crashes.push(e.message));
    await page.exposeFunction('__leoShown', (text) => { shown.push(text); });
    await page.addInitScript(() => {
      const seen = new WeakSet();
      const look = () => {
        for (const el of document.querySelectorAll('.toast, .msg-error, [role="alert"]')) {
          if (seen.has(el)) continue;
          seen.add(el);
          window.__leoShown(el.textContent || '');
        }
      };
      new MutationObserver(look).observe(document, { childList: true, subtree: true, characterData: true });
    });
    await use();
    expect(crashes, 'the page threw').toEqual([]);
    expect(shown.filter((t) => TECHNICAL.test(t)), 'a programmer error was shown to the user').toEqual([]);
  }, { auto: true }],
});
const fs = require('node:fs');
const path = require('node:path');

test.beforeEach(async ({ page }) => {
  const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
  await page.goto(`/?token=${token}`);
  await expect(page.locator('#app')).not.toBeEmpty();
});

async function goPlace(page, action) {
  if (await page.locator('#side').isHidden()) {
    await page.locator('#menu').click();
    return page.locator(`.sheet [data-action="${action}"]`).click();
  }
  await page.locator(`#side [data-action="${action}"]`).click();
}

function control(page, action) {
  return page.locator(`.fab[data-action="${action}"]:visible, #side [data-action="${action}"]:visible`).first();
}

async function openSearchBox(page) {
  if (await page.locator('#side').isHidden()) return page.locator('#search-toggle').click();
  await page.locator('#side [data-action="side-search"]').click();
}

async function dropFiles(page, selector, files) {
  const data = await page.evaluateHandle((list) => {
    const transfer = new DataTransfer();
    for (const f of list) transfer.items.add(new File([f.text], f.name, { type: f.type }));
    return transfer;
  }, files);
  await page.dispatchEvent(selector, 'dragover', { dataTransfer: data });
  await page.dispatchEvent(selector, 'drop', { dataTransfer: data });
}

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
  const refused = page.waitForResponse((r) => r.request().method() === 'PATCH' && r.status() === 500);
  await page.locator('#back').click();
  await refused;
  await page.reload();
  await expect(page.locator('.toast')).toContainText('Saving 1 edit that had not reached leo yet');
  await page.goto(`/#/n/${note.id}`);
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

test('the title placeholder sits behind the cursor and comes back when the title is cleared', async ({ page }) => {
  await page.goto('/#/new');
  const title = page.locator('#title');
  await expect(title).toHaveClass(/blank/);
  const before = await title.evaluate((el) => {
    const style = getComputedStyle(el, '::before');
    return { content: style.content, position: style.position };
  });
  expect(before).toEqual({ content: '"Title"', position: 'absolute' });
  await title.click();
  await page.locator('.blk').first().click();
  await title.click();
  expect(await title.evaluate(() => getSelection().anchorOffset)).toBe(0);
  await page.keyboard.type('Graphs');
  await expect(title).toHaveText('Graphs');
  await expect(title).not.toHaveClass(/blank/);
  for (let i = 0; i < 6; i++) await page.keyboard.press('Backspace');
  await expect(title).toHaveClass(/blank/);
  expect(await title.evaluate((el) => el.innerHTML)).toBe('');
});

test('a tap on any checkbox ticks it and saves, without opening the line', async ({ page }) => {
  const body = ['- [ ] dash', '* [ ] star', '+ [ ] plus', '1. [ ] numbered', '- [ ] parent', '  - [ ] nested', '- [x] done', '- [ ]', '-   [ ] wide', '', 'Plain line'].join('\n');
  const note = await (await page.request.post('/api/notes', { data: { title: `Ticks ${test.info().project.name}`, body } })).json();
  await page.goto(`/#/n/${note.id}`);
  const boxes = page.locator('.doc input[data-box]');
  await expect(boxes).toHaveCount(9);
  for (let i = 0; i < 9; i++) {
    const box = boxes.nth(i);
    const was = await box.isChecked();
    await box.click();
    await expect(boxes.nth(i), `box ${i + 1} toggles`).toBeChecked({ checked: !was });
    await expect(page.locator('.doc textarea')).toHaveCount(0);
  }
  await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body, { timeout: 5000 })
    .toBe(['- [x] dash', '* [x] star', '+ [x] plus', '1. [x] numbered', '- [x] parent', '  - [x] nested', '- [ ] done', '- [x]', '-   [x] wide', '', 'Plain line'].join('\n'));
  await page.locator('.doc .task-item', { hasText: 'dash' }).locator('span').click();
  await expect(page.locator('.doc textarea')).toHaveCount(1);
});

test('on a wide screen the sidebar reaches every place and can be narrowed', async ({ page }) => {
  const wide = test.info().project.name === 'desktop';
  await page.request.post('/api/dirs', { data: { path: 'side-course' } });
  await page.goto('/');
  const side = page.locator('#side');
  if (!wide) {
    await expect(side).toBeHidden();
    await expect(page.locator('#menu')).toBeVisible();
    return;
  }
  await expect(side).toBeVisible();
  await expect(page.locator('#menu')).toBeHidden();
  await expect(side.locator('[data-action="home"].side-row')).toHaveAttribute('aria-current', 'page');
  for (const [action, url] of [['map', /#\/map$/], ['trash', /#\/trash$/], ['storage', /#\/settings\/storage$/], ['settings', /#\/settings$/]]) {
    await side.locator(`[data-action="${action}"]`).click();
    await expect(page).toHaveURL(url);
    await expect(side.locator(`[data-action="${action}"]`)).toHaveAttribute('aria-current', 'page');
  }
  await side.locator('[data-action="open-folder"][data-dir="side-course"]').click();
  await expect(page).toHaveURL(/#\/f\/side-course$/);
  await expect(side.locator('[data-action="open-folder"][data-dir="side-course"]')).toHaveAttribute('aria-current', 'page');
  await side.locator('[data-action="chat"]').click();
  await expect(page.locator('#chat')).toBeVisible();
  await expect(side.locator('[data-action="chat"]')).toHaveClass(/\bon\b/);
  await page.locator('[data-chat="close"]').click();

  await page.keyboard.press('ControlOrMeta+k');
  await expect(page.locator('#search-input')).toBeFocused();
  await page.keyboard.press('Escape');

  const full = (await side.boundingBox()).width;
  await side.locator('[data-action="side-fold"]').click();
  await expect(page.locator('body')).toHaveClass(/side-mini/);
  const narrow = (await side.boundingBox()).width;
  expect(narrow).toBeLessThan(80);
  expect((await page.locator('main').boundingBox()).x).toBeGreaterThanOrEqual(narrow);
  await expect(side.locator('.side-row .side-label').first()).toBeHidden();
  await page.reload();
  await expect(page.locator('body')).toHaveClass(/side-mini/);
  await side.locator('[data-action="side-fold"]').click();
  expect((await side.boundingBox()).width).toBe(full);
});

test.describe('editing like Obsidian', () => {
  const body = ['First line here', 'Second **bold** line', '- Third item', 'Fourth line'].join('\n');
  async function open(page, tag) {
    const note = await (await page.request.post('/api/notes', { data: { title: `Select ${tag} ${test.info().project.name}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc .blk')).toHaveCount(4);
    return async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
  }
  async function select(page, from, to) {
    await page.evaluate(([a, b]) => {
      const text = (i) => {
        const walker = document.createTreeWalker(document.querySelectorAll('#doc .blk')[i], NodeFilter.SHOW_TEXT);
        return walker.nextNode();
      };
      const range = document.createRange();
      range.setStart(text(a[0]), a[1]);
      range.setEnd(text(b[0]), b[1]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.activeElement.blur();
    }, [from, to]);
  }

  test('a dragged selection across lines becomes editable, so Delete removes all of it', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const saved = await open(page, 'drag');
    const first = await page.locator('#doc .blk').nth(0).locator('p').boundingBox();
    const third = await page.locator('#doc .blk').nth(2).locator('li').boundingBox();
    await page.mouse.move(first.x + 2, first.y + first.height / 2);
    await page.mouse.down();
    await page.mouse.move(third.x + third.width - 2, third.y + third.height / 2, { steps: 8 });
    await page.mouse.up();
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toHaveCount(1);
    expect(await area.evaluate((a) => a.value.slice(a.selectionStart, a.selectionEnd))).toContain('Second **bold** line');
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe('\nFourth line');
  });

  test('typing over a selection replaces it, keeping the rest of both lines', async ({ page }) => {
    const saved = await open(page, 'type');
    await select(page, [0, 6], [2, 5]);
    await page.keyboard.press('X');
    await expect.poll(saved, { timeout: 5000 }).toBe(['First X item', 'Fourth line'].join('\n'));
    await expect(page.locator('#doc textarea.line-edit')).toBeFocused();
  });

  test('Shift+click from the caret selects every line between, and Cmd+A takes the whole note', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'Shift+click is for a mouse');
    const saved = await open(page, 'shift');
    await page.locator('#doc .blk').nth(1).click();
    const area = page.locator('#doc textarea.line-edit');
    await area.evaluate((a) => a.setSelectionRange(0, 0));
    const last = await page.locator('#doc .blk').nth(3).locator('p').boundingBox();
    await page.keyboard.down('Shift');
    await page.mouse.click(last.x + last.width - 1, last.y + last.height / 2);
    await page.keyboard.up('Shift');
    expect(await area.evaluate((a) => a.value.slice(a.selectionStart, a.selectionEnd))).toBe(['Second **bold** line', '- Third item', 'Fourth line'].join('\n'));
    await page.keyboard.press('Delete');
    await expect.poll(saved, { timeout: 5000 }).toBe('First line here\n');
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe('');
  });

  test('Cmd/Ctrl+Z brings back a deleted section and Shift+Cmd+Z takes it away again', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'undo is a keyboard shortcut');
    const saved = await open(page, 'undo');
    await select(page, [0, 0], [2, 5]);
    await page.keyboard.press('Backspace');
    await expect.poll(saved, { timeout: 5000 }).toBe(' item\nFourth line');
    await page.keyboard.type('New start');
    await expect.poll(saved, { timeout: 5000 }).toBe('New start item\nFourth line');
    await page.keyboard.press('ControlOrMeta+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(' item\nFourth line');
    await page.keyboard.press('ControlOrMeta+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(body);
    const area = page.locator('#doc textarea.line-edit');
    expect(await area.evaluate((a) => a.value.slice(a.selectionStart, a.selectionEnd))).toContain('Second **bold** line');
    await page.keyboard.press('ControlOrMeta+Shift+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(' item\nFourth line');
    await page.locator('#doc').click({ position: { x: 5, y: 5 } });
    await page.keyboard.press('Escape');
    await page.keyboard.press('ControlOrMeta+z');
    await expect.poll(saved, { timeout: 5000 }).toBe(body);
  });

  test('a selection that starts and ends outside the note leaves it alone', async ({ page }) => {
    const saved = await open(page, 'outside');
    await page.evaluate(() => {
      const range = document.createRange();
      range.setStartBefore(document.querySelector('#title'));
      range.setEndAfter(document.querySelector('#doc'));
      window.getSelection().removeAllRanges();
      window.getSelection().addRange(range);
    });
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(900);
    expect(await saved()).toBe(body);
  });
});

test.describe('math', () => {
  test('LaTeX in a note and in Felix is drawn as math, with its fonts', async ({ page }) => {
    const fonts = [];
    page.on('response', (r) => { if (r.url().includes('/vendor/katex-0.16.11/fonts/')) fonts.push(r.status()); });
    const body = 'Energy is $E = mc^2$, not $5.\n\n$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$';
    const note = await (await page.request.post('/api/notes', { data: { title: `Math ${test.info().project.name}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    const inline = page.locator('#doc .math[data-drawn="yes"]:not(.math-block) .katex');
    await expect(inline).toBeVisible({ timeout: 10000 });
    await expect(page.locator('#doc .math-block[data-drawn="yes"] .katex-display')).toBeVisible();
    await expect(page.locator('#doc')).toContainText('not $5.');
    await expect.poll(() => fonts.length).toBeGreaterThan(0);
    expect(fonts.every((status) => status === 200)).toBe(true);

    const said = 'The area is $\\pi r^2$, or \\(\\pi r^2\\), and in full:\n\\[\nA = \\int_0^r 2\\pi t\\,dt\n\\]\n\\begin{aligned}\na &= b\n\\end{aligned}';
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { t: said }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('area of a circle?');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat .msg.leo .math[data-drawn="yes"]:not(.math-block) .katex')).toHaveCount(2);
    await expect(page.locator('#chat .msg.leo .math-block[data-drawn="yes"] .katex-display')).toHaveCount(2);
    const outside = await page.locator('#chat .msg.leo .prose').evaluate((el) => {
      const copy = el.cloneNode(true);
      copy.querySelectorAll('.katex, .katex-display').forEach((k) => k.remove());
      return copy.textContent;
    });
    expect(outside).not.toMatch(/\\\(|\\\[|\\begin|\$/);
  });
});

test.describe('diagrams', () => {
  test('a mermaid block in a note is drawn, a broken one says why, and labels cannot run code', async ({ page }) => {
    const body = [
      'Breadth-first search:',
      '',
      '```mermaid',
      'flowchart LR',
      '  A["Start (source)"] --> B[Visit neighbours]',
      '  B --> C{Queue empty?}',
      '  C -- no --> B',
      '```',
      '',
      '```mermaid',
      'flowchart LR',
      '  X["<img src=x onerror=window.__pwned=1>"] --> Y',
      '```',
      '',
      '```mermaid',
      'this is not a diagram',
      '```',
    ].join('\n');
    const note = await (await page.request.post('/api/notes', { data: { title: `Diagram ${test.info().project.name}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    const drawn = page.locator('#doc figure.diagram[data-drawn="yes"]');
    await expect(drawn).toHaveCount(2, { timeout: 15000 });
    await expect(drawn.first().locator('svg')).toBeVisible();
    await expect(drawn.first()).toContainText('Visit neighbours');
    await expect(drawn.first().locator('.diagram-src')).toBeHidden();
    await expect(page.locator('#doc .diagram-error')).toContainText('This diagram could not be drawn');
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined();
    await expect(page.locator('#doc figure.diagram img[onerror]')).toHaveCount(0);
  });

  test('Felix can answer with a diagram, drawn once the answer is complete', async ({ page }) => {
    const answer = 'Here is how it flows:\n\n```mermaid\nflowchart TD\n  Q[Queue] --> V[Visit]\n```\n';
    const lines = [{ sources: [] }, ...answer.match(/[\s\S]{1,9}/g).map((t) => ({ t })), { done: true }];
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('draw bfs');
    await page.locator('#chat-input').press('Enter');
    const figure = page.locator('#chat .msg.leo figure.diagram[data-drawn="yes"]');
    await expect(figure.locator('svg')).toBeVisible({ timeout: 15000 });
    await expect(figure).toContainText('Visit');
  });
});

test.describe('drag and drop', () => {
  test('a note and a folder drag into other folders, and Undo puts them back', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    const tag = Date.now().toString(36);
    const [a, b] = [`dd-a-${tag}`, `dd-b-${tag}`];
    for (const dir of [a, b]) await page.request.post('/api/dirs', { data: { path: dir } });
    const note = await (await page.request.post('/api/notes', { data: { title: `Drag me ${tag}`, body: 'x' } })).json();
    const where = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).directory;
    await page.goto('/');
    const card = page.locator(`.card[data-id="${note.id}"]`);
    await card.dragTo(page.locator(`main .folder[data-dir="${a}"]`));
    await expect(page.locator('.toast')).toContainText(`Moved “Drag me ${tag}” to ${a}`);
    expect(await where()).toBe(a);
    await expect(card).toHaveCount(0);
    await page.locator('.toast button').click();
    await expect(card).toBeVisible();
    expect(await where()).toBe('');

    await card.dragTo(page.locator(`#side [data-action="open-folder"][data-dir="${b}"]`));
    await expect.poll(where).toBe(b);

    await page.locator(`main .folder[data-dir="${b}"]`).dragTo(page.locator(`main .folder[data-dir="${a}"]`));
    await expect(page.locator('.toast')).toContainText(`Moved ${b} into ${a}`);
    expect(await where()).toBe(`${a}/${b}`);
    await expect(page.locator(`main .folder[data-dir="${b}"]`)).toHaveCount(0);
    const inside = await (await page.request.get(`/api/dirs?parent=${a}`)).json();
    expect(inside.map((d) => d.name)).toEqual([b]);
    await page.locator('.toast button').click();
    await expect(page.locator(`main .folder[data-dir="${b}"]`)).toBeVisible();
    expect(await where()).toBe(b);

    await page.goto(`/#/f/${b}`);
    await page.locator(`.card[data-id="${note.id}"]`).dragTo(page.locator('#side .side-row[data-action="home"]'));
    await expect.poll(where).toBe('');
  });

  test('files dropped on the page make a note, and on Felix go to him', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'dragging is for a mouse');
    await page.goto('/');
    await dropFiles(page, 'main', [{ name: 'dropped.txt', type: 'text/plain', text: 'Heaps keep the minimum at the root.' }]);
    await expect(page.locator('#upload-list')).toContainText('dropped.txt');
    await page.keyboard.press('Escape');

    const note = await (await page.request.post('/api/notes', { data: { title: 'Dragged to Felix', body: 'x' } })).json();
    await page.reload();
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await dropFiles(page, '#chat', [{ name: 'for-felix.txt', type: 'text/plain', text: 'Heaps keep the minimum at the root.' }]);
    await expect(chat.locator('#chat-refs .file-card', { hasText: 'for-felix.txt' })).toBeVisible();
    await expect(page.locator('.scrim')).toHaveCount(0);
    await page.locator(`.card[data-id="${note.id}"]`).dragTo(chat.locator('#chat-input'));
    await expect(chat.locator('.chat-ref', { hasText: 'Dragged to Felix' })).toBeVisible();
  });
});

test('on a phone a sheet swipes down to close, and a small drag springs back', async ({ page }) => {
  test.skip(test.info().project.name !== 'phone', 'sheets slide up from the bottom on phones');
  await page.goto('/');
  const swipe = (distance, ms) => page.evaluate(async ([distance, ms]) => {
    const sheet = document.querySelector('.sheet');
    const box = sheet.getBoundingClientRect();
    const at = (y) => [new Touch({ identifier: 1, target: sheet, clientX: box.left + 40, clientY: y })];
    const top = box.top + 20;
    sheet.dispatchEvent(new TouchEvent('touchstart', { touches: at(top), changedTouches: at(top), bubbles: true, cancelable: true }));
    const steps = 6;
    for (let i = 1; i <= steps; i++) {
      await new Promise((r) => setTimeout(r, ms / steps));
      const y = top + (distance * i) / steps;
      sheet.dispatchEvent(new TouchEvent('touchmove', { touches: at(y), changedTouches: at(y), bubbles: true, cancelable: true }));
    }
    sheet.dispatchEvent(new TouchEvent('touchend', { touches: [], changedTouches: at(top + distance), bubbles: true, cancelable: true }));
  }, [distance, ms]);
  await page.locator('#menu').click();
  await expect(page.locator('.sheet')).toBeVisible();
  await swipe(20, 400);
  await page.waitForTimeout(300);
  await expect(page.locator('.sheet')).toBeVisible();
  await swipe(160, 300);
  await expect(page.locator('.scrim')).toHaveCount(0);
  await page.locator('#menu').click();
  await swipe(40, 40);
  await expect(page.locator('.scrim'), 'a quick flick closes it too').toHaveCount(0);
});

test('search finds a note named after a command', async ({ page }) => {
  await page.request.post('/api/notes', { data: { title: 'backup checklist', body: 'Check my backups' } });
  await openSearchBox(page);
  await page.locator('#search-input').fill('backup');
  await expect(page.locator('.card').filter({ hasText: 'backup checklist' })).toBeVisible();
});


test.describe('plain HTTP access', () => {
  test('a new note can be created without secure-context browser APIs', async ({ page }) => {
    const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
    await page.goto(`http://leo-http.test:31831/?token=${token}`);
    expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
    expect(await page.evaluate(() => typeof crypto.randomUUID)).toBe('undefined');
    await control(page, 'new').click();
    await expect(page.locator('#title')).toBeVisible();
    await page.locator('#title').fill('Created over plain HTTP');
    await expect(page.locator('#save-state')).toHaveText('Saved');
    const saved = await page.evaluate(async () => (await fetch('/api/notes')).json());
    expect(saved.some((note) => note.title === 'Created over plain HTTP')).toBe(true);
  });
});

test.describe('knowledge graph', () => {
  async function seed(page) {
    const made = {};
    for (const [title, body, directory] of [
      ['Graph traversals', 'BFS uses a queue. See [[Scheduling]].', 'cs130'],
      ['Scheduling', 'Round robin takes from a ready queue.', 'cs162'],
      ['Heaps', 'A binary heap backs a priority queue.', 'cs130'],
    ]) {
      await page.request.post('/api/dirs', { data: { path: directory } });
      made[title] = await (await page.request.post('/api/notes', { data: { title, body, directory } })).json();
    }
    const id = (t) => made[t].id;
    const cache = {
      notes: {
        [id('Graph traversals')]: { hash: 'old', summary: 'How BFS explores a graph with a queue', concepts: ['breadth-first search', 'queue'] },
        [id('Scheduling')]: { hash: 'old', summary: 'Round robin runs the next process from a queue', concepts: ['round robin', 'queue'] },
        [id('Heaps')]: { hash: 'old', summary: 'Binary heaps implement priority queues', concepts: ['binary heap'] },
      },
      pairs: {
        '1:0-0': {
          hash: 'old',
          links: [
            { a: id('Scheduling'), b: id('Graph traversals'), kind: 'same method', strength: 3, why: 'Both take the next piece of work from a queue' },
            { a: id('Graph traversals'), b: id('Heaps'), kind: 'builds on', strength: 1, why: 'Dijkstra swaps the queue for a heap' },
          ],
        },
      },
      built_at: '2026-10-06T00:00:00Z',
    };
    fs.writeFileSync(path.join(process.env.LEO_BROWSER_HOME, 'graph.json'), JSON.stringify(cache));
    return made;
  }

  test('search finds a note through the ideas on its map and says so', async ({ page }) => {
    const made = await seed(page);
    await page.goto('/#/search/breadth');
    const card = page.locator(`.card[data-id="${made['Graph traversals'].id}"]`);
    await expect(card).toBeVisible();
    await expect(card.locator('.card-why')).toHaveText('Through the idea “breadth-first search” in the knowledge graph');
    await page.goto('/#/search/binary%20heaps%20priority');
    await expect(page.locator(`.card[data-id="${made.Heaps.id}"]`)).toBeVisible();
  });

  test('Tidy up lays the graph out again and says so', async ({ page }) => {
    await seed(page);
    await page.goto('/#/map');
    await expect(page.locator('#map-canvas')).toBeVisible();
    await page.locator('[data-action="map-tidy"]').click();
    await expect(page.locator('.toast')).toContainText('Tidied up');
    await expect(page.locator('#map-canvas')).toBeVisible();
  });

  test('a note opens on the map with its connections and why', async ({ page }) => {
    const made = await seed(page);
    await page.goto(`/#/n/${made.Scheduling.id}`);
    await page.locator('[data-action="note-map"]').click();
    await expect(page).toHaveURL(new RegExp(`#/map/${made.Scheduling.id}$`));
    await expect(page.locator('#map-canvas')).toBeVisible();
    const panel = page.locator('#map-panel');
    await expect(panel.locator('h3')).toHaveText('Scheduling');
    await expect(panel).toContainText('Round robin runs the next process from a queue');
    await expect(panel.locator('h4').first()).toContainText('Across classes');
    const row = panel.locator('.map-row', { hasText: 'Graph traversals' });
    await expect(row).toContainText('Same method');
    await expect(row).toContainText('Both take the next piece of work from a queue');
    await row.click();
    await expect(panel.locator('h3')).toHaveText('Graph traversals');
    await expect(panel.locator('.map-row', { hasText: 'Heaps' })).toContainText('Builds on');
    await expect(panel.locator('.map-row', { hasText: 'Scheduling' }).first()).toContainText('Same method');
    await panel.locator('[data-action="map-focus"]').click();
    await expect(page.locator('[data-action="map-unfocus"]')).toBeVisible();
    await page.locator('[data-action="map-unfocus"]').click();
    await panel.locator('[data-action="map-clear"]').click();
    await expect(panel.locator('h3')).toHaveText('Strongest connections');
    await expect(panel.locator('.map-row').first()).toContainText('Both take the next piece of work from a queue');
    await page.locator('#map-find').fill('heap');
    await page.locator('#map-found .map-row').first().click();
    await expect(panel.locator('h3')).toHaveText('Heaps');
    await expect(panel.locator('.map-row', { hasText: 'Graph traversals' })).toContainText('Built on by');
    await panel.locator('[data-action="open-note"]').click();
    await expect(page.locator('#title')).toHaveText('Heaps');
  });

  test('with Felix open, the map and its details stay beside him', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'Felix covers the page on phones');
    await page.setViewportSize({ width: 1440, height: 900 });
    const made = await seed(page);
    await page.goto(`/#/map/${made.Scheduling.id}`);
    const panel = page.locator('#map-panel');
    await expect(panel.locator('h3')).toHaveText('Scheduling');
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat')).toBeVisible();
    const chat = await page.locator('#chat').boundingBox();
    const side = await page.locator('#side').boundingBox();
    const map = await page.locator('#map').boundingBox();
    const details = await panel.boundingBox();
    const tools = await page.locator('.map-tools').boundingBox();
    expect(map.x).toBeGreaterThanOrEqual(side.x + side.width - 1);
    expect(map.x + map.width).toBeLessThanOrEqual(chat.x + 1);
    expect(details.x + details.width).toBeLessThanOrEqual(chat.x);
    expect(details.width).toBeGreaterThan(220);
    expect(tools.x + tools.width).toBeLessThanOrEqual(details.x);
    await expect(panel.locator('h3')).toBeInViewport();
    await page.locator('[data-chat="close"]').click();
    expect((await page.locator('#map').boundingBox()).width).toBeGreaterThan(map.width + 300);
  });

  test('the toggles show connections across classes and shared ideas', async ({ page }) => {
    await seed(page);
    await page.goto('/#/map');
    const across = page.locator('[data-action="map-across"]');
    await expect(across).toContainText('Across classes');
    await across.click();
    await expect(across).toHaveAttribute('aria-pressed', 'true');
    const ideas = page.locator('[data-action="map-ideas"]');
    await ideas.click();
    await expect(ideas).toHaveAttribute('aria-pressed', 'true');
    await page.locator('#map-find').fill('queue');
    await page.locator('#map-found .map-row', { hasText: 'queue' }).first().click();
    await expect(page.locator('#map-panel h3')).toHaveText('queue');
    await expect(page.locator('#map-panel')).toContainText('In 2 notes across 2 classes');
  });

  test('rebuilding from scratch asks first, then starts a fresh build', async ({ page }) => {
    await seed(page);
    const builds = [];
    await page.route('**/api/graph/build*', (route) => {
      builds.push(new URL(route.request().url()).search);
      return route.fulfill({ status: 202, json: { state: 'building', done: 0, total: 3, message: null, notes: 3, read: 0, stale: 3, requests: 3, rebuild_requests: 3, built_at: null } });
    });
    await page.goto('/#/map');
    await page.locator('[data-action="map-rebuild"]').click();
    await expect(page.locator('.sheet')).toContainText('Rebuild the knowledge graph from scratch?');
    await page.locator('[data-action="close"]').click();
    expect(builds).toEqual([]);
    await page.locator('[data-action="map-rebuild"]').click();
    await page.locator('[data-action="map-rebuild-now"]').click();
    await expect.poll(() => builds).toEqual(['?fresh=1']);
    await expect(page.locator('#map-status')).toContainText('Connecting 0/3');
  });

  test('connecting notes without an AI says how to choose one', async ({ page }) => {
    await seed(page);
    await page.route('**/api/graph/build', (route) => route.fulfill({ status: 202, json: { state: 'building', done: 0, total: 2, message: null, notes: 3, read: 0, stale: 3, requests: 2, built_at: null } }));
    await page.route('**/api/graph/status', (route) => route.fulfill({ json: { state: 'failed', done: 0, total: 2, message: 'no AI for writing is chosen — type :settings in leo and pick one under writing', notes: 3, read: 0, stale: 3, requests: 2, built_at: null } }));
    await page.goto('/#/map');
    await page.locator('#map-status [data-action="map-build"]').click();
    const confirm = page.locator('[data-action="map-build-now"]');
    if (await confirm.isVisible()) await confirm.click();
    await expect(page.locator('.toast.bad')).toContainText(':settings');
  });
});

test.describe('Felix', () => {
  test('quizzes from the open note, cites it, and dances on a right answer', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps', body: 'A binary heap keeps the minimum at the root.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      const sent = route.request().postDataJSON();
      asked.push(sent);
      const source = { n: 1, id: note.id, title: 'Heaps', folder: '', why: 'open' };
      const lines = asked.length === 1
        ? [{ sources: [source] }, { t: 'Where is the smallest element of a min-heap? ' }, { t: '[n1]' }, { done: true }]
        : [{ sources: [source] }, { t: '[[correct]] Yes, at the **root** [n1].' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await expect(chat).toBeVisible();
    await expect(chat.locator('.chat-context')).toContainText('Heaps');
    await chat.locator('[data-mode="study"]').click();
    await chat.locator('.starter').first().click();
    await expect(chat.locator('.msg.leo').first()).toContainText('Where is the smallest element');
    await expect(chat.locator('.msg.leo .cite').first()).toHaveText('Heaps');
    expect(asked[0].mode).toBe('study');
    expect(asked[0].note).toBe(note.id);
    await chat.locator('#chat-input').fill('At the root');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.verdict.correct')).toBeVisible();
    await expect(chat.locator('.msg.leo').last()).not.toContainText('[[correct]]');
    await expect(chat.locator('#chat-face .felix')).toHaveClass(/dance/);
    expect(asked[1].messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user']);
    await chat.locator('.msg.leo .cite').first().click();
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
    if (!(await chat.isVisible())) await page.locator('#chat-toggle').click();
    await chat.locator('[data-mode="chat"]').click();
    await expect(chat.locator('.msg'), 'switching style keeps the conversation').toHaveCount(4);
    await expect(chat.locator('.chat-switch')).toHaveText('Switched to Chat; the conversation carries on');
    await chat.locator('#chat-input').fill('now explain why');
    await chat.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(3);
    expect(asked[2].mode).toBe('chat');
    expect(asked[2].messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'user']);
    await expect(chat.locator('.chat-switch')).toHaveText('Now in Chat');
    await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).some((c) => c.mode === 'chat' && c.count === 6)).toBe(true);
  });

  test('notes are added with @ or the paperclip and go with every message', async ({ page }) => {
    const tag = test.info().project.name;
    const dijkstra = await (await page.request.post('/api/notes', { data: { title: `Dijkstra shortest paths ${tag}`, body: 'Uses a priority queue.' } })).json();
    const heaps = await (await page.request.post('/api/notes', { data: { title: `Binary heaps ${tag}`, body: 'Minimum at the root.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Both use a heap."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await expect(chat.locator('.chat-mode')).toHaveText(['Chat', 'Study']);
    await chat.locator('#chat-input').pressSequentially(`compare @dijkstra shortest paths ${tag}`);
    await expect(chat.locator('.chat-pick-row').first()).toContainText('Dijkstra shortest paths');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.chat-ref')).toHaveCount(1);
    await expect(chat.locator('#chat-input')).toHaveValue('compare ');
    await expect(chat.locator('.chat-pick')).toBeHidden();

    await chat.locator('[data-chat="attach"]').click();
    await chat.locator('[data-chat="attach-note"]').click();
    await chat.locator('#chat-pick-search').fill(`Binary heaps ${tag}`);
    await chat.locator('.chat-pick-row', { hasText: `Binary heaps ${tag}` }).click();
    await expect(chat.locator('.chat-ref')).toHaveCount(2);

    await chat.locator('#chat-input').fill('compare these two');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Both use a heap.');
    expect(asked[0].refs).toEqual([dijkstra.id, heaps.id]);
    await expect(chat.locator('.msg.user .msg-refs .cite')).toHaveCount(2);
    await expect(chat.locator('#chat-refs .chat-ref'), 'sent notes leave the box').toHaveCount(0);

    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat #chat-refs .chat-ref')).toHaveCount(0);
    await page.locator('#chat-input').fill('and now?');
    await page.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].refs, 'follow-ups still bring the notes attached earlier').toEqual([dijkstra.id, heaps.id]);
    await page.locator('#chat .chat-head [data-chat="new"]').click();
    await page.locator('#chat-input').fill('fresh start');
    await page.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(3);
    expect(asked[2].refs).toEqual([]);
  });

  test('past chats are kept on the computer and listed beside the chat', async ({ page }) => {
    const tag = test.info().project.name;
    await page.route('**/api/chat', async (route) => {
      const sent = route.request().postDataJSON();
      const last = sent.messages[sent.messages.length - 1].text;
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: `{"sources":[]}\n{"t":"About ${last}"}\n{"done":true}\n` });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const sidebar = chat.locator('#chat-history');
    const openSidebar = async () => {
      if (!(await sidebar.isVisible())) await chat.locator('.chat-head [data-chat="history"]').click();
      await expect(sidebar).toBeVisible();
    };
    await chat.locator('#chat-input').fill(`first question ${tag}`);
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText(`About first question ${tag}`);
    await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).some((c) => c.title === `first question ${tag}`)).toBe(true);

    await chat.locator('.chat-head [data-chat="new"]').click();
    await expect(chat.locator('.msg')).toHaveCount(0);
    await chat.locator('#chat-input').fill(`second question ${tag}`);
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText(`About second question ${tag}`);

    const width = async () => (await chat.boundingBox()).width;
    const before = await width();
    await openSidebar();
    expect(await width()).toBe(before);
    if (tag === 'desktop') {
      await chat.locator('.chat-head [data-chat="history"]').click();
      await expect(sidebar).toBeHidden();
      expect(await width()).toBe(before);
      await openSidebar();
    }
    await expect(sidebar.locator('.chat-history-item', { hasText: `second question ${tag}` })).toBeVisible();
    await sidebar.locator('.chat-history-item', { hasText: `first question ${tag}` }).click();
    await expect(chat.locator('.msg.leo').last()).toContainText(`About first question ${tag}`);
    await expect(chat.locator('.msg.user')).toHaveCount(1);

    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.locator('#chat-toggle').click();
    await openSidebar();
    const second = sidebar.locator('.chat-history-row', { hasText: `second question ${tag}` });
    await second.locator('.chat-history-item').click();
    await expect(chat.locator('.msg.leo').last()).toContainText(`About second question ${tag}`);
    await openSidebar();
    await second.locator('.chat-history-x').click();
    await expect(second.locator('.chat-history-x')).toHaveText('Delete');
    await second.locator('.chat-history-x').click();
    await expect(second).toHaveCount(0);
    await expect(chat.locator('.msg')).toHaveCount(0);
    expect((await (await page.request.get('/api/chats')).json()).some((c) => c.title === `second question ${tag}`)).toBe(false);
  });

  test('a suggestion asks which note or classes when nothing is open', async ({ page }) => {
    const tag = test.info().project.name;
    const note = await (await page.request.post('/api/notes', { data: { title: `Sorting lecture ${tag}`, body: 'Merge sort.' } })).json();
    await page.request.post('/api/dirs', { data: { path: `algos-${tag}` } });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Here we go."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-mode="study"]').click();
    await chat.locator('.starter', { hasText: 'Quiz me on this note' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which note should I quiz you on?');
    expect(asked.length).toBe(0);
    await chat.locator('#chat-pick-search').fill(`Sorting lecture ${tag}`);
    await chat.locator('.chat-pick-row', { hasText: `Sorting lecture ${tag}` }).click();
    await expect(chat.locator('.msg.leo').last()).toContainText('Here we go.');
    expect(asked[0].messages[0].text).toBe('Quiz me on this note');
    expect(asked[0].refs).toEqual([note.id]);

    await chat.locator('.chat-head [data-chat="new"]').click();
    await chat.locator('.starter', { hasText: 'Make me a 3-day review plan' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which classes should the plan cover?');
    await expect(chat.locator('[data-chat="classes-go"]')).toBeDisabled();
    await chat.locator('.chat-class', { hasText: `algos-${tag}` }).click();
    await chat.locator('[data-chat="classes-go"]').click();
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].messages[0].text).toBe(`Make me a 3-day review plan for algos-${tag}`);

    await chat.locator('.chat-head [data-chat="new"]').click();
    await chat.locator('[data-mode="chat"]').click();
    await chat.locator('.starter', { hasText: 'Explain this simply' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which note should I explain?');
    await chat.locator('[data-chat="ask-cancel"]').click();
    await expect(chat.locator('.chat-ask')).toHaveCount(0);
    expect(asked.length).toBe(2);
  });

  test('Felix thinks, talks, reacts to answers and to being tapped', async ({ page }) => {
    let release;
    const hold = new Promise((resolve) => { release = resolve; });
    let replies = 0;
    await page.route('**/api/chat', async (route) => {
      replies += 1;
      if (replies === 1) await hold;
      const text = replies === 1 ? 'Hello there' : '[[incorrect]] Not quite.';
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: `{"sources":[]}\n{"t":${JSON.stringify(text)}}\n{"done":true}\n` });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const face = page.locator('#chat-face .felix');
    await page.locator('#chat-input').fill('hi');
    await page.locator('#chat-input').press('Enter');
    await expect(face).toHaveClass(/think/);
    release();
    await expect(page.locator('.msg.leo').last()).toContainText('Hello there');
    await expect(face).not.toHaveClass(/think|talk/);
    await page.locator('#chat-input').fill('the root?');
    await page.locator('#chat-input').press('Enter');
    await expect(face).toHaveClass(/droop/);
    await expect(face).toHaveClass(/perk/, { timeout: 4000 });
    await face.click();
    await expect(face).toHaveClass(/boop|hop|spin|giggle/);
    const sizes = await face.evaluate((svg) => {
      const skin = svg.querySelector('.felix-skin');
      return [Number(skin.getAttribute('width')), Number(skin.getAttribute('height'))];
    });
    expect(sizes[0] / sizes[1]).toBeGreaterThan(1);
    expect(sizes[0] / sizes[1]).toBeLessThan(1.4);
  });

  test('a good answer is saved as a note in the folder of the open note', async ({ page }) => {
    const dir = `saved-${test.info().project.name}`;
    await page.request.post('/api/dirs', { data: { path: dir } });
    const open = await (await page.request.post('/api/notes', { data: { title: 'Graph traversals', body: 'BFS uses a queue.', directory: dir } })).json();
    await page.route('**/api/chat', (route) => route.fulfill({
      status: 200,
      headers: { 'content-type': 'application/x-ndjson' },
      body: `{"sources":[{"n":1,"id":"${open.id}","title":"Graph traversals","why":"open"}]}\n{"t":"BFS takes the oldest vertex from a queue [n1]."}\n{"done":true}\n`,
    }));
    await page.goto(`/#/n/${open.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('How does BFS pick the next vertex?');
    await chat.locator('#chat-input').press('Enter');
    await chat.locator('.msg.leo [data-chat="save"]').click();
    await expect(page.locator('.toast')).toContainText(`Saved as a note in ${dir}`);
    await expect(chat.locator('.msg.leo [data-chat="open"]', { hasText: 'Open the saved note' })).toBeVisible();
    const found = await (await page.request.get(`/api/notes?directory=${encodeURIComponent(dir)}`)).json();
    const saved = found.find((n) => n.title === 'How does BFS pick the next vertex?');
    expect(saved, JSON.stringify(found.map((n) => n.title))).toBeTruthy();
    expect(saved.body).toBe('**Q:** How does BFS pick the next vertex?\n\nBFS takes the oldest vertex from a queue [[Graph traversals]].');
  });

  test('a question missed a day ago is offered for review and asked again', async ({ page }) => {
    const id = `review-chat-${test.info().project.name}`;
    const note = await (await page.request.post('/api/notes', { data: { title: 'First in, first out', body: 'The oldest item leaves first.' } })).json();
    const question = `Which structure does BFS use (${test.info().project.name})?`;
    await page.request.put(`/api/chats/${id}`, {
      data: {
        mode: 'study',
        refs: [],
        messages: [
          { role: 'user', text: 'quiz me' },
          { role: 'assistant', text: `First question.\n\n${question}` },
          { role: 'user', text: 'a stack' },
          { role: 'assistant', text: '[[incorrect]] Not quite, it is a queue [n1].', sources: [{ n: 1, id: note.id, title: 'First in, first out' }], at: '2026-01-02T10:00:00Z' },
        ],
      },
    });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"First one: which structure?"}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const card = chat.locator('.chat-review');
    await expect(card).toContainText('Time to review');
    await card.locator('[data-chat="review"]').click();
    await expect(chat.locator('.msg.leo').last()).toContainText('First one: which structure?');
    const sent = asked[asked.length - 1];
    expect(sent.mode).toBe('study');
    expect(sent.refs).toContain(note.id);
    expect(sent.messages[sent.messages.length - 1].text).toContain(`${question} (last time I said: a stack)`);
    await expect.poll(async () => (await (await page.request.get('/api/review')).json()).some((m) => m.question === question)).toBe(false);
  });

  test('Felix shows what it looked at and its suggestions wait for the user', async ({ page }) => {
    const tag = test.info().project.name;
    const note = await (await page.request.post('/api/notes', { data: { title: `BFS facts ${tag}`, body: 'BFS takes the newest vertex.' } })).json();
    const lines = [
      { sources: [{ n: 1, id: note.id, title: note.title, folder: '', why: 'found by searching' }] },
      { step: 'Searched your notes for “BFS”', tool: 'search_notes', found: [note.title, 'Heaps'] },
      { step: `Suggested a change to “${note.title}”`, tool: 'edit_note', found: [] },
      { proposal: { kind: 'edit', note: note.id, title: note.title, find: 'newest', replace: 'oldest', why: 'a queue is first in, first out' } },
      { proposal: { kind: 'create', title: `Waiting lines ${tag}`, body: '## Waiting lines\n- first in, first out', folder: '' } },
      { t: 'Your note had BFS backwards [n1]; I suggested a fix and a new note.' },
      { done: true },
    ];
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('is my BFS note right?');
    await chat.locator('#chat-input').press('Enter');
    const answer = chat.locator('.msg.leo').last();
    const steps = answer.locator('.msg-step');
    await expect(steps).toHaveCount(2);
    await expect(steps.first()).toContainText('Searched your notes for “BFS”');
    await expect(steps.nth(1)).toContainText(`Suggested a change to “${note.title}”`);
    await expect(answer.locator('.msg-step-busy')).toHaveCount(0);
    await expect(steps.first().locator('.msg-step-count')).toHaveText('2');
    await expect(steps.first().locator('li').first()).toBeHidden();
    await steps.first().locator('summary').click();
    await expect(steps.first().locator('li')).toHaveText([note.title, 'Heaps']);
    const cards = answer.locator('.proposal');
    await expect(cards).toHaveCount(2);
    await expect(cards.first().locator('.proposal-old')).toHaveText('newest');
    await expect(cards.first().locator('.proposal-new')).toHaveText('oldest');
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the newest vertex.');
    await cards.first().locator('[data-chat="apply"]').click();
    await expect(cards.first().locator('.proposal-done')).toContainText('Applied');
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the oldest vertex.');
    await cards.nth(1).locator('[data-chat="apply"]').click();
    await expect(cards.nth(1).locator('.proposal-done')).toContainText('Made');
    const made = await (await page.request.get(`/api/search?q=${encodeURIComponent(`Waiting lines ${tag}`)}`)).json();
    expect(made.some((n) => n.title === `Waiting lines ${tag}`)).toBe(true);
    await page.reload();
    await page.locator('#chat-toggle').click();
    const again = page.locator('#chat .msg.leo').last().locator('.proposal');
    await expect(again.first().locator('.proposal-done')).toContainText('Applied');

    await again.first().locator('[data-chat="undo"]').click();
    await expect(page.locator('.toast')).toContainText(`Undid the change to “${note.title}”`);
    await expect(again.first().locator('[data-chat="apply"]')).toBeVisible();
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the newest vertex.');

    await again.nth(1).locator('[data-chat="undo"]').click();
    await expect(page.locator('.toast')).toContainText(`Moved “Waiting lines ${tag}” to the trash`);
    await expect(again.nth(1).locator('[data-chat="apply"]')).toBeVisible();
    const trashed = await (await page.request.get('/api/trash')).json();
    expect(trashed.some((t) => t.title === `Waiting lines ${tag}`)).toBe(true);

    await again.first().locator('[data-chat="apply"]').click();
    await expect(again.first().locator('.proposal-done')).toContainText('Applied');
    const current = await (await page.request.get(`/api/notes/${note.id}`)).json();
    await page.request.patch(`/api/notes/${note.id}`, { data: { body: `${current.body}\nEdited by hand.`, base: current.version } });
    await again.first().locator('[data-chat="undo"]').click();
    await expect(page.locator('.toast.bad')).toContainText('edited after this change');
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('BFS takes the oldest vertex.\nEdited by hand.');
  });

  test('Felix holds the prop for the tool he is using, and only that one', async ({ page }) => {
    await page.goto('/');
    const shown = await page.evaluate(() => {
      const out = {};
      for (const pose of ['tool-search', 'tool-open', 'tool-map', 'tool-edit', 'tool-create', 'idle']) {
        const holder = document.createElement('div');
        holder.innerHTML = window.leoChat.felix(80, pose);
        document.body.appendChild(holder);
        out[pose] = [...holder.querySelectorAll('.felix-tool')].filter((g) => getComputedStyle(g).opacity === '1').map((g) => g.getAttribute('class').replace('felix-tool ', ''));
        holder.remove();
      }
      return out;
    });
    expect(shown).toEqual({
      'tool-search': ['felix-tool-search'],
      'tool-open': ['felix-tool-open'],
      'tool-map': ['felix-tool-map'],
      'tool-edit': ['felix-tool-edit'],
      'tool-create': ['felix-tool-create'],
      idle: [],
    });
  });

  test('a file from this device is read for Felix, leaves the box once sent, and can be removed before', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Your file says heaps."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const attach = async (file) => {
      await chat.locator('[data-chat="attach"]').click();
      await expect(chat.locator('#chat-attach-menu')).toBeVisible();
      const chooser = page.waitForEvent('filechooser');
      await chat.locator('[data-chat="attach-file"]').click();
      await (await chooser).setFiles(file);
      await expect(chat.locator('#chat-attach-menu')).toBeHidden();
    };
    await attach({ name: 'week3.txt', mimeType: 'text/plain', buffer: Buffer.from('Heaps keep the minimum at the root.') });
    const chip = chat.locator('#chat-refs .file-card', { hasText: 'week3.txt' });
    await expect(chip).toBeVisible();
    await expect(chip.locator('.file-badge')).toHaveText('TXT');
    await expect(chip.locator('.file-page')).toContainText('Heaps keep the minimum at the root.');
    const card = await chip.boundingBox();
    expect(card.width).toBeGreaterThan(card.height);
    expect(card.height).toBeGreaterThan(90);
    await expect(chip).not.toHaveClass(/reading/);
    await chat.locator('#chat-input').fill('what does my file say?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Your file says heaps.');
    expect(asked[0].files.length).toBe(1);
    const files = await (await page.request.get(`/api/chats/${asked[0].chat}/files`)).json();
    expect(files.map((f) => f.name)).toEqual(['week3.txt']);
    expect(files[0].chars).toBe(35);
    await expect(chat.locator('.msg.user .file-card .file-name')).toHaveText('week3.txt');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);

    await chat.locator('#chat-input').fill('and where is the minimum?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo')).toHaveCount(2);
    await expect(chat.locator('.msg.user').last().locator('.file-card')).toHaveCount(0);
    expect(asked[1].files, 'Felix still has the file for questions after it').toEqual(asked[0].files);

    await attach({ name: 'draft.txt', mimeType: 'text/plain', buffer: Buffer.from('A draft not meant to go.') });
    const draft = chat.locator('#chat-refs .file-card', { hasText: 'draft.txt' });
    await expect(draft).not.toHaveClass(/reading/);
    await draft.locator('.file-x').click();
    await expect(draft).toHaveCount(0);
    await expect.poll(async () => (await (await page.request.get(`/api/chats/${asked[0].chat}/files`)).json()).map((f) => f.name)).toEqual(['week3.txt']);

    await page.reload();
    const listed = page.waitForResponse((r) => r.url().endsWith(`/api/chats/${asked[0].chat}/files`));
    await page.locator('#chat-toggle').click();
    await listed;
    await expect(chat.locator('.msg.user .file-card .file-name')).toHaveText('week3.txt');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);

    await attach({ name: 'song.mp3', mimeType: 'audio/mpeg', buffer: Buffer.from('ID3') });
    await expect(page.locator('.toast.bad')).toContainText('Felix could not read song.mp3');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);
  });

  test('the chat panel follows the size of the window', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'one browser is enough to resize');
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    for (const [width, expected] of [[390, 390], [768, 476], [1024, 560], [1440, 547], [2200, 760]]) {
      await page.setViewportSize({ width, height: 800 });
      await expect.poll(async () => Math.round((await chat.boundingBox()).width)).toBe(expected);
      const fits = await chat.evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
      expect(fits, `nothing spills sideways at ${width}px`).toBe(true);
      await expect(page.locator('#chat-input')).toBeInViewport();
      await expect(page.locator('#chat-send')).toBeInViewport();
      if (width >= 1200) {
        const panel = await chat.boundingBox();
        const list = await page.locator('main').boundingBox();
        expect(list.x + list.width, `the page moves over at ${width}px`).toBeLessThanOrEqual(panel.x + 1);
      }
    }
    await page.locator('#chat [data-chat="close"]').click();
    await expect.poll(async () => (await page.locator('main').boundingBox()).width).toBeGreaterThan(700);
  });

  test('with Felix beside the page, sheets and the note toolbar stay beside him', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the page only moves over on wide screens');
    await page.setViewportSize({ width: 1440, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Beside Felix', body: 'x' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = await page.locator('#chat').boundingBox();
    const bar = await page.locator('nav.actions').boundingBox();
    expect(bar.x + bar.width).toBeLessThanOrEqual(chat.x);
    await page.locator('[data-action="move"]').click();
    const sheet = await page.locator('.sheet').boundingBox();
    const side = await page.locator('#side').boundingBox();
    expect(sheet.x + sheet.width).toBeLessThanOrEqual(chat.x);
    expect(Math.abs(sheet.x + sheet.width / 2 - (side.x + side.width + chat.x) / 2)).toBeLessThan(30);
  });

  test('Felix can be dragged wider or narrower and keeps that width', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel takes the whole screen on phones');
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const settled = () => page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
    await settled();
    const before = await chat.boundingBox();
    const grip = page.locator('.chat-resize');
    const handle = await grip.boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, 300);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 - 120, 300, { steps: 6 });
    await page.mouse.up();
    const wider = await chat.boundingBox();
    expect(Math.abs(wider.width - (before.width + 120))).toBeLessThan(4);
    const main = await page.locator('main').boundingBox();
    expect(main.x + main.width).toBeLessThanOrEqual(wider.x + 1);

    await page.reload();
    await page.locator('#chat-toggle').click();
    await settled();
    expect(Math.abs((await chat.boundingBox()).width - wider.width)).toBeLessThan(2);

    await grip.focus();
    await page.keyboard.press('ArrowRight');
    expect(Math.abs((await chat.boundingBox()).width - (wider.width - 32))).toBeLessThan(2);

    const moved = await grip.boundingBox();
    await page.mouse.move(moved.x + moved.width / 2, 300);
    await page.mouse.down();
    await page.mouse.move(1400, 300, { steps: 4 });
    await page.mouse.up();
    expect((await chat.boundingBox()).width).toBe(420);

    await grip.dblclick();
    expect(Math.abs((await chat.boundingBox()).width - before.width)).toBeLessThan(2);
  });

  test('Felix can ask first, change notes himself, or only read, and remembers the choice', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Auto target ${test.info().project.name}`, body: 'BFS uses a queue.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const applied = { kind: 'edit', note: note.id, title: note.title, find: 'queue', replace: 'queue (first in, first out)', why: '', state: 'applied', before: 'BFS uses a queue.', after: 'v' };
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { step: `Changed “${note.title}”`, tool: 'edit_note', found: [] }, { proposal: applied }, { t: 'I changed it.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const access = chat.locator('#chat-access');
    await expect(access).toHaveText(/Ask first/);
    await access.click();
    await chat.locator('[data-chat="access-pick"][data-access="auto"]').click();
    await expect(access).toHaveText(/Auto/);
    await expect(chat.locator('#chat-access-menu')).toBeHidden();
    await chat.locator('#chat-input').fill('fix my bfs note');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.proposal-done')).toContainText('Applied');
    await expect(chat.locator('.proposal [data-chat="undo"]')).toBeVisible();
    await expect(chat.locator('.proposal [data-chat="apply"]')).toHaveCount(0);
    expect(asked[0].access).toBe('auto');

    await chat.locator('#chat-input').press('Shift+Tab');
    await expect(access).toHaveText(/Read only/);
    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat-access')).toHaveText(/Read only/);
    await page.locator('#chat-input').press('Shift+Tab');
    await expect(page.locator('#chat-access')).toHaveText(/Ask first/);
  });

  test('a message sent while a file is still being read waits for it, with Felix thinking', async ({ page }) => {
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    await page.route('**/api/chats/*/files', async (route) => {
      if (route.request().method() !== 'POST') return route.fulfill({ json: [] });
      await held;
      return route.fulfill({ status: 201, json: { id: 'doc-slow-1', name: 'slow.pdf', chars: 30, excerpt: 'Week 4: Graphs' } });
    });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"It covers graphs."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-chat="attach"]').click();
    const chooser = page.waitForEvent('filechooser');
    await chat.locator('[data-chat="attach-file"]').click();
    await (await chooser).setFiles({ name: 'slow.pdf', mimeType: 'application/pdf', buffer: Buffer.from('%PDF-1.4') });
    await expect(chat.locator('#chat-refs .file-card.reading')).toBeVisible();
    await chat.locator('#chat-input').fill('what is in it?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('#chat-refs .file-card')).toHaveCount(0);
    await expect(chat.locator('.msg.user .file-card.reading')).toContainText('slow.pdf');
    await expect(chat.locator('.msg.leo.pending')).toBeVisible();
    expect(asked).toHaveLength(0);
    release();
    await expect(chat.locator('.msg.leo').last()).toContainText('It covers graphs.');
    expect(asked[0].files).toEqual(['doc-slow-1']);
    await expect(chat.locator('.msg.user .file-card')).not.toHaveClass(/reading/);
    await expect(chat.locator('.msg.user .file-page')).toContainText('Week 4: Graphs');
  });

  test('moving to another note while Felix works keeps his answer and changes on the first note', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    const make = async (title, body, directory) => {
      await page.request.post('/api/dirs', { data: { path: directory } });
      return (await page.request.post('/api/notes', { data: { title, body, directory } })).json();
    };
    const first = await make(`Asked about ${tag}`, 'BFS uses a stack.', `switch-a-${tag}`);
    const other = await make(`Opened later ${tag}`, 'Untouched text.', `switch-b-${tag}`);
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await held;
      const proposal = { kind: 'edit', note: first.id, title: first.title, find: 'stack', replace: 'queue', why: 'BFS is first in, first out' };
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: [{ sources: [] }, { proposal }, { t: 'Your note says stack; BFS uses a queue.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${first.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('is this right?');
    await chat.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(1);
    await page.evaluate((id) => { location.hash = `#/n/${id}`; }, other.id);
    await expect(page.locator('#title')).toHaveText(other.title);
    release();
    await expect(chat.locator('.msg.leo').last()).toContainText('BFS uses a queue.');
    expect(asked[0].note).toBe(first.id);
    await chat.locator('.proposal [data-chat="apply"]').click();
    await expect(chat.locator('.proposal-done')).toContainText('Applied');
    const body = async (id) => (await (await page.request.get(`/api/notes/${id}`)).json()).body;
    expect(await body(first.id)).toBe('BFS uses a queue.');
    expect(await body(other.id)).toBe('Untouched text.');
    await expect(page.locator('#title')).toHaveText(other.title);
    await chat.locator('[data-chat="save"]').click();
    await expect(page.locator('.toast')).toContainText(`Saved as a note in switch-a-${tag}`);
  });

  test('the note picker keeps its search box while typing, until a note is chosen', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/notes', { data: { title: `Dijkstra picker ${tag}`, body: 'x' } });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-chat="attach"]').click();
    await chat.locator('[data-chat="attach-note"]').click();
    const search = chat.locator('#chat-pick-search');
    await expect(search).toBeFocused();
    await search.pressSequentially(`dijkstra picker ${tag}`, { delay: 30 });
    await expect(search).toHaveValue(`dijkstra picker ${tag}`);
    await expect(search).toBeFocused();
    await expect(chat.locator('.chat-pick-row').first()).toContainText(`Dijkstra picker ${tag}`);
    await chat.locator('.chat-pick-row').first().click();
    await expect(chat.locator('#chat-refs .chat-ref')).toContainText(`Dijkstra picker ${tag}`);
  });

  test('says plainly when no AI is set up', async ({ page }) => {
    await page.route('**/api/chat', (route) => route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"error":"no AI for writing is chosen — type :settings in leo and pick one under writing"}\n' }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('hello');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('.msg-error')).toContainText(':settings');
    await page.locator('#chat [data-chat="close"]').click();
    await expect(page.locator('#chat')).toBeHidden();
  });
});

test.describe('settings', () => {
  test('changes the writing AI and stores a key without ever showing it again', async ({ page }) => {
    await page.goto('/');
    await goPlace(page, 'settings');
    await expect(page).toHaveURL(/#\/settings$/);
    const writing = page.locator('[data-task-card="writing"]');
    await writing.locator('select[data-set="provider"]').selectOption('gemini');
    await expect(page.locator('.toast')).toContainText('Writing now uses Gemini');
    await expect(writing.locator('select[data-set="provider"]')).toHaveValue('gemini');
    const picker = writing.locator('.picker-button');
    await picker.click();
    await expect(writing.locator('.picker-menu')).toBeVisible();
    await writing.locator('.pick-model[data-model="gemini-3.1-flash-lite"]').click();
    await expect(page.locator('.toast')).toContainText('Model set to gemini-3.1-flash-lite');
    await expect(picker).toContainText('gemini-3.1-flash-lite');
    await writing.locator('[data-key-input="gemini"]').fill('gm-fake-key-123');
    await writing.locator('[data-action="set-key"]').click();
    await expect(page.locator('.toast')).toContainText('The Gemini key is stored on this computer');
    await expect(writing).toContainText('Stored on your computer');
    await expect(writing.locator('[data-key-input="gemini"]')).toHaveValue('');
    expect(await (await page.request.get('/api/settings')).text()).not.toContain('gm-fake-key-123');
    expect(await page.content()).not.toContain('gm-fake-key-123');
    await writing.locator('[data-action="remove-key"]').click();
    await page.locator('[data-action="remove-key-now"]').click();
    await expect(page.locator('.toast')).toContainText('The Gemini key is removed');
    await expect(writing).toContainText('Not added');
    const backup = page.locator('select[data-set="auto_push"]');
    await backup.selectOption('when_idle');
    await expect(page.locator('.toast')).toContainText('Backing up when idle');
    const bad = await page.request.post('/api/settings', { data: { set: 'provider', task: 'writing', value: 'parakeet' } });
    expect(bad.status()).toBe(400);

    await writing.locator('select[data-set="provider"]').selectOption('codex');
    await expect(page.locator('.toast')).toContainText('Writing now uses Codex');
    await writing.locator('.picker-button').click();
    await expect(writing.locator('.pick-chip')).toHaveText(['Low', 'Medium', 'High', 'Extra high']);
    await expect(writing.locator('.pick-chip.on')).toHaveText('Medium');
    await writing.locator('.pick-chip[data-effort="high"]').click();
    await expect(page.locator('.toast')).toContainText('Effort set to high.');
    await expect(writing.locator('.picker-button')).toContainText('high effort');
    await writing.locator('.picker-button').click();
    await page.keyboard.press('Escape');
    await expect(writing.locator('.picker-menu')).toBeHidden();
    await expect(page).toHaveURL(/#\/settings$/);
    await writing.locator('.picker-button').click();
    await writing.locator('.pick-chip[data-effort="medium"]').click();
    await expect(page.locator('.toast')).toContainText('Effort set to medium.');
    await writing.locator('select[data-set="provider"]').selectOption('gemini');
  });
});

test.describe('folders', () => {
  test('folders hold folders: the sidebar shows them as a tree and makes one inside another', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the sidebar is for wide screens');
    const top = `tree-${Date.now().toString(36)}`;
    await page.request.post('/api/dirs', { data: { path: `${top}/week1` } });
    await page.goto('/');
    const side = page.locator('#side');
    const row = (path) => side.locator(`.side-row[data-action="open-folder"][data-dir="${path}"]`);
    await expect(row(top)).toBeVisible();
    await expect(row(`${top}/week1`)).toHaveCount(0);
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`)).toBeVisible();
    await row(`${top}/week1`).hover();
    await side.locator(`.side-sub[data-parent="${top}/week1"]`).click();
    await expect(page.locator('.sheet h3')).toHaveText('New folder in week1');
    await page.locator('#folder-name').fill('lab');
    await page.locator('[data-action="create-folder"]').click();
    await expect.poll(() => decodeURIComponent(page.url())).toMatch(new RegExp(`#/f/${top}/week1/lab$`));
    await expect(row(`${top}/week1/lab`)).toHaveAttribute('aria-current', 'page');
    await page.reload();
    await expect(row(`${top}/week1`), 'open folders stay open').toBeVisible();
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await page.goto('/');
    await expect(row(`${top}/week1`)).toHaveCount(0);
    await page.goto(`/#/f/${top}/week1/lab`);
    await expect(row(`${top}/week1/lab`), 'the way to the open folder unfolds').toBeVisible();
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`), 'an arrow closes even on the way to the open folder').toHaveCount(0);
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`), 'and opens again').toBeVisible();
    await page.goto(`/#/f/${top}`);
    await expect(row(`${top}/week1`)).toBeVisible();
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`), 'the folder you are in can fold its subfolders').toHaveCount(0);
    await side.locator(`.side-twist[data-dir="${top}"]`).click();
    await expect(row(`${top}/week1`)).toBeVisible();
    await page.request.post('/api/dirs', { data: { path: `${top}-other` } });
    await page.goto(`/#/f/${top}-other`);
    await expect(row(`${top}-other`)).toHaveAttribute('aria-current', 'page');
    await expect(row(`${top}/week1`), 'opened folders stay open when another folder is opened').toBeVisible();
  });

  test('a new folder is made on the first try and opens', async ({ page }) => {
    const name = `fresh-${test.info().project.name}-${Date.now().toString(36)}`;
    await page.goto('/');
    if (await page.locator('#side').isHidden()) {
      await page.locator('#menu').click();
      await page.locator('.sheet [data-action="new-folder"]').click();
    } else {
      await page.locator('#side .side-group [data-action="new-folder"]').click();
    }
    await page.locator('#folder-name').fill(name);
    await page.locator('[data-action="create-folder"]').click();
    await expect(page).toHaveURL(new RegExp(`#/f/${name}$`));
    await expect(page.locator('.toast.bad')).toHaveCount(0);
    await expect(page.locator('.scrim')).toHaveCount(0);
  });


  test('chosen notes and folders move to the trash after asking', async ({ page }) => {
    const tag = test.info().project.name;
    const top = `pick-${tag}`;
    await page.request.post('/api/dirs', { data: { path: top } });
    await page.request.post('/api/dirs', { data: { path: `${top}/week1` } });
    const make = async (title, directory) => (await page.request.post('/api/notes', { data: { title, body: 'x', directory } })).json();
    const loose = await make('Loose note', top);
    const kept = await make('Keep me', top);
    const inside = await make('Inside week1', `${top}/week1`);
    await page.goto(`/#/f/${top}`);
    await expect(page.locator('.card', { hasText: 'Keep me' })).toBeVisible();

    await page.locator('[data-action="folder-select"]').click();
    await expect(page.locator('.select-bar')).toContainText('0 selected');
    await expect(page.locator('[data-action="folder-trash"]')).toBeDisabled();
    await page.locator('.folder', { hasText: 'week1' }).click();
    await page.locator('.pick-card', { hasText: 'Loose note' }).click();
    for (const item of [page.locator('.folder', { hasText: 'week1' }), page.locator('.pick-card', { hasText: 'Loose note' })]) {
      const box = await item.boundingBox();
      const tick = await item.locator('.pick-box').boundingBox();
      expect(box.x + box.width - (tick.x + tick.width), 'the checkbox sits at the right edge').toBeLessThan(24);
    }
    await expect(page.locator('.select-bar')).toContainText('2 selected');
    await expect(page).toHaveURL(new RegExp(`#/f/${top}$`));

    await page.locator('[data-action="folder-trash"]').click();
    await expect(page.locator('.sheet h3')).toHaveText('Move 1 note and 1 folder (with 1 note inside) to the trash?');
    await page.locator('[data-action="close"]').click();
    expect((await page.request.get(`/api/notes/${loose.id}`)).ok()).toBe(true);

    await page.locator('[data-action="folder-trash"]').click();
    await page.locator('[data-action="folder-trash-now"]').click();
    await expect(page.locator('.toast')).toContainText('Moved 2 notes and 1 folder to the trash');
    await expect(page.locator('.select-bar')).toHaveCount(0);
    await expect(page.locator('.card', { hasText: 'Keep me' })).toBeVisible();
    await expect(page.locator('.card', { hasText: 'Loose note' })).toHaveCount(0);
    await expect(page.locator('.folder', { hasText: 'week1' })).toHaveCount(0);
    const trashed = (await (await page.request.get('/api/trash')).json()).map((t) => t.id);
    expect(trashed).toEqual(expect.arrayContaining([loose.id, inside.id]));
    expect((await page.request.get(`/api/notes/${kept.id}`)).ok()).toBe(true);

    await page.locator('[data-action="folder-select"]').click();
    await page.locator('[data-folder-all]').check();
    await expect(page.locator('.select-bar')).toContainText('1 selected');
    await page.keyboard.press('Escape');
    await expect(page.locator('.select-bar')).toHaveCount(0);
    await expect(page.locator('.fabs')).toHaveCount(1);
  });

  test('undo in the toast brings back what was just moved to the trash', async ({ page }) => {
    const top = `undo-${test.info().project.name}`;
    for (const dir of [top, `${top}/week2`, `${top}/week2/empty`]) await page.request.post('/api/dirs', { data: { path: dir } });
    const inside = await (await page.request.post('/api/notes', { data: { title: 'Week 2 notes', body: 'x', directory: `${top}/week2` } })).json();
    await page.goto(`/#/f/${top}`);
    await page.locator('[data-action="folder-select"]').click();
    await page.locator('.folder', { hasText: 'week2' }).click();
    await page.locator('[data-action="folder-trash"]').click();
    await page.locator('[data-action="folder-trash-now"]').click();
    await expect(page.locator('.folder', { hasText: 'week2' })).toHaveCount(0);
    await page.locator('.toast button', { hasText: 'Undo' }).click();
    await expect(page.locator('.toast')).toContainText('Brought back');
    await expect(page.locator('.folder', { hasText: 'week2' })).toBeVisible();
    expect((await page.request.get(`/api/notes/${inside.id}`)).ok()).toBe(true);
    const dirs = await (await page.request.get(`/api/dirs?parent=${encodeURIComponent(`${top}/week2`)}`)).json();
    expect(dirs.map((d) => d.name)).toContain('empty');
  });
});

test.describe('pictures', () => {
  async function pastePicture(page, target, name = 'image.png') {
    await page.evaluate(async ({ target, name }) => {
      const canvas = document.createElement('canvas');
      canvas.width = 320;
      canvas.height = 200;
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = '#4f46e5';
      ctx.fillRect(0, 0, 320, 200);
      ctx.fillStyle = '#fff';
      ctx.fillRect(40, 40, 120, 80);
      const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
      const data = new DataTransfer();
      data.items.add(new File([blob], name, { type: 'image/png' }));
      const el = document.querySelector(target);
      el.focus();
      el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    }, { target, name });
  }

  test('a pasted picture goes into the note and shows in full', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Pictured ${test.info().project.name}`, body: 'Before the picture.' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Before the picture.');
    await pastePicture(page, '#doc');
    const img = page.locator('#doc img.note-img');
    await expect(img).toBeVisible();
    await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
    await expect(page.locator('.toast')).toContainText('Picture added');
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toMatch(/^Before the picture\.\n\n!\[Pasted picture\]\(attachments\/\d{8}-\d{6}-pasted-1\.png\)$/);
    await page.reload();
    await expect.poll(() => page.locator('#doc img.note-img').evaluate((el) => el.naturalWidth)).toBe(320);
  });

  test('the Picture button adds a picture from the device', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Chosen ${test.info().project.name}`, body: 'x' } })).json();
    await page.goto(`/#/n/${note.id}`);
    const chooser = page.waitForEvent('filechooser');
    await page.locator('[data-action="note-picture"]').click();
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMAAAAwqAQXxT7KxAAAAAElFTkSuQmCC', 'base64');
    await (await chooser).setFiles({ name: 'Heap diagram.png', mimeType: 'image/png', buffer: png });
    await expect(page.locator('#doc img.note-img')).toHaveAttribute('alt', 'Heap diagram');
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toContain('![Heap diagram](attachments/');
  });

  test('a picture pasted on a folder page opens the upload sheet with it', async ({ page }) => {
    await page.goto('/');
    await pastePicture(page, 'body');
    await expect(page.locator('.upload-file')).toContainText('pasted-1.png');
    await expect(page.locator('#upload-go')).toBeEnabled();
    await pastePicture(page, '#upload-title', 'board.png');
    await expect(page.locator('.upload-file')).toHaveCount(2);
  });

  test('a picture pasted into Felix shows as a thumbnail and goes with the message', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chats/*/files', (route) => {
      if (route.request().method() !== 'POST') return route.fulfill({ json: [] });
      return route.fulfill({ status: 201, json: { id: 'doc-pic-1', name: route.request().postDataJSON().name, chars: 40 } });
    });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"That diagram shows a heap."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await pastePicture(page, '#chat-input');
    const chip = page.locator('#chat #chat-refs .file-card');
    await expect(chip).toContainText('pasted-1.png');
    await expect(chip.locator('.file-peek img')).toBeVisible();
    await page.locator('#chat-input').fill('what is this?');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat .msg.leo').last()).toContainText('That diagram shows a heap.');
    await expect(page.locator('#chat .msg.user .file-card img')).toHaveAttribute('src', /^data:image\/jpeg;base64,/);
    expect(asked[0].files).toEqual(['doc-pic-1']);
    await expect(chip).toHaveCount(0);
  });
});

test.describe('large libraries', () => {
  test('a big folder loads in pages as you scroll, and Select all takes every note', async ({ page }) => {
    test.setTimeout(90000);
    const dir = `big-${test.info().project.name}-${test.info().retry}-${Date.now().toString(36)}`;
    await page.request.post('/api/dirs', { data: { path: dir } });
    for (let i = 0; i < 230; i += 10) {
      await Promise.all(Array.from({ length: 10 }, (_, j) => page.request.post('/api/notes', { data: { title: `Big note ${String(i + j).padStart(3, '0')}`, body: '- [x] done\n- [ ] not yet', directory: dir } })));
    }
    const listed = await page.request.get(`/api/notes?dir=${dir}&limit=200&brief=true`);
    expect(listed.headers()['x-total']).toBe('230');
    await page.goto(`/#/f/${dir}`);
    const cards = page.locator('.cards .card');
    await expect(cards).toHaveCount(200);
    await expect(page.locator('.section-title', { hasText: 'Notes' })).toHaveText('Notes · 230');
    await expect(cards.first().locator('.chip.progress')).toContainText('1/2');
    await page.locator('#more-notes').scrollIntoViewIfNeeded();
    await expect(cards).toHaveCount(230);
    await expect(page.locator('#more-notes')).toHaveCount(0);

    await page.evaluate(() => window.scrollTo(0, 0));
    await page.reload();
    await expect(cards).toHaveCount(200);
    await page.locator('[data-action="folder-select"]').click();
    await page.locator('[data-folder-all]').check();
    await expect(page.locator('.select-bar')).toContainText('230 selected');
  });
});

test.describe('readability', () => {
  async function lowContrast(page) {
    return page.evaluate(() => {
      const parse = (s) => {
        const m = /rgba?\(([^)]+)\)/.exec(s || '');
        if (!m) return [0, 0, 0, 0];
        const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
        return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
      };
      const over = (top, under) => {
        const a = top[3];
        return [0, 1, 2].map((i) => top[i] * a + under[i] * (1 - a)).concat(1);
      };
      const lum = (c) => {
        const [r, g, b] = c.slice(0, 3).map((v) => {
          v /= 255;
          return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      };
      const backdrop = (el) => {
        const layers = [];
        for (let at = el; at; at = at.parentElement) {
          const c = parse(getComputedStyle(at).backgroundColor);
          if (c[3] > 0) layers.push(c);
          if (c[3] >= 1) break;
        }
        let colour = parse(getComputedStyle(document.body).backgroundColor);
        if (colour[3] < 1) colour = [255, 255, 255, 1];
        for (const layer of layers.reverse()) colour = over(layer, colour);
        return colour;
      };
      const fade = (el) => {
        let o = 1;
        for (let at = el; at; at = at.parentElement) o *= Number(getComputedStyle(at).opacity);
        return o;
      };
      const found = [];
      for (const el of document.querySelectorAll('body *')) {
        if (![...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim())) continue;
        if (el.closest('[disabled], [aria-disabled="true"], .skeleton, canvas, svg, option, select, [hidden]')) continue;
        const style = getComputedStyle(el);
        const box = el.getBoundingClientRect();
        if (style.visibility !== 'visible' || box.width < 1 || box.height < 1 || style.display === 'none') continue;
        const seen = fade(el);
        if (seen < 0.05) continue;
        const back = backdrop(el);
        const fg = over(parse(style.color).slice(0, 3).concat(parse(style.color)[3] * seen), back);
        const [a, b] = [lum(fg), lum(back)].sort((x, y) => y - x);
        const ratio = (a + 0.05) / (b + 0.05);
        if (ratio < 3) found.push(`${ratio.toFixed(2)} ${el.tagName.toLowerCase()}.${[...el.classList].join('.')} "${el.textContent.trim().slice(0, 40)}"`);
      }
      return [...new Set(found)];
    });
  }

  for (const scheme of ['light', 'dark']) {
    test(`every page reads clearly in ${scheme} mode`, async ({ page }) => {
      test.skip(test.info().project.name !== 'desktop', 'colours do not depend on the screen size');
      test.setTimeout(90000);
      await page.emulateMedia({ colorScheme: scheme });
      await page.request.post('/api/dirs', { data: { path: 'readable' } });
      const note = await (await page.request.post('/api/notes', { data: { title: 'Readable note', body: '## Heading\n- [x] done\n- [ ] open\n\n> a quote\n\n`code` and a [link](https://example.com)', directory: 'readable' } })).json();
      const gone = await (await page.request.post('/api/notes', { data: { title: 'Readable gone', body: 'x' } })).json();
      await page.request.delete(`/api/notes/${gone.id}`);
      const problems = {};
      const look = async (name) => {
        await page.waitForTimeout(300);
        const low = await lowContrast(page);
        if (low.length) problems[name] = low;
      };
      for (const [name, hash] of [['folders', '#/'], ['folder', '#/f/readable'], ['note', `#/n/${note.id}`], ['search', '#/search/readable'], ['trash', '#/trash'], ['settings', '#/settings'], ['storage', '#/settings/storage'], ['record', '#/record'], ['map', '#/map']]) {
        await page.goto(`/${hash}`);
        await look(name);
      }
      await page.goto('/');
      if (await page.locator('#menu').isVisible()) {
        await page.locator('#menu').click();
        await look('menu');
        await page.keyboard.press('Escape');
      }
      await control(page, 'upload').click();
      await look('upload');
      await page.keyboard.press('Escape');
      await page.locator('#chat-toggle').click();
      await look('felix');
      expect(problems).toEqual({});
    });
  }
});

test.describe('background work', () => {
  test('work still going on shows its progress on every page until it is done', async ({ page }) => {
    let tasks = [
      { kind: 'upload', label: 'Making a note from slides.pdf', step: 'Writing the note', done: 1, total: 4, href: '#/' },
      { kind: 'recording', label: 'Writing the notes from a recording', step: 'Transcribing the recording', done: 0, total: 0, href: '#/record' },
    ];
    await page.route('**/api/activity', (route) => route.fulfill({ json: { tasks } }));
    await page.route('**/api/record', (route) => route.fulfill({ json: { available: true, local: false, job: null } }));
    await page.goto('/');
    await page.reload();
    const tray = page.locator('#activity');
    await expect(tray).toBeVisible();
    await expect(tray.locator('.activity-head')).toContainText('2 things in the background');
    const upload = tray.locator('.activity-row', { hasText: 'slides.pdf' });
    await expect(upload).toContainText('Writing the note · 1/4');
    expect(await upload.locator('.activity-bar i').evaluate((el) => el.style.width)).toBe('25%');
    await expect(tray.locator('.activity-row', { hasText: 'recording' }).locator('.activity-bar')).toHaveClass(/busy/);

    await tray.locator('.activity-head').click();
    await expect(tray.locator('.activity-row')).toHaveCount(0);
    await expect(tray.locator('.activity-fold')).toHaveText('Show');
    await tray.locator('.activity-head').click();

    await tray.locator('.activity-row', { hasText: 'recording' }).click();
    await expect(page).toHaveURL(/#\/record$/);
    await expect(tray.locator('.activity-row')).toHaveCount(1, { timeout: 4000 });
    await expect(tray).toContainText('slides.pdf');

    tasks = [];
    await expect(tray).toBeHidden({ timeout: 5000 });
  });
});

test.describe('trash', () => {
  test('deletes one note, the chosen notes, or everything for good, and restores the chosen ones', async ({ page }) => {
    const tag = test.info().project.name;
    const made = [];
    for (const n of [1, 2, 3, 4]) {
      const note = await (await page.request.post('/api/notes', { data: { title: `Trashed ${n} ${tag}`, body: 'x' } })).json();
      made.push(note);
      expect((await page.request.delete(`/api/notes/${note.id}`)).ok()).toBe(true);
    }
    await page.goto('/#/trash');
    const row = (n) => page.locator('.trash-row', { hasText: `Trashed ${n} ${tag}` });
    await expect(row(1)).toBeVisible();

    await row(1).locator('[data-action="trash-forget"]').click();
    await expect(page.locator('.sheet h3')).toContainText(`Trashed 1 ${tag}`);
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(page.locator('.toast')).toContainText('Deleted 1 note for good');
    await expect(row(1)).toHaveCount(0);

    await page.locator('[data-action="trash-select"]').click();
    await expect(page.locator('.select-bar')).toContainText('0 selected');
    await expect(page.locator('[data-action="trash-delete-picked"]')).toBeDisabled();
    await row(2).click();
    await expect(page.locator('.select-bar')).toContainText('1 selected');
    await page.locator('[data-action="trash-restore-picked"]').click();
    await expect(page.locator('.toast')).toContainText('Restored 1 note');
    expect((await page.request.get(`/api/notes/${made[1].id}`)).ok()).toBe(true);
    await expect(row(2)).toHaveCount(0);

    await page.locator('[data-action="trash-select"]').click();
    await page.locator('[data-trash-all]').check();
    await expect(page.locator('.trash-row.picked')).toHaveCount(await page.locator('.trash-row').count());
    await page.locator('[data-action="trash-delete-picked"]').click();
    await page.locator('[data-action="close"]').click();
    await expect(row(3)).toBeVisible();
    await page.locator('[data-action="trash-delete-picked"]').click();
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(page.locator('.empty')).toContainText('The trash is empty');
    await expect(page.locator('.select-bar')).toHaveCount(0);
    expect((await (await page.request.get('/api/trash')).json()).length).toBe(0);

    const again = await (await page.request.post('/api/notes', { data: { title: `Trashed 5 ${tag}`, body: 'x' } })).json();
    await page.request.delete(`/api/notes/${again.id}`);
    await page.goto('/#/');
    await page.goto('/#/trash');
    await page.locator('[data-action="trash-empty"]').click();
    await page.locator('[data-action="trash-delete-now"]').click();
    await expect(page.locator('.empty')).toContainText('The trash is empty');
  });
});

test.describe('storage', () => {
  test('shows what leo keeps and deletes only what was chosen, after asking', async ({ page }) => {
    const tag = test.info().project.name;
    for (const n of [1, 2]) {
      const put = await page.request.put(`/api/chats/storage-${tag}-${n}`, { data: { mode: 'chat', refs: [], messages: [{ role: 'user', text: `storage chat ${tag} ${n}` }] } });
      expect(put.ok()).toBe(true);
    }
    const note = await (await page.request.post('/api/notes', { data: { title: `Throwaway ${tag}`, body: 'x' } })).json();
    expect((await page.request.delete(`/api/notes/${note.id}`)).ok()).toBe(true);

    await page.goto('/#/settings');
    await page.locator('main [data-action="storage"]').click();
    await expect(page).toHaveURL(/#\/settings\/storage$/);
    await expect(page.locator('.store-big')).toContainText(/KB|MB|bytes/);
    const chats = page.locator('details.store-area[data-area="chats"]');
    await chats.locator('summary').click();
    const first = chats.locator('.store-item', { hasText: `storage chat ${tag} 1` });
    await expect(first).toBeVisible();
    const deleteSelected = chats.locator('[data-act="delete"]');
    await expect(deleteSelected).toBeDisabled();
    await first.locator('input').check();
    await expect(deleteSelected).toContainText('(1)');
    await deleteSelected.click();
    await expect(page.locator('.sheet')).toContainText('deleted for good');
    await page.locator('[data-action="storage-go"]').click();
    await expect(page.locator('.toast')).toContainText('Deleted 1 chat.');
    await expect(chats).toHaveAttribute('open', '');
    await expect(chats.locator('.store-item', { hasText: `storage chat ${tag} 1` })).toHaveCount(0);
    await expect(chats.locator('.store-item', { hasText: `storage chat ${tag} 2` })).toBeVisible();

    const trash = page.locator('details.store-area[data-area="trash"]');
    await trash.locator('summary').click();
    await expect(trash).toContainText(`Throwaway ${tag}`);
    await trash.locator('[data-act="empty"]').click();
    await page.locator('[data-action="close"]').click();
    await expect(trash).toContainText(`Throwaway ${tag}`);
    await trash.locator('[data-act="empty"]').click();
    await page.locator('[data-action="storage-go"]').click();
    await expect(page.locator('.toast')).toContainText('Emptied the trash');
    expect((await (await page.request.get('/api/trash')).json()).length).toBe(0);
    await expect(page.locator('details.store-area[data-area="notes"] [data-action="storage-act"]')).toHaveCount(0);
    await page.locator('#back').click();
    await expect(page).toHaveURL(/#\/settings$/);
  });
});

test.describe('signed-in browsers', () => {
  test('opening the link again in the same browser does not add another sign-in', async ({ page }) => {
    const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
    const count = async () => (await (await page.request.get('/api/sessions')).json()).sessions.length;
    const before = await count();
    for (let i = 0; i < 3; i++) {
      await page.goto(`/?token=${token}`);
      await expect(page.locator('#app')).not.toBeEmpty();
    }
    expect(await count()).toBe(before);
    await page.goto('/#/settings/storage');
    const mine = page.locator('.session-row', { hasText: 'This browser' });
    await expect(mine).toContainText('on this computer');
  });
});

test.describe('storage of pictures', () => {
  test('pictures are listed with the notes that use them, and unused ones can be deleted', async ({ page }) => {
    const tag = test.info().project.name;
    const data = await page.evaluate(() => {
      const c = document.createElement('canvas');
      c.width = 64;
      c.height = 48;
      c.getContext('2d').fillRect(0, 0, 30, 20);
      return c.toDataURL('image/png').split(',')[1];
    });
    const used = (await (await page.request.post('/api/images', { data: { name: `used-${tag}.png`, data } })).json()).path;
    const spare = (await (await page.request.post('/api/images', { data: { name: `spare-${tag}.png`, data } })).json()).path;
    await page.request.post('/api/notes', { data: { title: `Pictured ${tag} for storage`, body: `![board](${used})` } });
    await page.goto('/#/settings/storage');
    const pictures = page.locator('details.store-area[data-area="pictures"]');
    await expect(pictures).toContainText('Pictures in notes');
    await pictures.locator('summary').click();
    const name = (p) => p.split('/').pop();
    await expect(pictures.locator('.store-item', { hasText: name(used) })).toContainText(`In “Pictured ${tag} for storage”`);
    await expect(pictures.locator('.store-item', { hasText: name(spare) })).toContainText('Not in any note');
    await pictures.locator('[data-act="unused"]').click();
    await expect(page.locator('.sheet')).toContainText('no note shows');
    await page.locator('[data-action="storage-go"]').click();
    await expect(page.locator('.toast')).toContainText(/Deleted \d+ pictures?\./);
    await expect(pictures.locator('.store-item', { hasText: name(spare) })).toHaveCount(0);
    await expect(pictures.locator('.store-item', { hasText: name(used) })).toBeVisible();
    expect((await page.request.get(`/api/image?path=${encodeURIComponent(used)}`)).ok()).toBe(true);
    expect((await page.request.get(`/api/image?path=${encodeURIComponent(spare)}`)).status()).toBe(404);
  });
});

test.describe('export and this browser', () => {
  test('exports a zip with the parts chosen', async ({ page }) => {
    await page.goto('/#/settings/storage');
    const link = page.locator('#export-link');
    await expect(link).toHaveAttribute('href', '/api/export?uploads=true&chats=true&trash=false');
    await page.locator('[data-export="chats"]').uncheck();
    await page.locator('[data-export="trash"]').check();
    await expect(link).toHaveAttribute('href', '/api/export?uploads=true&chats=false&trash=true');
    const zip = await page.request.get('/api/export?uploads=false&chats=true&trash=false');
    expect(zip.headers()['content-type']).toBe('application/zip');
    expect(zip.headers()['content-disposition']).toMatch(/^attachment; filename="leo-export-\d{4}-\d{2}-\d{2}\.zip"; filename\*=UTF-8''leo-export-\d{4}-\d{2}-\d{2}\.zip$/);
    const bytes = await zip.body();
    expect(bytes.subarray(0, 2).toString()).toBe('PK');
    expect(bytes.includes(Buffer.from('leo/README.txt'))).toBe(true);
    expect(bytes.includes(Buffer.from('serve-token'))).toBe(false);

    await expect(page.locator('.store-browser')).toHaveCount(0);
  });

  test('Felix is the tab icon, even before signing in', async ({ page, playwright }) => {
    await expect(page.locator('link[rel="icon"]')).toHaveAttribute('href', '/favicon.svg');
    const stranger = await playwright.request.newContext({ baseURL: 'http://127.0.0.1:31831' });
    const icon = await stranger.get('/favicon.svg');
    expect(icon.status()).toBe(200);
    expect(icon.headers()['content-type']).toBe('image/svg+xml');
    expect(await icon.text()).toContain('#b4cfe7');
    expect((await stranger.get('/api/notes')).status()).toBe(401);
    await stranger.dispose();
  });
});

test.describe('advanced settings', () => {
  test('lists signed-in browsers, signs one out, and makes a new link', async ({ page, browser }) => {
    const token = fs.readFileSync(path.join(process.env.LEO_BROWSER_HOME, 'serve-token'), 'utf8').trim();
    const other = await browser.newContext({ userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Version/18.0 Mobile/15E148 Safari/604.1' });
    const phone = await other.newPage();
    await phone.goto(`/?token=${token}`);
    await expect(phone.locator('#app')).not.toBeEmpty();
    expect((await phone.request.get('/api/notes')).status()).toBe(200);

    await page.goto('/#/settings/storage');
    const card = page.locator('.store-sessions');
    await expect(card.locator('.session-row').first()).toContainText('This browser');
    const iphone = card.locator('.session-row', { hasText: 'Safari on iPhone' }).first();
    await iphone.locator('[data-action="session-end"]').click();
    await expect(page.locator('.sheet h3')).toHaveText('Sign out Safari on iPhone?');
    await page.locator('[data-action="session-end-now"]').click();
    await expect(page.locator('.toast')).toContainText('Signed out 1 browser');
    expect((await phone.request.get('/api/notes')).status()).toBe(401);

    await card.locator('[data-action="session-new-link"]').click();
    await page.locator('[data-action="session-new-link-now"]').click();
    const link = await page.locator('#new-link').inputValue();
    expect(link).toMatch(/^http:\/\/127\.0\.0\.1:31831\/\?token=[0-9a-f]{32}$/);
    expect(link).not.toContain(token);
    await page.locator('[data-action="close"]').click();
    expect((await page.request.get('/api/notes')).status()).toBe(200);
    await phone.goto(`/?token=${token}`);
    await expect(phone.locator('body')).toContainText('This page needs its link');
    await phone.goto(link.replace('http://127.0.0.1:31831', ''));
    expect((await phone.request.get('/api/notes')).status()).toBe(200);
    await other.close();
  });

  test('chooses how long the trash and chats are kept, asking before deleting sooner', async ({ page }) => {
    await page.goto('/#/settings/storage');
    const trash = page.locator('select[data-keep="trash_days"]');
    const chats = page.locator('select[data-keep="chat_days"]');
    await expect(trash).toHaveValue('30');
    await expect(chats).toHaveValue('forever');
    const kept = async () => {
      const k = await (await page.request.get('/api/keep')).json();
      return [k.trash_days, k.chat_days];
    };
    await trash.selectOption('forever');
    await expect.poll(kept).toEqual([null, null]);
    await chats.selectOption('30');
    await expect(page.locator('.sheet h3')).toHaveText('Keep chats with Felix for 30 days?');
    await page.locator('[data-action="keep-cancel"]').click();
    await expect(chats).toHaveValue('forever');
    expect(await kept()).toEqual([null, null]);
    await chats.selectOption('90');
    await page.locator('[data-action="keep-now"]').click();
    await expect.poll(kept).toEqual([null, 90]);
    await expect(chats).toHaveValue('90');
    await page.goto('/#/trash');
    await page.request.post('/api/keep', { data: { trash_days: 7, chat_days: null } });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Keep check', body: 'x' } })).json();
    await page.request.delete(`/api/notes/${note.id}`);
    await page.goto('/#/');
    await page.goto('/#/trash');
    await expect(page.locator('.hint')).toContainText('stay here for 7 days');
    await page.request.post('/api/keep', { data: { trash_days: 30, chat_days: null } });
  });

  test('an uploaded PDF, picture or text opens in a viewer inside the page', async ({ page, browser }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Viewer ${test.info().project.name}`, body: 'x' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    const maker = await browser.newPage();
    await maker.setContent('<h1>Neuro symbolic</h1><p>Rules and networks.</p>');
    fs.writeFileSync(path.join(folder, 'paper.pdf'), await maker.pdf());
    await maker.close();
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMAAAAwqAQXxT7KxAAAAAElFTkSuQmCC', 'base64');
    fs.writeFileSync(path.join(folder, 'board.png'), png);
    fs.writeFileSync(path.join(folder, 'notes.txt'), 'Plain text from the upload.');
    fs.writeFileSync(path.join(folder, 'slides.pptx'), 'pptx');
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('.chip.original')).toHaveCount(5);
    await expect(page.locator('.chip.original', { hasText: 'slides.pptx' })).not.toHaveAttribute('data-action', /./);

    const served = page.waitForResponse((r) => r.url().includes('paper.pdf?view=1'));
    await page.locator('.chip.original', { hasText: 'paper.pdf' }).click();
    const viewer = page.locator('.sheet.viewer');
    await expect(viewer.locator('iframe.viewer-frame')).toHaveAttribute('src', /paper\.pdf\?view=1$/);
    const pdf = await served;
    expect(pdf.status()).toBe(200);
    expect(pdf.headers()['content-disposition']).toMatch(/^inline/);
    expect(pdf.headers()['x-frame-options']).toBe('SAMEORIGIN');
    await expect(viewer.locator('a[download]')).toHaveText('Download');
    await page.keyboard.press('Escape');
    await expect(viewer).toHaveCount(0);

    await page.locator('.chip.original', { hasText: 'board.png' }).click();
    const img = page.locator('.viewer-img');
    await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(2);
    const before = (await img.boundingBox()).width;
    await page.locator('[data-action="viewer-zoom"][data-step="1"]').click();
    await expect.poll(async () => (await img.boundingBox()).width).toBeGreaterThan(before * 1.4);
    await page.mouse.click(5, 5);
    await expect(page.locator('.sheet.viewer')).toHaveCount(0);

    await page.locator('.chip.original', { hasText: 'notes.txt' }).click();
    await expect(page.locator('.viewer-text')).toHaveText('Plain text from the upload.');
    await page.locator('.sheet.viewer [data-action="close"]').click();
    await expect(page.locator('.sheet.viewer')).toHaveCount(0);
    expect(page.url()).toContain(`#/n/${note.id}`);
  });

  test('a note with several uploaded files downloads them together', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Lecture with slides', body: 'x' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'slides.pdf'), '%PDF fake');
    fs.writeFileSync(path.join(folder, 'board.jpg'), 'jpg');
    await page.goto(`/#/n/${note.id}`);
    const all = page.locator('.chip.original.all');
    await expect(all).toHaveText(/Download all 2/);
    const zip = await page.request.get(await all.getAttribute('href'));
    expect(zip.headers()['content-type']).toBe('application/zip');
    expect(zip.headers()['content-disposition']).toContain('Lecture with slides originals.zip');
    const bytes = await zip.body();
    expect(bytes.includes(Buffer.from('Lecture with slides/slides.pdf'))).toBe(true);
  });
});

test.describe('uploads', () => {
  test('a file becomes a note and opens when it is ready', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'Lecture 9: Sorting', body: '## Merge sort\n- divide and conquer' } })).json();
    let sent = null;
    let polls = 0;
    await page.route('**/api/import', async (route) => {
      sent = route.request().postDataJSON();
      await route.fulfill({ status: 202, json: { id: 'job-1' } });
    });
    await page.route('**/api/import/job-1', (route) => {
      polls += 1;
      route.fulfill({ json: polls < 2 ? { state: 'working', step: 'Writing the note', done: 0, total: 1 } : { state: 'done', step: '', done: 1, total: 1, note: made.id } });
    });
    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'sorting.txt', mimeType: 'text/plain', buffer: Buffer.from('Merge sort splits the list in half.') });
    await expect(page.locator('.upload-file')).toContainText('sorting.txt');
    await page.locator('#upload-title').fill('Sorting');
    await page.locator('#upload-wants').fill('The algorithm, then the code, explained step by step');
    await page.locator('#upload-go').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`));
    expect(sent.title).toBe('Sorting');
    expect(sent.wants).toBe('The algorithm, then the code, explained step by step');
    expect(sent.files[0].name).toBe('sorting.txt');
    expect(Buffer.from(sent.files[0].data, 'base64').toString()).toBe('Merge sort splits the list in half.');
  });

  test('a finished upload shows up in the folder that is open, without a refresh', async ({ page }) => {
    const title = `Uploaded later ${test.info().project.name}`;
    let polls = 0;
    let made = null;
    await page.route('**/api/import', (route) => route.fulfill({ status: 202, json: { id: 'job-late' } }));
    await page.route('**/api/import/job-late', async (route) => {
      polls += 1;
      if (polls < 3) return route.fulfill({ json: { state: 'working', step: 'Writing the note', done: 0, total: 1 } });
      if (!made) made = await (await page.request.post('/api/notes', { data: { title, body: 'From the upload.' } })).json();
      return route.fulfill({ json: { state: 'done', step: '', done: 1, total: 1, note: made.id } });
    });
    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'later.txt', mimeType: 'text/plain', buffer: Buffer.from('From the upload.') });
    await page.locator('#upload-go').click();
    await expect(page.locator('.upload-working')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('.card', { hasText: title })).toBeVisible({ timeout: 8000 });
    await expect(page.locator('.toast')).toContainText('Your note from the upload is ready.');
    expect(page.url()).not.toContain('#/n/');
  });

  test('a failed upload says why and offers to try again', async ({ page }) => {
    await page.route('**/api/import', (route) => route.fulfill({ status: 202, json: { id: 'job-2' } }));
    await page.route('**/api/import/job-2', (route) => route.fulfill({ json: { state: 'failed', step: '', done: 0, total: 1, error: 'qwen3:8b cannot read images' } }));
    await page.goto('/');
    await goPlace(page, 'upload');
    await page.locator('#upload-input').setInputFiles({ name: 'board.png', mimeType: 'image/png', buffer: Buffer.from('89504e470d0a1a0a', 'hex') });
    await page.locator('#upload-go').click();
    await expect(page.locator('.upload-error')).toContainText('cannot read images');
    await expect(page.locator('.sheet [data-action="upload"]')).toBeVisible();
  });
});

test.describe('recording', () => {
  function stubRecorder(page, { noteId, local = true }) {
    const seen = { audioBytes: 0, posts: 0, started: null, points: [], stopped: false, paused: false, levels: [], source: 'browser' };
    let polls = 0;
    const view = (over = {}) => ({ wants: seen.wants ?? (seen.started && seen.started.profile ? seen.started.profile.wants || '' : ''), id: 'rec-1', source: seen.source, state: seen.paused ? 'paused' : 'recording', secs: 3, step: '', steps: null, transcript: 'Today we cover breadth first search.', warnings: [], points: seen.points.map((t) => [3, t]), levels: seen.levels, note: null, error: null, ...over });
    page.route('**/api/record', async (route) => {
      if (route.request().method() === 'GET') return route.fulfill({ json: { available: true, local, job: null } });
      seen.started = route.request().postDataJSON();
      return route.fulfill({ status: 202, json: { id: 'rec-1' } });
    });
    page.route('**/api/record/rec-1', (route) => {
      if (!seen.stopped) return route.fulfill({ json: view() });
      polls += 1;
      return route.fulfill({ json: polls < 2 ? view({ state: 'writing', step: 'Writing the notes', steps: [1, 2] }) : view({ state: 'done', note: noteId }) });
    });
    page.route('**/api/record/rec-1/audio*', (route) => {
      seen.audioBytes += route.request().postDataBuffer().length;
      seen.posts += 1;
      return route.fulfill({ json:{next_seq:Number(new URL(route.request().url()).searchParams.get('seq'))+1} });
    });
    page.route('**/api/record/rec-1/wants', (route) => {
      seen.wants = route.request().postDataJSON().text;
      return route.fulfill({ json: view({ wants: seen.wants }) });
    });
    page.route('**/api/record/rec-1/point', (route) => {
      seen.points.push(route.request().postDataJSON().text);
      return route.fulfill({ json: view() });
    });
    page.route('**/api/record/rec-1/pause', (route) => {
      seen.paused = route.request().postDataJSON().paused;
      return route.fulfill({ json: view() });
    });
    page.route('**/api/record/rec-1/stop', (route) => {
      seen.stopped = true;
      return route.fulfill({ json: view({ state: 'writing', step: 'Transcribing the recording' }) });
    });
    return seen;
  }

  test('the microphone streams to leo, takes points, and the note opens when written', async ({ page, context }) => {
    await context.grantPermissions(['microphone']);
    const made = await (await page.request.post('/api/notes', { data: { title: 'BFS lecture', body: '## BFS\n- queue' } })).json();
    const seen = stubRecorder(page, { noteId: made.id });
    await page.goto('/');
    await control(page, 'record').click();
    await expect(page).toHaveURL(/#\/record$/);
    await expect(page.locator('.rec-kind')).toHaveCount(3);
    await expect(page.locator('.rec-kind b')).toHaveText(['Microphone', 'Screen', 'Call']);
    await page.locator('#rec-title').fill('Graphs');
    await page.locator('#rec-wants').fill('Focus on BFS');
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-transcript')).toContainText('breadth first search');
    await expect(page.locator('.rec-notepad #rec-point-text')).toBeVisible();
    expect(await page.evaluate(() => {
      const pad = document.querySelector('.rec-notepad');
      const heard = document.querySelector('#rec-heard');
      return Boolean(pad.compareDocumentPosition(heard) & Node.DOCUMENT_POSITION_FOLLOWING) && !heard.open;
    })).toBe(true);
    await expect(page.locator('#rec-transcript')).toBeHidden();
    await page.locator('#rec-heard > summary').click();
    await expect(page.locator('#rec-transcript')).toBeVisible();
    await page.waitForTimeout(1500);
    await expect(page.locator('#rec-heard')).toHaveAttribute('open', '');
    await expect.poll(() => seen.audioBytes, { timeout: 8000 }).toBeGreaterThan(16000);
    expect(seen.audioBytes % 2).toBe(0);
    await expect(page.locator('#rec-wave')).toBeVisible();
    await expect(page.locator('#rec-hear')).toHaveText('Hearing sound', { timeout: 8000 });
    const drawn = await page.locator('#rec-wave').evaluate((canvas) => {
      const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      let painted = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i] > 0) painted++;
      return painted;
    });
    expect(drawn).toBeGreaterThan(200);
    const leftEdge = await page.locator('#rec-wave').evaluate((canvas) => {
      const data = canvas.getContext('2d').getImageData(0, 0, Math.round(canvas.width * 0.1), canvas.height).data;
      let painted = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i] > 0) painted++;
      return painted;
    });
    expect(leftEdge, 'the wave reaches the left edge of its box').toBeGreaterThan(0);
    expect(seen.started).toEqual({ source: 'browser', directory: '', title: 'Graphs', profile: { context: '', wants: 'Focus on BFS' } });
    const live = page.locator('#rec-wants-live');
    await expect(live).toHaveValue('Focus on BFS');
    await live.click();
    await live.press('End');
    await live.pressSequentially(' and the exam');
    await expect.poll(() => seen.wants).toBe('Focus on BFS and the exam');
    await expect(live).toBeFocused();
    await expect(page.locator('#rec-wants-state')).toContainText('Saved');

    await page.locator('#rec-point-text').fill('exam question on BFS');
    await page.locator('#rec-point-text').press('Enter');
    await expect(page.locator('.rec-points')).toContainText('exam question on BFS');
    await expect(page.locator('#rec-point-text')).toHaveValue('');

    await page.locator('[data-action="rec-pause"]').click();
    await expect(page.locator('[data-action="rec-pause"]')).toHaveText('Resume');
    await page.locator('[data-action="rec-pause"]').click();
    await expect(page.locator('[data-action="rec-pause"]')).toHaveText('Pause');

    await page.locator('#back').click();
    await expect(page.locator('#rec-pill')).toContainText('Recording');
    await page.locator('#rec-pill').click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    await page.locator('[data-action="rec-stop"]').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`), { timeout: 8000 });
    await expect(page.locator('#rec-pill')).toHaveCount(0);
  });

  test('a recording finished while away shows up in the open folder, without a refresh', async ({ page, context }) => {
    await context.grantPermissions(['microphone']);
    const title = `Recorded later ${test.info().project.name} ${test.info().repeatEachIndex}`;
    const seen = stubRecorder(page, { noteId: null });
    let polls = 0;
    let made = null;
    await page.route('**/api/record/rec-1', async (route) => {
      if (!seen.stopped) return route.fulfill({ json: { id: 'rec-1', source: 'browser', state: 'recording', secs: 3, step: '', steps: null, transcript: '', warnings: [], points: [], levels: [], note: null, error: null } });
      polls += 1;
      if (polls < 4) return route.fulfill({ json: { id: 'rec-1', source: 'browser', state: 'writing', secs: 3, step: 'Writing the notes', steps: null, transcript: '', warnings: [], points: [], levels: [], note: null, error: null } });
      if (!made) made = await (await page.request.post('/api/notes', { data: { title, body: 'From the recording.' } })).json();
      return route.fulfill({ json: { id: 'rec-1', source: 'browser', state: 'done', secs: 3, step: '', steps: null, transcript: '', warnings: [], points: [], levels: [], note: made.id, error: null } });
    });
    await page.goto('/#/record');
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('[data-action="rec-stop"]')).toBeVisible();
    await Promise.all([page.waitForResponse('**/api/record/rec-1/stop'), page.locator('[data-action="rec-stop"]').click()]);
    await page.evaluate(() => { location.hash = '#/'; });
    await expect(page.locator('.card', { hasText: title })).toBeVisible({ timeout: 10000 });
    await expect(page.locator('.toast')).toContainText('Your recording is now a note.');
    expect(page.url()).not.toContain('#/n/');
  });

  test('a tab’s sound is shared and recorded, and stopping the share saves it', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'Shared tab lecture', body: 'x' } })).json();
    const seen = stubRecorder(page, { noteId: made.id, local: false });
    await page.goto('/#/record');
    await page.evaluate(() => {
      navigator.mediaDevices.getDisplayMedia = async () => {
        const ctx = new AudioContext();
        const tone = ctx.createOscillator();
        const out = ctx.createMediaStreamDestination();
        tone.connect(out);
        tone.start();
        const canvas = document.createElement('canvas');
        const stream = new MediaStream([...out.stream.getAudioTracks(), ...canvas.captureStream().getVideoTracks()]);
        window.sharedStream = stream;
        return stream;
      };
    });
    await page.locator('.rec-kind', { hasText: 'Screen' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect.poll(() => seen.audioBytes, { timeout: 8000 }).toBeGreaterThan(16000);
    expect(seen.started.source).toBe('tab');
    await page.evaluate(() => window.sharedStream.getAudioTracks()[0].dispatchEvent(new Event('ended')));
    await expect.poll(() => seen.stopped).toBe(true);
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`), { timeout: 8000 });
  });

  test('the computer’s own sound shows its wave too, and says when nothing is heard', async ({ page }) => {
    const made = await (await page.request.post('/api/notes', { data: { title: 'Computer sound lecture', body: 'x' } })).json();
    const seen = stubRecorder(page, { noteId: made.id, local: true });
    seen.source = 'screen';
    seen.levels = Array(12).fill(0.1);
    await page.goto('/#/record');
    await page.locator('.rec-kind', { hasText: 'Screen' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-wave')).toBeVisible();
    await expect(page.locator('#rec-hear')).toHaveText('Hearing sound');
    expect(seen.started.source).toBe('screen');
    expect(seen.audioBytes).toBe(0);
    seen.levels = [...Array(12).fill(0.1), ...Array(30).fill(0)];
    await expect(page.locator('#rec-hear')).toHaveText('No sound for a while: is something playing on the computer?', { timeout: 12000 });
    await expect(page.locator('#rec-hear')).toHaveClass(/silent/);
  });

  test('a share without sound says how to include it and starts nothing', async ({ page }) => {
    let started = false;
    await page.route('**/api/record', (route) => {
      if (route.request().method() === 'POST') started = true;
      return route.fulfill({ json: { available: true, local: false, job: null } });
    });
    await page.goto('/#/record');
    await page.evaluate(() => {
      navigator.mediaDevices.getDisplayMedia = async () => new MediaStream(document.createElement('canvas').captureStream().getVideoTracks());
    });
    await page.locator('.rec-kind', { hasText: 'Screen' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('.toast.bad')).toContainText('Share tab audio');
    expect(started).toBe(false);
  });

  test('a page opened over the internet records its own device, not the computer', async ({ page, context }) => {
    await context.grantPermissions(['microphone']);
    stubRecorder(page, { noteId: 'x', local: false });
    await page.goto('/#/record');
    await expect(page.locator('[data-action="rec-start"]')).toBeVisible();
    await expect(page.locator('.rec-kind b')).toHaveText(['Microphone', 'Screen', 'Call']);
    await expect(page.locator('.rec-kind', { hasText: 'Screen' })).toContainText('Share tab audio');
  });

  test('the class or meeting happening now names the recording and tells the AI who and what, without overwriting a typed title', async ({ page, context }) => {
    await context.grantPermissions(['microphone']);
    const seen = stubRecorder(page, { noteId: 'x', local: true });
    let release;
    const late = new Promise((resolve) => { release = resolve; });
    const now = Date.now();
    await page.route('**/api/calendar', async (route) => {
      await late;
      return route.fulfill({ json: { connected: true, calendars: [{ id: 'c1', name: 'Work' }], events: [
        { id: 'e1', title: 'Design review', start: new Date(now - 5 * 60000).toISOString(), end: new Date(now + 55 * 60000).toISOString(), location: 'Room 4', context: 'Calendar event: Design review\nInvited: Sam, Priya' },
        { id: 'e2', title: 'Algorithms lecture', start: new Date(now + 3 * 3600000).toISOString(), end: new Date(now + 4 * 3600000).toISOString(), location: '', context: 'Calendar event: Algorithms lecture' },
      ] } });
    });
    await page.goto('/#/record');
    const title = page.locator('#rec-title');
    await title.click();
    await title.pressSequentially('Weekly sync');
    release();
    await expect(page.locator('#rec-event')).toHaveValue('0');
    await expect(title).toHaveValue('Weekly sync');
    await expect(title).toBeFocused();
    await expect(page.locator('#rec-wants')).toHaveValue(/Meeting notes/);
    await page.locator('#rec-event').selectOption('1');
    await expect(page.locator('#rec-event')).toHaveValue('1');
    await page.locator('#rec-event').selectOption('0');
    await page.locator('[data-action="rec-start"]').click();
    await expect.poll(() => seen.started && seen.started.profile.context).toContain('Invited: Sam, Priya');
    expect(seen.started.title).toBe('Weekly sync');
  });

  test('without a calendar the Record page points to Settings, and a broken calendar is not fatal', async ({ page }) => {
    await page.route('**/api/record', (route) => route.fulfill({ json: { available: true, local: false, job: null, pending: [] } }));
    await page.route('**/api/calendar', (route) => route.fulfill({ json: { connected: false, calendars: [], events: [] } }));
    await page.goto('/#/record');
    await expect(page.locator('.rec-idle a[href="#/settings"]')).toContainText('connect a calendar in Settings');
    await expect(page.locator('#rec-event')).toHaveCount(0);
    await page.unroute('**/api/calendar');
    await page.route('**/api/calendar', (route) => route.fulfill({ status: 500, json: { error: 'The calendars could not be read.' } }));
    await page.goto('/#/');
    await page.goto('/#/record');
    await expect(page.locator('[data-action="rec-start"]')).toBeEnabled();
    await expect(page.locator('#rec-wants')).toBeVisible();
  });

  test('a refused microphone says how to allow it and starts nothing', async ({ page }) => {
    let started = false;
    await page.route('**/api/record', (route) => {
      if (route.request().method() === 'POST') started = true;
      return route.fulfill({ json: { available: true, local: false, job: null } });
    });
    await page.goto('/#/record');
    await page.evaluate(() => {
      navigator.mediaDevices.getUserMedia = () => Promise.reject(Object.assign(new Error('denied'), { name: 'NotAllowedError' }));
    });
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('.toast.bad')).toContainText('Allow it for this site');
    expect(started).toBe(false);
  });
});

test.describe('the kinds of bugs that reached people before', () => {
  const kind = () => test.info().project.name;
  const stamp = () => `${kind()}-${Date.now().toString(36)}`;
  const bodyOf = async (page, id) => (await (await page.request.get(`/api/notes/${id}`)).json()).body;

  async function typeWithPauses(page, box, first, second) {
    await box.pressSequentially(first, { delay: 40 });
    await page.waitForTimeout(900);
    await expect(box, 'still typing in the same box after a pause').toBeFocused();
    await box.pressSequentially(second, { delay: 40 });
    await expect(box).toBeFocused();
  }

  test('every text box keeps focus and every letter while results and saves arrive', async ({ page }) => {
    const tag = stamp();
    const note = await (await page.request.post('/api/notes', { data: { title: `Typing target ${tag}`, body: 'Line one' } })).json();
    await page.request.post('/api/notes', { data: { title: `Dijkstra typing ${tag}`, body: 'Priority queue.' } });

    await page.goto('/');
    await openSearchBox(page);
    const search = page.locator('#search-input');
    await typeWithPauses(page, search, 'dijkstra', ' typing');
    await expect(search).toHaveValue('dijkstra typing');
    await page.keyboard.press('Escape');

    await page.goto('/');
    if (await page.locator('#side').isHidden()) {
      await page.locator('#menu').click();
      await page.locator('.sheet [data-action="new-folder"]').click();
    } else {
      await page.locator('#side .side-group [data-action="new-folder"]').click();
    }
    const folder = page.locator('#folder-name');
    await typeWithPauses(page, folder, 'week', ' three');
    await expect(folder).toHaveValue('week three');
    await page.locator('.sheet [data-action="close"]').click();

    await page.goto('/');
    await control(page, 'upload').click();
    await page.locator('#upload-input').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('Merge sort.') });
    for (const id of ['#upload-title', '#upload-wants']) {
      const box = page.locator(id);
      await box.click();
      await typeWithPauses(page, box, 'merge', ' sort');
      await expect(box).toHaveValue('merge sort');
    }
    await page.locator('.sheet [data-action="close"]').first().click();

    await page.goto(`/#/n/${note.id}`);
    await page.locator('.blk').first().click();
    const line = page.locator('.line-edit');
    await line.press('End');
    await typeWithPauses(page, line, ' and', ' more');
    await expect.poll(() => bodyOf(page, note.id), { timeout: 5000 }).toBe('Line one and more');
    await expect(line).toBeFocused();

    const title = page.locator('#title');
    await title.click();
    await page.keyboard.press('End');
    await typeWithPauses(page, title, ' A', 'B');
    await expect(title).toHaveText(`Typing target ${tag} AB`);

    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const input = page.locator('#chat-input');
    await typeWithPauses(page, input, 'compare @dijkstra', ' typing');
    await expect(page.locator('.chat-pick-row').first()).toContainText(`Dijkstra typing ${tag}`);
    await expect(input).toHaveValue('compare @dijkstra typing');
    await page.locator('#chat [data-chat="close"]').click();

    await page.goto('/#/settings');
    const key = page.locator('[data-task-card="writing"] [data-key-input]').first();
    if (await key.count()) {
      await key.click();
      await typeWithPauses(page, key, 'sk-not', '-real');
      await expect(key).toHaveValue('sk-not-real');
      await key.fill('');
    }

    await page.goto('/#/record');
    const rec = page.locator('#rec-title');
    await rec.click();
    await typeWithPauses(page, rec, 'graph', ' lecture');
    await expect(rec).toHaveValue('graph lecture');
  });

  test('an edit typed just before opening another note is saved to the note it was typed in', async ({ page }) => {
    const tag = stamp();
    const first = await (await page.request.post('/api/notes', { data: { title: `Typed in ${tag}`, body: 'Start' } })).json();
    const second = await (await page.request.post('/api/notes', { data: { title: `Opened next ${tag}`, body: 'Leave me' } })).json();
    await page.goto(`/#/n/${first.id}`);
    await page.locator('.blk').first().click();
    await page.locator('.line-edit').press('End');
    await page.keyboard.type(' typed fast');
    await page.evaluate((id) => { location.hash = `#/n/${id}`; }, second.id);
    await expect(page.locator('#title')).toHaveText(second.title);
    await expect.poll(() => bodyOf(page, first.id), { timeout: 5000 }).toBe('Start typed fast');
    await page.waitForTimeout(900);
    expect(await bodyOf(page, second.id)).toBe('Leave me');
    await expect(page.locator('#doc')).toContainText('Leave me');
    await expect(page.locator('#doc')).not.toContainText('typed fast');
    await page.goBack();
    await expect(page.locator('#doc')).toContainText('Start typed fast');
  });

  test('an answer still coming when a new chat starts stays in the chat it was asked in', async ({ page }) => {
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await held;
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Late answer about heaps."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('what is a heap?');
    await chat.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(1);
    await chat.locator('.chat-head [data-chat="new"]').click();
    release();
    await page.waitForTimeout(600);
    await expect(chat.locator('.msg')).toHaveCount(0);
    await expect(chat).not.toContainText('Late answer about heaps.');
    await chat.locator('#chat-input').fill('a new question');
    await expect(chat.locator('#chat-input')).toHaveValue('a new question');
    const old = await (await page.request.get(`/api/chats/${asked[0].chat}`)).json();
    expect(old.messages[0].text).toBe('what is a heap?');
    expect(old.messages).toHaveLength(2);
  });

  test('a page that loads slowly never draws over the page opened after it', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Slow note ${kind()}`, body: 'Slow body' } })).json();
    await page.request.post('/api/dirs', { data: { path: `slow-${kind()}` } });
    const slow = [
      ['#/settings', '**/api/settings'],
      ['#/trash', '**/api/trash'],
      ['#/settings/storage', '**/api/storage'],
      ['#/map', '**/api/graph'],
      ['#/search/slow', '**/api/search?*'],
      [`#/f/slow-${kind()}`, '**/api/dirs?*'],
      [`#/n/${note.id}`, `**/api/notes/${note.id}`],
    ];
    for (const [hash, api] of slow) {
      await page.goto('/');
      let release;
      const held = new Promise((resolve) => { release = resolve; });
      let seen = false;
      await page.route(api, async (route) => {
        seen = true;
        await held;
        await route.continue().catch(() => {});
      });
      await page.evaluate((h) => { location.hash = h; }, hash);
      await expect.poll(() => seen, { message: `${hash} asked leo` }).toBe(true);
      await page.evaluate(() => { location.hash = '#/record'; });
      await expect(page.locator('#rec-title')).toBeVisible();
      release();
      await page.waitForTimeout(700);
      await expect(page.locator('#rec-title'), `${hash} finished late and must not replace Record`).toBeVisible();
      await page.unroute(api);
    }
  });

  test('each checkbox changes its own line, whatever comes before it', async ({ page }) => {
    const body = [
      '- [ ] first',
      '```',
      '- [ ] inside code',
      '```',
      '- [ ] after code',
      '> - [ ] inside a quote',
      '> [!todo]- Folded tasks',
      '> - [ ] inside a callout',
      '$$',
      'x^2',
      '$$',
      '```mermaid',
      'graph TD; A-->B',
      '```',
      '1) [ ] paren number',
      '- [X] capital done',
      '    - [ ] deep',
      '| a | b |',
      '|---|---|',
      '| - [ ] cell | 2 |',
      '- [ ] last',
    ].join('\n');
    const note = await (await page.request.post('/api/notes', { data: { title: `Boxes everywhere ${kind()}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    const boxes = page.locator('.doc input[data-box]');
    await expect(boxes.first()).toBeVisible();
    const count = await boxes.count();
    expect(count).toBeGreaterThanOrEqual(6);
    let before = body.split('\n');
    for (let i = 0; i < count; i++) {
      const box = boxes.nth(i);
      if (!(await box.isVisible())) continue;
      const label = (await box.evaluate((el) => (el.closest('li, .task-item') || el.parentElement).textContent)).trim();
      await box.click();
      await expect(page.locator('.doc textarea')).toHaveCount(0);
      let after;
      await expect.poll(async () => {
        after = (await bodyOf(page, note.id)).split('\n');
        return after.filter((l, n) => l !== before[n]).length;
      }, { timeout: 5000, message: `box ${i + 1} (${label}) changes one line` }).toBe(1);
      const changed = after.findIndex((l, n) => l !== before[n]);
      expect(after[changed], `box ${i + 1} changed the line it shows`).toContain(label.split('\n')[0].trim());
      expect(before[changed].replace(/\[[ xX]\]/, '')).toBe(after[changed].replace(/\[[ xX]\]/, ''));
      before = after;
    }
  });

  test.describe('a selection across any kind of block deletes exactly what it covers', () => {
    const kinds = {
      'a code block': ['```js', 'const a = 1;', '```'],
      'a table': ['| a | b |', '|---|---|', '| 1 | 2 |'],
      'a folded callout': ['> [!example]- Why', '> Because.'],
      'a quote': ['> quoted', '> twice'],
      'a math block': ['$$', 'e^{i\\pi} + 1 = 0', '$$'],
      'a diagram': ['```mermaid', 'graph TD; A-->B', '```'],
      'checkboxes': ['- [ ] one', '- [x] two'],
      'a heading and a rule': ['## Heading', '---'],
      'a nested list': ['1. one', '   - inner', '2. two'],
    };
    for (const [name, lines] of Object.entries(kinds)) {
      for (const backwards of [false, true]) {
        test(`${name}${backwards ? ', selected backwards' : ''}`, async ({ page }) => {
          const body = ['Start here', '', ...lines, '', 'End here'].join('\n');
          const note = await (await page.request.post('/api/notes', { data: { title: `Across ${name} ${backwards} ${kind()}`, body } })).json();
          await page.goto(`/#/n/${note.id}`);
          await expect(page.locator('#doc .blk').last()).toContainText('End here');
          await page.evaluate((back) => {
            const blocks = document.querySelectorAll('#doc .blk');
            const text = (block, word) => {
              const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
              for (let node = walker.nextNode(); node; node = walker.nextNode()) if (node.data.includes(word)) return node;
              return null;
            };
            const start = text(blocks[0], 'Start here');
            const end = text(blocks[blocks.length - 1], 'End here');
            const selection = window.getSelection();
            selection.removeAllRanges();
            if (back) selection.setBaseAndExtent(end, end.data.indexOf('End here') + 4, start, start.data.indexOf('Start here') + 5);
            else selection.setBaseAndExtent(start, start.data.indexOf('Start here') + 5, end, end.data.indexOf('End here') + 4);
            document.activeElement.blur();
          }, backwards);
          await page.keyboard.press('Backspace');
          await expect.poll(() => bodyOf(page, note.id), { timeout: 5000 }).toBe('Starthere');
        });
      }
    }
  });

  test('pressing Create twice makes one folder and no error', async ({ page }) => {
    const name = `twice-${stamp()}`;
    await page.goto('/');
    if (await page.locator('#side').isHidden()) {
      await page.locator('#menu').click();
      await page.locator('.sheet [data-action="new-folder"]').click();
    } else {
      await page.locator('#side .side-group [data-action="new-folder"]').click();
    }
    await page.locator('#folder-name').fill(name);
    await page.locator('#folder-name').press('Enter');
    await page.locator('[data-action="create-folder"]').click({ force: true, timeout: 1000 }).catch(() => {});
    await expect(page).toHaveURL(new RegExp(`#/f/${name}$`));
    await page.waitForTimeout(500);
    await expect(page.locator('.toast.bad')).toHaveCount(0);
    const folders = await (await page.request.get('/api/folders')).json();
    expect(folders.filter((f) => f.name === name)).toHaveLength(1);
  });

  test('pressing Apply twice on a suggestion changes the note once, without an error', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: `Apply twice ${kind()}`, body: 'BFS uses a stack.' } })).json();
    await page.route('**/api/chat', (route) => route.fulfill({
      status: 200,
      headers: { 'content-type': 'application/x-ndjson' },
      body: [{ sources: [] }, { proposal: { kind: 'edit', note: note.id, title: note.title, find: 'stack', replace: 'queue', why: '' } }, { t: 'Fixed.' }, { done: true }].map((l) => JSON.stringify(l)).join('\n') + '\n',
    }));
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await page.locator('#chat-input').fill('fix it');
    await page.locator('#chat-input').press('Enter');
    const apply = page.locator('#chat .proposal [data-chat="apply"]');
    await expect(apply).toBeVisible();
    await apply.dblclick();
    await expect(page.locator('#chat .proposal-done')).toContainText('Applied');
    await page.waitForTimeout(500);
    expect(await bodyOf(page, note.id)).toBe('BFS uses a queue.');
    await expect(page.locator('.toast.bad')).toHaveCount(0);
  });

  test('every successful answer from leo is JSON, or 204 with nothing', async ({ page }) => {
    const tag = stamp();
    const answers = [];
    const call = async (method, url, data) => {
      const response = await page.request.fetch(url, { method, data });
      answers.push({ what: `${method} ${url}`, status: response.status(), type: response.headers()['content-type'] || '', text: await response.text() });
      return answers[answers.length - 1];
    };
    const json = (answer) => JSON.parse(answer.text);
    const made = json(await call('POST', '/api/notes', { title: `Contract ${tag}`, body: '- [ ] box\nold words' }));
    await call('PATCH', `/api/notes/${made.id}`, { body: '- [ ] box\nnew words' });
    await call('POST', `/api/notes/${made.id}/toggle?checkbox=1`);
    await call('POST', '/api/dirs', { path: `contract-${tag}` });
    await call('POST', '/api/dirs', { path: `contract-${tag}/inner` });
    await call('POST', `/api/notes/${made.id}/move`, { directory: `contract-${tag}` });
    await call('POST', '/api/dirs', { path: `contract-moved-${tag}` });
    await call('POST', '/api/dirs/move', { from: `contract-moved-${tag}`, into: `contract-${tag}` });
    await call('POST', `/api/notes/${made.id}/suggestion`, { find: 'new', replace: 'newer' });
    await call('PUT', `/api/chats/contract-${tag}`, { title: 'Contract', mode: 'chat', messages: [{ role: 'user', text: 'hi' }] });
    for (const url of ['/api/notes', '/api/folders', '/api/dirs', `/api/search?q=contract`, '/api/trash', '/api/chats', `/api/chats/contract-${tag}`, '/api/review', '/api/activity', '/api/settings', '/api/storage', '/api/keep', '/api/sessions', '/api/graph/status', '/api/record']) {
      await call('GET', url);
    }
    const trashed = json(await call('POST', '/api/trash/move', { notes: [made.id], dirs: [] }));
    await call('POST', '/api/trash/restore', { ids: trashed.ids || [made.id] });
    await call('DELETE', `/api/notes/${made.id}`);
    await call('DELETE', `/api/chats/contract-${tag}`);

    for (const a of answers) {
      expect(a.status, `${a.what} succeeded`).toBeLessThan(300);
      if (a.status === 204) {
        expect(a.text, `${a.what} is 204, so it has no body`).toBe('');
        continue;
      }
      expect(a.type, `${a.what} says it is JSON`).toContain('application/json');
      expect(() => JSON.parse(a.text), `${a.what} answers JSON`).not.toThrow();
    }
  });
});

test.describe('Felix asks, quizzes and listens while he works', () => {
  const lines = (list) => list.map((l) => JSON.stringify(l)).join('\n') + '\n';
  const answerWith = (list) => ({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines(list) });

  async function openFelix(page) {
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    return page.locator('#chat');
  }

  test('a multiple choice question is checked at once, Felix reacts, and he is told how it went', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      const body = route.request().postDataJSON();
      asked.push(body);
      if (asked.length === 1) {
        return route.fulfill(answerWith([{ answer: 'a1' }, { sources: [{ n: 1, id: 'note-bfs', title: 'Graph traversals', folder: '', why: 'open' }] }, { step: 'Asked you a practice question', tool: 'quiz', found: [] }, { quiz: { kind: 'multiple_choice', question: 'What does BFS use?', options: ['a stack', 'a queue', 'a heap'], answer: 'a queue', explain: 'BFS takes the oldest vertex first [n1].' } }, { t: 'Give it a try!' }, { done: true }]));
      }
      return route.fulfill(answerWith([{ answer: 'a2' }, { sources: [] }, { t: '[[incorrect]] A stack gives the newest vertex; BFS needs the oldest.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me on BFS');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.quiz-card');
    await expect(card.locator('.quiz-kind')).toHaveText(/Multiple choice/i);
    await expect(card.locator('.quiz-option')).toHaveCount(3);
    await card.locator('.quiz-option', { hasText: 'a stack' }).click();
    await expect(card.locator('.quiz-option.wrong')).toContainText('a stack');
    await expect(card.locator('.quiz-option.right')).toContainText('a queue');
    await expect(card.locator('.quiz-result')).toContainText('Not quite. The answer: a queue');
    await expect(card).toHaveClass(/wrong/);
    await expect(card.locator('.quiz-option').first()).toBeDisabled();
    await expect(card.locator('.quiz-reply')).toContainText('BFS needs the oldest');
    await expect(card.locator('.quiz-reply')).not.toContainText('[[incorrect]]');
    await expect(chat.locator('.msg.user'), 'no message pretends to be the user').toHaveCount(1);
    await expect(chat.locator('.msg.leo')).toHaveCount(1);
    expect(asked[0].practice).toBe(false);
    expect(asked[1].practice, 'a card answer is marked as practice, so it is never nudged into a note change').toBe(true);
    const told = asked[1].messages.at(-1).text;
    expect(told).toContain('Question: What does BFS use?');
    expect(told).toContain('with the quiz tool');
    expect(told).toContain('My answer: a stack');
    expect(told).toContain('the right answer is a queue');
    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat .quiz-result')).toContainText('Not quite', { timeout: 5000 });
    await expect(page.locator('#chat .quiz-option').first()).toBeDisabled();
    await expect(page.locator('#chat .quiz-reply')).toContainText('BFS needs the oldest');
  });

  test('a fill in the blank takes typed words, and a free answer goes to Felix to mark', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const n = asked.length;
      if (n === 1) return route.fulfill(answerWith([{ sources: [] }, { quiz: { kind: 'fill_blank', question: 'BFS takes the ___ vertex first, in $O(V+E)$ time.', options: [], answer: 'oldest | earliest', explain: '' } }, { t: 'Fill it in.' }, { done: true }]));
      if (n === 2) return route.fulfill(answerWith([{ sources: [] }, { t: '[[correct]] Yes.' }, { quiz: { kind: 'free_response', question: 'Why does BFS find shortest paths?', options: [], answer: 'It explores level by level.', explain: '' } }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { t: '[[correct]] Right: level by level.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('practice');
    await chat.locator('#chat-input').press('Enter');
    const blank = chat.locator('.quiz-card').first();
    await expect(blank.locator('.quiz-blank')).toHaveCount(1);
    await expect(blank.locator('.quiz-q .math[data-drawn="yes"] .katex'), 'math in a question is drawn').toBeVisible();
    const box = blank.locator('.quiz-input');
    await box.pressSequentially('  Earliest ', { delay: 20 });
    await box.press('Enter');
    await expect(blank.locator('.quiz-result')).toHaveText('Correct');
    await expect(blank).toHaveClass(/right/);
    await expect(blank.locator('.quiz-reply')).toContainText('Yes.');
    await expect(chat.locator('.quiz-card')).toHaveCount(2);
    const free = chat.locator('.quiz-card').nth(1);
    await expect(free.locator('textarea.quiz-input')).toBeEnabled();
    await free.locator('textarea.quiz-input').fill('Because it goes out one level at a time.');
    await free.locator('[data-chat="quiz-check"]').click();
    await expect(free.locator('textarea.quiz-input')).toBeDisabled();
    await expect(free).toHaveClass(/right/);
    await expect(free.locator('.quiz-reply')).toContainText('Right: level by level.');
    await expect(chat.locator('.msg.user'), 'answers stay in their cards').toHaveCount(1);
    const told = asked[2].messages.at(-1).text;
    expect(told).toContain('[[correct]] or [[incorrect]]');
    expect(told).toContain('It explores level by level.');
  });

  test('Felix asks which one, and a tap on a choice answers him', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) return route.fulfill(answerWith([{ sources: [] }, { step: 'Asked you a question', tool: 'ask_user', found: [] }, { ask: { question: 'Which week should I cover?', options: ['Week 1', 'Week 2'] } }, { t: 'Tell me which week.' }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { t: 'Week 2 covers heaps.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('summarise the lecture');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.ask-card');
    await expect(card.locator('.ask-q')).toHaveText('Which week should I cover?');
    await card.locator('.ask-option', { hasText: 'Week 2' }).click();
    await expect(chat.locator('.msg.leo').last()).toContainText('Week 2 covers heaps.');
    expect(asked[1].messages.at(-1).text).toBe('Week 2');
    await expect(chat.locator('.ask-option.chosen')).toHaveText('Week 2');
    await expect(chat.locator('.ask-option').first()).toBeDisabled();
    await expect(chat.locator('.ask-hint')).toHaveText('You answered: Week 2');
  });

  test('a message sent while Felix works reaches him, and one too late is asked next', async ({ page }) => {
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    const steers = [];
    const asked = [];
    await page.route('**/api/chat/answer-1/steer', async (route) => {
      steers.push(route.request().postDataJSON().text);
      await route.fulfill({ status: 202, json: { waiting: steers.length } });
    });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) {
        await held;
        return route.fulfill(answerWith([{ answer: 'answer-1' }, { sources: [] }, { step: 'Searched your notes for “heap”', tool: 'search_notes', found: [] }, { steered: ['only the min-heap part'] }, { t: 'A min-heap keeps the smallest on top.' }, { done: true }]));
      }
      return route.fulfill(answerWith([{ answer: 'answer-2' }, { sources: [] }, { t: 'And in Python, heapq.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    const input = chat.locator('#chat-input');
    await input.fill('explain heaps');
    await input.press('Enter');
    await expect(chat.locator('.msg.leo.pending')).toBeVisible();
    await expect(input).toHaveAttribute('placeholder', 'Add to what Felix is doing…');
    await input.fill('only the min-heap part');
    await expect(chat.locator('#chat-send')).not.toHaveClass(/stop/);
    await input.press('Enter');
    await expect(chat.locator('.msg.user.queued')).toContainText('only the min-heap part');
    await expect(chat.locator('.msg.user.queued .msg-note')).toHaveText('Felix reads this at his next step');
    await input.fill('and in Python?');
    await chat.locator('#chat-send').click();
    await expect(chat.locator('.msg.user.queued')).toHaveCount(2);
    await expect(chat.locator('#chat-send')).toHaveClass(/stop/);
    release();
    await expect.poll(() => steers).toEqual(['only the min-heap part', 'and in Python?']);
    await expect(chat.locator('.msg.leo').last()).toContainText('And in Python, heapq.');
    const texts = await chat.locator('.msg').evaluateAll((all) => all.map((m) => (m.classList.contains('user') ? 'U: ' : 'F: ') + m.querySelector('.bubble, .prose').textContent.trim()));
    expect(texts).toEqual(['U: explain heaps', 'U: only the min-heap part', 'F: A min-heap keeps the smallest on top.', 'U: and in Python?', 'F: And in Python, heapq.']);
    await expect(chat.locator('.msg-note', { hasText: 'Sent while Felix worked' })).toHaveCount(1);
    expect(asked[1].messages.map((m) => m.text.split('\n').pop())).toEqual(['explain heaps', 'only the min-heap part', 'A min-heap keeps the smallest on top.', 'and in Python?']);
    expect(asked[1].messages[2].text).toContain('[What Felix did for this answer: Searched your notes for “heap”]');
  });

  test('a broken first try leaves no trace, and a verdict from leo colours a free answer', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) {
        return route.fulfill(answerWith([{ sources: [] }, { t: 'Half an answer' }, { quiz: { kind: 'free_response', question: 'Broken card?', options: [], answer: 'x', explain: '' } }, { reset: true }, { quiz: { kind: 'free_response', question: 'Why compare the endpoints?', options: [], answer: 'A minimum can sit at an end.', explain: '' } }, { t: 'Try the card below.' }, { done: true }]));
      }
      return route.fulfill(answerWith([{ sources: [] }, { t: 'Close, but you left out the ends.' }, { verdict: 'incorrect' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Try the card below.');
    await expect(chat.locator('.msg.leo').last()).not.toContainText('Half an answer');
    await expect(chat.locator('.quiz-card')).toHaveCount(1);
    await expect(chat.locator('.quiz-q')).toHaveText('Why compare the endpoints?');
    await chat.locator('textarea.quiz-input').fill('no idea');
    await chat.locator('[data-chat="quiz-check"]').click();
    await expect(chat.locator('.quiz-card')).toHaveClass(/wrong/);
    await expect(chat.locator('.quiz-reply')).toContainText('left out the ends');
    expect(asked[1].mark).toBe(true);
  });

  test('Esc stops Felix while he answers, and closes the chat only when he is idle', async ({ page }) => {
    await page.route('**/api/chat', async () => {});
    const chat = await openFelix(page);
    const input = chat.locator('#chat-input');
    await input.fill('explain heaps');
    await input.press('Enter');
    await expect(chat.locator('.msg.leo.pending')).toBeVisible();
    await input.press('Escape');
    await expect(chat.locator('.msg-error')).toHaveText('Stopped.');
    await expect(chat, 'the chat stays open').toBeVisible();
    await input.press('Escape');
    await expect(chat).toBeHidden();
  });

  test('stopping Felix puts messages he never read back in the box', async ({ page }) => {
    await page.route('**/api/chat', async () => {});
    await page.route('**/api/chat/*/steer', (route) => route.fulfill({ status: 410, json: { error: 'That answer has finished.' } }));
    const chat = await openFelix(page);
    const input = chat.locator('#chat-input');
    await input.fill('explain heaps');
    await input.press('Enter');
    await input.fill('shorter please');
    await input.press('Enter');
    await expect(chat.locator('.msg.user.queued')).toHaveCount(1);
    await chat.locator('#chat-send').click();
    await expect(chat.locator('.msg.user.queued')).toHaveCount(0);
    await expect(input).toHaveValue('shorter please');
    await expect(chat.locator('.msg-error')).toHaveText('Stopped.');
  });
});

test.describe('switching models and styles mid-chat', () => {
  test('the next model is told what the last one looked up, and the switch is shown', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const first = asked.length === 1;
      const lines = first
        ? [{ sources: [{ n: 1, id: 'x1', title: 'Graph traversals', folder: '', why: 'opened by Felix' }] }, { step: 'Searched your notes for “queue”', tool: 'search_notes', found: ['Graph traversals'] }, { t: 'BFS uses a queue.' }, { spent: { by: 'Codex', model: 'gpt-6.1-sol', plan: true, steps: 2 } }, { done: true }]
        : [{ sources: [] }, { t: 'Building on that: oldest first.' }, { spent: { by: 'Claude Code', model: 'claude-opus-5-5', plan: true, steps: 1 } }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('what does bfs use?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('BFS uses a queue.');
    await chat.locator('#chat-input').fill('why?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Building on that');
    const before = asked[1].messages[1].text;
    expect(before).toContain('[Written by Codex · gpt-6.1-sol, in chat style]');
    expect(before).toContain('[What Felix did for this answer: Searched your notes for “queue” (found: Graph traversals)]');
    expect(before.endsWith('BFS uses a queue.')).toBe(true);
    expect(asked[1].recent).toEqual(['x1']);
    await expect(chat.locator('.chat-switch')).toHaveText('Now answered by Claude Code · claude-opus-5-5, with everything said so far');
  });
});

test.describe('past chats', () => {
  test('chats are found by what was said in them, and switching style does not reorder them', async ({ page }) => {
    const tag = `${test.info().project.name}${Date.now().toString(36)}`;
    const put = (id, text, title) => page.request.put(`/api/chats/${id}`, { data: { title, mode: 'chat', messages: [{ role: 'user', text }, { role: 'assistant', text: 'ok' }] } });
    await put(`chat-dij-${tag}`, `how does dijkstra${tag} relax edges`, `Shortest paths ${tag}`);
    await put(`chat-heap-${tag}`, `what is a heap${tag}`, `Heaps ${tag}`);
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    const history = chat.locator('#chat-history');
    if (!(await history.isVisible())) await chat.locator('.chat-head [data-chat="history"]').click();
    const search = chat.locator('#chat-history-search');
    await search.pressSequentially(`dijkstra${tag}`, { delay: 15 });
    await expect(chat.locator('.chat-history-item')).toHaveCount(1);
    await expect(chat.locator('.chat-history-title')).toHaveText(`Shortest paths ${tag}`);
    await expect(search).toBeFocused();
    await search.fill(`nothing${tag}`);
    await expect(chat.locator('.chat-history-none')).toHaveText('No chat mentions that.');
    await search.fill('');
    await expect(chat.locator('.chat-history-title').first()).toHaveText(`Heaps ${tag}`);
    await chat.locator('.chat-history-item', { hasText: `Shortest paths ${tag}` }).click();
    await chat.locator('[data-mode="study"]').click();
    await expect.poll(async () => (await (await page.request.get('/api/chats')).json()).filter((c) => c.id.endsWith(tag)).map((c) => c.id)).toEqual([`chat-heap-${tag}`, `chat-dij-${tag}`]);
  });
});

test('a calendar is connected in Settings by pasting one link, and can be removed', async ({ page }) => {
  let linked = [];
  let posted = null;
  await page.route('**/api/calendar', async (route) => {
    if (route.request().method() === 'POST') {
      posted = route.request().postDataJSON();
      if (!posted.link.includes('calendar.google.com')) return route.fulfill({ status: 400, json: { error: 'That link does not lead to a calendar. In Google Calendar, copy “Secret address in iCal format”.' } });
      linked = [{ id: 'c1', name: 'Classes', added_at: new Date().toISOString(), problem: null }];
    }
    return route.fulfill({ json: { connected: linked.length > 0, calendars: linked, events: linked.length ? [{ id: 'e', title: 'Algorithms lecture', start: new Date(Date.now() + 3600000).toISOString(), end: new Date(Date.now() + 7200000).toISOString(), context: '' }] : [], refreshing: false } });
  });
  await page.route('**/api/calendar/c1', (route) => {
    linked = [];
    return route.fulfill({ json: { connected: false, calendars: [], events: [] } });
  });
  await goPlace(page, 'settings');
  const card = page.locator('.cal-card');
  await expect(card.locator('h3')).toHaveText('Calendar');
  await expect(card).toContainText('Not connected');
  await expect(card.locator('#calendar-link')).toBeVisible();
  await expect(card.locator('a[href="https://calendar.google.com/calendar/r/settings"]')).toBeVisible();
  await card.locator('#calendar-link').fill('https://example.com/nope');
  await card.locator('[data-action="calendar-add"]').click();
  await expect(page.locator('.toast.bad')).toContainText('Secret address in iCal format');
  await expect(card.locator('#calendar-link')).toHaveValue('https://example.com/nope');
  await card.locator('#calendar-link').fill('https://calendar.google.com/calendar/ical/me/private-abc/basic.ics');
  await card.locator('[data-action="calendar-add"]').click();
  await expect(page.locator('.cal-card')).toContainText('Classes');
  await expect(page.locator('.cal-card .cal-next')).toContainText('Algorithms lecture');
  await expect(page.locator('#calendar-link')).toHaveCount(0);
  await page.locator('.cal-card [data-action="calendar-more"]').click();
  await expect(page.locator('#calendar-link')).toBeFocused();
  await page.locator('.cal-card [data-action="calendar-cancel"]').click();
  await expect(page.locator('#calendar-link')).toHaveCount(0);
  expect(posted.link).toContain('private-abc');
  await page.locator('.cal-card [data-action="calendar-remove"]').click();
  await expect(page.locator('.cal-card')).toContainText('Not connected');
});

test('dropping one note on another offers to combine them, shows the result first, and Undo brings both back', async ({ page }) => {
  const tag = Date.now().toString(36);
  const folder = `combine-${tag}`;
  await page.request.post('/api/dirs', { data: { path: folder } });
  const a = await (await page.request.post('/api/notes', { data: { title: `BFS ${tag}`, body: 'BFS uses a queue.', directory: folder } })).json();
  const b = await (await page.request.post('/api/notes', { data: { title: `Graphs ${tag}`, body: 'A graph has nodes and edges.', directory: folder } })).json();
  let release;
  const late = new Promise((resolve) => { release = resolve; });
  await page.route(`**/api/notes/${b.id}/combine`, async (route) => {
    await late;
    const versions = async (id) => (await (await page.request.get(`/api/notes/${id}`)).json()).version;
    return route.fulfill({ json: { title: b.title, with_title: a.title, body: '## Graphs\nA graph has nodes and edges.\n\n## BFS\nBFS uses a queue.', added: ['![Tree](tree.png)'], kept: 0.7, kept_enough: false, base: await versions(b.id), with_base: await versions(a.id) } });
  });
  await page.goto(`/#/f/${folder}`);
  const from = page.locator(`.card[data-id="${a.id}"]`);
  const onto = page.locator(`.card[data-id="${b.id}"]`);
  await from.dragTo(onto);
  const sheet = page.locator('.sheet');
  await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
  await sheet.locator('#combine-go').click();
  await expect(sheet).toContainText('Combining your notes');
  release();
  await expect(sheet.locator('.combine-preview')).toContainText('BFS uses a queue.');
  await expect(sheet).toContainText('put back one thing');
  await expect(sheet.locator('.set-note.warn')).toContainText('about 70%');
  await sheet.locator('#combine-save').click();
  await expect(page.locator('.toast')).toContainText(`Combined into “${b.title}”`);
  await expect(page.locator(`.card[data-id="${a.id}"]`)).toHaveCount(0);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('BFS uses a queue.');
  await page.locator('.toast button', { hasText: 'Undo' }).click();
  await expect(page.locator(`.card[data-id="${a.id}"]`)).toBeVisible();
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toBe('A graph has nodes and edges.');
});

test('only the folders scroll in the sidebar; the places above and below stay put', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the sidebar is for wide windows');
  const tag = Date.now().toString(36);
  for (let i = 0; i < 40; i++) await page.request.post('/api/dirs', { data: { path: `scroll-${tag}-${String(i).padStart(2, '0')}` } });
  await page.setViewportSize({ width: 1280, height: 640 });
  await page.reload();
  const tree = page.locator('#side .side-tree');
  await expect(tree).toBeVisible();
  const settings = page.locator('#side .side-foot [data-action="settings"]');
  const record = page.locator('#side [data-action="record"]');
  const before = [await settings.boundingBox(), await record.boundingBox()];
  expect(await tree.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
  await tree.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await page.mouse.wheel(0, 2000);
  const after = [await settings.boundingBox(), await record.boundingBox()];
  expect(after.map((b) => Math.round(b.y))).toEqual(before.map((b) => Math.round(b.y)));
  expect(before[0].y + before[0].height).toBeLessThanOrEqual(640);
  const scrolled = await tree.evaluate((el) => el.scrollTop);
  expect(scrolled).toBeGreaterThan(0);
  await page.locator('#side [data-action="home"]').first().click();
  await expect.poll(() => tree.evaluate((el) => el.scrollTop)).toBe(scrolled);
});
