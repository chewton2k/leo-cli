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

async function dragBetween(page, source, target) {
  await source.scrollIntoViewIfNeeded();
  await source.evaluate(el => el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' }));
  const from = await source.boundingBox();
  expect(from).not.toBeNull();
  const x = from.x + Math.min(16, from.width / 2);
  const y = from.y + Math.min(20, from.height / 2);
  await page.mouse.move(x, y);
  await page.mouse.down();
  try {
    await page.mouse.move(x + 8, y + 8, { steps: 3 });
    await target.evaluate(el => el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' }));
    const into = await target.boundingBox();
    expect(into).not.toBeNull();
    const end = { x: into.x + into.width / 2, y: into.y + into.height / 2 };
    expect(await target.evaluate((el, point) => el.contains(document.elementFromPoint(point.x, point.y)), end), 'the drop target is visible above fixed controls').toBe(true);
    await page.mouse.move(end.x, end.y, { steps: 3 });
    await page.mouse.move(end.x, end.y);
  } finally {
    await page.mouse.up();
  }
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
    await dragBetween(page, card, page.locator(`main .folder[data-dir="${a}"]`));
    await expect(page.locator('.toast')).toContainText(`Moved “Drag me ${tag}” to ${a}`);
    expect(await where()).toBe(a);
    await expect(card).toHaveCount(0);
    await page.locator('.toast button').click();
    await expect(card).toBeVisible();
    await expect(card.locator('.card-meta')).not.toContainText(a);
    expect(await where()).toBe('');

    await dragBetween(page, card, page.locator(`#side [data-action="open-folder"][data-dir="${b}"]`));
    await expect.poll(where).toBe(b);

    await dragBetween(page, page.locator(`main .folder[data-dir="${b}"]`), page.locator(`main .folder[data-dir="${a}"]`));
    await expect(page.locator('.toast')).toContainText(`Moved ${b} into ${a}`);
    expect(await where()).toBe(`${a}/${b}`);
    await expect(page.locator(`main .folder[data-dir="${b}"]`)).toHaveCount(0);
    const inside = await (await page.request.get(`/api/dirs?parent=${a}`)).json();
    expect(inside.map((d) => d.name)).toEqual([b]);
    await page.locator('.toast button').click();
    await expect(page.locator(`main .folder[data-dir="${b}"]`)).toBeVisible();
    expect(await where()).toBe(b);

    await page.goto(`/#/f/${b}`);
    await dragBetween(page, page.locator(`.card[data-id="${note.id}"]`), page.locator('#side .side-row[data-action="home"]'));
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
    await dragBetween(page, page.locator(`.card[data-id="${note.id}"]`), chat.locator('#chat-input'));
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
    await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
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
    await expect.poll(async () => (await page.locator('#map').boundingBox()).width).toBeGreaterThan(map.width + 300);
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
    await expect(chat).not.toHaveClass(/chat-leaving/);
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
    await expect(chat.locator('.chat-mode')).toHaveText(['Chat', 'Study', 'Interview']);
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
    await chat.locator('.starter', { hasText: 'Start a study session on this note' }).click();
    await expect(chat.locator('.chat-ask')).toContainText('Which note should the session cover?');
    expect(asked.length).toBe(0);
    await chat.locator('#chat-pick-search').fill(`Sorting lecture ${tag}`);
    await chat.locator('.chat-pick-row', { hasText: `Sorting lecture ${tag}` }).click();
    await expect(chat.locator('.msg.leo').last()).toContainText('Here we go.');
    expect(asked[0].messages[0].text).toBe('Start a study session on this note');
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

  test('tapping Felix can tickle him, and about one tap in ten he turns into a monster and back', async ({ page }) => {
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const face = page.locator('#chat-face .felix');
    await page.evaluate(() => { Math.random = () => 0.99; });
    await face.click();
    await expect(face).toHaveClass(/\btickle\b/);
    await page.evaluate(() => { Math.random = () => 0.01; });
    await face.click();
    await expect(face).toHaveClass(/\bmonster\b/);
    await expect(face.locator('.felix-rawr')).toHaveCount(1);
    await face.click();
    await expect(face).toHaveClass(/\bmonster\b/, { timeout: 500 });
    await expect(face).not.toHaveClass(/\bmonster\b/, { timeout: 5000 });
    await expect(face.locator('.felix-monster')).toHaveCSS('opacity', '0');
  });

  test('Demon Felix from Settings has scary taps, and tickling either Felix too long swaps them for a moment', async ({ page }) => {
    await page.goto('/#/settings');
    await page.locator('[data-action="felix-pick"][data-felix="demon"]').click();
    await expect(page.locator('html')).toHaveAttribute('data-felix', 'demon');
    await page.reload();
    await expect(page.locator('[data-action="felix-pick"][data-felix="demon"]')).toHaveAttribute('aria-pressed', 'true');
    await page.locator('#chat-toggle').click();
    const face = page.locator('#chat-face .felix');
    await expect(face.locator('.felix-skin')).toHaveCSS('fill', 'rgb(123, 92, 207)');
    await expect(face.locator('.felix-monster')).toHaveCSS('opacity', '1');
    await page.evaluate(() => { Math.random = () => 0.5; });
    await face.click();
    await expect(face).toHaveClass(/\b(rawr|cackle|blaze|glare|swipe)\b/);
    await expect(face).not.toHaveClass(/\b(boop|hop|spin|giggle|tickle)\b/);
    await page.waitForTimeout(1600);
    for (let i = 0; i < 8; i++) await face.click({ delay: 10 });
    await expect(face).toHaveClass(/\bsweet\b/);
    await expect(face).not.toHaveClass(/\bsweet\b/, { timeout: 5000 });
    await page.locator('[data-chat="close"]').click();
    await expect(page.locator('aside.chat')).toBeHidden();
    await page.goto('/#/settings');
    await page.locator('[data-action="felix-pick"][data-felix="cute"]').click();
    await expect(page.locator('html')).not.toHaveAttribute('data-felix', /./);
    expect(await page.evaluate(() => localStorage.getItem('leo-felix'))).toBeNull();
    await page.locator('#chat-toggle').click();
    const cute = page.locator('#chat-face .felix');
    await expect(cute.locator('.felix-skin')).toHaveCSS('fill', 'rgb(180, 207, 231)');
    await page.waitForTimeout(1600);
    for (let i = 0; i < 8; i++) await cute.click({ delay: 10 });
    await expect(cute).toHaveClass(/\bmonster\b/);
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
    await page.evaluate(() => { Math.random = () => 0.5; });
    await face.click();
    await expect(face).toHaveClass(/boop|hop|spin|giggle|tickle/);
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
        await expect.poll(async () => {
          const list = await page.locator('main').boundingBox();
          return list.x + list.width;
        }, { message: `the page moves over at ${width}px` }).toBeLessThanOrEqual(panel.x + 1);
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
    await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
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
    const stored = writing.locator('[data-key-input="gemini"]');
    await expect(stored).toHaveValue('');
    await expect(stored).toHaveAttribute('placeholder', 'gm-…••••••••');
    await stored.fill('gm-typed-but-never-saved');
    await page.reload();
    await expect(writing.locator('[data-key-input="gemini"]'), 'an unsaved key changes nothing').toHaveAttribute('placeholder', 'gm-…••••••••');
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
    const uploaded = [];
    let finishReading;
    const reading = new Promise(resolve => { finishReading = resolve; });
    await page.route('**/api/chats/*/files', async (route) => {
      if (route.request().method() !== 'POST') return route.fulfill({ json: [] });
      const body = route.request().postDataJSON();
      uploaded.push(body);
      await reading;
      return route.fulfill({ status: 201, json: { id: 'doc-pic-1', name: body.name, chars: 40 } });
    });
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"That diagram shows a heap."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    await pastePicture(page, '#chat-input');
    const chip = page.locator('#chat #chat-refs .file-card');
    try {
      await expect(chip).toContainText('pasted-1.png');
      await expect(chip).toHaveClass(/reading/);
      await expect(chip.locator('.file-peek img')).toBeVisible();
    } finally {
      finishReading();
    }
    await expect(chip).not.toHaveClass(/reading/);
    await expect(chip).toContainText('pasted-1.jpg');
    await expect(chip.locator('.file-peek img')).toBeVisible();
    expect(uploaded).toHaveLength(1);
    expect(uploaded[0]).toMatchObject({ name: 'pasted-1.jpg', type: 'image/jpeg' });
    expect(Buffer.from(uploaded[0].data, 'base64').subarray(0, 3)).toEqual(Buffer.from([0xff, 0xd8, 0xff]));
    await page.locator('#chat-input').fill('what is this?');
    await page.locator('#chat-input').press('Enter');
    await expect(page.locator('#chat .msg.leo').last()).toContainText('That diagram shows a heap.');
    await expect(page.locator('#chat .msg.user .file-card img')).toHaveAttribute('src', /^data:image\/jpeg;base64,/);
    expect(asked[0].files).toEqual(['doc-pic-1']);
    await expect(chip).toHaveCount(0);
  });
});

test('hovering the storage bar names the part and its size, and highlights it', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'hovering needs a mouse');
  await page.request.post('/api/notes', { data: { title: 'Storage hover', body: 'x' } });
  await page.goto('/#/settings/storage');
  const seg = page.locator('.store-bar i').first();
  await expect(seg).toBeVisible();
  const name = await seg.getAttribute('data-name');
  const size = await seg.getAttribute('data-size');
  await seg.hover();
  const tip = page.locator('#store-tip');
  await expect(tip).toBeVisible();
  await expect(tip).toContainText(name);
  await expect(tip).toContainText(size);
  await expect(seg).toHaveClass(/on/);
  await expect(page.locator('.store-bar')).toHaveClass(/hovering/);
  await expect(page.locator(`.store-key.on`)).toContainText(name);
  await page.mouse.move(5, 5);
  await expect(tip).toBeHidden();
  await expect(page.locator('.store-bar')).not.toHaveClass(/hovering/);
  await seg.click();
  await expect(page.locator(`.store-area[data-area="${await seg.getAttribute('data-area')}"]`)).toHaveAttribute('open', '');
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
  async function lowContrast(page, within = 'body') {
    return page.evaluate((within) => {
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
      for (const el of document.querySelectorAll(`${within} *`)) {
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
    }, within);
  }

  for (const scheme of ['light', 'dark']) {
    test(`interview submission results read clearly in ${scheme} mode`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await practiceWithResults(page, `readable-results-${scheme}`);
      await page.locator('#interview-results').scrollIntoViewIfNeeded();
      await page.locator('.interview-review-more > summary').click();
      await page.waitForTimeout(300);
      expect(await lowContrast(page, '#interview-results')).toEqual([]);
      await page.locator('[data-interview="result-case"]').first().click();
      expect(await lowContrast(page, '#interview-results')).toEqual([]);
    });

    test(`filled danger controls read clearly in ${scheme} mode`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      const tag = `${test.info().project.name}-${scheme}-${Date.now()}`;
      const task = await (await page.request.post('/api/tasks', { data: { title: `Readable task ${tag}` } })).json();
      await page.goto('/#/calendar');
      await page.locator(`.sched-main[data-id="${task.id}"]:visible`).click();
      await expect(page.locator('.sheet .btn.danger')).toBeVisible();
      await page.waitForTimeout(300);
      expect(await lowContrast(page)).toEqual([]);
      await page.keyboard.press('Escape');
      await page.goto('/#/settings/storage');
      await page.locator('#tasks-restore').setInputFiles({ name: 'tasks.json', mimeType: 'application/json', buffer: Buffer.from('{"wrong":[]}') });
      await expect(page.locator('.toast.bad')).toContainText('Choose tasks.json');
      await page.waitForTimeout(300);
      expect(await lowContrast(page)).toEqual([]);
      await page.request.put(`/api/chats/other-${tag}`, { data: { title: `Other chat ${tag}`, mode: 'chat', messages: [{ role: 'user', text: 'Hello' }, { role: 'assistant', text: 'Hi' }] } });
      await page.request.put(`/api/chats/readable-${tag}`, { data: { title: `Readable chat ${tag}`, mode: 'chat', messages: [{ role: 'user', text: 'Hello' }, { role: 'assistant', text: 'Hi' }] } });
      await page.goto('/');
      await page.locator('#chat-toggle').click();
      const history = page.locator('.chat-history');
      if (await history.isHidden()) await page.getByRole('button', { name: 'Your chats', exact: true }).click();
      await history.locator(`[data-chat="forget"][data-id="readable-${tag}"]`).click();
      await expect(history.locator('.chat-history-x.sure')).toHaveText('Delete');
      await page.waitForTimeout(300);
      expect(await lowContrast(page)).toEqual([]);
    });

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
        await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== 'running' || a.effect.getTiming().iterations === Infinity));
        const low = await lowContrast(page);
        if (low.length) problems[name] = low;
      };
      for (const [name, hash] of [['folders', '#/'], ['folder', '#/f/readable'], ['note', `#/n/${note.id}`], ['search', '#/search/readable'], ['trash', '#/trash'], ['settings', '#/settings'], ['storage', '#/settings/storage'], ['record', '#/record'], ['calendar', '#/calendar'], ['map', '#/map']]) {
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

    await page.goto('/');
    await goPlace(page, 'storage');
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
    await expect(card.locator('.quiz-result'), 'what is said about the answer comes from Felix, not the page').toHaveCount(0);
    await expect(card).toHaveClass(/wrong/);
    await expect(card.locator('[data-chat="quiz-hint"]'), 'no hint once it is answered').toHaveCount(0);
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
    expect(told).toContain('why each of the other choices is wrong');
    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat .quiz-card')).toHaveClass(/wrong/, { timeout: 5000 });
    await expect(page.locator('#chat .quiz-option').first()).toBeDisabled();
    await expect(page.locator('#chat .quiz-reply')).toContainText('BFS needs the oldest');
  });

  test('a citation with lines opens the note at those lines, and one into an uploaded file shows that passage', async ({ page }) => {
    const tag = Date.now().toString(36);
    const note = await (await page.request.post('/api/notes', { data: { title: `Cited ${tag}`, body: '# Graphs\n\n## BFS\nUses a queue.\n\n## DFS\nGoes deep first.\nUses a stack.' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'lecture.txt'), 'Week 2\nBFS explores level by level.\nDFS goes as deep as it can before it backtracks.\n');
    await page.route('**/api/chat', (route) => route.fulfill(answerWith([
      { sources: [{ n: 1, id: note.id, title: note.title, folder: '', why: 'open' }] },
      { files: [{ n: 1, note: note.id, title: note.title, name: 'lecture.txt' }] },
      { t: 'DFS goes deep first [n1 L7-8]. The lecture says "DFS goes as deep as it can before it backtracks." [f1 L3]' },
      { done: true },
    ])));
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('where does it talk about DFS?');
    await chat.locator('#chat-input').press('Enter');
    const lineChip = chat.locator('.cite[data-line="7-8"]');
    await expect(lineChip).toContainText('lines 7–8');
    const fileChip = chat.locator('.cite-file');
    await expect(fileChip).toContainText('lecture.txt');
    await expect(fileChip).toContainText('line 3');
    await fileChip.click();
    const sheet = page.locator('.sheet');
    await expect(sheet.locator('h3')).toHaveText('lecture.txt');
    await expect(sheet.locator('.passage-text')).toHaveText('DFS goes as deep as it can before it backtracks.');
    await sheet.locator('[data-action="close"]').click();
    if (!(await lineChip.isVisible())) await page.locator('#chat-toggle').click();
    await lineChip.click();
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
    const marked = page.locator('#doc .blk-cited');
    await expect(marked).toHaveCount(2);
    await expect(marked.first()).toContainText('Goes deep first.');
    await expect(marked.last()).toContainText('Uses a stack.');
  });

  test('a hint helps without giving the answer away, and the card can still be answered', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      if (asked.length === 1) return route.fulfill(answerWith([{ sources: [] }, { quiz: { kind: 'multiple_choice', question: 'An interior stationary point of $f(x)=x^2+4\\cos x$ satisfies?', options: ['$x = 2\\sin x$', '$x = 0$'], answer: '$x = 2\\sin x$', explain: '' } }, { t: 'Try it.' }, { done: true }]));
      if (asked.length === 2) return route.fulfill(answerWith([{ sources: [] }, { t: 'Set the derivative $f\'(x)$ to zero first.' }, { done: true }]));
      if (asked.length === 3) return route.fulfill(answerWith([{ sources: [] }, { t: 'The derivative is $2x - 4\\sin x$.' }, { done: true }]));
      return route.fulfill(answerWith([{ sources: [] }, { t: '[[correct]] Well done! Setting $2x - 4\\sin x = 0$ gives it.' }, { done: true }]));
    });
    const chat = await openFelix(page);
    await chat.locator('#chat-input').fill('quiz me');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.quiz-card');
    await card.locator('[data-chat="quiz-hint"]').click();
    await expect(card.locator('.quiz-hint')).toContainText('Set the derivative');
    await expect(card.locator('[data-chat="quiz-hint"]')).toHaveText('Another hint');
    expect(asked[1].practice).toBe(true);
    expect(asked[1].mark, 'a hint is not a marking').toBe(false);
    expect(asked[1].messages.at(-1).text).toContain('Do not give the answer');
    await card.locator('[data-chat="quiz-hint"]').click();
    await expect(card.locator('.quiz-hint')).toHaveCount(2);
    await expect(card.locator('.quiz-hint').last()).toContainText('The derivative is');
    expect(asked[2].messages.at(-1).text).toContain('Hints you already gave me');
    await expect(card.locator('.quiz-option').first()).toBeEnabled();
    await card.locator('.quiz-option').first().click();
    await expect(card).toHaveClass(/right/);
    await expect(card.locator('.quiz-reply')).toContainText('Well done!');
    await expect(card.locator('.quiz-hint').first()).toContainText('Set the derivative');
    await expect(card.locator('[data-chat="quiz-hint"]'), 'no more hints once answered').toHaveCount(0);
    expect(asked[3].messages.at(-1).text).toContain('praise me briefly');
  });

  test('a fill in the blank and a free answer are both marked by Felix reading what was meant', async ({ page }) => {
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
    await box.pressSequentially('the earliest one', { delay: 20 });
    await box.press('Enter');
    await expect(blank).toHaveClass(/right/);
    await expect(blank.locator('.quiz-reply')).toContainText('Yes.');
    expect(asked[1].mark, 'a blank is marked by Felix reading the answer, not by matching text').toBe(true);
    expect(asked[1].messages.at(-1).text).toContain('Accepted answers: oldest or earliest.');
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

test('opening a practice while Felix is open switches him to that practice\'s chat', async ({ page }) => {
  const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
  const id = `practice-chat-${tag}`;
  const challenge = { id: 'practice-chat-problem', title: 'Pair sum', question: 'Find two indices.', minutes: 20, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter: 'def pair_sum(nums, target):\n    pass', difficulty: 'easy', examples: [{ input: '[2,3], 5', output: '[0,1]' }], constraints: ['One pair.'] } };
  const item = { id, revision: 1, framework: { title: `Switching practice ${tag}`, track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace: { answer: challenge.coding.starter, reasoning: '', board: [] }, problems: [{ challenge, workspace: { answer: challenge.coding.starter, reasoning: '', board: [] } }], attempts: [] };
  await page.request.put(`/api/chats/${id}`, { data: { title: `Practice chat ${tag}`, mode: 'interview', messages: [{ role: 'user', text: `Practice question ${tag}` }, { role: 'assistant', text: 'Practice answer' }] } });
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  const panel = page.locator('aside.chat');
  await expect(panel).toBeVisible();
  await expect(panel).not.toContainText(`Practice question ${tag}`);
  await page.evaluate((hash) => { location.hash = hash; }, `#/interviews/${id}`);
  await expect(panel).toContainText(`Practice question ${tag}`);
  await expect(panel.locator('.chat-mode.on')).toHaveText('Interview');
  await expect(page.locator('.toast')).toContainText(`Felix switched to this practice’s chat: Switching practice ${tag}`);
  await page.evaluate(() => { location.hash = '#/'; document.querySelectorAll('.toast').forEach((t) => t.remove()); });
  await page.evaluate((hash) => { location.hash = hash; }, `#/interviews/${id}`);
  await expect(page.locator('.interview-heading h1')).toHaveText(`Switching practice ${tag}`);
  await expect(panel).toContainText(`Practice question ${tag}`);
  await page.waitForTimeout(500);
  await expect(page.locator('.toast', { hasText: 'Felix switched' })).toHaveCount(0);
  await page.request.delete(`/api/chats/${id}`);
});

test('a problem shows its diagrams in the statement and under an example, in light and dark', async ({ page }) => {
  const id = 'diagram-problem-ui';
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 40"><title>Array</title><rect width="120" height="40" fill="#fff"/><text x="10" y="26" font-size="14">[2, 7, 11]</text></svg>';
  const challenge = { id: 'diagram-problem-01', title: 'Tree depth', question: 'Return the depth of the tree.\n\n```mermaid\nflowchart TD\n  A((3)) --> B((9))\n  A --> C((20))\n```', minutes: 20, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter: 'def depth(root):\n    pass', difficulty: 'easy', examples: [{ input: '[3,9,20]', output: '2', diagram: '```svg\n' + svg + '\n```' }, { input: '[]', output: '0' }], constraints: ['Up to 10^4 nodes.'] } };
  const item = { id, revision: 1, framework: { title: 'Pictures', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace: { answer: challenge.coding.starter, reasoning: '', board: [] }, problems: [{ challenge, workspace: { answer: challenge.coding.starter, reasoning: '', board: [] } }], attempts: [] };
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  for (const scheme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto(`/#/interviews/${id}`);
    const question = page.locator('#interview-pane-question');
    await expect(question.locator('.prose figure.diagram .diagram-art svg')).toBeVisible();
    await expect(question.locator('.prose figure.diagram .diagram-art')).toContainText('20');
    const examples = question.locator('.interview-example');
    await expect(examples).toHaveCount(2);
    const picture = examples.nth(0).locator('.interview-example-diagram figure.drawing img');
    await expect(picture).toBeVisible();
    await expect(picture).toHaveAttribute('alt', 'Array');
    expect(await picture.evaluate((img) => img.naturalWidth > 0)).toBe(true);
    await expect(examples.nth(1).locator('.interview-example-diagram')).toHaveCount(0);
  }
});

test('changing the language in Preferences rewrites the open problem\'s starting code, with Undo', async ({ page }) => {
  const id = 'language-switch-ui';
  const python = 'def pair_sum(nums, target):\n    pass';
  const java = 'class Solution {\n    public int[] pair_sum(int[] nums, int target) {\n        return new int[0];\n    }\n}';
  const mine = 'def pair_sum(nums, target):\n    seen = {}\n    return []';
  const challenge = { id: 'language-problem-01', title: 'Pair sum', question: 'Find two indices that add up to target.', minutes: 20, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter: python, difficulty: 'easy', examples: [{ input: '[2,3], 5', output: '[0,1]' }], constraints: ['One pair.'] } };
  let item = { id, revision: 1, framework: { title: 'Switching', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace: { answer: mine, reasoning: '', board: [] }, problems: [{ challenge, workspace: { answer: mine, reasoning: '', board: [] } }], attempts: [] };
  const asked = [];
  await page.route('**/api/interviews/*', (route) => {
    if (route.request().method() === 'PUT') {
      const body = route.request().postDataJSON();
      if (body.framework) item.framework = body.framework;
      if (body.workspace) item.workspace = body.workspace;
      item.revision++;
    }
    return route.fulfill({ json: item });
  });
  await page.route('**/api/interviews/*/language', async (route) => {
    const body = route.request().postDataJSON();
    asked.push(body);
    const previous = { language: item.challenge.coding.language, starter: item.challenge.coding.starter, answer: item.workspace.answer };
    const starter = body.restore ? body.restore.starter : java;
    const fresh = { ...item.challenge, coding: { ...item.challenge.coding, language: body.language, starter } };
    item = { ...item, revision: item.revision + 1, challenge: fresh, workspace: { ...item.workspace, answer: body.restore ? body.restore.answer : java }, problems: [{ challenge: fresh, workspace: item.workspace }] };
    return route.fulfill({ json: { item, changed: true, previous, replaced: true } });
  });
  await page.goto(`/#/interviews/${id}`);
  await expect(page.locator('#interview-answer')).toHaveValue(mine);
  await page.locator('[data-interview="configure"]').click();
  await page.locator('#interview-language').selectOption('Java');
  await page.locator('#interview-form-save').click();
  await expect(page.locator('#interview-answer')).toHaveValue(java);
  await expect(page.locator('label[for="interview-answer"]')).toHaveText('Java');
  expect(asked[0]).toEqual({ challenge: challenge.id, language: 'Java' });
  const toast = page.locator('.toast');
  await expect(toast).toContainText('now in Java');
  await expect(toast).toContainText('Your Python code is kept in Undo');
  await toast.getByRole('button', { name: 'Undo' }).click();
  await expect(page.locator('#interview-answer')).toHaveValue(mine);
  expect(asked[1]).toEqual({ challenge: challenge.id, language: 'Python', restore: { from: java, starter: python, answer: mine } });
});

test('Settings chooses light or dark for this browser, or follows the device', async ({ page }) => {
  const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/#/settings');
  const pick = (name) => page.locator(`[data-action="theme-pick"][data-theme="${name}"]`);
  await expect(pick('system')).toHaveAttribute('aria-pressed', 'true');
  const light = await background();
  await pick('dark').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(pick('dark')).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(background).not.toBe(light);
  const dark = await background();
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(await background()).toBe(dark);
  await expect(pick('dark')).toHaveAttribute('aria-pressed', 'true');
  await page.emulateMedia({ colorScheme: 'dark' });
  await pick('light').click();
  await expect.poll(background).toBe(light);
  await pick('system').click();
  await expect(page.locator('html')).not.toHaveAttribute('data-theme', /./);
  await expect.poll(background).toBe(dark);
  expect(await page.evaluate(() => localStorage.getItem('leo-theme'))).toBeNull();
  await page.emulateMedia({ colorScheme: 'light' });
  await expect.poll(background).toBe(light);
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

test('dropping one note on another combines them in the background with progress, then shows the result first, and Undo brings both back', async ({ page }) => {
  const tag = Date.now().toString(36);
  const folder = `combine-${tag}`;
  await page.request.post('/api/dirs', { data: { path: folder } });
  const a = await (await page.request.post('/api/notes', { data: { title: `BFS ${tag}`, body: 'BFS uses a queue.', directory: folder } })).json();
  const b = await (await page.request.post('/api/notes', { data: { title: `Graphs ${tag}`, body: 'A graph has nodes and edges.', directory: folder } })).json();
  const versions = async (id) => (await (await page.request.get(`/api/notes/${id}`)).json()).version;
  const job = { id: `job-${tag}`, into: b.id, with: a.id, into_title: b.title, with_title: a.title, state: 'working' };
  let percent = 30;
  let forgotten = false;
  await page.route(`**/api/notes/${b.id}/combine`, (route) => {
    expect(route.request().postDataJSON()).toEqual({ with: a.id });
    return route.fulfill({ json: job });
  });
  await page.route(`**/api/combines/${job.id}`, (route) => {
    if (route.request().method() === 'DELETE') { forgotten = true; return route.fulfill({ status: 204 }); }
    return route.fulfill({ json: job });
  });
  await page.route('**/api/activity', (route) => route.fulfill({ json: { tasks: forgotten ? [] : [{ kind: 'combine', label: `Combining “${a.title}” into “${b.title}”`, step: job.state === 'working' ? 'Felix is writing one note' : 'Ready to review', done: job.state === 'working' ? percent : 100, total: 100, href: `#combine/${job.id}`, ...(job.state === 'working' ? {} : { ready: true }) }] } }));
  await page.goto(`/#/f/${folder}`);
  await dragBetween(page, page.locator(`.card[data-id="${a.id}"]`), page.locator(`.card[data-id="${b.id}"]`));
  const sheet = page.locator('.sheet');
  await expect(sheet.locator('h3')).toHaveText('Combine these notes?');
  await expect(sheet).toContainText('runs in the background with a progress bar');
  await sheet.locator('#combine-go').click();
  await expect(sheet).toHaveCount(0);
  await expect(page.locator('.toast')).toContainText('Combining in the background');
  const panel = page.locator('#activity');
  await expect(panel).toContainText('Felix is writing one note · 30%');
  await expect(panel.locator('.activity-bar i')).toHaveAttribute('style', /width:\s*(3\d)%/);
  await page.goto('/#/');
  percent = 70;
  await expect(panel).toContainText('· 70%');
  Object.assign(job, { state: 'ready', result: { title: b.title, with_title: a.title, body: '## Graphs\nA graph has nodes and edges.\n\n## BFS\nBFS uses a queue.', added: ['![Tree](tree.png)'], kept: 0.7, kept_enough: false, base: await versions(b.id), with_base: await versions(a.id) } });
  await expect(page.locator('.toast')).toContainText('are combined');
  await expect(panel).toContainText('Ready to review');
  await expect(panel).toContainText('Ready in the background');
  await panel.locator('.activity-row').click();
  const review = page.locator('.sheet');
  await expect(review.locator('.combine-preview')).toContainText('BFS uses a queue.');
  await expect(review).toContainText('put back one thing');
  await expect(review.locator('.set-note.warn')).toContainText('about 70%');
  await review.locator('#combine-save').click();
  await expect(page.locator('.toast')).toContainText(`Combined into “${b.title}”`);
  await expect.poll(() => forgotten).toBe(true);
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toContain('BFS uses a queue.');
  await page.locator('.toast button', { hasText: 'Undo' }).click();
  await expect(page.locator('.toast')).toContainText('Both notes are back');
  expect((await (await page.request.get(`/api/notes/${b.id}`)).json()).body).toBe('A graph has nodes and edges.');
});

test('PDF export leaves out the tasks panel and every overlay, with the same full-width page on any screen', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the tasks panel is for wider windows');
  const tag = Date.now().toString(36);
  const note = await (await page.request.post('/api/notes', { data: { title: `Print ${tag}`, body: 'A paragraph to print.' } })).json();
  const task = await (await page.request.post('/api/tasks', { data: { title: `Print task ${tag}` } })).json();
  const covering = () => page.evaluate(() => [...document.body.querySelectorAll('*')].filter((el) => {
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return ['fixed', 'sticky'].includes(s.position) && s.display !== 'none' && s.visibility !== 'hidden' && r.width > 0 && r.height > 0;
  }).map((el) => el.id || String(el.className)));
  const layout = () => page.evaluate(() => {
    const main = document.querySelector('main');
    const body = getComputedStyle(document.body);
    return { left: Math.round(main.getBoundingClientRect().left), width: Math.round(main.getBoundingClientRect().width), page: document.documentElement.clientWidth, padding: [body.paddingLeft, body.paddingRight, getComputedStyle(main).paddingLeft] };
  });
  for (const width of [1440, 1100, 760]) {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ media: 'screen' });
    await page.goto(`/#/n/${note.id}`);
    await page.reload();
    await expect(page.locator('#title, .note-head h1').first()).toContainText(`Print ${tag}`);
    if (width >= 1000) await expect(page.locator('#tasks-window')).toBeVisible();
    await page.emulateMedia({ media: 'print' });
    expect(await covering(), `${width}px`).toEqual([]);
    const shape = await layout();
    expect(shape.padding, `${width}px`).toEqual(['0px', '0px', '0px']);
    expect(shape.left, `${width}px`).toBe(0);
    expect(shape.width, `${width}px`).toBe(shape.page);
  }
  await page.request.delete(`/api/tasks/${task.id}`);
});

test('only the folders scroll in the sidebar; the places above and below stay put', async ({ page }) => {
  test.skip(test.info().project.name === 'phone', 'the sidebar is for wide windows');
  const tag = Date.now().toString(36);
  for (let i = 0; i < 40; i++) await page.request.post('/api/dirs', { data: { path: `scroll-${tag}-${String(i).padStart(2, '0')}` } });
  await page.setViewportSize({ width: 1280, height: 640 });
  await page.reload();
  const tree = page.locator('#side .side-tree');
  await expect(tree).toBeVisible();
  expect((await tree.boundingBox()).height).toBeGreaterThanOrEqual(100);
  const settings = page.locator('#side .side-foot [data-action="settings"]');
  const record = page.locator('#side [data-action="record"]');
  const before = [await settings.boundingBox(), await record.boundingBox()];
  expect(await tree.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
  await tree.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await expect(page.locator('#side .side-top [data-dir=""]')).toBeInViewport();
  await page.mouse.wheel(0, 2000);
  const after = [await settings.boundingBox(), await record.boundingBox()];
  expect(after.map((b) => Math.round(b.y))).toEqual(before.map((b) => Math.round(b.y)));
  expect(before[0].y + before[0].height).toBeLessThanOrEqual(640);
  const scrolled = await tree.evaluate((el) => el.scrollTop);
  expect(scrolled).toBeGreaterThan(0);
  await page.locator('#side [data-action="home"]').first().click();
  await expect.poll(() => tree.evaluate((el) => el.scrollTop)).toBe(scrolled);
});

test('note thumbnails are read only, cards open directly and layout choices persist', async ({ page }) => {
  const made = await page.request.post('/api/notes', { data: { title: 'Library thumbnail fixture', body: '# Traversals\nBreadth first search visits each layer.\n- [ ] Homework due Friday\n```svg\n<svg xmlns="http://www.w3.org/2000/svg" width="160" height="80"><rect width="160" height="80" fill="#b4cfe7"/></svg>\n```' } });
  expect(made.status()).toBe(201);
  const note = await made.json();
  await page.goto('/#/');
  const card = page.locator(`.library-cards.grid .card[data-id="${note.id}"]`);
  await expect(card).toBeVisible();
  await expect(card.locator('.card-snippet')).toContainText('Breadth first search');
  await card.scrollIntoViewIfNeeded();
  await expect(card.locator('.note-thumb img')).toBeVisible();
  await expect(card.locator('.note-thumb input:enabled')).toHaveCount(0);
  await expect(page.locator('[data-action="note-preview"]')).toHaveCount(0);
  const aligned = await page.locator('[data-action="library-view"]').evaluateAll((buttons) => buttons.map((button) => {
    const icon = button.querySelector('svg').getBoundingClientRect();
    const text = button.querySelector('span').getBoundingClientRect();
    const box = button.getBoundingClientRect();
    return { offsets: [Math.abs(icon.y + icon.height / 2 - text.y - text.height / 2), Math.abs((icon.x + text.right) / 2 - box.x - box.width / 2)], gap: text.left - icon.right };
  }));
  for (const item of aligned) {
    for (const offset of item.offsets) expect(offset).toBeLessThanOrEqual(1);
    expect(item.gap).toBeGreaterThanOrEqual(8);
  }
  await page.locator('[data-action="library-view"][data-view="list"]').click();
  await expect(page.locator('.library-cards.list')).toBeVisible();
  await page.reload();
  await expect(page.locator('.library-cards.list')).toBeVisible();
  await expect(page.locator('[data-action="library-sort"]')).toHaveCount(0);
  await page.locator('[data-action="library-view"][data-view="grid"]').click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await card.click();
  await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
  await expect(page.locator('#doc')).toContainText('Homework due Friday');
});

test('notes come newest edit first, sort and show choices filter the folder and are remembered', async ({ page }) => {
  const dir = `library-sort-${Date.now()}`;
  const make = async (title, body) => (await (await page.request.post('/api/notes', { data: { title, directory: dir, body } })).json()).id;
  const zebra = await make('Zebra notes', '- [ ] feed the zebra');
  const apple = await make('Apple notes', 'Nothing to do.');
  await page.goto(`/#/f/${encodeURIComponent(dir)}`);
  const ids = () => page.locator('.library-cards .card').evaluateAll((els) => els.map((el) => el.dataset.id));
  await expect.poll(ids).toEqual([apple, zebra]);
  await page.locator('select[data-library="sort"]').selectOption('title-desc');
  await expect.poll(ids).toEqual([zebra, apple]);
  await page.locator('select[data-library="show"]').selectOption('tasks');
  await expect.poll(ids).toEqual([zebra]);
  await page.reload();
  await expect(page.locator('select[data-library="sort"]')).toHaveValue('title-desc');
  await expect(page.locator('select[data-library="show"]')).toHaveValue('tasks');
  await expect.poll(ids).toEqual([zebra]);
  await page.locator('select[data-library="show"]').selectOption('pinned');
  await expect(page.locator('.library-none')).toBeVisible();
  await page.locator('[data-action="library-show-all"]').click();
  await expect.poll(ids).toEqual([zebra, apple]);
  await page.locator('select[data-library="sort"]').selectOption('updated');
  await expect.poll(ids).toEqual([apple, zebra]);
});

test('grid and list keep the library and header width in every layout', async ({ page }) => {
  const dir = `library-width-${Date.now()}`;
  await page.request.post('/api/notes', { data: { title: 'Stable library width', directory: dir, body: 'Keep the layout steady.' } });
  const bounds = () => page.locator('main, .bar-row, .search').evaluateAll((els) => els.map((el) => {
    const rect = el.getBoundingClientRect();
    return { left: rect.left, width: rect.width };
  }));
  for (const folder of ['', dir]) {
    await page.goto(folder ? `/#/f/${encodeURIComponent(folder)}` : '/#/');
    const layouts = test.info().project.name === 'desktop' ? ['wide', 'mini', 'chat'] : ['phone'];
    for (const layout of layouts) {
      if (layout === 'mini') await page.locator('[data-action="side-fold"]').click();
      if (layout === 'chat') {
        await page.locator('[data-action="side-fold"]').click();
        await page.locator('#chat-toggle').click();
        await expect(page.locator('.chat')).toBeVisible();
      }
      for (const views of [['grid', 'list'], ['list', 'grid']]) {
        await page.locator(`[data-action="library-view"][data-view="${views[0]}"]`).click();
        const before = await bounds();
        await page.locator(`[data-action="library-view"][data-view="${views[1]}"]`).click();
        expect(await bounds()).toEqual(before);
        expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
      }
      if (layout === 'chat') {
        await page.locator('.chat [data-chat="close"]').click();
        await expect(page.locator('body')).not.toHaveClass(/chat-open|chat-leaving/);
      }
    }
  }
});

test('sidebar folders can move to the top level with their contents and Undo', async ({ page }) => {
  test.skip(test.info().project.name !== 'desktop', 'sidebar dragging needs a mouse');
  const tag = Date.now().toString(36);
  const from = `sidebar-root-${tag}/Nested-${tag}`;
  const root = from.split('/').pop();
  const note = await (await page.request.post('/api/notes', { data: { title: 'Nested folder move', directory: `${from}/Child`, body: 'Keep descendants.' } })).json();
  const where = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).directory;
  await page.goto(`/#/f/${encodeURIComponent(from)}`);
  const target = page.locator('#side [data-action="open-folder"][data-dir=""]');
  await expect(target).toHaveText('Top level');
  await dragBetween(page, page.locator(`#side [data-action="open-folder"][data-dir="${from}"]`), target);
  await expect.poll(where).toBe(`${root}/Child`);
  await expect(page).toHaveURL(new RegExp(`#/f/${encodeURIComponent(root)}$`));
  await expect(page.locator(`#side [data-action="open-folder"][data-dir="${root}"]`)).toBeVisible();
  await page.locator('.toast button', { hasText: 'Undo' }).last().click();
  await expect.poll(where).toBe(`${from}/Child`);
  await expect(page).toHaveURL(new RegExp(`#/f/${encodeURIComponent(from)}$`));
  const moves = [];
  page.on('request', request => { if (request.method() === 'POST' && new URL(request.url()).pathname === '/api/dirs/move') moves.push(request.postDataJSON()); });
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  const parent = from.split('/')[0];
  for (const [source, into] of [[parent, from], [from, parent], [parent, '']]) {
    const dragged = `#side [data-action="open-folder"][data-dir="${source}"]`;
    const dropped = `#side [data-action="open-folder"][data-dir="${into}"]`;
    await page.dispatchEvent(dragged, 'dragstart', { dataTransfer: transfer });
    await page.dispatchEvent(dropped, 'dragover', { dataTransfer: transfer });
    await expect(page.locator(dropped)).not.toHaveClass(/drop-ok/);
    await page.dispatchEvent(dropped, 'drop', { dataTransfer: transfer });
  }
  expect(moves).toEqual([]);
  await page.dispatchEvent(`#side [data-action="open-folder"][data-dir="${from}"]`, 'dragstart', { dataTransfer: transfer });
  await page.dispatchEvent('#side .side-tree', 'drop', { dataTransfer: transfer });
  await expect.poll(where).toBe(`${root}/Child`);
  expect(moves).toEqual([{ from, into: '' }]);
});

test('folder navigation keeps the header steady while loading and animates only content', async ({ page }) => {
  const dir = `folder-arrival-${Date.now()}`;
  await page.request.post('/api/notes', { data: { title: 'Folder arrival fixture', directory: dir, body: 'A smooth arrival.' } });
  await page.addInitScript(() => {
    const animate = Element.prototype.animate;
    window.folderAnimations = [];
    Element.prototype.animate = function (frames, options) {
      if (this.id === 'app') window.folderAnimations.push({ frames, options });
      return animate.call(this, frames, options);
    };
  });
  await page.reload();
  await page.goto('/#/');
  await expect(page.locator(`main .folder[data-dir="${dir}"]`)).toBeVisible();
  const header = page.locator('.bar-row');
  const before = await header.boundingBox();
  let release;
  const hold = new Promise(resolve => { release = resolve; });
  let waiting = false;
  await page.route('**/api/notes?*', async route => {
    if (new URL(route.request().url()).searchParams.get('dir') === dir) { waiting = true; await hold; }
    await route.continue();
  });
  await page.locator(`main .folder[data-dir="${dir}"]`).click();
  try {
    await expect.poll(() => waiting).toBe(true);
    const loading = await header.boundingBox();
    expect(loading.width).toBe(before.width);
    expect(loading.x).toBe(before.x);
  } finally { release(); }
  await expect(page.locator('.library-cards')).toContainText('Folder arrival fixture');
  await expect.poll(() => page.evaluate(() => window.folderAnimations.length)).toBe(1);
  await page.evaluate(() => { window.keptCrumb = document.querySelector('#crumbs button:last-child'); });
  for (const view of ['list', 'grid']) await page.locator(`[data-action="library-view"][data-view="${view}"]`).click();
  expect(await page.evaluate(() => window.keptCrumb === document.querySelector('#crumbs button:last-child'))).toBe(true);
  expect(await page.evaluate(() => window.folderAnimations.length)).toBe(1);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.locator('#crumbs [data-action="home"]').click();
  await expect(page.locator(`main .folder[data-dir="${dir}"]`)).toBeVisible();
  expect(await page.evaluate(() => window.folderAnimations.length)).toBe(1);
});

test('top level and folders show only their own notes and ignore old recency preferences', async ({ page }) => {
  const dir = `folder-scope-${Date.now()}`;
  const notes = [];
  for (const [title, directory] of [['Zebra scope fixture', ''], ['Alpha scope fixture', ''], ['Nested scope fixture', dir], ['Child scope fixture', `${dir}/Child`]]) {
    notes.push(await (await page.request.post('/api/notes', { data: { title, directory, body: 'Only in its own folder.' } })).json());
  }
  await page.addInitScript(() => localStorage.setItem('leo-library', JSON.stringify({ view: 'grid', sort: 'opened' })));
  await page.reload();
  await page.goto('/#/');
  await expect(page.locator(`.card[data-id="${notes[1].id}"]`)).toBeVisible();
  for (const view of ['grid', 'list']) {
    await page.locator(`[data-action="library-view"][data-view="${view}"]`).click();
    await expect(page.locator(`.card[data-id="${notes[2].id}"], .card[data-id="${notes[3].id}"]`)).toHaveCount(0);
    const ids = await page.locator('.card[data-id]').evaluateAll(els => els.map(el => el.dataset.id));
    expect(ids.indexOf(notes[1].id)).toBeLessThan(ids.indexOf(notes[0].id));
    await expect(page.locator('[data-action="library-sort"]')).toHaveCount(0);
  }
  await page.goto(`/#/f/${encodeURIComponent(dir)}`);
  await expect(page.locator('.card[data-id]')).toHaveCount(1);
  await expect(page.locator(`.card[data-id="${notes[2].id}"]`)).toBeVisible();
});

test('folder controls rename, move to root and delete from inside the folder with Undo', async ({ page }) => {
  const original = `folder-controls-${Date.now()}/nested`;
  const made = await page.request.post('/api/notes', { data: { title: 'Folder controls fixture', directory: original, body: 'Keep this note through renaming and moving.' } });
  expect(made.status()).toBe(201);
  const note = await made.json();
  const renamed = `Renamed-${note.id}`;
  const full = original.split('/')[0] + '/' + renamed;
  await page.goto(`/#/f/${encodeURIComponent(original)}`);
  await page.locator('[data-action="folder-options"]').click();
  await page.locator('[data-action="folder-rename"]').click();
  await expect(page.locator('#folder-rename')).toBeFocused();
  await page.locator('#folder-rename').fill(renamed);
  await page.locator('#folder-rename').press('Enter');
  await expect(page).toHaveURL(new RegExp(`#/f/${encodeURIComponent(full)}$`));
  await expect(page.locator(`.card[data-id="${note.id}"]`)).toBeVisible();
  if (test.info().project.name === 'desktop') {
    await dragBetween(page, page.locator(`#crumbs [data-dir="${full}"]`), page.locator('#crumbs [data-action="home"]'));
  } else {
    await page.locator('[data-action="folder-options"]').click();
    await page.locator('[data-action="folder-root"]').click();
  }
  await expect(page).toHaveURL(new RegExp(`#/f/${encodeURIComponent(renamed)}$`));
  await page.locator('[data-action="folder-options"]').click();
  await page.locator('[data-action="folder-delete"]').click();
  await page.locator('[data-action="folder-delete-now"]').click();
  await expect(page).toHaveURL(/#\/$/);
  expect((await page.request.get(`/api/notes/${note.id}`)).status()).toBe(404);
  await page.locator('.toast button', { hasText: 'Undo' }).last().click();
  await expect(async () => expect((await page.request.get(`/api/notes/${note.id}`)).status()).toBe(200)).toPass();
});

test('dropdowns and nested disclosures share real icons and keep their native controls', async ({ page }) => {
  const inspect = async () => {
    const selects = page.locator('select:visible');
    expect(await selects.count()).toBeGreaterThan(0);
    const issues = await selects.evaluateAll(els => els.flatMap(el => {
      const wrap = el.closest('.select-control');
      const arrow = wrap && wrap.querySelector(':scope > svg[aria-hidden="true"]');
      if (!arrow) return [`${el.id || el.dataset.set || el.dataset.keep || el.dataset.library}: missing chevron`];
      const box = el.getBoundingClientRect();
      const icon = arrow.getBoundingClientRect();
      const style = getComputedStyle(el);
      return Math.abs(box.y + box.height / 2 - icon.y - icon.height / 2) > 1 || icon.right > box.right - 4 || icon.left < box.left || parseFloat(style.paddingRight) < icon.width + 12
        ? [`${el.id || el.dataset.set || el.dataset.keep || el.dataset.library}: misaligned chevron`]
        : [];
    }));
    expect(issues).toEqual([]);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  };
  await page.goto('/#/');
  await inspect();
  const sort = page.locator('select[data-library="sort"]');
  await sort.focus();
  await expect(sort).toBeFocused();
  await sort.selectOption('title');
  await expect(sort).toHaveValue('title');
  await page.locator('[data-action="library-view"][data-view="list"]').click();
  const align = await page.locator('.library-views .btn').evaluateAll(els => els.map(el => {
    const icon = el.querySelector('svg').getBoundingClientRect();
    const box = el.getBoundingClientRect();
    return Math.abs(icon.y + icon.height / 2 - box.y - box.height / 2);
  }));
  expect(align.every(offset => offset < 1)).toBe(true);
  await page.goto('/#/settings');
  await expect(page.locator('[data-task-card="writing"]')).toBeVisible();
  await inspect();
  await page.goto('/#/settings/storage');
  await expect(page.locator('select[data-keep="trash_days"]')).toBeVisible();
  await inspect();
  await page.goto('/#/');
  await control(page, 'upload').click();
  await expect(page.locator('#upload-dir')).toBeVisible();
  await inspect();
  await page.locator('.sheet [data-action="close"]').click();
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job: null, pending: [] } }));
  await page.route('**/api/calendar', route => route.fulfill({ json: { connected: true, tasks: [], events: [{ id: 'controls-event', title: 'A class with a long title for the folder selector', start: new Date().toISOString(), end: new Date(Date.now() + 3600000).toISOString(), all_day: false }], warnings: [] } }));
  await page.goto('/#/record');
  await expect(page.locator('#rec-event')).toBeVisible();
  await inspect();
  await page.locator('#rec-event').selectOption('');
  await expect(page.locator('#rec-event')).toHaveValue('');
  await page.emulateMedia({ forcedColors: 'active' });
  await expect(page.locator('#rec-dir').locator('..').locator('svg')).toBeHidden();
  expect(await page.locator('#rec-dir').evaluate(el => getComputedStyle(el).appearance)).toBe('auto');
  await page.emulateMedia({ forcedColors: 'none' });
  await page.goto('/#/calendar');
  await page.locator('#app [data-action="task-new"]').first().click();
  const details = page.locator('.task-more');
  await expect(details.locator('summary > svg')).toHaveCount(1);
  await details.locator('summary').focus();
  await page.keyboard.press('Enter');
  await expect(details).toHaveAttribute('open', '');
  await page.locator('.sheet [data-action="close"]').click();
  const id = 'controls-interview';
  const challenge = { id: 'controls-challenge', title: 'Pair sum', question: 'Find a pair.', minutes: 25, skills: ['Approach'], origin: 'generated', sources: [] };
  const workspace = { answer: 'My submitted approach', reasoning: '', board: [] };
  await page.route('**/api/interviews/*', route => route.fulfill({ json: { id, revision: 1, framework: { title: 'Coding practice', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [{ id: 'controls-attempt', at: new Date().toISOString(), challenge, workspace, feedback: null }], schedule: [] } }));
  await page.goto(`/#/interviews/${id}`);
  await page.locator('#interview-panes [data-pane="submissions"]').click();
  const attempt = page.locator('.interview-attempt');
  await expect(attempt.locator(':scope > summary > svg')).toHaveCount(1);
  await attempt.locator(':scope > summary').click();
  const submitted = attempt.locator('details');
  const angle = el => getComputedStyle(el).transform;
  await expect(submitted.locator('summary > svg')).toHaveCount(1);
  expect(await submitted.locator('summary > svg').evaluate(angle)).toBe('none');
  await submitted.locator('summary').focus();
  await page.keyboard.press('Enter');
  await expect(submitted).toHaveAttribute('open', '');
  await expect(submitted.locator('.interview-submitted')).toHaveText(workspace.answer);
  await expect.poll(() => submitted.locator('summary > svg').evaluate(angle)).toBe('matrix(0, 1, -1, 0, 0, 0)');
  await page.locator('#chat-toggle').click();
  await page.locator('[data-chat="attach"]').click();
  await expect(page.locator('#chat-attach-menu [data-chat="attach-note"] > svg')).toHaveCount(1);
  await expect(page.locator('#chat-attach-menu [data-chat="attach-file"] > svg')).toHaveCount(1);
  await expect(page.locator('[data-chat="attach"]')).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('[data-chat="attach-note"]')).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(page.locator('[data-chat="attach-file"]')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#chat-attach-menu')).toBeHidden();
  await expect(page.locator('[data-chat="attach"]')).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('[data-chat="attach"]')).toBeFocused();
  await expect(page.locator('#chat')).toBeVisible();
  await page.locator('[data-chat="access"]').click();
  await page.keyboard.press('End');
  await expect(page.locator('[data-chat="access-pick"][data-access="read"]')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#chat-access-menu')).toBeHidden();
  await expect(page.locator('#chat-access')).toBeFocused();
  await page.route('**/api/chat', route => route.fulfill({ contentType: 'application/x-ndjson', body: [{ step: 'Found relevant notes', tool: 'search_notes', found: ['Queues', 'Heaps'] }, { t: 'Here are the notes.' }, { done: true }].map(line => JSON.stringify(line)).join('\n') + '\n' }));
  await page.locator('#chat-input').fill('Find my algorithm notes');
  await page.locator('#chat-input').press('Enter');
  const step = page.locator('details.msg-step').last();
  await expect(step.locator('summary > .msg-step-chevron')).toHaveCount(1);
  await step.locator('summary').click();
  await expect(step.locator('li')).toHaveText(['Queues', 'Heaps']);
  await page.locator('[data-chat="close"]').click();
  const job = { id: 'controls-failed', source: 'call', state: 'failed', secs: 0, step: '', steps: null, transcript: '', warnings: ['Recovered audio from the last interrupted recording.'], points: [], wants: '', levels: [], levels_start: 0, next_seq: 0, note: null, error: 'Others: No sound was recorded.; You: No sound was recorded.' };
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job, pending: [] } }));
  await page.route('**/api/record/controls-failed', route => route.fulfill({ json: job }));
  await page.goto('/#/record');
  await expect(page.locator('.rec-error-details summary > svg')).toHaveCount(1);
  await page.locator('.rec-error-details summary').click();
  await expect(page.locator('.rec-error-details')).toHaveAttribute('open', '');
});

