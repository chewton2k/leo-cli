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
  const refused = page.waitForResponse((r) => r.request().method() === 'PATCH' && r.status() === 500);
  await page.locator('#back').click();
  await refused;
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

  test('rebuilding from scratch asks first, then starts a fresh build', async ({ page }) => {
    await seed(page);
    const builds = [];
    await page.route('**/api/graph/build*', (route) => {
      builds.push(new URL(route.request().url()).search);
      return route.fulfill({ status: 202, json: { state: 'building', done: 0, total: 3, message: null, notes: 3, read: 0, stale: 3, requests: 3, rebuild_requests: 3, built_at: null } });
    });
    await page.goto('/#/map');
    await page.locator('[data-action="map-rebuild"]').click();
    await expect(page.locator('.sheet')).toContainText('Rebuild the map from scratch?');
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
    await expect(chat.locator('.msg')).toHaveCount(0);
    await expect(chat.locator('.chat-hello')).toBeVisible();
    const kept = await (await page.request.get('/api/chats')).json();
    expect(kept.some((c) => c.mode === 'study' && c.count === 4)).toBe(true);
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

    await chat.locator(`.chat-ref-x[data-id="${heaps.id}"]`).click();
    await page.reload();
    await page.locator('#chat-toggle').click();
    await expect(page.locator('#chat .chat-ref')).toHaveCount(1);
    await page.locator('#chat-input').fill('and now?');
    await page.locator('#chat-input').press('Enter');
    await expect.poll(() => asked.length).toBe(2);
    expect(asked[1].refs).toEqual([dijkstra.id]);
    await page.locator('#chat .chat-head [data-chat="new"]').click();
    await expect(page.locator('#chat .chat-ref')).toHaveCount(0);
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

  test('a file from this device is read for Felix, goes with each message, and can be removed', async ({ page }) => {
    const asked = [];
    await page.route('**/api/chat', async (route) => {
      asked.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, headers: { 'content-type': 'application/x-ndjson' }, body: '{"sources":[]}\n{"t":"Your file says heaps."}\n{"done":true}\n' });
    });
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    await chat.locator('[data-chat="attach"]').click();
    await expect(chat.locator('#chat-attach-menu')).toBeVisible();
    const chooser = page.waitForEvent('filechooser');
    await chat.locator('[data-chat="attach-file"]').click();
    await (await chooser).setFiles({ name: 'week3.txt', mimeType: 'text/plain', buffer: Buffer.from('Heaps keep the minimum at the root.') });
    await expect(chat.locator('#chat-attach-menu')).toBeHidden();
    const chip = chat.locator('.chat-ref.doc', { hasText: 'week3.txt' });
    await expect(chip).toBeVisible();
    await expect(chip).not.toHaveClass(/reading/);
    await chat.locator('#chat-input').fill('what does my file say?');
    await chat.locator('#chat-input').press('Enter');
    await expect(chat.locator('.msg.leo').last()).toContainText('Your file says heaps.');
    expect(asked[0].files.length).toBe(1);
    const files = await (await page.request.get(`/api/chats/${asked[0].chat}/files`)).json();
    expect(files.map((f) => f.name)).toEqual(['week3.txt']);
    expect(files[0].chars).toBe(35);
    await expect(chat.locator('.msg.user .cite.doc')).toHaveText('week3.txt');
    await chip.locator('.chat-ref-x').click();
    await expect(chip).toHaveCount(0);
    await expect.poll(async () => (await (await page.request.get(`/api/chats/${asked[0].chat}/files`)).json()).length).toBe(0);
    await chat.locator('[data-chat="attach"]').click();
    const again = page.waitForEvent('filechooser');
    await chat.locator('[data-chat="attach-file"]').click();
    await (await again).setFiles({ name: 'song.mp3', mimeType: 'audio/mpeg', buffer: Buffer.from('ID3') });
    await expect(page.locator('.toast.bad')).toContainText('Felix could not read song.mp3');
    await expect(chat.locator('.chat-ref.doc')).toHaveCount(0);
  });

  test('the chat panel follows the size of the window', async ({ page }) => {
    test.skip(test.info().project.name !== 'desktop', 'one browser is enough to resize');
    await page.goto('/');
    await page.locator('#chat-toggle').click();
    const chat = page.locator('#chat');
    for (const [width, expected] of [[390, 390], [768, 476], [1024, 560], [1440, 691], [2200, 820]]) {
      await page.setViewportSize({ width, height: 800 });
      await expect.poll(async () => Math.round((await chat.boundingBox()).width)).toBe(expected);
      const fits = await chat.evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
      expect(fits, `nothing spills sideways at ${width}px`).toBe(true);
      await expect(page.locator('#chat-input')).toBeInViewport();
      await expect(page.locator('#chat-send')).toBeInViewport();
      if (width >= 1200) {
        const panel = await chat.boundingBox();
        const button = await page.locator('.fab[data-action="new"]').boundingBox();
        const list = await page.locator('main').boundingBox();
        expect(button.x + button.width, `the New note button stays beside the chat at ${width}px`).toBeLessThanOrEqual(panel.x);
        expect(list.x + list.width, `the page moves over at ${width}px`).toBeLessThanOrEqual(panel.x + 1);
      }
    }
    await page.locator('#chat [data-chat="close"]').click();
    await expect.poll(async () => (await page.locator('main').boundingBox()).width).toBeGreaterThan(700);
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

test.describe('folders', () => {
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
    await expect(page.locator('.fab[data-action="new"]')).toBeVisible();
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
    await page.locator('[data-action="storage"]').click();
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

test.describe('export and this browser', () => {
  test('exports a zip with the parts chosen and clears drafts kept in this browser', async ({ page }) => {
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

    const note = await (await page.request.post('/api/notes', { data: { title: `Draft to clear ${test.info().project.name}`, body: 'saved text' } })).json();
    await page.route('**/api/notes/*', (route) => (route.request().method() === 'PATCH' ? route.fulfill({ status: 500 }) : route.continue()));
    await page.goto(`/#/n/${note.id}`);
    await page.locator('.blk').first().click();
    await page.locator('.line-edit').fill('unsaved text');
    await expect(page.locator('#save-state')).toContainText('draft kept');
    await page.goto('/#/settings/storage');
    const browser = page.locator('.store-browser');
    await expect(browser).toContainText('1 unsaved draft');
    await browser.locator('[data-action="drafts-clear"]').click();
    await page.locator('[data-action="drafts-clear-now"]').click();
    await expect(page.locator('.toast')).toContainText('Cleared 1 draft');
    await expect(browser).toContainText('No unsaved drafts are kept here.');
    expect(await page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith('leo-draft-v1:')))).toEqual([]);
    expect((await (await page.request.get(`/api/notes/${note.id}`)).json()).body).toBe('saved text');
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
    await page.locator('.fab[data-action="upload"]').click();
    await page.locator('#upload-input').setInputFiles({ name: 'sorting.txt', mimeType: 'text/plain', buffer: Buffer.from('Merge sort splits the list in half.') });
    await expect(page.locator('.upload-file')).toContainText('sorting.txt');
    await page.locator('#upload-title').fill('Sorting');
    await page.locator('#upload-go').click();
    await expect(page).toHaveURL(new RegExp(`#/n/${made.id}$`));
    expect(sent.title).toBe('Sorting');
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
    await page.locator('.fab[data-action="upload"]').click();
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
    await page.locator('#menu').click();
    await page.locator('.sheet [data-action="upload"]').click();
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
    const view = (over = {}) => ({ id: 'rec-1', source: seen.source, state: seen.paused ? 'paused' : 'recording', secs: 3, step: '', steps: null, transcript: 'Today we cover breadth first search.', warnings: [], points: seen.points.map((t) => [3, t]), levels: seen.levels, note: null, error: null, ...over });
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
    page.route('**/api/record/rec-1/audio', (route) => {
      seen.audioBytes += route.request().postDataBuffer().length;
      seen.posts += 1;
      return route.fulfill({ status: 204 });
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
    await page.locator('.fab[data-action="record"]').click();
    await expect(page).toHaveURL(/#\/record$/);
    await expect(page.locator('.rec-source')).toHaveCount(4);
    await page.locator('#rec-title').fill('Graphs');
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-transcript')).toContainText('breadth first search');
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
    expect(seen.started).toEqual({ source: 'browser', directory: '', title: 'Graphs' });

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
    await page.locator('.rec-source', { hasText: 'A tab’s or screen’s sound' }).click();
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
    await page.locator('.rec-source', { hasText: 'Computer’s sound' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('#rec-wave')).toBeVisible();
    await expect(page.locator('#rec-hear')).toHaveText('Hearing sound');
    expect(seen.started.source).toBe('screen');
    expect(seen.audioBytes).toBe(0);
    seen.levels = [...Array(12).fill(0.1), ...Array(30).fill(0)];
    await expect(page.locator('#rec-hear')).toHaveText('No sound for a while: is something playing on the computer?', { timeout: 5000 });
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
    await page.locator('.rec-source', { hasText: 'A tab’s or screen’s sound' }).click();
    await page.locator('[data-action="rec-start"]').click();
    await expect(page.locator('.toast.bad')).toContainText('Share tab audio');
    expect(started).toBe(false);
  });

  test('a page opened over the internet records its own device, not the computer', async ({ page, context }) => {
    await context.grantPermissions(['microphone']);
    stubRecorder(page, { noteId: 'x', local: false });
    await page.goto('/#/record');
    await expect(page.locator('[data-action="rec-start"]')).toBeVisible();
    await expect(page.locator('.rec-source')).toHaveText(['This device’s microphone', 'A tab’s or screen’s sound']);
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
