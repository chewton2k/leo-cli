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

test.describe('map of ideas', () => {
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
    await chat.locator('[data-mode="quiz"]').click();
    await chat.locator('.starter').first().click();
    await expect(chat.locator('.msg.leo').first()).toContainText('Where is the smallest element');
    await expect(chat.locator('.msg.leo .cite').first()).toHaveText('Heaps');
    expect(asked[0].mode).toBe('quiz');
    expect(asked[0].note).toBe(note.id);
    await chat.locator('#chat-input').fill('At the root');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.verdict.correct')).toBeVisible();
    await expect(chat.locator('.msg.leo').last()).not.toContainText('[[correct]]');
    await expect(chat.locator('#chat-face .felix')).toHaveClass(/dance/);
    expect(asked[1].messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user']);
    await chat.locator('.msg.leo .cite').first().click();
    await expect(page).toHaveURL(new RegExp(`#/n/${note.id}$`));
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
    await page.locator('#menu').click();
    await page.locator('[data-action="settings"]').click();
    await expect(page).toHaveURL(/#\/settings$/);
    const writing = page.locator('[data-task-card="writing"]');
    await writing.locator('select[data-set="provider"]').selectOption('gemini');
    await expect(page.locator('.toast')).toContainText('Writing now uses Gemini');
    await expect(writing.locator('select[data-set="provider"]')).toHaveValue('gemini');
    await writing.locator('select[data-set="model"]').selectOption('gemini-3.1-flash-lite');
    await expect(page.locator('.toast')).toContainText('Model set to gemini-3.1-flash-lite');
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
  });
});