test('a failed call has calm details and can be dismissed or deleted directly', async ({ page }) => {
  let job = { id: 'failed-call-fixture', source: 'call', state: 'failed', secs: 0, step: '', steps: null, transcript: '', warnings: ['Recovered audio from the last interrupted recording.'], points: [], wants: '', levels: [], levels_start: 0, next_seq: 0, note: null, error: 'Others: No sound was recorded. Check your microphone.; You: No sound was recorded. Check your microphone.' };
  let deletions = 0;
  await page.route('**/api/record', route => route.fulfill({ json: { available: true, local: true, job, pending: [] } }));
  await page.route('**/api/record/failed-call-fixture', route => {
    if (route.request().method() === 'DELETE') { deletions++; job = null; return route.fulfill({ json: { deleted: true } }); }
    return route.fulfill({ json: job });
  });
  await page.goto('/#/record');
  await expect(page.locator('.rec-failed-card h3')).toHaveText('No audio was captured');
  await expect(page.locator('.rec-error-details')).not.toHaveAttribute('open', '');
  await expect(page.locator('.rec-failed-card .hint')).not.toContainText('Others:');
  await page.locator('[data-action="rec-dismiss"]').click();
  await expect(page.locator('.rec-idle')).toBeVisible();
  await page.reload();
  await expect(page.locator('.rec-idle')).toBeVisible();
  expect(deletions).toBe(0);
  await page.evaluate(() => sessionStorage.removeItem('leo-record-dismissed'));
  await page.reload();
  await expect(page.locator('.rec-failed-card')).toBeVisible();
  await page.evaluate(async () => {
    const queue = window.leoAudioQueue.indexed();
    await queue.put({ id: 'failed-call-fixture', seq: 0, bytes: new ArrayBuffer(4) });
    await queue.put({ id: 'keep-fixture', seq: 0, bytes: new ArrayBuffer(4) });
  });
  await page.locator('[data-action="rec-discard"]').click();
  await page.locator('.scrim [data-action="close"]').click();
  expect(deletions).toBe(0);
  await page.locator('[data-action="rec-discard"]').click();
  await page.locator('[data-action="rec-discard-now"]').click();
  await expect(page.locator('.rec-idle')).toBeVisible();
  expect(deletions).toBe(1);
  expect(await page.evaluate(() => window.leoAudioQueue.indexed().count('failed-call-fixture'))).toBe(0);
  expect(await page.evaluate(() => window.leoAudioQueue.indexed().count('keep-fixture'))).toBe(1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await page.reload();
  await expect(page.locator('.rec-idle')).toBeVisible();
});

test('interview practice starts without a note, keeps its style and has no practice schedule', async ({ page }) => {
  await page.request.post('/api/tasks', { data: { title: 'A task alongside interview practice' } });
  await page.evaluate(() => localStorage.setItem('leo-tasks-folded', '0'));
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: '{"t":"Ready to practise."}\n{"done":true}\n' }));
  await page.goto('/#/interviews');
  await page.locator('[data-interview="create"][data-track="systems"]').click();
  await page.getByText('More preferences', { exact: true }).click();
  await expect(page.locator('#interview-budget')).toHaveCount(0);
  await page.locator('#interview-title').fill('System design interview');
  await page.locator('#interview-company').fill('Example company');
  await page.locator('[data-interview="form-style"][data-style="mock"]').click();
  await page.locator('#interview-form-save').click();
  await expect(page.locator('.interview-heading h1')).toHaveText('System design interview');
  await expect(page.locator('[data-chat="close"]')).toBeVisible();
  await page.locator('[data-chat="close"]').click();
  const id = decodeURIComponent(new URL(page.url()).hash.split('/').pop());
  if (!test.info().project.name.includes('phone')) {
    const panel = page.locator('#tasks-window');
    await expect(panel.locator('.tasks-body')).toHaveCount(0);
    await panel.locator('[data-action="tasks-fold"]').click();
    await expect(panel.locator('.tasks-body')).toBeVisible();
    await panel.locator('[data-action="tasks-fold"]').click();
    expect(await page.evaluate(() => localStorage.getItem('leo-tasks-folded'))).toBe('0');
  }
  await expect(page.getByText('Practice schedule', { exact: true })).toHaveCount(0);
  await expect(page.locator('[data-interview="plan"], [data-interview="add-session"]')).toHaveCount(0);
  expect((await (await page.request.get(`/api/interviews/${id}`)).json()).schedule).toBeUndefined();
  await page.reload();
  await expect(page.locator('.interview-heading .eyebrow')).toContainText('Mock interview');
  await expect(page.locator('.interview-heading h1')).toHaveText('System design interview');
  await page.request.delete(`/api/interviews/${id}`);
});

