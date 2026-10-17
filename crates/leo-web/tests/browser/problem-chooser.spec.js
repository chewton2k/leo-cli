const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

test('problem titles are searchable and deletion keeps drafts and offers Undo', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const html = fs.readFileSync(path.join(web, 'index.html'), 'utf8');
  const style = html.match(/<style>([\s\S]*?)<\/style>/)[1];
  const core = fs.readFileSync(path.join(web, 'app/core.js'), 'utf8');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const sheets = fs.readFileSync(path.join(web, 'app/sheets.js'), 'utf8');
  const sheet = sheets.slice(sheets.indexOf('function sheet('), sheets.indexOf('function menu()'));
  const toast = core.slice(core.indexOf('function toast('), core.indexOf('const LEAVE_MS'));
  const work = answer => ({ answer, reasoning: '', board: [], values: {} });
  const challenge = (n, title, topics) => ({ id: `chooser-problem-${n}`, title, topics, question: 'Explain your solution.', minutes: 20, skills: ['Approach'], origin: 'generated', sources: [], coding: { language: 'Python', starter: 'def solve():\n    pass', difficulty: 'easy', examples: [{ input: '1', output: '1' }], constraints: ['n < 100'] } });
  const problems = [challenge(1, 'Shortest Pin Feed', ['Sliding window']), challenge(2, 'Number of islands', ['Graphs']), challenge(3, 'Longest path', ['Graphs']), challenge(4, '<unsafe> 答😀 practice', [])].map((challenge, index) => ({ challenge, workspace: work(`answer ${index + 1}`) }));
  let item = { id: 'chooser-practice', revision: 1, framework: { title: 'Interview practice', track: 'coding', company: '', role: '', language: 'Python', style: 'coach', date: '', weekly_minutes: 120, skills: ['Approach'], wants: '' }, problems, challenge: problems[0].challenge, workspace: problems[0].workspace, attempts: [{ id: 'chooser-attempt', challenge: problems[1].challenge, workspace: problems[1].workspace, at: new Date().toISOString(), checks: [{ pass: true }], feedback: null }] };
  let removed = null;
  let failDelete = false;
  const changes = [];
  const fixture = `const enc = encodeURIComponent;
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const $ = selector => document.querySelector(selector), app = $('#app');
    const md = {render: text => '<p>' + esc(text) + '</p>'};
    let seq = 0, state = {}, toastTimer = 0;
    const LEAVE_MS = 150, MOTION_MS = 180;
    ${icons}
    const chat = {isOpen: () => false, setInterviewContext: context => window.problemContext = context, isBusy: () => false};
    function chrome() { document.body.classList.add('on-interviews'); }
    const go = () => {}, fold = el => { el.hidden = true; }, unfold = el => { el.hidden = false; };
    const fail = error => toast(error.message, {bad:true});
    const stillMotion = () => true;
    function keepOpen(box, html) { if (box) box.innerHTML = html; }
    async function api(url, options = {}) { const response = await fetch(url, {method:options.method || 'GET',headers:{'Content-Type':'application/json'},body:options.body ? JSON.stringify(options.body) : undefined}); const body = await response.json(); if (!response.ok) throw new Error(body.error); return body; }
    ${sheet}
    ${toast}
    document.addEventListener('click', e => { if (e.target.closest('[data-action="close"]')) closeSheet(); });
    document.addEventListener('keydown', e => { if (e.key === 'Escape' && $('.scrim')) closeSheet(); });
    window.audit = {state: () => interviewPage, open: () => showInterviews('chooser-practice'), refresh: () => refreshInterview('chooser-practice'), externalDelete: async () => {window.lastUndo = (await api('/api/interviews/chooser-practice', {method:'PUT',body:{action:'delete_problem',challenge:interviewPage.item.challenge.id,revision:interviewPage.item.revision}})).undo; await refreshInterview('chooser-practice');}, restore: async () => {await api('/api/felix/act', {method:'POST',body:lastUndo}); await refreshInterview('chooser-practice');}};`;
  const server = http.createServer(async (request, response) => {
    response.setHeader('Content-Type', 'application/json');
    const json = value => response.end(JSON.stringify(value));
    if (request.url === '/api/interviews/chooser-practice' && request.method === 'PUT') {
      let raw = '';
      for await (const bytes of request) raw += bytes;
      const body = JSON.parse(raw);
      changes.push(body);
      if (body.action === 'delete_problem' && failDelete) { failDelete = false; response.statusCode = 409; return json({ error: 'This practice changed. Reload before deleting.' }); }
      if (body.action === 'workspace') {
        const problem = item.problems.find(p => p.challenge.id === body.challenge);
        if (!problem) { response.statusCode = 409; return json({ error: 'That problem was deleted. Your draft is kept.' }); }
        problem.workspace = body.workspace;
        if (item.challenge?.id === body.challenge) item.workspace = body.workspace;
      }
      if (body.action === 'select') {
        const problem = item.problems.find(p => p.challenge.id === body.challenge);
        item.challenge = problem.challenge;
        item.workspace = problem.workspace;
      }
      if (body.action === 'delete_problem') {
        const index = item.problems.findIndex(p => p.challenge.id === body.challenge);
        removed = { problem: item.problems[index], index, selected: item.challenge.id === body.challenge };
        item.problems.splice(index, 1);
        item.attempts = item.attempts.filter(a => a.challenge.id !== body.challenge);
        if (removed.selected) {
          const next = item.problems[Math.min(index, item.problems.length - 1)];
          item.challenge = next?.challenge || null;
          item.workspace = next?.workspace || work('');
        }
        item.revision++;
        return json({ item, undo: { kind: 'restore_interview_problem', interview: item.id, token: 'undo-token', title: 'Restore problem', summary: 'Undo deletion' } });
      }
      item.revision++;
      return json(item);
    }
    if (request.url === '/api/felix/act') {
      item.problems.splice(removed.index, 0, removed.problem);
      if (removed.selected) { item.challenge = removed.problem.challenge; item.workspace = removed.problem.workspace; }
      item.revision++;
      return json({ item });
    }
    if (request.url === '/api/interviews/chooser-practice') return json(item);
    if (request.url.startsWith('/api/')) return json({ status: 'none' });
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript'); return response.end(fixture); }
    if (request.url === '/interviews-app.js') { response.setHeader('Content-Type', 'text/javascript'); return response.end(fs.readFileSync(path.join(web, 'app/interviews.js'))); }
    if (request.url === '/interview.js') { response.setHeader('Content-Type', 'text/javascript'); return response.end(fs.readFileSync(path.join(web, 'interview.js'))); }
    if (request.url === '/code-tools.js') { response.setHeader('Content-Type', 'text/javascript'); return response.end(fs.readFileSync(path.join(web, 'vendor/code-tools.min.js'))); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css'); return response.end(style); }
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="stylesheet" href="/style.css"></head><body><main id="app"></main><script src="/interview.js"></script><script src="/fixture.js"></script><script src="/interviews-app.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(() => audit.open());
    const chooser = page.locator('[data-interview="problem-menu"]');
    const search = page.getByRole('searchbox', { name: 'Search problems' });
    const rows = page.locator('#interview-problem-list [data-interview="problem"]');
    await expect(chooser).toContainText('Shortest Pin Feed');
    await expect(page.locator('#interview-problems [data-interview="problem"]')).toHaveCount(0);
    await chooser.click();
    await expect(search).toBeFocused();
    await expect(rows).toHaveCount(4);
    expect(changes).toEqual([]);
    await search.fill('graphs');
    await expect(rows).toHaveCount(2);
    await expect(search).toBeFocused();
    await page.evaluate(() => audit.refresh());
    await expect(search).toHaveValue('graphs');
    await expect(search).toBeFocused();
    await page.getByLabel('Problem status').selectOption('solved');
    await expect(rows).toHaveCount(1);
    await expect(rows.first()).toContainText('Number of islands');
    await page.getByLabel('Problem status').selectOption('all');
    await search.fill('答😀');
    await expect(rows.first()).toContainText('<unsafe> 答😀 practice');
    await expect(page.locator('#interview-problem-list unsafe')).toHaveCount(0);
    await search.fill('');
    for (const theme of ['light', 'dark']) {
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 844 });
        await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        const layout = await page.locator('.sheet').evaluate(el => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, width: innerWidth }; });
        expect(layout.left).toBeGreaterThanOrEqual(0);
        expect(layout.right).toBeLessThanOrEqual(layout.width);
        const controls = await page.locator('.interview-browser-filters').evaluate(el => [...el.querySelectorAll('input, select')].map(control => { const r = control.getBoundingClientRect(); return {left:r.left,right:r.right}; }));
        for (const control of controls) { expect(control.left).toBeGreaterThanOrEqual(layout.left); expect(control.right).toBeLessThanOrEqual(layout.right); }
        const row = await page.locator('.interview-problem-row').first().evaluate(el => { const buttons = el.querySelectorAll('button'); const a = buttons[0].getBoundingClientRect(), b = buttons[1].getBoundingClientRect(); return {left:a.right,right:b.left,top:a.top,bottom:b.bottom}; });
        expect(row.right).toBeGreaterThanOrEqual(row.left);
        const contrast = await page.locator('.interview-problem-list').evaluate(el => {
          const luminance = color => color.match(/[\d.]+/g).slice(0, 3).map(Number).map(v => { const s = v / 255; return s <= .04045 ? s / 12.92 : ((s + .055) / 1.055) ** 2.4; }).reduce((sum, v, i) => sum + v * [.2126, .7152, .0722][i], 0);
          return [...el.querySelectorAll('b, .hint, .interview-problem-status')].map(text => {
            const row = text.closest('.interview-problem-row');
            const background = getComputedStyle(row).backgroundColor;
            const a = luminance(getComputedStyle(text).color), b = luminance(background === 'rgba(0, 0, 0, 0)' ? getComputedStyle(el.closest('.sheet')).backgroundColor : background);
            return (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
          });
        });
        for (const ratio of contrast) expect(ratio).toBeGreaterThanOrEqual(3);


      }
    }
    await page.keyboard.press('Escape');
    await expect(chooser).toBeFocused();
    const area = page.locator('#interview-answer');
    await area.fill('my evolving solution');
    await area.dispatchEvent('input');
    await chooser.click();
    await page.getByRole('button', { name: 'Problem 2: Number of islands', exact: true }).click();
    await expect(area).toHaveValue('answer 2');
    expect(changes.some(c => c.action === 'workspace' && c.workspace.answer === 'my evolving solution')).toBe(true);
    expect(await page.evaluate(() => problemContext.number)).toBe(2);
    await chooser.click();
    await page.getByRole('button', { name: 'Problem 1: Shortest Pin Feed', exact: true }).click();
    await expect(area).toHaveValue('my evolving solution');
    failDelete = true;
    await chooser.click();
    await page.getByRole('button', { name: 'Delete Shortest Pin Feed', exact: true }).click();
    await page.getByRole('button', { name: 'Delete problem', exact: true }).click();
    await expect(page.locator('.toast')).toContainText('Reload before deleting');
    await expect(area).toHaveValue('my evolving solution');
    await page.getByRole('button', { name: 'Delete problem', exact: true }).click();
    await expect(chooser).toContainText('Number of islands');
    await expect(area).toHaveValue('answer 2');
    await page.locator('.toast button').click();
    await expect(chooser).toContainText('Shortest Pin Feed');
    await expect(area).toHaveValue('my evolving solution');
    expect(item.problems).toHaveLength(4);
    await area.fill('newer draft while Felix answers');
    await area.dispatchEvent('input');
    await page.evaluate(() => audit.externalDelete());
    await expect(chooser).toContainText('Number of islands');
    await page.evaluate(() => audit.restore());
    await expect(area).toHaveValue('newer draft while Felix answers');
    await chooser.click();
    await page.getByRole('button', { name: 'Delete Longest path', exact: true }).click();
    await page.getByRole('button', { name: 'Delete problem', exact: true }).click();
    await expect(chooser).toContainText('Shortest Pin Feed');
    await expect(area).toHaveValue('newer draft while Felix answers');
    await page.locator('.toast button').click();
    await expect.poll(() => item.problems.length).toBe(4);
    for (const title of ['Number of islands', 'Longest path', '<unsafe> 答😀 practice', 'Shortest Pin Feed']) {
      await chooser.click();
      await page.getByRole('button', { name: `Delete ${title}`, exact: true }).click();
      await page.getByRole('button', { name: 'Delete problem', exact: true }).click();
      await expect(page.locator('.scrim')).toHaveCount(0);
    }
    await expect(chooser).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Start practice', exact: true })).toBeVisible();
    await page.locator('.toast button').click();
    await expect(chooser).toContainText('Shortest Pin Feed');
    await expect(area).toHaveValue('newer draft while Felix answers');
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await new Promise(resolve => server.close(resolve));
  }
});