test('interview answers keep their focus and draft through saves, a conflict and reload', async ({ page }) => {
  const id = 'interview-browser-01';
  let item = { id, revision: 1, updated: new Date().toISOString(), framework: { title: 'Coding practice', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach', 'Testing'], wants: '' }, challenge: { id: 'challenge-browser-01', title: 'Pair sum', question: 'Find two distinct indices whose values sum to a target. Explain duplicates.', minutes: 25, skills: ['Approach', 'Testing'], origin: 'generated', sources: [] }, workspace: { answer: '', board: [] }, attempts: [], schedule: [] };
  let conflict = false;
  let holdSave = false;
  let releaseSave;
  let saveStarted;
  const files = new Map();
  let uploads = 0;
  let refuseImage = false;
  await page.route(`**/api/interviews/${id}`, async (route) => {
    if (route.request().method() === 'PUT') {
      const change = route.request().postDataJSON();
      if (conflict) { conflict = false; return route.fulfill({ status: 409, json: { error: 'This practice changed in another window. Reload before saving; your draft is kept.' } }); }
      if (holdSave) { saveStarted(); await new Promise((resolve) => { releaseSave = resolve; }); holdSave = false; }
      await new Promise((resolve) => setTimeout(resolve, 100));
      if (change.action === 'submit') {
        item.attempts.push({ id: change.attempt, at: new Date().toISOString(), challenge: structuredClone(item.challenge), workspace: change.workspace, feedback: null });
        item.workspace = change.workspace;
      } else item.workspace = change.workspace;
      item.revision++;
    }
    await route.fulfill({ json: item });
  });
  await page.route(`**/api/chats/${id}`, (route) => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No conversation yet' } }) : route.fulfill({ json: { id } }));
  await page.route(`**/api/chats/${id}/files**`, async (route) => {
    const request = route.request();
    if (request.method() === 'DELETE') { files.delete(request.url().split('/').at(-1)); return route.fulfill({ status: 204 }); }
    if (request.method() === 'POST') {
      if (refuseImage) return route.fulfill({ status: 422, json: { error: 'This model cannot read images.' } });
      const body = request.postDataJSON();
      expect(body.type).toMatch(/^image\//);
      expect(body.data.length).toBeGreaterThan(100);
      const doc = { id: String(++uploads).padStart(32, '0'), name: body.name, excerpt: 'Client and server whiteboard.' };
      files.set(doc.id, doc);
      return route.fulfill({ json: doc });
    }
    return route.fulfill({ json: [...files.values()] });
  });
  await page.route('**/api/chat', (route) => {
    if (refuseImage) expect(route.request().postDataJSON().messages.at(-1).text).toContain('freehand drawing details could not be assessed');
    const attempt = item.attempts.at(-1);
    attempt.feedback = { attempt: attempt.id, summary: 'Good approach; review edge cases.', scores: [{ skill: 'Approach', value: 3, evidence: 'You described a hash map.', next: 'Trace duplicates.' }] };
    item.revision++;
    return route.fulfill({ contentType: 'application/x-ndjson', body: `${JSON.stringify({ interview: { id, revision: item.revision } })}\n${JSON.stringify({ t: 'A good start.' })}\n${JSON.stringify({ done: true })}\n` });
  });
  await page.goto(`/#/interviews/${id}`);
  const answer = page.locator('#interview-answer');
  await answer.fill('Use a hash map. Check duplicates.');
  await answer.evaluate((el) => el.setSelectionRange(4, 8));
  await expect.poll(() => item.workspace.answer).toBe('Use a hash map. Check duplicates.');
  await expect(answer).toBeFocused();
  expect(await answer.evaluate((el) => [el.selectionStart, el.selectionEnd])).toEqual([4, 8]);
  conflict = true;
  await answer.fill('Use a hash map. Check duplicates and negatives.');
  await expect(page.locator('#interview-save-error')).toBeVisible();
  await page.locator('[data-interview="retry"]').click();
  await expect.poll(() => item.workspace.answer).toBe('Use a hash map. Check duplicates and negatives.');
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  await boardLabel(page, 'Client → Server', 0.15, 0.2);
  await expect.poll(() => item.workspace.board.length).toBe(1);
  await expect(page.locator('#interview-board text')).toHaveText('Client → Server');
  await page.reload();
  await expect(answer).toHaveValue('Use a hash map. Check duplicates and negatives.');
  await expect(page.locator('#interview-board text')).toHaveText('Client → Server');
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await page.locator('[data-interview="submit"]').click();
  await expect(page.locator('#interview-progress')).toContainText('1 reviewed attempt');
  await page.locator('[data-chat="close"]').click();
  expect(item.attempts[0].workspace.answer).toBe('Use a hash map. Check duplicates and negatives.');
  expect(item.attempts[0].workspace.board[0].text).toBe('Client → Server');
  await expect(answer).toHaveValue('Use a hash map. Check duplicates and negatives.');
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  await boardLabel(page, 'Another drawing', 0.15, 0.45);
  await page.locator('[data-interview="submit"]').click();
  await expect(page.locator('#interview-progress')).toContainText('2 reviewed attempts');
  expect(item.attempts[0].workspace.board).toHaveLength(1);
  expect(item.workspace.board).toHaveLength(2);
  await page.locator('[data-chat="close"]').click();
  expect(files.size).toBe(1);
  expect(uploads).toBe(2);
  refuseImage = true;
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  await boardLabel(page, 'A readable label', 0.15, 0.7);
  await page.locator('[data-interview="submit"]').click();
  await expect(page.locator('#interview-progress')).toContainText('3 reviewed attempts');
  await expect(page.locator('#chat .msg.user').last()).not.toContainText('interview_feedback');
  await page.locator('[data-chat="close"]').click();
  await page.locator('[data-interview="tab"][data-tab="code"]').click();

  holdSave = true;
  const started = new Promise((resolve) => { saveStarted = resolve; });
  await answer.fill('A hint request can be cancelled by navigation.');
  await page.locator('[data-interview="hint"]').click();
  await started;
  await page.evaluate(() => { location.hash = '#/interviews'; });
  await expect(page.locator('h1')).toHaveText('Interview practice');
  const response = page.waitForResponse((r) => r.url().endsWith(`/api/interviews/${id}`) && r.request().method() === 'PUT');
  releaseSave();
  await (await response).finished();
  await page.waitForTimeout(500);
  await expect(page.locator('body')).not.toHaveClass(/chat-open/);
});

test.describe('passages', () => {
  test('selected note text goes to Felix as a passage he is asked about, not an edit', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps', body: 'Intro line\n\nA binary heap keeps the minimum at the root.\n\nLast line' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const lines = [{ sources: [{ n: 1, id: note.id, title: 'Heaps', folder: '', why: 'open' }] }, { t: 'Because the heap property holds [n1].' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#doc .blk').nth(2).click();
    const area = page.locator('#doc textarea.line-edit');
    await expect(area).toHaveCount(1);
    await area.evaluate((a) => {
      const at = a.value.indexOf('keeps the minimum');
      a.focus();
      a.setSelectionRange(at, at + 'keeps the minimum'.length);
      a.dispatchEvent(new Event('select'));
    });
    const ask = page.locator('.ask-float');
    await expect(ask).toBeVisible();
    await ask.click();
    const chat = page.locator('#chat');
    await expect(chat).toBeVisible();
    await expect(chat.locator('.chat-passage')).toContainText('keeps the minimum');
    await chat.locator('#chat-input').fill('Why is that true?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('heap property');
    expect(asked[0].selection).toEqual({ title: 'Heaps', from: 3, to: 3, text: 'keeps the minimum' });
    expect(asked[0].refs).toContain(note.id);
    expect(asked[0].messages.at(-1).text).toBe('Why is that true?');
    await expect(chat.locator('.msg.user .msg-passage')).toContainText('Heaps · line 3');
    await expect(chat.locator('.chat-passage')).toHaveCount(0);
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('Intro line\n\nA binary heap keeps the minimum at the root.\n\nLast line');
  });

  test('the toolbar asks about the selection too, and says so when nothing is selected', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Toolbar ask', body: 'Only line here' } })).json();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('[data-action="ask-selection"]').click();
    await expect(page.locator('.toast')).toContainText('Select some text in the note first');
  });

  test('part of an answer is added to the open note exactly once, and Undo takes it out', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Collect', body: 'Mine already' } })).json();
    await page.route('**/api/chat', async (route) => {
      const lines = [{ sources: [{ n: 1, id: note.id, title: 'Collect', folder: '', why: 'open' }] }, { t: 'First idea stays out.\n\nThe **key** point is here [n1].\n\nLast bit.' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('Tell me');
    await chat.locator('#chat-input').press('Enter');
    const answer = chat.locator('.msg.leo').last();
    await expect(answer).toContainText('Last bit');
    await answer.locator('strong', { hasText: 'key' }).evaluate((el) => {
      const range = document.createRange();
      range.selectNodeContents(el.parentElement);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    });
    const add = chat.locator('.float-act[data-chat="add-picked"]');
    await expect(add).toBeVisible();
    await add.click();
    const sheet = page.locator('.sheet');
    await expect(sheet.locator('.add-title')).toHaveText('Collect');
    await expect(sheet.locator('.add-preview')).toContainText('The key point is here');
    await expect(sheet.locator('.add-preview')).not.toContainText('First idea');
    await sheet.locator('#add-go').dblclick();
    await expect(page.locator('.toast')).toContainText('Added to “Collect”');
    const body = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).body;
    await expect.poll(body).toBe('Mine already\n\nThe **key** point is here [[Collect]].\n');
    await page.locator('.toast button').click();
    await expect.poll(body).toBe('Mine already');
  });
});

test.describe('study sessions', () => {
  test('a session shows its ideas, ticks one off, and ends with a recap', async ({ page }) => {
    const note = await (await page.request.post('/api/notes', { data: { title: 'Heaps', body: 'A heap keeps the minimum at the root.' } })).json();
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      const source = { n: 1, id: note.id, title: 'Heaps', folder: '', why: 'open' };
      const lines = asked.length === 1
        ? [{ sources: [source] }, { step: 'Planned a study session', tool: 'study_plan', found: [] }, { plan: { ideas: ['Root', 'Sift down'] } }, { quiz: { kind: 'multiple_choice', question: 'What sits at the root?', options: ['the minimum', 'the maximum'], answer: 'the minimum', idea: 1 } }, { t: 'First one.' }, { done: true }]
        : [{ sources: [source] }, { t: '[[correct]] Yes, the minimum.' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto(`/#/n/${note.id}`);
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-mode="study"]').click();
    await chat.locator('#chat-input').fill('quiz me on heaps');
    await chat.locator('#chat-input').press('Enter');
    const progress = chat.locator('#study-progress');
    await expect(progress).toBeVisible();
    await expect(progress).toContainText('Root');
    await expect(progress).toContainText('0 of 2');
    await chat.locator('.quiz-card button', { hasText: 'the minimum' }).first().click();
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].session).toEqual({ ideas: ['Root', 'Sift down'], closed: [true, false], asked: 1 });
    await expect(progress).toContainText('1 of 2');
    await expect(progress.locator('.study-idea.first')).toContainText('Root');
    await progress.locator('[data-chat="session-finish"]').click();
    await expect(progress).toBeHidden();
    const recap = chat.locator('.session-recap');
    await expect(recap).toContainText('1 of 2 ideas covered');
    await expect(recap).toContainText('Not reached: Sift down');
    await recap.locator('[data-chat="session-done"]').click();
    await expect(recap).toHaveCount(0);
  });
});

test.describe('reading', () => {
  test('related notes sit under a note and open with a tap', async ({ page }) => {
    const other = await (await page.request.post('/api/notes', { data: { title: 'Graph basics', body: 'Vertices and edges.' } })).json();
    const note = await (await page.request.post('/api/notes', { data: { title: 'Dijkstra', body: 'Builds on [[Graph basics]].' } })).json();
    await page.goto(`/#/n/${note.id}`);
    const related = page.locator('#related');
    await expect(related).toBeVisible();
    await expect(related.locator('.related-row')).toHaveCount(1);
    await expect(related).toContainText('Graph basics');
    await expect(related).toContainText('Linked from this note');
    await related.locator('.related-row').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${other.id}$`));
    await expect(page.locator('#related')).toBeHidden();
  });

  test('a long note opens again where it was being read', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'scrolled with a mouse wheel');
    const body = Array.from({ length: 160 }, (_, i) => `Paragraph number ${i + 1} about reading.`).join('\n\n');
    const note = await (await page.request.post('/api/notes', { data: { title: 'Long read', body } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc .blk').first()).toBeVisible();
    await page.mouse.move(200, 300);
    await page.mouse.wheel(0, 2400);
    await expect.poll(async () => (await (await page.request.get(`/api/notes/${note.id}/reading`)).json()).anchor, { timeout: 8000 }).not.toBeNull();
    const saved = (await (await page.request.get(`/api/notes/${note.id}/reading`)).json()).anchor;
    expect(saved.text).toMatch(/^Paragraph number \d+/);
    await page.goto('/#/');
    await page.goto(`/#/n/${note.id}`);
    await expect.poll(() => page.evaluate(() => window.scrollY), { timeout: 5000 }).toBeGreaterThan(1500);
  });
});

test.describe('calendar', () => {
  test('tasks are added, dated, ticked off and sent to Google Calendar from the Calendar page', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.addInitScript(() => {
      window.__opened = [];
      window.open = (url) => {
        window.__opened.push(String(url));
        return null;
      };
    });
    const undated = await (await page.request.post('/api/tasks', { data: { title: `Ask tutor ${tag}` } })).json();
    const note = await (await page.request.post('/api/notes', { data: { title: `Lecture ${tag}`, body: '## Action items\n- [ ] Submit HW 4 — due October 16, 2026 at 11:59 pm.\n## Check yourself\n- [ ] What is a heap?' } })).json();
    await page.goto('/');
    await goPlace(page, 'calendar');
    await expect(page).toHaveURL(/#\/calendar$/);
    const box = page.locator('#sched-page');
    await expect(box.locator('.sched-head h1')).toHaveText('Calendar');
    const noDate = box.locator('.sched-list:visible', { hasText: 'No date' });
    await expect(noDate).toContainText(`Ask tutor ${tag}`);

    await noDate.locator(`.sched-row[data-task="${undated.id}"] [data-action="task-date"]`).click();
    await page.locator('.sheet [data-pick-day]').filter({ hasText: 'Tomorrow' }).click();
    await page.locator('.sheet [data-pick-time="09:00"]').click();
    await page.locator('#pick-save').click();
    await expect(page.locator('.toast')).toContainText('Date saved');
    const row = box.locator(`.sched-agenda .sched-row[data-task="${undated.id}"]`);
    await expect(row).toContainText(`Ask tutor ${tag}`);
    const saved = (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === undated.id);
    expect(saved.time).toBe('09:00:00');

    await row.locator('.sched-main').click();
    await page.locator('.sheet [data-action="task-google"]').click();
    const opened = await page.evaluate(() => window.__opened);
    expect(opened.length).toBe(1);
    const link = new URL(opened[0]);
    expect(link.host).toBe('calendar.google.com');
    expect(link.searchParams.get('action')).toBe('TEMPLATE');
    expect(link.searchParams.get('details')).toContain(`leo-task:${undated.id.slice(0, 12)}`);
    await expect(page.locator('.toast')).toContainText('Opened in Google Calendar');

    await row.locator('.sched-check').click();
    await expect(page.locator('.toast')).toContainText('Done');
    await expect.poll(async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === undated.id).done).toBe(true);
    await page.locator('.toast button', { hasText: 'Undo' }).click();
    await expect.poll(async () => (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.id === undated.id).done).toBe(false);

    const hw = (await (await page.request.get('/api/tasks')).json()).tasks.find((t) => t.note && t.note.id === note.id);
    expect(hw.title).toContain('Submit HW 4');
    expect(hw.date).toBe('2026-10-16');
    const all = (await (await page.request.get('/api/tasks')).json()).tasks;
    expect(all.some((t) => t.title === 'What is a heap?')).toBe(false);
  });

  test('a new task can be typed in, and a note checkbox can be added to tasks', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    const note = await (await page.request.post('/api/notes', { data: { title: `Plans ${tag}`, body: 'Ideas\n- [ ] Book the room\nEnd' } })).json();
    await page.goto('/#/calendar');
    await page.locator('.sched-head [data-action="task-new"]').click();
    await page.locator('#task-title').fill(`Send slides ${tag}`);
    await page.locator('#task-title').press('Enter');
    await expect(page.locator('.toast')).toContainText(`Added “Send slides ${tag}”`);
    await expect(page.locator('#sched-page .sched-list:visible').getByText(`Send slides ${tag}`)).toBeVisible();

    await page.goto(`/#/n/${note.id}`);
    await page.locator('[data-action="note-tasks"]').click();
    const sheet = page.locator('.sheet');
    await expect(sheet).toContainText('Book the room');
    await sheet.locator('[data-action="task-promote"]').click();
    await expect(page.locator('.toast')).toContainText('Added to your tasks');
    await expect(page.locator('.sheet [data-action="task-open-here"]')).toHaveText('In your tasks');
    const tasks = (await (await page.request.get('/api/tasks')).json()).tasks;
    expect(tasks.some((t) => t.title === 'Book the room' && t.note.id === note.id)).toBe(true);
  });
});

test.describe('beside the note', () => {
  test('on a wide screen an original opens beside its note and closes when you leave', async ({ page, browser }) => {
    test.skip(test.info().project.name !== 'desktop', 'the pane needs a wide screen');
    await page.setViewportSize({ width: 1600, height: 900 });
    const note = await (await page.request.post('/api/notes', { data: { title: 'Beside the source', body: 'My notes on the paper.' } })).json();
    const folder = path.join(process.env.LEO_BROWSER_HOME, 'attachments', note.id);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'notes.txt'), 'Plain text from the upload.');
    const maker = await browser.newPage();
    await maker.setContent('<h1>Paper</h1>');
    fs.writeFileSync(path.join(folder, 'paper.pdf'), await maker.pdf());
    await maker.close();
    await page.goto(`/#/n/${note.id}`);
    await page.locator('.chip.original', { hasText: 'notes.txt' }).click();
    const pane = page.locator('#source-pane');
    await expect(pane).toBeVisible();
    await expect(pane.locator('.source-text')).toHaveText('Plain text from the upload.');
    await expect(page.locator('.sheet.viewer')).toHaveCount(0);
    const paneBox = await pane.boundingBox();
    const doc = await page.locator('#doc').boundingBox();
    expect(doc.x + doc.width).toBeLessThanOrEqual(paneBox.x + 1);
    await pane.locator('[data-action="source-file"]', { hasText: 'paper.pdf' }).click();
    await expect(pane.locator('iframe.source-frame')).toHaveAttribute('src', /paper\.pdf\?view=1$/);
    await page.locator('#side [data-action="home"]').first().click();
    await expect(pane).toBeHidden();
  });

  test('the Tasks panel shows open tasks in the bottom corner, stays across pages and folds', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'the panel is for wider screens');
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/tasks', { data: { title: `Panel task ${tag}` } });
    await page.goto('/');
    const box = page.locator('#dock #tasks-window');
    await expect(box).toBeVisible();
    await expect(box).toContainText(`Panel task ${tag}`);
    await expect(page.locator('#side [data-action="tasks-toggle"]')).toHaveCount(0);
    await page.goto('/#/settings');
    await expect(box).toBeVisible();
    const wide = (await box.boundingBox()).width;
    const folding = await box.evaluate((el) => {
      el.querySelector('[data-action="tasks-fold"]').click();
      return { clips: el.getAnimations().filter((run) => !run.animationName).map((run) => run.effect.getKeyframes().map((k) => k.clipPath)), body: Boolean(el.querySelector('.tasks-body')) };
    });
    expect(folding.body).toBe(true);
    expect(folding.clips[0][0]).toBe('inset(0px round 14px)');
    expect(folding.clips[0][1]).toMatch(/round 23px/);
    await expect(box.locator('.tasks-body')).toHaveCount(0);
    const pill = box.locator('.tasks-pill');
    await expect(pill).toHaveAttribute('aria-label', /^Show tasks: .*to do$/);
    await expect(pill).toBeFocused();
    await expect(box.locator('.tasks-badge')).toHaveText(/^\d+\+?$/);
    const small = await box.boundingBox();
    expect(small.width).toBeLessThan(60);
    expect(small.width).toBeLessThan(wide / 4);
    await page.reload();
    await expect(page.locator('#dock #tasks-window .tasks-pill')).toBeVisible();
    const opening = await box.evaluate((el) => {
      el.querySelector('.tasks-pill').click();
      return { runs: el.getAnimations().filter((run) => !run.animationName).length, body: Boolean(el.querySelector('.tasks-body')) };
    });
    expect(opening).toEqual({ runs: 1, body: true });
    await expect(page.locator('#dock #tasks-window .tasks-body')).toBeVisible();
    await page.emulateMedia({ reducedMotion: 'reduce' });
    const still = await box.evaluate((el) => {
      el.querySelector('[data-action="tasks-fold"]').click();
      return { runs: el.getAnimations().filter((run) => !run.animationName).length, pill: Boolean(el.querySelector('.tasks-pill')) };
    });
    expect(still).toEqual({ runs: 0, pill: true });
    await box.locator('.tasks-pill').click();
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await expect(page.locator('#dock #tasks-window [data-action="tasks-fold"]')).toContainText('Hide');
    await page.goto('/#/calendar');
    await expect(page.locator('#tasks-window')).toBeHidden();
  });

  test('with Felix open, the notes scrollbar sits beside the notes, not past the chat', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'Felix covers the page on phones');
    const body = Array.from({ length: 160 }, (_, i) => `Line ${i + 1} of a long note to scroll through.`).join('\n\n');
    const note = await (await page.request.post('/api/notes', { data: { title: `Long scroll ${Date.now().toString(36)}`, body } })).json();
    await page.goto(`/#/n/${note.id}`);
    await expect(page.locator('#doc')).toContainText('Line 160');
    const bar = page.locator('.page-bar');
    await expect(bar).toBeHidden();
    expect(await page.evaluate(() => document.documentElement.classList.contains('page-bar-on'))).toBe(false);
    await page.locator('#chat-toggle').click();
    await expect(page.locator('aside.chat')).toBeVisible();
    await page.locator('aside.chat').evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await expect(bar).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.classList.contains('page-bar-on'))).toBe(true);
    const chatLeft = (await page.locator('aside.chat').boundingBox()).x;
    const barBox = await bar.boundingBox();
    expect(barBox.x + barBox.width).toBeLessThanOrEqual(chatLeft + 1);
    expect(barBox.x + barBox.width).toBeGreaterThan(chatLeft - 4);
    const thumb = page.locator('.page-bar-thumb');
    const start = await thumb.boundingBox();
    await page.mouse.move(start.x + start.width / 2, start.y + 10);
    await page.mouse.down();
    await page.mouse.move(start.x + start.width / 2, start.y + 210, { steps: 5 });
    await page.mouse.up();
    await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(200);
    const moved = await thumb.boundingBox();
    expect(moved.y).toBeGreaterThan(start.y + 150);
    await page.mouse.wheel(0, -100000);
    await expect.poll(() => page.evaluate(() => scrollY)).toBe(0);
    await page.locator('[data-chat="close"]').click();
    await expect(page.locator('aside.chat')).toBeHidden();
    await expect(bar).toBeHidden();
    expect(await page.evaluate(() => document.documentElement.classList.contains('page-bar-on'))).toBe(false);
    await page.request.delete(`/api/notes/${note.id}`);
  });
});

for (const company of ['Pinterest', '']) {
  test(`interview start prepares a coding problem in one action for ${company || 'general practice'}`, async ({ page }) => {
    const requests = [];
    let item;
    let ready;
    let release;
    const starter = 'def pair_sum(nums, target):\n    pass';
    await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/interviews/*', (route) => {
      const req = route.request();
      if (req.method() === 'PUT') {
        const change = req.postDataJSON();
        if (change.action === 'framework') item = { id: req.url().split('/').at(-1), revision: 1, framework: change.framework, workspace: { answer: '', reasoning: '', board: [] }, challenge: null, attempts: [], schedule: [] };
        else { item.workspace = change.workspace; item.revision++; }
      }
      return route.fulfill({ json: item });
    });
    await page.route('**/api/chats/*', (route) => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No conversation yet' } }) : route.fulfill({ json: { id: item.id, title: 'Interview practice', updated_at: new Date().toISOString(), count: 2, named: true } }));
    await page.route('**/api/chat', async (route) => {
      requests.push(route.request().postDataJSON());
      ready();
      await new Promise((resolve) => { release = resolve; });
      item.challenge = { id: 'challenge-start-01', title: 'Matching pair', question: 'Find two distinct indices whose values sum to the target.', minutes: 25, skills: item.framework.skills, origin: 'generated', sources: [], coding: { language: 'Python', starter, difficulty: 'easy', examples: [{ input: 'nums=[2,3], target=5', output: '[0,1]' }], constraints: ['Exactly one pair exists.'] } };
      item.workspace = { answer: starter, reasoning: '', board: [] };
      item.revision++;
      await route.fulfill({ contentType: 'application/x-ndjson', body: [{ interview: { id: item.id, revision: item.revision } }, { t: 'Your challenge is ready.' }, { done: true }].map((v) => JSON.stringify(v)).join('\n') + '\n' });
    });
    await page.goto('/#/interviews');
    await page.locator('[data-interview="create"][data-track="coding"]').click();
    await page.locator('#interview-company').fill(company);
    await page.locator('#interview-role').fill('Software engineer intern');
    const started = new Promise((resolve) => { ready = resolve; });
    await page.locator('#interview-form-save').click();
    await started;
    await expect(page.locator('#interview-answer')).toHaveCount(0);
    await expect(page.locator('#interview-challenge')).toContainText('Preparing your first challenge');
    expect(requests).toHaveLength(1);
    const instruction = requests[0].messages.at(-1).text;
    if (company) expect(instruction).toContain('Search the web before');
    else expect(instruction).toContain('Do not ask for a company');
    expect(instruction).toContain('interview_challenge');
    release();
    await expect(page.locator('#interview-answer')).toHaveValue(starter);
    await page.locator('[data-chat="close"]').click();
    await expect(page.locator('[data-interview="research"]')).toHaveCount(0);
    await expect(page.locator('.interview-example')).toContainText('[0,1]');
    await page.locator('[data-interview="submit"]').click();
    expect(requests).toHaveLength(1);
    await page.locator('[data-interview="tab"][data-tab="write"]').click();
    const approach = page.locator('#interview-reasoning');
    await approach.fill('Use a hash map and check duplicates.');
    await approach.evaluate((el) => el.setSelectionRange(4, 8));
    await expect.poll(() => item.workspace.reasoning).toContain('check duplicates');
    await expect(approach).toBeFocused();
    expect(await approach.evaluate((el) => [el.selectionStart, el.selectionEnd])).toEqual([4, 8]);
    await page.locator('[data-interview="tab"][data-tab="code"]').click();
    await expect(page.locator('#interview-answer')).toHaveValue(starter);
    await page.reload();
    await expect(page.locator('#interview-answer')).toHaveValue(starter);
    expect(requests).toHaveLength(1);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  });
}

test('Felix code suggestions accept and decline independently, merge current typing and guard stale code', async ({ page }) => {
  const id = 'code-suggestions-ui';
  const starter = 'def solve():\n    left = 0\n    right = 1\n    return left';
  const challenge = { id: 'suggestion-problem-01', title: 'Independent changes', question: 'Explain this solution.', minutes: 30, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter, difficulty: 'easy', examples: [{ input: '1', output: '1' }], constraints: ['Describe'] } };
  let item = { id, revision: 1, framework: { title: 'Code changes', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace: { answer: starter, reasoning: 'Keep my approach', board: [] }, problems: [{ challenge, workspace: { answer: starter, reasoning: 'Keep my approach', board: [] } }], attempts: [], schedule: [] };
  const proposal = (find, replace, summary) => ({ kind: 'interview_code_edit', title: 'Problem 1 · Independent changes', summary, interview: id, challenge: challenge.id, find, replace });
  const edits = [proposal('0', '2', 'Initialize the left index'), proposal('right = 1', 'right = 3', 'Initialize the right index'), proposal('1', '(', 'Begin an expression')];
  let hold, release, acts = 0;
  await page.route('**/api/interviews/*', route => { if (route.request().method() === 'PUT') { item.workspace = route.request().postDataJSON().workspace || item.workspace; item.revision++; } return route.fulfill({ json: item }); });
  await page.route('**/api/chats/*', route => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No chat yet' } }) : route.fulfill({ json: { id, title: 'Code changes', named: true, count: 2, updated_at: new Date().toISOString() } }));
  await page.route('**/api/chat', route => route.fulfill({ contentType: 'application/x-ndjson', body: [...edits.map(proposal => ({ proposal })), { t: 'Review each suggested change.' }, { done: true }].map(e => JSON.stringify(e)).join('\n') + '\n' }));
  await page.route('**/api/felix/act', async route => {
    acts++;
    const p = route.request().postDataJSON();
    if (hold) await hold;
    const code = item.workspace.answer;
    if (code.indexOf(p.find) < 0 || code.indexOf(p.find) !== code.lastIndexOf(p.find)) return route.fulfill({ status: 409, json: { error: 'This code changed. Your draft was kept; request a new suggestion.' } });
    item.workspace.answer = code.replace(p.find, p.replace); item.revision++;
    const undo = p.replace === '(' ? { ...p, find: 'right = (', replace: 'right = 1' } : { ...p, find: p.replace, replace: p.find };
    return route.fulfill({ json: { revision: item.revision, undo } });
  });
  await page.goto(`/#/interviews/${id}`);
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('Suggest focused fixes to my code.');
  await page.locator('#chat-input').press('Enter');
  const cards = page.locator('.proposal-code');
  await expect(cards).toHaveCount(3);
  await expect(cards.nth(0).locator('.proposal-old')).toHaveText('0');
  await expect(cards.nth(0).locator('.proposal-new')).toHaveText('2');
  expect(item.workspace.answer).toBe(starter);
  if (await page.locator('#interview-answer').isVisible()) await page.locator('#interview-answer').fill(starter + '\n# Keep my newest draft');
  else { await page.locator('[data-chat="close"]').click(); await page.locator('#interview-answer').fill(starter + '\n# Keep my newest draft'); await page.locator('#chat-toggle').click(); }
  await cards.nth(0).locator('[data-chat="apply"]').click();
  await expect(cards.nth(0)).toContainText('Accepted');
  await expect.poll(() => item.workspace.answer).toContain('left = 2');
  expect(item.workspace.answer).toContain('Keep my newest draft');
  await cards.nth(1).locator('[data-chat="dismiss"]').click();
  await expect(cards.nth(1)).toContainText('Declined');
  expect(acts).toBe(1);
  expect(item.workspace.answer).toContain('right = 1');
  await cards.nth(0).locator('[data-chat="undo"]').click();
  await expect(cards.nth(0).locator('[data-chat="apply"]')).toBeVisible();
  await expect.poll(() => item.workspace.answer).toContain('left = 0');
  const after = item.workspace.answer;
  await page.locator('#chat-input').fill('Do not overwrite this next question');
  hold = new Promise(resolve => { release = resolve; });
  await cards.nth(0).locator('[data-chat="apply"]').click();
  await expect(cards.nth(0)).toContainText('Saving code');
  if (await page.locator('#interview-answer').isVisible()) {
    await page.locator('#interview-answer').fill(after + '\n# Typed during application');
    release(); hold = null;
    await expect(cards.nth(0)).toContainText('Accepted');
    await expect.poll(() => item.workspace.answer).toContain('Typed during application');
  } else { release(); hold = null; await expect(cards.nth(0)).toContainText('Accepted'); }
  await expect(page.locator('#chat-input')).toHaveValue('Do not overwrite this next question');
  expect(item.workspace.reasoning).toBe('Keep my approach');
  await cards.nth(0).locator('[data-chat="undo"]').click();
  await expect(cards.nth(0).locator('[data-chat="apply"]')).toBeVisible();
  await page.locator('#interview-answer').evaluate(el => el.focus({ preventScroll: true }));
  await cards.nth(2).locator('[data-chat="apply"]').evaluate(el => el.click());
  await expect(cards.nth(2)).toContainText('Accepted');
  await expect(page.locator('#interview-answer')).toHaveValue(item.workspace.answer);
  expect(item.workspace.answer).toContain('right = (\n');
  expect(item.workspace.answer).not.toContain('right = ()');
  await cards.nth(2).locator('[data-chat="undo"]').click();
  await expect(cards.nth(2).locator('[data-chat="apply"]')).toBeVisible();
  item.workspace.answer = item.workspace.answer.replace('left = 0', 'left = 99');
  await cards.nth(0).locator('[data-chat="apply"]').click();
  await expect(page.locator('.toast')).toContainText('Your draft was kept');
  expect(item.workspace.answer).toContain('left = 99');
});

test('coding typing pairs brackets, preserves scroll and Undo across every supported language with a default language dropdown', async ({ page }) => {
  const languages = ['Python', 'C++', 'Rust', 'Java', 'SQL', 'TypeScript', 'JavaScript', 'Go'];
  const id = 'typing-languages-01';
  let item;
  await page.route('**/api/interviews', route => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}`, route => {
    if (route.request().method() === 'PUT') { const data = route.request().postDataJSON(); item.workspace = data.workspace || item.workspace; item.revision++; }
    return route.fulfill({ json: item });
  });
  await page.route('**/api/interviews/*/key?*', route => route.fulfill({ json: { status: 'unavailable', tests: [], asks: [], rubric: [] } }));
  await page.goto('/#/interviews');
  await page.locator('[data-interview="create"][data-track="coding"]').click();
  const dropdown = page.locator('select#interview-language');
  await expect(dropdown).toHaveValue('Python');
  expect(await dropdown.locator('option').allTextContents()).toEqual(languages);
  await expect(dropdown.locator('..').locator(':scope > svg')).toBeVisible();
  await dropdown.selectOption('C++');
  await expect(dropdown).toHaveValue('C++');
  await page.locator('.sheet [data-action="close"]').click();
  for (const language of languages) {
    const challenge = { id: 'typing-problem-01', title: 'Typing practice', question: 'Work through this exercise.', minutes: 30, skills: ['Approach'], origin: 'generated', sources: [], coding: { language, starter: '', difficulty: 'easy', examples: [{ input: '1', output: '1' }], constraints: ['Describe the approach.'] } };
    item = { id, revision: 1, framework: { title: 'Typing', track: 'coding', company: '', role: '', language, style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace: { answer: '', reasoning: '', board: [] }, problems: [], attempts: [], schedule: [] };
    await page.goto(`/#/interviews/${id}`);
    await page.reload();
    const area = page.locator('#interview-answer');
    await expect(page.locator('#interview-colors')).toBeVisible();
    if (language === 'Python') {
      const header = 'class Solution:\n    def solve(self):';
      await area.fill(header); await area.press('End'); await area.press('Enter');
      await expect(area).toHaveValue(header + '\n        ');
    }
    const fragment = '        value = 1;';
    await area.fill(fragment); await area.press('End'); await area.press('Enter');
    await expect(area).toHaveValue(fragment + '\n        ');
    await expect(area).toBeFocused();
    await area.press('ControlOrMeta+z'); await expect(area).toHaveValue(fragment);
    await area.press('ControlOrMeta+Shift+z'); await expect(area).toHaveValue(fragment + '\n        ');
    await area.evaluate(el=>el.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertLineBreak',bubbles:true,cancelable:true})));
    await expect(area).toHaveValue(fragment + '\n\n        ');
    await expect.poll(()=>item.workspace.answer).toBe(fragment + '\n\n        ');
    if (language === 'Rust') {
      await area.fill('&'); await area.press("'"); await area.press('a');
      await expect(area).toHaveValue("&'a");
    }
    await area.fill('');
    await area.press('('); await area.press('['); await area.press('x');
    await expect(area).toHaveValue('([x])');
    await area.press(']'); await area.press(')');
    await expect(area).toHaveValue('([x])');
    expect(await area.evaluate(el => el.selectionStart)).toBe(5);
    await area.fill(''); await area.press('{');
    await expect(area).toHaveValue('{}');
    await area.press('Backspace'); await expect(area).toHaveValue('');
    const long = 'value;\n'.repeat(60) + '{}';
    await area.fill(long);
    await area.scrollIntoViewIfNeeded();
    await area.evaluate(el => { const at = el.value.length - 1; el.setSelectionRange(at, at); el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event('scroll')); });
    const position = await area.evaluate(el => ({ page: scrollY, editor: el.scrollTop }));
    expect(position.editor).toBeGreaterThan(100);
    await area.press('Enter');
    const expanded = long.slice(0, -1) + '\n    \n}';
    await expect(area).toHaveValue(expanded);
    await expect(area).toBeFocused();
    expect(Math.abs(await page.evaluate(() => scrollY) - position.page)).toBeLessThan(2);
    expect(await area.evaluate(el => el.scrollTop)).toBeGreaterThan(position.editor - 24);
    await area.press('ControlOrMeta+z'); await expect(area).toHaveValue(long);
    await area.press('ControlOrMeta+Shift+z'); await expect(area).toHaveValue(expanded);
    await area.press('Tab');
    await expect(area).toHaveValue(expanded.replace('\n    \n}', '\n        \n}'));
    await area.press('Shift+Tab'); await expect(area).toHaveValue(expanded);
    await area.fill('a\nb'); await area.selectText();
    await area.press('Tab'); await expect(area).toHaveValue('    a\n    b');
    await area.press('Shift+Tab'); await expect(area).toHaveValue('a\nb');
    await area.fill('答😀'); await area.selectText(); await area.press('[');
    await expect(area).toHaveValue('[答😀]');
    expect(await area.evaluate(el => el.value.slice(el.selectionStart, el.selectionEnd))).toBe('答😀');
    await expect.poll(() => item.workspace.answer).toBe('[答😀]');
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  }
});

test('coding colors and indentation preserve input, Undo, saves and scroll in every common language', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  const samples = [['C++', 'int solve() {\nreturn 2;\n}', 3], ['Rust', 'fn solve() -> i32 {\n2\n}', 3], ['Java', 'class Solution {\nint solve() {\nreturn 2;\n}\n}', 3], ['Python', 'def solve():\n  return 2', 2], ['SQL', 'SELECT name\nFROM users\nWHERE id = 2;', 3], ['TypeScript', 'function solve(value: number): number {\nreturn value + 2;\n}', 3], ['JavaScript', 'function solve(value) {\nreturn value + 2;\n}', 3], ['Go', 'package main\nfunc solve() int {\nreturn 2\n}', 3]];
  const id = 'coding-colors-01';
  let item;
  let requests = 0;
  await page.route(`**/api/interviews/${id}`, async (route) => {
    if (route.request().method() === 'PUT') {
      const body = route.request().postDataJSON();
      if (body.action === 'workspace') item.workspace = body.workspace;
      item.revision++;
    }
    await route.fulfill({ json: item });
  });
  await page.route('**/api/chat', async (route) => { requests++; await route.fulfill({ status: 500, json: { error: 'No model is needed here.' } }); });
  for (const [index, [language, code, tabWidth]] of samples.entries()) {
    item = { id, revision: 1, framework: { title: 'Coding colors', track: 'coding', company: '', role: '', language, style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge: { id: `challenge-colors-${index}`, title: 'Solve the problem', question: 'Solve it.', minutes: 25, skills: ['Approach'], origin: 'generated', sources: [], coding: { language, starter: code, difficulty: 'easy', examples: [{ input: '2', output: '2' }], constraints: ['Be clear.'] } }, workspace: { answer: code, reasoning: '', board: [] }, attempts: [], schedule: [] };
    await page.goto(`/#/interviews/${id}`);
    if (index) await page.reload();
    const area = page.locator('#interview-answer');
    const colors = page.locator('#interview-colors');
    await expect(area).toHaveValue(code);
    await expect(colors.locator('.code-keyword').first()).toBeVisible();
    expect(await colors.textContent()).toBe(code);
    const originalColor = await colors.locator('.code-keyword').first().evaluate(el => getComputedStyle(el).color);
    await page.emulateMedia({ colorScheme: 'dark' });
    expect(await colors.locator('.code-keyword').first().evaluate(el => getComputedStyle(el).color)).not.toBe(originalColor);
    await page.emulateMedia({ colorScheme: 'light' });
    await area.focus();
    await area.evaluate(el => el.setSelectionRange(el.value.length, el.value.length));
    await area.press('Tab');
    const tab = ' '.repeat(tabWidth);
    await expect(area).toHaveValue(code + tab);
    await expect(area).toBeFocused();
    expect(await area.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([code.length + tabWidth, code.length + tabWidth]);
    await expect.poll(() => item.workspace.answer).toBe(code + tab);
    await area.press('ControlOrMeta+z');
    await expect(area).toHaveValue(code);
    await expect.poll(() => item.workspace.answer).toBe(code);
    const long = Array.from({ length: 25 }, (_, i) => `${code}\n${' '.repeat(130)}${i}`).join('\n');
    await area.fill(long);
    await expect.poll(() => colors.textContent()).toBe(long);
    await area.evaluate(el => { el.scrollTop = el.scrollHeight; el.scrollLeft = el.scrollWidth; el.dispatchEvent(new Event('scroll')); });
    await expect.poll(() => colors.evaluate(el => [el.scrollTop, el.scrollLeft])).toEqual(await area.evaluate(el => [el.scrollTop, el.scrollLeft]));
    await expect(area).toBeFocused();
    await expect.poll(() => item.workspace.answer).toBe(long);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  }
  expect(requests).toBe(0);
  await page.emulateMedia({ forcedColors: 'active' });
  await expect(page.locator('#interview-colors')).toBeHidden();
  expect(await page.locator('#interview-answer').evaluate(el => getComputedStyle(el).color)).not.toBe('rgba(0, 0, 0, 0)');
});

test('typing before syntax tools load and a failed load keep the code editable', async ({ page }) => {
  const id = 'coding-lazy-01';
  let workspace = { answer: 'def solve():\n    pass', reasoning: '', board: [] };
  await page.route(`**/api/interviews/${id}`, route => {
    if (route.request().method() === 'PUT') workspace = route.request().postDataJSON().workspace || workspace;
    return route.fulfill({ json: { id, revision: 1, framework: { title: 'Lazy tools', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge: { id: 'challenge-lazy-01', title: 'A problem', question: 'Solve it.', minutes: 25, skills: ['Approach'], origin: 'generated', sources: [] }, workspace, attempts: [], schedule: [] } });
  });
  let release;
  const held = new Promise(resolve => { release = resolve; });
  await page.route('**/code-tools.js', async route => { await held; await route.continue(); });
  await page.goto(`/#/interviews/${id}`, { waitUntil: 'domcontentloaded' });
  const area = page.locator('#interview-answer');
  const value = 'def solve():\n    return "<img src=x onerror=alert(1)>答え"';
  await area.fill(value);
  await area.evaluate(el => el.setSelectionRange(4, 9));
  release();
  await expect(page.locator('#interview-colors .code-keyword').first()).toBeVisible();
  await expect(area).toHaveValue(value);
  await expect(area).toBeFocused();
  expect(await area.evaluate(el => [el.selectionStart, el.selectionEnd])).toEqual([4, 9]);
  await expect(page.locator('#interview-colors img')).toHaveCount(0);
  await expect.poll(() => workspace.answer).toBe(value);
  await page.unroute('**/code-tools.js');
  await page.route('**/code-tools.js', route => route.abort());
  await page.reload();
  await expect(area).toHaveValue(value);
  await expect(page.locator('[data-interview="indent-code"]')).toHaveCount(0);
  await expect(page.locator('#interview-colors')).toBeHidden();
  await area.fill('def solve():\n    return 3');
  await expect.poll(() => workspace.answer).toContain('return 3');
  await expect(area).toBeFocused();
});

test.describe('Felix organizes', () => {
  test('a suggested move is applied from its card and undone', async ({ page }) => {
    const tag = `${test.info().project.name}-${Date.now().toString(36)}`;
    await page.request.post('/api/dirs', { data: { path: `archive-${tag}` } });
    const note = await (await page.request.post('/api/notes', { data: { title: `Move me ${tag}`, body: 'x' } })).json();
    await page.route('**/api/chat', async (route) => {
      const lines = [{ sources: [] }, { proposal: { kind: 'move_note', summary: `Move “Move me ${tag}” to archive-${tag}`, note: note.id, title: `Move me ${tag}`, to: `archive-${tag}` } }, { t: 'I suggest moving it.' }, { done: true }];
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: lines.map((l) => JSON.stringify(l)).join('\n') + '\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('#chat-input').fill('tidy my notes');
    await chat.locator('#chat-input').press('Enter');
    const card = chat.locator('.proposal.organize');
    await expect(card).toContainText(`Move “Move me ${tag}” to archive-${tag}`);
    await card.locator('[data-chat="apply"]').click();
    await expect(card).toContainText('Done');
    const where = async () => (await (await page.request.get(`/api/notes/${note.id}`)).json()).directory;
    await expect.poll(where).toBe(`archive-${tag}`);
    await card.locator('[data-chat="undo"]').click();
    await expect.poll(where).toBe('');
    await expect(card.locator('[data-chat="apply"]')).toBeVisible();
  });
});


test('coding problems keep separate drafts, wrap long functions and open their own Felix chat', async ({ page }) => {
  const id = 'coding-problems-ui';
  const starter = 'def shortest_category_segment(feed: list[str], required_categories_with_counts: list[str], preferred_category_order: list[str]) -> list[int]:\n    pass';
  const challenge = (n) => ({ id: `challenge-problem-${n}`, title: `Category coverage ${n}`, question: 'Find the shortest contiguous segment covering every required count. Explain duplicates, ties and complexity.', minutes: 45, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter, difficulty: 'medium', examples: [{ input: 'feed=["art","food"], required=["food"]', output: '[1,1]' }], constraints: ['Up to 200000 categories.'] } });
  const first = { challenge: challenge(1), workspace: { answer: starter, reasoning: 'A sliding window', board: [] } };
  const second = { challenge: challenge(2), workspace: { answer: 'def other():\n    return [0, 1]', reasoning: 'Earlier explanation', board: [{ kind: 'text', text: 'Earlier diagram', points: [[20, 40]] }] } };
  let item = { id, revision: 1, framework: { title: 'Coding practice', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge: first.challenge, workspace: first.workspace, problems: [first, second], attempts: [], schedule: [] };
  let failSelect = false;
  let prepareRelease;
  let archiveRelease;
  const requests = [];
  await page.route('**/api/interviews/*', async (route) => {
    if (route.request().method() === 'PUT') {
      const change = route.request().postDataJSON();
      if (change.action === 'select') {
        if (failSelect) { failSelect = false; return route.fulfill({ status: 409, json: { error: 'This practice changed. Reload before switching; your draft is kept.' } }); }
        const problem = item.problems.find((p) => p.challenge.id === change.challenge);
        item.challenge = problem.challenge;
        item.workspace = problem.workspace;
      } else if (change.action === 'submit') {
        item.attempts.push({ id: change.attempt, challenge: structuredClone(item.challenge), workspace: structuredClone(change.workspace), at: new Date().toISOString(), feedback: null });
        item.workspace = change.workspace;
      } else if (change.action === 'workspace') {
        const problem = item.problems.find((p) => p.challenge.id === change.challenge);
        if (item.challenge.id !== change.challenge) await new Promise((resolve) => { archiveRelease = resolve; });
        problem.workspace = change.workspace;
        if (item.challenge.id === change.challenge) item.workspace = change.workspace;
      }
      item.revision++;
    }
    await route.fulfill({ json: item });
  });
  await page.route('**/api/chats/*', (route) => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No conversation yet' } }) : route.fulfill({ json: { id, title: 'Coding practice', updated_at: new Date().toISOString(), count: 2, named: true } }));
  await page.route('**/api/chat', async (route) => {
    const request = route.request().postDataJSON();
    requests.push(request);
    if (!request.messages.at(-1).text.includes('interview_challenge')) return route.fulfill({ contentType: 'application/x-ndjson', body: JSON.stringify({ t: 'Your code stays in this workspace. Keep iterating.' }) + '\n' + JSON.stringify({ done: true }) + '\n' });
    await new Promise((resolve) => { prepareRelease = resolve; });
    const third = { challenge: challenge(3), workspace: { answer: starter, reasoning: '', board: [] } };
    item.problems.push(third);
    item.challenge = third.challenge;
    item.workspace = third.workspace;
    item.revision++;
    await route.fulfill({ contentType: 'application/x-ndjson', body: [{ interview: { id, revision: item.revision } }, { t: 'Problem 3 is ready in your workspace.' }, { done: true }].map((event) => JSON.stringify(event)).join('\n') + '\n' });
  });
  await page.goto(`/#/interviews/${id}`);
  const area = page.locator('#interview-answer');
  await expect(area).toHaveValue(starter);
  await expect(page.locator('#interview-colors .code-keyword').first()).toBeVisible();
  const wrapping = await area.evaluate((el) => ({ whiteSpace: getComputedStyle(el).whiteSpace, scroll: el.scrollWidth, width: el.clientWidth }));
  expect(wrapping.whiteSpace).toBe('pre-wrap');
  expect(wrapping.scroll).toBeLessThanOrEqual(wrapping.width);
  await expect(page.locator('[data-interview="indent-code"]')).toHaveCount(0);
  await area.fill(starter + '\n        ');
  await area.evaluate((el) => el.setSelectionRange(el.value.length, el.value.length));
  await area.press('Backspace');
  await expect(area).toHaveValue(starter + '\n    ');
  await area.press('ControlOrMeta+z');
  await expect(area).toHaveValue(starter + '\n        ');
  const draft = starter.replace('pass', 'return [0, 2]');
  await area.fill(draft);
  await page.getByRole('button', { name: 'Problem 2', exact: true }).click();
  await expect(area).toHaveValue(second.workspace.answer);
  expect(item.problems[0].workspace.answer).toBe(draft);
  await page.locator('[data-interview="tab"][data-tab="write"]').click();
  await expect(page.locator('#interview-reasoning')).toHaveValue('Earlier explanation');
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  await expect(page.locator('#interview-board')).toContainText('Earlier diagram');
  failSelect = true;
  await page.getByRole('button', { name: /^Problem 1(?: · Submitted)?$/ }).click();
  await expect(page.locator('.toast')).toContainText('Reload before switching');
  expect(item.challenge.id).toBe(second.challenge.id);
  await page.getByRole('button', { name: /^Problem 1(?: · Submitted)?$/ }).click();
  await page.locator('[data-interview="tab"][data-tab="code"]').click();
  await expect(area).toHaveValue(draft);
  await page.locator('[data-interview="submit"]').click();
  await expect(page.locator('.msg.leo').last()).toContainText('Keep iterating');
  await expect(area).toHaveValue(draft);
  expect(item.attempts[0].workspace.answer).toBe(draft);
  await page.locator('[data-chat="close"]').click();
  await page.locator('#chat-toggle').click();
  await expect(page.locator('.chat')).toBeVisible();
  await page.locator('#chat-input').fill('Which problem am I working on?');
  await page.locator('#chat-input').press('Enter');
  await expect.poll(() => requests.length).toBe(2);
  expect(requests[1].mode).toBe('interview');
  expect(requests[1].chat).toBe(id);
  await expect(page.locator('.msg.leo').last()).toContainText('Keep iterating');
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await page.locator('[data-chat="close"]').click();
  await page.getByRole('button', { name: '+ Problem', exact: true }).click();
  await expect.poll(() => typeof prepareRelease).toBe('function');
  const lateDraft = draft + '\n    result = [0, 2]';
  await area.fill(lateDraft);
  prepareRelease();
  await expect.poll(() => typeof archiveRelease).toBe('function');
  await expect(area).toHaveJSProperty('readOnly', true);
  archiveRelease();
  await expect(page.getByRole('button', { name: 'Problem 3', exact: true })).toBeVisible();
  expect(item.problems[0].workspace.answer).toBe(lateDraft);
  await expect(area).toHaveValue(starter);
  expect(requests).toHaveLength(3);
  expect(requests[0].mode).toBe('interview');
  expect(requests[0].chat).toBe(id);
  expect(requests.at(-1).messages.at(-1).text).toContain('interview_challenge');
  await page.locator('[data-chat="close"]').click();
  await page.getByRole('button', { name: /^Problem 1(?: · Submitted)?$/ }).click();
  await expect(area).toHaveValue(lateDraft);
  await page.reload();
  await expect(area).toHaveValue(lateDraft);
  expect(item.problems).toHaveLength(3);
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.locator('#back').click();
  await expect(page).toHaveURL(/#\/interviews$/);
  await expect(page.getByRole('heading', { name: 'Interview practice', exact: true })).toBeVisible();
});

test('whiteboards pan, labels drag and typed labels land where clicked while Reset affects only the active problem tab', async ({ page }) => {
  const id = 'whiteboard-workspace';
  const starter = 'def solve(values):\n    pass';
  const make = n => ({ challenge: { id: `whiteboard-problem-${n}`, title: `Problem title ${n}`, question: 'Explain the system and implement its queue.', minutes: 30, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter, difficulty: 'medium', examples: [{ input: '[1]', output: '1' }], constraints: ['Explain trade-offs'] } }, workspace: { answer: `${starter}\n# draft ${n}`, reasoning: `Approach ${n}`, board: [] } });
  const problems = [make(1), make(2)];
  let item = { id, revision: 1, framework: { title: 'Interview workspace', track: 'coding', company: 'Example', role: 'Engineer', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge: problems[0].challenge, workspace: problems[0].workspace, problems, attempts: [], schedule: [] };
  const chats = [];
  await page.route('**/api/interviews/*', route => {
    if (route.request().method() === 'PUT') {
      const change = route.request().postDataJSON();
      const problem = item.problems.find(p => p.challenge.id === change.challenge);
      if (change.action === 'select') { item.challenge = problem.challenge; item.workspace = problem.workspace; }
      else if (change.action === 'workspace') { problem.workspace = change.workspace; if (item.challenge.id === change.challenge) item.workspace = change.workspace; }
      item.revision++;
    }
    return route.fulfill({ json: item });
  });
  await page.route('**/api/chats/*', route => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No chat yet' } }) : route.fulfill({ json: { id, title: 'Interview workspace', count: 2, named: true, updated_at: new Date().toISOString() } }));
  await page.route('**/api/chat', route => { chats.push({ body: route.request().postDataJSON(), challenge: item.challenge.id, workspace: structuredClone(item.workspace) }); return route.fulfill({ contentType: 'application/x-ndjson', body: '{"t":"I see the active problem and your draft."}\n{"done":true}\n' }); });
  await page.goto(`/#/interviews/${id}`);
  await expect(page.locator('[data-interview="clear-workspace"], [data-interview="reset-code"], [data-interview="clear-board"], [data-interview="chat"]')).toHaveCount(0);
  await expect(page.locator('.interview-submit [data-interview="reset"]')).toHaveText('Reset');
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  await boardLabel(page, 'Client → Queue', 0.2, 0.25);
  await expect(page.locator('#interview-board text')).toHaveText('Client → Queue');
  await expect(page.locator('[data-interview="tool"][data-tool="select"]')).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(() => item.workspace.board.length).toBe(1);
  const original = structuredClone(item.workspace.board[0]);
  const label = await page.locator('#interview-board text').boundingBox();
  await page.mouse.move(label.x + label.width / 2, label.y + label.height / 2);
  await page.mouse.down();
  await page.mouse.move(label.x + label.width / 2 + 45, label.y + label.height / 2 + 35, { steps: 4 });
  await page.mouse.up();
  await expect.poll(() => item.workspace.board[0].points[0][0]).toBeGreaterThan(original.points[0][0]);
  const moved = structuredClone(item.workspace.board[0]);
  await page.locator('[data-interview="undo-board"]').click();
  await expect.poll(() => item.workspace.board[0].points).toEqual(original.points);
  const board = page.locator('#interview-board');
  const before = await board.locator('svg').getAttribute('viewBox');
  const saved = JSON.stringify(item.workspace.board);
  await board.scrollIntoViewIfNeeded();
  const box = await board.boundingBox();
  await board.focus();
  await page.keyboard.down('Space');
  await page.mouse.move(box.x + box.width * .75, box.y + box.height * .7);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * .75 + 60, box.y + box.height * .7 + 40, { steps: 3 });
  await page.mouse.up();
  await page.keyboard.up('Space');
  expect(await board.locator('svg').getAttribute('viewBox')).not.toBe(before);
  expect(JSON.stringify(item.workspace.board)).toBe(saved);
  await board.dispatchEvent('wheel', { deltaY: 1800, deltaMode: 0, clientX: box.x + 80, clientY: box.y + 80 });
  await boardLabel(page, 'Far away label', 0.2, 0.5);
  await expect.poll(() => item.workspace.board.length).toBe(2);
  expect(item.workspace.board[1].points[0][1]).toBeGreaterThan(1000);
  await page.locator('[data-interview="tool"][data-tool="box"]').click();
  const current = await board.boundingBox();
  await page.mouse.move(current.x + current.width * .4, current.y + current.height * .4);
  await page.mouse.down();
  await page.mouse.move(current.x + current.width * .7, current.y + current.height * .7, { steps: 3 });
  await page.mouse.up();
  await expect.poll(() => item.workspace.board.length).toBe(3);
  expect(item.workspace.board[2].kind).toBe('box');
  await expect(page.locator('[data-interview="tool"][data-tool="select"]')).toHaveAttribute('aria-pressed', 'true');
  await page.locator('[data-interview="fit-board"]').click();
  expect(await board.locator('svg').getAttribute('viewBox')).not.toBe(before);
  const reset = async () => { await page.locator('.interview-submit [data-interview="reset"]').click(); await page.locator('[data-interview="confirm-reset"]').click(); };
  const code = item.workspace.answer, reasoning = item.workspace.reasoning;
  await reset();
  await expect.poll(() => item.workspace.board.length).toBe(0);
  expect(item.workspace.answer).toBe(code);
  expect(item.workspace.reasoning).toBe(reasoning);
  await page.locator('[data-interview="undo-board"]').click();
  await expect.poll(() => item.workspace.board.length).toBe(3);
  const drawing = JSON.stringify(item.workspace.board);
  await page.locator('[data-interview="tab"][data-tab="write"]').click();
  await reset();
  await expect.poll(() => item.workspace.reasoning).toBe('');
  expect(item.workspace.answer).toBe(code);
  expect(JSON.stringify(item.workspace.board)).toBe(drawing);
  await page.locator('[data-interview="tab"][data-tab="code"]').click();
  await reset();
  await expect.poll(() => item.workspace.answer).toBe(starter);
  expect(JSON.stringify(item.workspace.board)).toBe(drawing);
  await page.locator('[data-interview="problem"][data-id="whiteboard-problem-2"]').click();
  await expect(page.locator('#interview-answer')).toHaveValue(`${starter}\n# draft 2`);
  await page.locator('#chat-toggle').click();
  await expect(page.locator('#chat-context')).toContainText('Problem 2');
  await expect(page.locator('#chat-context')).toContainText('Problem title 2');
  if (await page.locator('#interview-answer').isVisible()) await page.locator('#interview-answer').fill('Newest unsaved code for problem two');
  else { await page.locator('[data-chat="close"]').click(); await page.locator('#interview-answer').fill('Newest unsaved code for problem two'); await page.locator('#chat-toggle').click(); }
  await page.locator('#chat-input').fill('Can you help me with this?');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat .msg:not(.user)').last()).toContainText('active problem');
  expect(chats.at(-1).body.mode).toBe('interview');
  expect(chats.at(-1).body.interview_problem).toBe('whiteboard-problem-2');
  expect(chats.at(-1).challenge).toBe('whiteboard-problem-2');
  expect(chats.at(-1).workspace.answer).toBe('Newest unsaved code for problem two');
  expect(item.problems[0].workspace.answer).toBe(starter);
  await page.locator('[data-chat="close"]').click();
  await page.reload();
  await page.locator('[data-interview="problem"][data-id="whiteboard-problem-1"]').click();
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  await expect(board.locator('text')).toHaveText(['Client → Queue', 'Far away label']);
  expect(JSON.stringify(item.workspace.board)).toBe(drawing);
  const shape = board.locator('[data-board-shape="0"]');
  await shape.focus();
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => item.workspace.board[0].points[0][0]).toBeGreaterThan(original.points[0][0]);
  await expect(board.locator('[data-board-shape="0"]')).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  item.framework.track = 'systems'; item.challenge.coding = null;
  await page.reload();
  const geometry = await page.locator('.interview-practice').evaluate(el => ({ width: el.getBoundingClientRect().width, parent: el.parentElement.clientWidth - 32 }));
  expect(Math.abs(geometry.width - geometry.parent)).toBeLessThan(2);
  await page.locator('#chat-toggle').click();
  const overlap = await page.evaluate(() => { const workspace = document.querySelector('.interview-workspace').getBoundingClientRect(), chat = document.querySelector('#chat').getBoundingClientRect(); return innerWidth >= 900 && workspace.right > chat.left + 1; });
  expect(overlap).toBe(false);
  expect(moved.points).not.toEqual(original.points);
});

test('Felix permission changes reach an active reply and Ask first keeps later edits as proposals', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('leo-felix-access', 'auto'));
  const updates = [];
  const acts = [];
  let access = 'auto';
  let fail = false;
  await page.route('**/api/felix/act', route => {
    const proposal = route.request().postDataJSON();
    acts.push(proposal);
    return route.fulfill({ json: { revision: acts.length, undo: { ...proposal, before: proposal.schedule, schedule: proposal.before } } });
  });
  await page.route('**/api/chat/*/access', route => { const value = route.request().postDataJSON().access; updates.push(value); if (fail) return route.fulfill({ status: 503, json: { error: 'Unavailable' } }); access = value; return route.fulfill({ json: { access } }); });
  await page.goto('/#/');
  await page.reload();
  await page.evaluate(() => {
    const real = window.fetch;
    window.fetch = async (url, options) => {
      if (url !== '/api/chat') return real(url, options);
      const encoder = new TextEncoder();
      const stream = new ReadableStream({ start(controller) {
        controller.enqueue(encoder.encode('{"answer":"active-permission-reply"}\n{"t":"I am preparing the change."}\n'));
        window.finishPermissionReply = proposal => { if (proposal) controller.enqueue(encoder.encode(JSON.stringify({ proposal }) + '\n')); controller.enqueue(encoder.encode('{"t":" Ready."}\n{"done":true}\n')); controller.close(); };
        options.signal.addEventListener('abort', () => controller.error(new DOMException('Stopped', 'AbortError')), { once: true });
      } });
      return new Response(stream, { status: 200, headers: { 'Content-Type': 'application/x-ndjson' } });
    };
  });
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('Create a study note');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat .msg:not(.user)').last()).toContainText('preparing');
  await page.locator('#chat-access').click();
  await page.locator('[data-chat="access-pick"][data-access="ask"]').click();
  await expect.poll(() => updates).toEqual(['ask']);
  expect(access).toBe('ask');
  await page.evaluate(() => window.finishPermissionReply({ kind: 'create', title: 'Daily study', body: 'Study on weekdays', folder: '' }));
  await expect(page.locator('.proposal [data-chat="apply"]')).toBeVisible();
  await expect(page.locator('#chat .msg:not(.user)').last()).toContainText('I am preparing the change. Ready.');
  await page.locator('#chat-input').fill('Plan my weekday interview studying');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
  await page.evaluate(() => window.finishPermissionReply({ kind: 'add_task', summary: 'Add the task “Study weekdays” · every Mon, Tue, Wed, Thu, Fri', title: 'Study weekdays', date: '2026-10-12', repeat: [0, 1, 2, 3, 4] }));
  const taskCard = page.locator('.proposal.add_task');
  await expect(taskCard).toContainText('Study weekdays');
  expect(acts).toHaveLength(0);
  await taskCard.locator('[data-chat="apply"]').click();
  await expect(taskCard.locator('[data-chat="undo"]')).toBeVisible();
  expect(acts[0].kind).toBe('add_task');
  expect(acts[0].repeat).toEqual([0, 1, 2, 3, 4]);
  await taskCard.locator('[data-chat="undo"]').click();
  await expect(taskCard.locator('[data-chat="apply"]')).toBeVisible();
  expect(acts).toHaveLength(2);
  await page.locator('#chat-input').fill('One more study task');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
  await page.locator('#chat-access').click();
  await page.locator('[data-chat="access-pick"][data-access="read"]').click();
  await expect.poll(() => updates.at(-1)).toBe('read');
  await page.evaluate(() => window.finishPermissionReply(null));
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Send');
  await page.locator('#chat-input').fill('One more study plan');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Stop');
  fail = true;
  await page.locator('#chat-access').click();
  await page.locator('[data-chat="access-pick"][data-access="ask"]').click();
  await expect(page.locator('.toast.bad')).toContainText('Permissions could not be changed');
  await expect(page.locator('#chat-send')).toHaveAttribute('aria-label', 'Send');
  expect(await page.locator('#chat-access').textContent()).toContain('Ask first');
});

test('Felix tool activity stays between the response passages it belongs to after reload', async ({ page }) => {
  const events = [{ t: 'I will read your submitted code first.\n\n' }, { step: 'Read your submitted attempt', tool: 'interview_attempt' }, { t: 'Your sliding-window approach fits the problem.\n\n' }, { step: 'Looked at your interview progress', tool: 'interview_progress' }, { step: 'Reviewed your interview attempt', tool: 'interview_feedback' }, { t: 'Next, check repeated categories and ties.' }, { done: true }];
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: events.map((event) => JSON.stringify(event)).join('\n') + '\n' }));
  await page.goto('/');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('Review my code.');
  await page.locator('#chat-input').press('Enter');
  const answer = page.locator('.msg.leo').last();
  await expect(answer).toContainText('Next, check repeated categories and ties.');
  const ordered = async () => {
    const content = await answer.innerText();
    const pieces = ['I will read your submitted code first.', 'Read your submitted attempt', 'Your sliding-window approach fits the problem.', 'Looked at your interview progress', 'Reviewed your interview attempt', 'Next, check repeated categories and ties.'];
    for (let i = 1; i < pieces.length; i++) expect(content.indexOf(pieces[i])).toBeGreaterThan(content.indexOf(pieces[i - 1]));
  };
  await ordered();
  await page.reload();
  await page.locator('#chat-toggle').click();
  await expect(answer).toContainText('Next, check repeated categories and ties.');
  await ordered();
});

test('the code runner runs only your code in a sandbox, stops endless loops and cannot reach leo', async ({ page }) => {
  const run = (job) => page.evaluate((job) => new Promise((resolve) => {
    const frame = document.createElement('iframe');
    frame.setAttribute('sandbox', 'allow-scripts');
    frame.hidden = true;
    const started = Date.now();
    const heard = (e) => {
      if (e.source !== frame.contentWindow) return;
      if (e.data.kind === 'ready') frame.contentWindow.postMessage({ ...job, kind: 'run', run: 'r1' }, '*');
      if (e.data.kind === 'done') { window.removeEventListener('message', heard); frame.remove(); resolve({ ...e.data, took: Date.now() - started }); }
    };
    window.addEventListener('message', heard);
    frame.src = '/runner.html';
    document.body.appendChild(frame);
  }), job);
  const ok = await run({ code: 'function pair_sum(nums, target) { console.log("seen", nums.length); const at = new Map(); for (let i = 0; i < nums.length; i++) { if (at.has(target - nums[i])) return [at.get(target - nums[i]), i]; at.set(nums[i], i); } return []; }', entry: 'pair_sum', tests: [{ id: 't1', args: [[2, 7, 11], 9] }, { id: 't2', args: [[], 1] }] });
  expect(ok.error).toBeUndefined();
  expect(ok.results.map((r) => r.got)).toEqual([[0, 1], []]);
  expect(ok.logs[0]).toBe('seen 3');
  const solution = await run({ code: 'class Solution { twice(x) { return x * 2; } }', entry: 'twice', tests: [{ id: 't1', args: [4] }] });
  expect(solution.results[0].got).toBe(8);
  const ticking = page.evaluate(() => new Promise((resolve) => { let n = 0; const timer = setInterval(() => { n++; if (n === 5) { clearInterval(timer); resolve(n); } }, 100); }));
  const loop = await run({ code: 'function spin() { while (true) {} }', entry: 'spin', tests: [{ id: 't1', args: [] }], limit: 1000 });
  expect(loop.error).toMatch(/Stopped after 1 seconds/);
  expect(await ticking).toBe(5);
  const nosy = await run({ code: 'function steal() { const x = new XMLHttpRequest(); x.open("GET", "/api/notes", false); x.send(); return x.status; }', entry: 'steal', tests: [{ id: 't1', args: [] }] });
  expect(nosy.results[0].error).toBeTruthy();
  expect(nosy.results[0].got).toBeUndefined();
  const broken = await run({ code: 'function f( {', entry: 'f', tests: [] });
  expect(broken.error).toMatch(/did not run/);
  const injected = await run({ code: 'function f() { return 1; }', entry: 'f; fetch("/api/notes")', tests: [] });
  expect(injected.error).toMatch(/no function to call/);
});

test('Storage shows interview practices and browser copies, opens them and confirms selective deletion', async ({ page }) => {
  await page.addInitScript(() => {
    let queue;
    Object.defineProperty(window, 'leoAudioQueue', {configurable:true,get:()=>queue,set:value=>{
      const pick = value.pick;
      value.pick = () => { const data = pick(); const summaries = data.summaries; data.summaries = async () => { await new Promise(resolve=>setTimeout(resolve,750)); return summaries(); }; return data; };
      queue = value;
    }});
  });
  await page.reload();
  const tag = `${test.info().project.name}-${Date.now()}`;
  const ids = [`storage-practice-one-${tag}`, `storage-practice-two-${tag}`];
  for (const [index, id] of ids.entries()) {
    const response = await page.request.put(`/api/interviews/${id}`, { data: { action: 'framework', revision: 0, framework: { title: `Storage practice ${index + 1} ${tag}`, track: 'coding', company: 'Pinterest', role: 'Intern', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' } } });
    expect(response.ok()).toBe(true);
  }
  const chatId = `storage-open-chat-${tag}`;
  expect((await page.request.put(`/api/chats/${chatId}`, {data:{mode:'chat',refs:[],messages:[{role:'user',text:`Stored conversation ${tag}`}]} })).ok()).toBe(true);
  await page.evaluate((ids) => {
    localStorage.setItem('leo-interview-drafts', JSON.stringify(Object.fromEntries(ids.map(id => [`${id}:problem-one`, {answer:'unsaved private code',reasoning:'',board:[]}]))));
    localStorage.setItem('another-app-data', 'keep');
  }, ids);
  await page.evaluate(async () => { await window.leoAudioQueue.pick().put({id:'storage-recovery-01',seq:0,bytes:new Uint8Array(16).buffer}); });
  await page.goto('/#/settings/storage');
  const restore = await page.locator('#tasks-restore').elementHandle();
  await page.locator('[data-export="chats"]').uncheck();
  await page.locator('#tasks-restore').setInputFiles({name:'tasks.json',mimeType:'application/json',buffer:Buffer.from('{"wrong":[]}')});
  await expect(page.locator('.toast.bad')).toContainText('Choose tasks.json');
  const chats = page.locator('[data-area="chats"].store-area');
  await expect(page.locator('[data-area="browser-local"].store-area')).toBeVisible();
  expect(await restore.evaluate(el=>el.isConnected)).toBe(true);
  await expect(page.locator('[data-export="chats"]')).not.toBeChecked();
  await chats.locator('summary').click();
  await chats.locator('.store-item', {hasText:`Stored conversation ${tag}`}).getByRole('button', {name:'Open chat'}).click();
  await expect(page.locator('#chat')).toContainText(`Stored conversation ${tag}`);
  await page.locator('#chat [data-chat="close"]').click();
  const practices = page.locator('[data-area="interviews"].store-area');
  await expect(page.locator('[data-area="browser-local"].store-area')).toBeVisible();
  if (!(await practices.evaluate(el => el.open))) await practices.locator('summary').click();
  await expect(practices).toContainText('Storage practice 1');
  await expect(practices).toContainText('Pinterest');
  await expect(practices).toContainText('saved problems');
  const first = practices.locator('.store-item', {hasText:`Storage practice 1 ${tag}`});
  await first.getByRole('link', {name:'Open practice'}).click();
  await expect(page).toHaveURL(new RegExp(`#/interviews/${ids[0]}$`));
  await expect(page.locator('#app')).toContainText('Storage practice 1');
  await page.goto('/#/settings/storage');
  await expect(practices).toContainText('Storage practice 1');
  if (!(await practices.evaluate(el => el.open))) await practices.locator('summary').click();
  const local = page.locator('[data-area="browser-local"].store-area');
  await expect(local).toBeVisible();
  await local.locator('summary').click();
  await expect(local).toContainText('Interview draft backup');
  await expect(page.locator('.storage')).not.toContainText('unsaved private code');
  await first.locator('input').check();
  await practices.locator('[data-act="delete"]').click();
  await expect(page.locator('.sheet')).toContainText('deleted permanently');
  await page.locator('.sheet [data-action="close"]').click();
  await expect(first).toBeVisible();
  await expect(first.locator('input')).toBeChecked();
  await practices.locator('[data-act="delete"]').click();
  await page.locator('.sheet [data-action="storage-go"]').click();
  await expect(first).toHaveCount(0);
  await expect(practices).toContainText('Storage practice 2');
  expect((await page.request.get(`/api/interviews/${ids[0]}`)).status()).toBe(404);
  expect((await page.request.get(`/api/interviews/${ids[1]}`)).ok()).toBe(true);
  const remaining = await page.evaluate(() => Object.keys(JSON.parse(localStorage.getItem('leo-interview-drafts'))));
  expect(remaining).toEqual([`${ids[1]}:problem-one`]);
  const recovery = page.locator('[data-area="browser-recovery"].store-area');
  await recovery.locator('summary').click();
  const audio = recovery.locator('.store-item', {hasText:'storage-recovery-01'});
  await expect(audio).toContainText('16 bytes');
  await audio.locator('input').check();
  await recovery.locator('[data-act="delete"]').click();
  await expect(page.locator('.sheet')).toContainText('Already saved server audio and notes stay');
  await page.locator('.sheet [data-action="storage-go"]').click();
  await expect(audio).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem('another-app-data'))).toBe('keep');
  await page.evaluate(() => { for (let n = 0; n < 125; n++) localStorage.setItem(`leo-storage-test-${n}`, '{}'); });
  await page.reload();
  await expect(local).toBeVisible();
  await local.evaluate(el => { el.open = true; });
  await expect(local.locator('[data-store-item]')).toHaveCount(100);
  const selected = local.locator('[data-store-item]').first();
  const value = await selected.getAttribute('value');
  await selected.check();
  await local.getByRole('button', {name:/Show more/}).click();
  await expect(local.locator('[data-store-item]')).not.toHaveCount(100);
  await expect(local.locator(`[data-store-item][value="${value}"]`)).toBeChecked();
  await page.request.delete(`/api/interviews/${ids[1]}`);
  await page.request.delete(`/api/chats/${chatId}`);
});

test('interview attempts and late feedback stay under their own problem while practice progress stays shared', async ({ page }) => {
  const id = 'scoped-attempts-ui';
  const problem = n => ({challenge:{id:`scoped-problem-${n}`,title:'Same exercise title',question:`Solve problem ${n}.`,minutes:30,skills:['Approach'],origin:'generated',sources:[],coding:{language:'Python',starter:'def solve():\n    pass',difficulty:'easy',examples:[{input:'1',output:'1'}],constraints:['Explain it.']}},workspace:{answer:'def solve():\n    pass',reasoning:'',board:[]}});
  const problems = [problem(3),problem(4),problem(5)];
  const feedback = text => ({summary:text,scores:[{skill:'Approach',value:3,evidence:'Clear steps',next:'Check boundaries'}]});
  const attempt = (n, suffix, summary) => ({id:`scoped-attempt-${n}-${suffix}`,at:'2026-10-13T12:00:00Z',challenge:structuredClone(problems[n-3].challenge),workspace:structuredClone(problems[n-3].workspace),feedback:summary?feedback(summary):null});
  let item = {id,revision:1,framework:{title:'Attempts by problem',track:'coding',company:'',role:'',language:'Python',style:'coach',date:'',weekly_minutes:120,skills:['Approach'],wants:''},challenge:problems[1].challenge,workspace:problems[1].workspace,problems,attempts:[attempt(3,'first','Feedback for problem 3'),attempt(4,'first','First feedback for problem 4'),attempt(4,'second','Second feedback for problem 4')],schedule:[]};
  await page.route('**/api/interviews/*/key?*', route=>route.fulfill({json:{status:'unavailable',tests:[],asks:[],rubric:[]}}));
  await page.route(`**/api/interviews/${id}`, async route => {
    if (route.request().method()==='PUT') { const change=route.request().postDataJSON(); if(change.action==='select'){const p=problems.find(p=>p.challenge.id===change.challenge);item.challenge=p.challenge;item.workspace=p.workspace;} item.revision++; }
    await route.fulfill({json:item});
  });
  await page.route('**/api/chats/*', route=>route.request().method()==='GET'?route.fulfill({status:404,json:{error:'No conversation'}}):route.fulfill({json:{id,title:'Attempts',updated_at:new Date().toISOString(),count:2,named:true}}));
  await page.route('**/api/chat', route=>{
    item.attempts[0].feedback=feedback('Late feedback for problem 3');item.revision++;
    return route.fulfill({contentType:'application/x-ndjson',body:[{interview:{id,revision:item.revision}},{t:'Review saved.'},{done:true}].map(e=>JSON.stringify(e)).join('\n')+'\n'});
  });
  await page.goto(`/#/interviews/${id}`);
  const attempts = page.locator('#interview-attempts');
  await expect(attempts.locator('.interview-attempt')).toHaveCount(2);
  await expect(attempts).not.toContainText('Feedback for problem 3');
  await expect(attempts).toContainText('First feedback for problem 4');
  await expect(attempts).toContainText('Second feedback for problem 4');
  expect(await attempts.locator('.prose').allTextContents()).toEqual(['Second feedback for problem 4','First feedback for problem 4']);
  expect(await attempts.evaluate(el=>document.querySelector('#interview-pane-submissions').contains(el) && document.querySelector('#interview-challenge').contains(el))).toBe(true);
  await expect(page.locator('#interview-panes [data-pane="submissions"]')).toHaveText('Submissions · 2');
  await page.locator('#interview-panes [data-pane="submissions"]').click();
  await expect(attempts).toBeVisible();
  await expect(page.locator('#interview-progress')).toContainText('3 reviewed attempts');
  await page.locator('#chat-toggle').click();
  await page.locator('#chat-input').fill('Review the earlier submission.');
  await page.locator('#chat-input').press('Enter');
  await expect(page.locator('.msg.leo').last()).toContainText('Review saved');
  await expect(attempts).not.toContainText('Late feedback for problem 3');
  await expect(attempts.locator('.interview-attempt')).toHaveCount(2);
  await page.locator('#chat [data-chat="close"]').click();
  await page.getByRole('button',{name:'Problem 1 · Submitted',exact:true}).click();
  await expect(attempts.locator('.interview-attempt')).toHaveCount(1);
  await expect(attempts).toContainText('Late feedback for problem 3');
  await expect(attempts).not.toContainText('feedback for problem 4');
  await page.getByRole('button',{name:'Problem 3',exact:true}).click();
  await expect(attempts.locator('.interview-attempt')).toHaveCount(0);
  await expect(attempts).toContainText('No submissions for this problem yet');
  await expect(page.locator('#interview-progress')).toContainText('3 reviewed attempts');
  await page.getByRole('button',{name:'Problem 2 · Submitted',exact:true}).click();
  await expect(attempts.locator('.interview-attempt')).toHaveCount(2);
  await page.reload();
  await expect(attempts.locator('.interview-attempt')).toHaveCount(2);
  await expect(attempts).not.toContainText('Late feedback for problem 3');
});

test('submission results show a compact case strip with one detail panel that keeps its place', async ({ page }) => {
  const id = 'results-cases-01';
  const challenge = (n) => ({ id: `results-problem-0${n}`, title: `Results problem ${n}`, question: 'Return the sum of the list.', minutes: 20, skills: ['Approach', 'Correctness'], origin: 'generated', sources: [], coding: { language: 'Go', starter: 'func solve(nums []int) int {\n}', difficulty: 'easy', examples: [{ input: '[1,2]', output: '3' }], constraints: ['n <= 10'] } });
  const first = { challenge: challenge(1), workspace: { answer: 'func solve(nums []int) int {\n}', reasoning: '', board: [] } };
  const second = { challenge: challenge(2), workspace: { answer: 'func solve(nums []int) int {\n}', reasoning: '', board: [] } };
  const item = { id, revision: 1, framework: { title: 'Results practice', track: 'coding', company: '', role: '', language: 'Go', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach', 'Correctness'], wants: '' }, challenge: first.challenge, workspace: first.workspace, problems: [first, second], attempts: [], schedule: [] };
  const long = Array.from({ length: 400 }, (_, i) => `line ${i} of a very long output that keeps going`).join('\n');
  const mixed = [
    ...Array.from({ length: 10 }, (_, i) => ({ id: `t${i + 1}`, kind: 'test', label: i < 3 ? `solve([${i}, ${i + 1}])` : 'Hidden test', pass: ![2, 5, 7].includes(i), given: i === 5 ? long : String(2 * i + 1), expected: String(2 * i + 1) })),
    { id: 'a1', kind: 'value', label: 'Time complexity in n', pass: false, given: 'n^2', expected: 'n' },
    { id: 's1', kind: 'section', label: 'Edge cases', pass: true, given: '', expected: '' },
  ];
  let checks = mixed;
  let gradeRelease;
  const asked = [];
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'solve', tests: [], hidden_tests: 10, asks: [], sections: [], criteria: 2 } } }));
  await page.route(`**/api/interviews/${id}/grade`, async (route) => {
    const attempt = route.request().postDataJSON().attempt;
    await new Promise((resolve) => { gradeRelease = resolve; });
    const found = item.attempts.find((a) => a.id === attempt);
    found.grade = { criteria: [{ id: 'c1', text: 'Handles an empty list', verdict: 'missed', why: 'Returns garbage for [].' }, { id: 'c2', text: 'Explains the complexity', verdict: 'met', why: 'Says linear.' }], overall: 'A sound start; cover the empty list.', at: new Date().toISOString() };
    item.revision++;
    await route.fulfill({ json: item });
  });
  await page.route(`**/api/interviews/${id}`, async (route) => {
    if (route.request().method() === 'PUT') {
      const change = route.request().postDataJSON();
      if (change.action === 'submit') item.attempts.push({ id: change.attempt, challenge: structuredClone(item.challenge), workspace: structuredClone(change.workspace), at: new Date().toISOString(), feedback: null, checks: structuredClone(checks) });
      if (change.action === 'select') {
        const problem = item.problems.find((p) => p.challenge.id === change.challenge);
        item.challenge = problem.challenge;
        item.workspace = problem.workspace;
      }
      if (change.action === 'workspace') item.workspace = change.workspace;
      item.revision++;
    }
    await route.fulfill({ json: item });
  });
  await page.route('**/api/chats/*', (route) => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No conversation yet' } }) : route.fulfill({ json: { id, title: 'Results practice', updated_at: new Date().toISOString(), count: 2, named: true } }));
  await page.route('**/api/chat', (route) => {
    asked.push(route.request().postDataJSON());
    return route.fulfill({ contentType: 'application/x-ndjson', body: JSON.stringify({ t: 'Noted.' }) + '\n' + JSON.stringify({ done: true }) + '\n' });
  });
  await page.goto(`/#/interviews/${id}`);
  const area = page.locator('#interview-answer');
  await area.fill('func solve(nums []int) int {\n\ttotal := 0\n\treturn total\n}');
  const tabs = page.locator('#interview-panes [data-interview="pane"]');
  await expect(tabs).toHaveText(['Question', 'Submissions']);
  await expect(page.getByText('Practice schedule', { exact: true })).toHaveCount(0);
  await page.locator('[data-interview="submit"]').click();
  const results = page.locator('#interview-results');
  await expect(tabs).toHaveText(['Question', 'Result · 8/12', 'Submissions · 1']);
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('#interview-pane-question')).toBeHidden();
  await expect(results).toBeVisible();
  await expect(results.locator('.interview-result-summary')).toContainText('8 passed');
  await expect(results.locator('.interview-result-summary')).toContainText('4 failed');
  const chips = results.locator('[data-interview="result-case"]');
  await expect(chips).toHaveCount(12);
  const strip = await results.locator('.interview-cases').evaluate((el) => ({ tops: [...el.children].map((c) => c.offsetTop), scroll: el.scrollWidth, width: el.clientWidth }));
  expect(new Set(strip.tops).size).toBe(1);
  expect(strip.scroll).toBeGreaterThan(strip.width);
  await expect(chips.nth(2)).toHaveAttribute('aria-selected', 'true');
  const panel = results.locator('#interview-case');
  await expect(panel).toContainText('Case 3');
  await expect(panel).toContainText('Failed');
  await expect(panel.locator('.interview-io').first()).toContainText('solve([2, 3])');
  await expect(panel).toContainText('Your output');
  await expect(panel).toContainText('Expected output');
  await expect(panel.locator('[data-interview="check-wrong"]')).toBeVisible();
  await results.locator('.interview-cases').scrollIntoViewIfNeeded();
  const before = await page.evaluate(() => ({ y: scrollY, answer: document.querySelector('#interview-answer').value }));
  await chips.nth(0).click();
  await expect(panel).toContainText('Passed');
  await expect(panel.locator('[data-interview="check-wrong"]')).toHaveCount(0);
  await chips.nth(5).click();
  await expect(panel).toContainText('hidden test');
  await expect(panel.getByText('Input', { exact: true })).toHaveCount(0);
  const output = await panel.locator('.interview-output').first().evaluate((el) => ({ height: el.clientHeight, scroll: el.scrollHeight, wraps: getComputedStyle(el).whiteSpace }));
  expect(output.height).toBeLessThanOrEqual(170);
  expect(output.scroll).toBeGreaterThan(output.height);
  expect(output.wraps).toBe('pre-wrap');
  await chips.nth(10).click();
  await expect(panel).toContainText('Time complexity in n');
  await expect(panel).toContainText('Your answer');
  await chips.nth(11).click();
  await expect(panel).toContainText('Your answer covers this part.');
  await chips.nth(10).focus();
  await page.keyboard.press('ArrowRight');
  await expect(chips.nth(11)).toBeFocused();
  await expect(chips.nth(11)).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Home');
  await expect(chips.nth(0)).toBeFocused();
  const after = await page.evaluate(() => ({ y: scrollY, answer: document.querySelector('#interview-answer').value }));
  expect(after.answer).toBe(before.answer);
  expect(Math.abs(after.y - before.y)).toBeLessThan(2);
  await tabs.nth(0).click();
  await expect(page.locator('#interview-pane-question')).toContainText('Return the sum of the list.');
  await expect(results).toBeHidden();
  await tabs.nth(0).focus();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.nth(1)).toBeFocused();
  await expect(results).toBeVisible();
  await expect(chips.nth(0)).toHaveAttribute('aria-selected', 'true');
  await chips.nth(7).click();
  await expect.poll(() => typeof gradeRelease).toBe('function');
  await expect(results.locator('#interview-grade-meter[role="progressbar"]')).toContainText('Quick review ·');
  gradeRelease();
  await expect(results.locator('.interview-review')).toContainText('A sound start');
  await expect(results.locator('.interview-review .interview-tally')).toHaveText(['1 met', '1 missed']);
  await expect(results.locator('.interview-review-more')).not.toHaveAttribute('open', '');
  await expect(results.getByText('Returns garbage for [].')).toBeHidden();
  await expect(chips.nth(7)).toHaveAttribute('aria-selected', 'true');
  await expect(panel).toContainText('Case 8');
  await expect.poll(() => asked.length).toBe(1);
  await expect(page.locator('.msg.leo').last()).toContainText('Noted.');
  await page.locator('[data-chat="close"]').click();
  await results.locator('.interview-review-more > summary').click();
  await expect(results.getByText('Returns garbage for [].')).toBeVisible();
  await expect(chips.nth(7)).toHaveAttribute('aria-selected', 'true');
  await chips.nth(2).click();
  await panel.locator('[data-interview="check-wrong"]').click();
  await expect.poll(() => asked.length).toBe(2);
  expect(asked[1].messages.at(-1).text).toContain('solve([2, 3])');
  await expect(page.locator('.msg.leo').last()).toContainText('Noted.');
  await page.locator('[data-chat="close"]').click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await page.getByRole('button', { name: 'Problem 2', exact: true }).click();
  await expect(results.locator('.interview-result')).toHaveCount(0);
  await expect(tabs).toHaveText(['Question', 'Submissions']);
  await expect(page.locator('#interview-pane-question')).toContainText('Return the sum of the list.');
  await page.getByRole('button', { name: /^Problem 1(?: · Submitted)?$/ }).click();
  await expect(results.locator('[data-interview="result-case"]')).toHaveCount(12);
  checks = mixed.map((c) => ({ ...c, pass: true }));
  await area.fill('func solve(nums []int) int {\n\treturn 0\n}');
  await page.locator('[data-interview="submit"]').click();
  await expect(tabs.nth(1)).toHaveText('Result · 12/12');
  await expect(results).toBeVisible();
  await expect(results.locator('.interview-result-summary')).toContainText('12 passed');
  await expect(results.locator('.interview-result-summary')).not.toContainText('failed');
  await expect(results.locator('[data-interview="result-case"]').first()).toHaveAttribute('aria-selected', 'true');
  await expect(panel).toContainText('Passed');
  await expect(panel.locator('[data-interview="check-wrong"]')).toHaveCount(0);
  gradeRelease();
});

async function practiceWithResults(page, id) {
  const challenge = { id: `${id}-problem`, title: 'Two sum', question: 'Return two indices.', minutes: 20, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Go', starter: 'func twoSum(nums []int, target int) []int {\n}', difficulty: 'easy', examples: [{ input: '[2,7], 9', output: '[0,1]' }], constraints: ['n <= 100'] } };
  const workspace = { answer: 'func twoSum(nums []int, target int) []int {\n}', reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Results', track: 'coding', company: '', role: '', language: 'Go', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [], schedule: [] };
  const checks = [{ id: 't1', kind: 'test', label: 'twoSum([2, 7], 9)', pass: true, given: '[0,1]', expected: '[0,1]' }, { id: 't2', kind: 'test', label: 'Hidden test', pass: false, given: 'Error: index out of range', expected: '[]' }, { id: 'a1', kind: 'value', label: 'Time complexity in n', pass: false, given: 'n^2', expected: 'n' }, { id: 's1', kind: 'section', label: 'Edge cases', pass: true, given: '', expected: '' }];
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'twoSum', tests: [], hidden_tests: 1, asks: [], sections: [], criteria: 3 } } }));
  await page.route(`**/api/interviews/${id}/grade`, (route) => {
    item.attempts.at(-1).grade = { criteria: [{ id: 'c1', text: 'Handles duplicates', verdict: 'partly', why: 'Misses equal pairs.' }, { id: 'c2', text: 'States complexity', verdict: 'met', why: 'Linear.' }, { id: 'c3', text: 'Explains the plan', verdict: 'missed', why: 'No plan given.' }], overall: 'Correct idea; explain the plan first.', at: new Date().toISOString() };
    return route.fulfill({ json: item });
  });
  await page.route(`**/api/interviews/${id}`, (route) => {
    if (route.request().method() === 'PUT') {
      const change = route.request().postDataJSON();
      if (change.action === 'submit') item.attempts.push({ id: change.attempt, challenge, workspace: change.workspace, at: new Date().toISOString(), feedback: null, checks });
      item.revision++;
    }
    return route.fulfill({ json: item });
  });
  await page.route('**/api/chats/*', (route) => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No conversation yet' } }) : route.fulfill({ json: { id, title: 'Results', updated_at: new Date().toISOString(), count: 2, named: true } }));
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: JSON.stringify({ t: 'Here is my review.' }) + '\n' + JSON.stringify({ done: true }) + '\n' }));
  await page.goto(`/#/interviews/${id}`);
  await page.locator('#interview-answer').fill('func twoSum(nums []int, target int) []int {\n\treturn nil\n}');
  await page.locator('[data-interview="submit"]').click();
  await expect(page.locator('.interview-review')).toBeVisible();
  await expect(page.locator('.msg.leo').last()).toContainText('Here is my review.');
  await page.locator('[data-chat="close"]').click();
}

test('Run shows its results in the Test cases tab beside the question', async ({ page }) => {
  const id = 'run-cases-01';
  const challenge = { id: 'run-cases-problem', title: 'Sum a list', question: 'Return the sum of the list.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'JavaScript', starter: 'function solve(nums) {\n}', difficulty: 'easy', examples: [{ input: '[1,2]', output: '3' }], constraints: ['n <= 10'] } };
  const workspace = { answer: 'function solve(nums) {\n}', reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Run practice', track: 'coding', company: '', role: '', language: 'JavaScript', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'solve', tests: [{ id: 't1', args: [[1, 2]], expected: 3, hidden: false, unordered: false }, { id: 't2', args: [[]], expected: 0, hidden: false, unordered: false }], hidden_tests: 3, asks: [], sections: [], criteria: 1 } } }));
  await page.route(`**/api/interviews/${id}`, (route) => {
    if (route.request().method() === 'PUT') { item.workspace = route.request().postDataJSON().workspace || item.workspace; item.revision++; }
    return route.fulfill({ json: item });
  });
  await page.goto(`/#/interviews/${id}`);
  const tabs = page.locator('#interview-panes [data-interview="pane"]');
  await expect(tabs).toHaveText(['Question', 'Test cases', 'Submissions']);
  await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true');
  await page.locator('#interview-answer').fill('function solve(nums) {\n  return nums.length ? nums.reduce((a, b) => a + b) : 1;\n}');
  const before = await page.evaluate(() => document.querySelector('#interview-answer').value);
  await page.locator('[data-interview="run"]').click();
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  const cases = page.locator('#interview-pane-cases');
  await expect(cases).toBeVisible();
  await expect(cases.locator('.interview-tally')).toHaveText('1/2 passed');
  await expect(cases.locator('li.pass')).toHaveCount(1);
  await expect(cases.locator('li.fail')).toContainText('yours: 1');
  await expect(page.locator('#interview-checks .interview-tests')).toHaveCount(0);
  expect(await page.evaluate(() => document.querySelector('#interview-answer').value)).toBe(before);
  await tabs.nth(0).click();
  await expect(page.locator('#interview-pane-question')).toContainText('Return the sum of the list.');
  await expect(cases).toBeHidden();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
});

test('test cases are checked against a reference solution first, Run is kept for Felix, and hidden tests show after Submit', async ({ page }) => {
  const id = 'checked-cases-01';
  const challenge = { id: 'checked-cases-problem', title: 'Sum a list', question: 'Return the sum of the list.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'JavaScript', starter: 'function solve(nums) {\n}', difficulty: 'easy', examples: [{ input: '[1,2]', output: '3' }], constraints: ['n <= 10'] } };
  const workspace = { answer: 'function solve(nums) {\n}', reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Checked practice', track: 'coding', company: '', role: '', language: 'JavaScript', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  const t1 = { id: 't1', args: [[1, 2]], expected: 3, hidden: false, unordered: false };
  const t2 = { id: 't2', args: [[2, 2]], expected: 5, hidden: false, unordered: false };
  const t3 = { id: 't3', args: [[]], expected: 0, hidden: true, unordered: false };
  const base = { entry: 'solve', asks: [], sections: [], criteria: 1 };
  let key = { ...base, tests: [t1, t2], hidden_tests: 1, revealed: false, verify: true };
  let checked = null;
  let saved = null;
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key } }));
  await page.route(`**/api/interviews/${id}/key/check`, (route) => route.fulfill({ json: { entry: 'solve', solution: 'function solve(nums) { return nums.reduce((a, b) => a + b, 0); }', stamp: 'stamp-1', tests: [t1, t2, t3] } }));
  await page.route(`**/api/interviews/${id}/key/checked`, (route) => {
    checked = route.request().postDataJSON();
    key = { ...base, tests: [t1], hidden_tests: 1, revealed: false, verify: false };
    return route.fulfill({ json: { status: 'ready', key, dropped: 1 } });
  });
  await page.route(`**/api/interviews/${id}/run`, (route) => { saved = route.request().postDataJSON(); return route.fulfill({ status: 204 }); });
  await page.route(`**/api/interviews/${id}/hidden-tests`, (route) => route.fulfill({ json: { tests: [t3] } }));
  await page.route(`**/api/interviews/${id}/grade`, (route) => route.fulfill({ status: 400, json: { error: 'No quick review in this test.' } }));
  await page.route(`**/api/interviews/${id}`, (route) => {
    if (route.request().method() === 'PUT') {
      const change = route.request().postDataJSON();
      if (change.action === 'submit') {
        item.attempts.push({ id: change.attempt, challenge: structuredClone(challenge), workspace: structuredClone(change.workspace), at: new Date().toISOString(), feedback: null, checks: change.tests.map((r) => { const t = [t1, t3].find((x) => x.id === r.id); return { id: r.id, kind: 'test', label: `solve(${JSON.stringify(t.args[0])})`, pass: r.pass, given: r.got, expected: JSON.stringify(t.expected), ...(t.hidden ? { hidden: true } : {}) }; }) });
        key = { ...base, tests: [t1, t3], hidden_tests: 1, revealed: true, verify: false };
      } else if (change.workspace) item.workspace = change.workspace;
      item.revision++;
    }
    return route.fulfill({ json: item });
  });
  await page.route('**/api/chats/*', (route) => route.request().method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'No conversation yet' } }) : route.fulfill({ json: { id, title: 'Checked practice', updated_at: new Date().toISOString(), count: 2, named: true } }));
  await page.route('**/api/chat', (route) => route.fulfill({ contentType: 'application/x-ndjson', body: JSON.stringify({ t: 'Noted.' }) + '\n' + JSON.stringify({ done: true }) + '\n' }));
  await page.goto(`/#/interviews/${id}`);
  await expect.poll(() => checked && checked.stamp).toBe('stamp-1');
  expect(checked.reports.map((r) => [r.id, r.pass])).toEqual([['t1', true], ['t2', false], ['t3', true]]);
  expect(checked.error).toBe('');
  const tabs = page.locator('#interview-panes [data-interview="pane"]');
  await expect(tabs).toHaveText(['Question', 'Test cases', 'Submissions']);
  await tabs.nth(1).click();
  const cases = page.locator('#interview-pane-cases');
  await expect(cases.locator('li')).toHaveCount(1);
  await expect(cases).not.toContainText('[2,2]');
  await page.locator('#interview-answer').fill('function solve(nums) {\n  return nums.reduce((a, b) => a + b);\n}');
  await page.locator('[data-interview="run"]').click();
  await expect(cases.locator('.interview-tally')).toHaveText('1/1 passed');
  await expect.poll(() => saved && saved.challenge).toBe(challenge.id);
  expect(saved.reports).toEqual([{ id: 't1', pass: true, got: '3' }]);
  await page.locator('[data-interview="submit"]').click();
  await expect(tabs.nth(2)).toHaveText('Result · 1/2');
  const results = page.locator('#interview-results');
  await expect(results.locator('#interview-case-t3')).toHaveAttribute('aria-selected', 'true');
  await expect(results).toContainText('solve([])');
  await expect(results).toContainText('hidden test');
  await expect(page.locator('.msg.leo').last()).toContainText('Noted.');
  await page.locator('[data-chat="close"]').click();
  await tabs.nth(1).click();
  await expect(cases.locator('li')).toHaveCount(2);
  await expect(cases.locator('li').nth(1)).toContainText('Hidden');
  await expect(cases).toContainText('including the ones hidden until you submitted');
});

for (const [language, starter, answer, tests, passed] of [
  ['TypeScript', 'function solve(nums: number[]): number {\n}', 'function solve(nums: number[]): number {\n  const total: number = nums.reduce((a: number, b: number) => a + b, 0);\n  return total;\n}', [{ id: 't1', args: [[1, 2]], expected: 3, hidden: false, unordered: false }, { id: 't2', args: [[]], expected: 0, hidden: false, unordered: false }], '2/2 passed'],
  ['SQL', 'SELECT -- your query', 'SELECT name FROM people WHERE age >= 18 ORDER BY name;', [{ id: 't1', args: ["CREATE TABLE people(name TEXT, age INTEGER); INSERT INTO people VALUES ('Ana', 30), ('Bo', 12), ('Cy', 18);"], expected: [['Ana'], ['Cy']], hidden: false, unordered: false }, { id: 't2', args: ['CREATE TABLE people(name TEXT, age INTEGER);'], expected: [['Nobody']], hidden: false, unordered: false }], '1/2 passed'],
]) {
  test(`${language} problems run their tests in the browser sandbox`, async ({ page }) => {
    const id = `run-${language.toLowerCase()}-01`;
    const challenge = { id: `run-${language.toLowerCase()}-problem`, title: `${language} practice`, question: 'Do the thing.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language, starter, difficulty: 'easy', examples: [{ input: 'x', output: 'y' }], constraints: ['small'] } };
    const workspace = { answer: starter, reasoning: '', board: [] };
    const item = { id, revision: 1, framework: { title: `${language} practice`, track: 'coding', company: '', role: '', language, style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
    await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
    await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: language === 'SQL' ? 'sql' : 'solve', tests, hidden_tests: 0, revealed: false, verify: false, asks: [], sections: [], criteria: 1 } } }));
    await page.route(`**/api/interviews/${id}/run`, (route) => route.fulfill({ status: 204 }));
    await page.route(`**/api/interviews/${id}`, (route) => {
      if (route.request().method() === 'PUT') { item.workspace = route.request().postDataJSON().workspace || item.workspace; item.revision++; }
      return route.fulfill({ json: item });
    });
    await page.goto(`/#/interviews/${id}`);
    await page.locator('#interview-answer').fill(answer);
    await page.locator('[data-interview="run"]').click();
    const cases = page.locator('#interview-pane-cases');
    await expect(cases.locator('.interview-tally')).toHaveText(passed, { timeout: 20000 });
    if (language === 'SQL') {
      await expect(cases.locator('li').first()).toContainText('CREATE TABLE people');
      await expect(cases.locator('li.fail')).toContainText('yours: []');
    }
  });
}

test('preparing instant checks shows a moving progress bar with time left, says when it runs long, and fills before the checks appear', async ({ page }) => {
  const id = 'progress-key-01';
  const challenge = { id: 'progress-key-problem', title: 'Sum a list', question: 'Return the sum.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'JavaScript', starter: 'function solve(nums) {\n}', difficulty: 'easy', examples: [{ input: '[1,2]', output: '3' }], constraints: ['n <= 10'] } };
  const workspace = { answer: 'function solve(nums) {\n}', reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Progress practice', track: 'coding', company: '', role: '', language: 'JavaScript', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  let fraction = 0.2;
  let ready = false;
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: ready ? { status: 'failed', error: 'stopped for the test' } : { status: 'preparing', progress: { fraction, seconds: Math.round(fraction * 60), expected: 45, left: Math.max(0, 45 - Math.round(fraction * 60)), slow: fraction > 0.9 } } }));
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.goto(`/#/interviews/${id}`);
  const meter = page.locator('#interview-key-meter');
  await expect(meter).toHaveAttribute('role', 'progressbar');
  await expect(meter).toContainText('Preparing instant checks · 20% · 12 s · about 33 s left');
  fraction = 0.6;
  await expect(meter).toHaveAttribute('aria-valuenow', '60');
  await expect(meter).toContainText('60% · 36 s · about 9 s left');
  fraction = 0.95;
  await expect(meter).toContainText('95% · 57 s · taking longer than usual');
  await page.evaluate(() => {
    window.meterSeen = [];
    new MutationObserver(() => {
      const m = document.getElementById('interview-key-meter');
      const now = m ? `${m.getAttribute('aria-valuenow')} ${m.querySelector('.meter-text').textContent}` : 'gone';
      if (window.meterSeen.at(-1) !== now) window.meterSeen.push(now);
    }).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
  });
  ready = true;
  await expect(meter).toHaveCount(0);
  expect((await page.evaluate(() => window.meterSeen)).filter((seen) => !seen.startsWith('95 '))).toEqual(['100 Preparing instant checks · done', 'gone']);
});

test('C++ downloads its compiler once with a progress bar and explains types it cannot fill in', async ({ page }) => {
  const id = 'cpp-wiring-01';
  const starter = 'ListNode* reverse(ListNode* head) {\n}';
  const challenge = { id: 'cpp-wiring-problem', title: 'Reverse a list', question: 'Reverse it.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'C++', starter, difficulty: 'easy', examples: [{ input: '[1,2]', output: '[2,1]' }], constraints: ['small'] } };
  const workspace = { answer: starter, reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'C++ practice', track: 'coding', company: '', role: '', language: 'C++', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  let done = 0;
  let asked = 0;
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'reverse', tests: [{ id: 't1', args: [[1, 2]], expected: [2, 1], hidden: false, unordered: false }], hidden_tests: 0, revealed: false, verify: false, asks: [], sections: [], criteria: 1 } } }));
  await page.route(`**/api/interviews/${id}/run`, (route) => route.fulfill({ status: 204 }));
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.route('**/api/cpp', (route) => {
    if (route.request().method() === 'POST') asked++;
    else if (!asked) return route.fulfill({ json: { status: 'missing', mb: 27 } });
    if (done >= 3) return route.fulfill({ json: { status: 'ready' } });
    done++;
    return route.fulfill({ json: { status: 'fetching', mb: 27, done: done * 9000000, total: 27112311 } });
  });
  await page.goto(`/#/interviews/${id}`);
  await page.locator('[data-interview="run"]').click();
  await expect(page.locator('#interview-runtime-meter')).toContainText('Getting C++ ready, once (about 27 MB)');
  const cases = page.locator('#interview-pane-cases');
  await expect(cases.locator('.set-note.warn')).toContainText('types the browser can’t fill in yet');
  await expect(cases.locator('li.fail')).toHaveCount(1);
  expect(asked).toBe(1);
});

test('Go downloads once with a progress bar and explains code it cannot test', async ({ page }) => {
  const id = 'go-wiring-01';
  const starter = 'func main() {\n}\n\nfunc twoSum(nums []int, target int) []int {\n}';
  const challenge = { id: 'go-wiring-problem', title: 'Two sum', question: 'Return indices.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Go', starter, difficulty: 'easy', examples: [{ input: '[2,7], 9', output: '[0,1]' }], constraints: ['small'] } };
  const workspace = { answer: starter, reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Go practice', track: 'coding', company: '', role: '', language: 'Go', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  let done = 0;
  let asked = 0;
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'twoSum', tests: [{ id: 't1', args: [[2, 7], 9], expected: [0, 1], hidden: false, unordered: false }], hidden_tests: 0, revealed: false, verify: false, asks: [], sections: [], criteria: 1 } } }));
  await page.route(`**/api/interviews/${id}/run`, (route) => route.fulfill({ status: 204 }));
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.route('**/api/go', (route) => {
    if (route.request().method() === 'POST') asked++;
    else if (!asked) return route.fulfill({ json: { status: 'missing', mb: 21 } });
    if (done >= 3) return route.fulfill({ json: { status: 'ready' } });
    done++;
    return route.fulfill({ json: { status: 'fetching', mb: 21, done: done * 7000000, total: 20900000 } });
  });
  await page.goto(`/#/interviews/${id}`);
  await page.locator('[data-interview="run"]').click();
  await expect(page.locator('#interview-runtime-meter')).toContainText('Getting Go ready, once (about 21 MB)');
  const cases = page.locator('#interview-pane-cases');
  await expect(cases.locator('.set-note.warn')).toContainText('Remove func main');
  await expect(cases.locator('li.fail')).toHaveCount(1);
  expect(asked).toBe(1);
});

test('Java downloads once with a progress bar and explains types it cannot fill in', async ({ page }) => {
  const id = 'java-wiring-01';
  const starter = 'class Solution {\n    public int count(Map<String, Integer> words) {\n    }\n}';
  const challenge = { id: 'java-wiring-problem', title: 'Count', question: 'Count them.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Java', starter, difficulty: 'easy', examples: [{ input: '{}', output: '0' }], constraints: ['small'] } };
  const workspace = { answer: starter, reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Java practice', track: 'coding', company: '', role: '', language: 'Java', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  let done = 0;
  let asked = 0;
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'count', tests: [{ id: 't1', args: [{}], expected: 0, hidden: false, unordered: false }], hidden_tests: 0, revealed: false, verify: false, asks: [], sections: [], criteria: 1 } } }));
  await page.route(`**/api/interviews/${id}/run`, (route) => route.fulfill({ status: 204 }));
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.route('**/api/java', (route) => {
    if (route.request().method() === 'POST') asked++;
    else if (!asked) return route.fulfill({ json: { status: 'missing', mb: 5 } });
    if (done >= 3) return route.fulfill({ json: { status: 'ready' } });
    done++;
    return route.fulfill({ json: { status: 'fetching', mb: 5, done: done * 1400000, total: 4320000 } });
  });
  await page.goto(`/#/interviews/${id}`);
  await page.locator('[data-interview="run"]').click();
  await expect(page.locator('#interview-runtime-meter')).toContainText('Getting Java ready, once (about 5 MB)');
  const cases = page.locator('#interview-pane-cases');
  await expect(cases.locator('.set-note.warn')).toContainText('types the browser can’t fill in yet');
  expect(asked).toBe(1);
});

test('Rust downloads once with a progress bar and explains code it cannot test', async ({ page }) => {
  const id = 'rust-wiring-01';
  const starter = 'fn main() {\n}\n\nimpl Solution {\n    pub fn two_sum(nums: Vec<i32>, target: i32) -> Vec<i32> {\n    }\n}';
  const challenge = { id: 'rust-wiring-problem', title: 'Two sum', question: 'Return indices.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Rust', starter, difficulty: 'easy', examples: [{ input: '[2,7], 9', output: '[0,1]' }], constraints: ['small'] } };
  const workspace = { answer: starter, reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Rust practice', track: 'coding', company: '', role: '', language: 'Rust', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  let done = 0;
  let asked = 0;
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'ready', key: { entry: 'two_sum', tests: [{ id: 't1', args: [[2, 7], 9], expected: [0, 1], hidden: false, unordered: false }], hidden_tests: 0, revealed: false, verify: false, asks: [], sections: [], criteria: 1 } } }));
  await page.route(`**/api/interviews/${id}/run`, (route) => route.fulfill({ status: 204 }));
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.route('**/api/rust', (route) => {
    if (route.request().method() === 'POST') asked++;
    else if (!asked) return route.fulfill({ json: { status: 'missing', mb: 46 } });
    if (done >= 3) return route.fulfill({ json: { status: 'ready' } });
    done++;
    return route.fulfill({ json: { status: 'fetching', mb: 46, done: done * 15000000, total: 46200000 } });
  });
  await page.goto(`/#/interviews/${id}`);
  await page.locator('[data-interview="run"]').click();
  await expect(page.locator('#interview-runtime-meter')).toContainText('Getting Rust ready, once (about 46 MB)');
  const cases = page.locator('#interview-pane-cases');
  await expect(cases.locator('.set-note.warn')).toContainText('Remove fn main');
  expect(asked).toBe(1);
});

async function boardLabel(page, text, fx = 0.2, fy = 0.25) {
  await page.locator('[data-interview="tool"][data-tool="text"]').click();
  const board = page.locator('#interview-board');
  await board.scrollIntoViewIfNeeded();
  const box = await board.boundingBox();
  await page.mouse.click(box.x + box.width * fx, box.y + box.height * fy);
  const editor = page.locator('.interview-board-editor');
  await expect(editor).toBeFocused();
  await editor.fill(text);
  await editor.press('Enter');
  await expect(editor).toHaveCount(0);
}

test('the whiteboard works like a slide editor: icon tools, selecting inside shapes, drag boxes, handles, editing text and shortcuts', async ({ page }) => {
  const id = 'slide-board-01';
  const challenge = { id: 'slide-board-problem', title: 'Design a cache', question: 'Sketch it.', minutes: 30, skills: ['Approach'], origin: 'generated', sources: [] };
  const workspace = { answer: 'Draft', reasoning: '', board: [
    { kind: 'box', points: [[100, 100], [300, 220]], text: '' },
    { kind: 'text', points: [[500, 170]], text: 'Cache' },
    { kind: 'arrow', points: [[310, 160], [480, 160]], text: '' },
    { kind: 'pen', points: [[150, 420], [250, 470], [350, 430]], text: '' },
  ] };
  const item = { id, revision: 1, framework: { title: 'Board practice', track: 'systems', company: '', role: '', language: '', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'none' } }));
  await page.route(`**/api/interviews/${id}`, (route) => {
    if (route.request().method() === 'PUT') { item.workspace = route.request().postDataJSON().workspace || item.workspace; item.revision++; }
    return route.fulfill({ json: item });
  });
  await page.goto(`/#/interviews/${id}`);
  await page.locator('[data-interview="tab"][data-tab="board"]').click();
  const board = page.locator('#interview-board');
  await board.scrollIntoViewIfNeeded();
  const tools = page.locator('[data-interview="tool"]');
  await expect(tools).toHaveCount(5);
  expect(await tools.evaluateAll((els) => els.map((el) => [el.dataset.tool, el.textContent.trim(), Boolean(el.querySelector('svg')), el.getAttribute('aria-label')]))).toEqual([['select', '', true, 'Select and move'], ['pen', '', true, 'Pen'], ['box', '', true, 'Box'], ['arrow', '', true, 'Arrow'], ['text', '', true, 'Text']]);
  await expect(page.locator('[data-interview="tool"][data-tool="pan"]')).toHaveCount(0);
  await expect(tools.first()).toHaveAttribute('aria-pressed', 'true');
  expect(await board.evaluate((el) => [getComputedStyle(el).backgroundColor, getComputedStyle(el.querySelector('svg')).color])).toEqual(['rgb(255, 255, 255)', 'rgb(25, 25, 27)']);
  const at = (x, y) => board.locator('svg').evaluate((svg, [x, y]) => { const p = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM()); return { x: p.x, y: p.y }; }, [x, y]);
  const click = async (x, y, { shift = false } = {}) => { const p = await at(x, y); if (shift) await page.keyboard.down('Shift'); await page.mouse.click(p.x, p.y); if (shift) await page.keyboard.up('Shift'); };
  const drag = async (from, to) => { const a = await at(...from), b = await at(...to); await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, { steps: 5 }); await page.mouse.up(); };
  const chosen = () => board.locator('[data-board-shape][aria-pressed="true"]').evaluateAll((els) => els.map((el) => Number(el.dataset.boardShape)).sort());
  await click(200, 160);
  await expect.poll(chosen).toEqual([0]);
  await expect(board.locator('[data-board-handle]')).toHaveCount(8);
  await drag([300, 220], [380, 260]);
  await expect.poll(() => item.workspace.board[0].points).toEqual([[100, 100], [380, 260]]);
  await page.keyboard.press('Escape');
  await expect.poll(chosen).toEqual([]);
  await drag([60, 60], [620, 300]);
  await expect.poll(chosen).toEqual([0, 1, 2]);
  await click(200, 445, { shift: true });
  await expect.poll(chosen).toEqual([0, 1, 2, 3]);
  await click(200, 445, { shift: true });
  await expect.poll(chosen).toEqual([0, 1, 2]);
  await expect(board.locator('[data-board-handle]')).toHaveCount(0);
  await page.keyboard.press('Delete');
  await expect.poll(() => item.workspace.board.map((s) => s.kind)).toEqual(['pen']);
  await page.keyboard.press('ControlOrMeta+z');
  await expect.poll(() => item.workspace.board.length).toBe(4);
  await page.keyboard.press('ControlOrMeta+Shift+z');
  await expect.poll(() => item.workspace.board.length).toBe(1);
  await page.locator('[data-interview="undo-board"]').click();
  await expect.poll(() => item.workspace.board.length).toBe(4);
  const label = await at(530, 160);
  await page.mouse.dblclick(label.x, label.y);
  const editor = page.locator('.interview-board-editor');
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue('Cache');
  await editor.fill('Redis cache');
  await editor.press('Enter');
  await expect.poll(() => item.workspace.board[1].text).toBe('Redis cache');
  const middle = await at(240, 180);
  await page.mouse.dblclick(middle.x, middle.y);
  await expect(editor).toBeFocused();
  await editor.fill('API');
  await editor.press('Enter');
  await expect.poll(() => item.workspace.board[0].text).toBe('API');
  await expect(board.locator('text', { hasText: 'API' })).toHaveCount(1);
  await page.mouse.dblclick(label.x, label.y);
  await editor.fill('Never saved');
  await editor.press('Escape');
  await expect.poll(() => item.workspace.board[1].text).toBe('Redis cache');
  await board.focus();
  await page.keyboard.press('ControlOrMeta+a');
  await expect.poll(chosen).toEqual([0, 1, 2, 3]);
  await page.keyboard.press('ControlOrMeta+d');
  await expect.poll(() => item.workspace.board.length).toBe(8);
  await expect.poll(chosen).toEqual([4, 5, 6, 7]);
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => item.workspace.board[4].points[0][0]).toBeGreaterThan(item.workspace.board[0].points[0][0]);
  await page.keyboard.press('Escape');
  await page.keyboard.press('t');
  await expect(page.locator('[data-interview="tool"][data-tool="text"]')).toHaveAttribute('aria-pressed', 'true');
  await click(600, 380);
  await expect(editor).toBeFocused();
  await editor.fill('Read replica');
  await editor.press('Enter');
  await expect.poll(() => item.workspace.board.at(-1)).toMatchObject({ kind: 'text', text: 'Read replica' });
  await expect(page.locator('[data-interview="tool"][data-tool="select"]')).toHaveAttribute('aria-pressed', 'true');
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
});

test('the question and workspace split can be dragged, nudged, reset and is remembered', async ({ page }) => {
  const id = 'split-practice-01';
  const challenge = { id: 'split-problem', title: 'Sum a list', question: 'Return the sum.', minutes: 10, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter: 'def solve(nums):\n    pass', difficulty: 'easy', examples: [{ input: '[1,2]', output: '3' }], constraints: ['n <= 10'] } };
  const workspace = { answer: 'def solve(nums):\n    pass', reasoning: '', board: [] };
  const item = { id, revision: 1, framework: { title: 'Split practice', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, challenge, workspace, problems: [{ challenge, workspace }], attempts: [] };
  await page.route('**/api/interviews', (route) => route.fulfill({ json: [] }));
  await page.route(`**/api/interviews/${id}/key?*`, (route) => route.fulfill({ json: { status: 'none' } }));
  await page.route(`**/api/interviews/${id}`, (route) => route.fulfill({ json: item }));
  await page.evaluate(() => localStorage.removeItem('leo-interview-split'));
  await page.goto(`/#/interviews/${id}`);
  const handle = page.locator('.interview-split');
  if (test.info().project.name.includes('phone')) {
    await expect(page.locator('#interview-answer')).toBeVisible();
    await expect(handle).toBeHidden();
    return;
  }
  await expect(handle).toBeVisible();
  const share = () => page.evaluate(() => { const solve = document.querySelector('.interview-solve').getBoundingClientRect(); return Math.round(document.querySelector('.interview-question').getBoundingClientRect().width / solve.width * 100); });
  expect(Math.abs(await share() - 45)).toBeLessThanOrEqual(1);
  const box = await handle.boundingBox();
  const solve = await page.locator('.interview-solve').boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + 40);
  await page.mouse.down();
  await page.mouse.move(solve.x + solve.width * 0.6, box.y + 40, { steps: 6 });
  await page.mouse.up();
  expect(Math.abs(await share() - 60)).toBeLessThanOrEqual(2);
  const moved = await handle.boundingBox();
  expect(moved.x).toBeGreaterThan(box.x + 50);
  await page.mouse.move(moved.x + moved.width / 2, box.y + 40);
  await page.mouse.down();
  await page.mouse.move(solve.x + solve.width * 0.95, box.y + 40, { steps: 6 });
  await page.mouse.up();
  expect(await share()).toBeLessThanOrEqual(70);
  await page.reload();
  await expect(handle).toBeVisible();
  expect(Math.abs(await share() - 70)).toBeLessThanOrEqual(1);
  await handle.focus();
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('Shift+ArrowLeft');
  expect(Math.abs(await share() - 58)).toBeLessThanOrEqual(1);
  await expect(handle).toHaveAttribute('aria-valuenow', '58');
  await page.keyboard.press('Home');
  expect(Math.abs(await share() - 45)).toBeLessThanOrEqual(1);
  await page.keyboard.press('ArrowRight');
  await handle.dblclick();
  expect(Math.abs(await share() - 45)).toBeLessThanOrEqual(1);
  await expect(page).toHaveURL(new RegExp(`#/interviews/${id}$`));
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
});
