const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

test('isolated runner stops forged Python completion, serializes runs and releases failed startup listeners', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const runner = fs.readFileSync(path.join(web, 'runner.js'), 'utf8');
  const interviews = fs.readFileSync(path.join(web, 'app/interviews.js'), 'utf8');
  const start = interviews.includes('const MOST_RUNNER_JOBS') ? interviews.indexOf('const MOST_RUNNER_JOBS') : interviews.indexOf('const runner =');
  const parent = interviews.slice(start, interviews.indexOf('async function interviewGoToolchain', start));
  const assets = fs.readFileSync(path.resolve(web, '../routes/assets.rs'), 'utf8');
  const policy = assets.match(/const RUNNER_POLICY: &str = "([\s\S]*?)";/)[1].replace(/\\\n\s*/g, '');
  let heartbeats = 0, failStartup = false;
  const server = http.createServer((request, response) => {
    response.setHeader('Content-Type', 'text/javascript');
    if (request.url === '/runner.js') {
      response.setHeader('Content-Security-Policy', policy);
      return response.end(runner);
    }
    if (request.url === '/runner.html') {
      response.setHeader('Content-Type', 'text/html');
      response.setHeader('Content-Security-Policy', policy);
      return response.end(failStartup ? '<title>Unavailable fixture</title>' : '<script src="/runner.js"></script>');
    }
    if (request.url === '/pyodide/pyodide.js') return response.end(`self.loadPyodide = async () => ({globals: {set(name, value) { self.job = JSON.parse(value); }}, runPython() {
      if (self.job.code === 'forge') {
        self.postMessage({});
        const until = performance.now() + 500;
        while (performance.now() < until) {}
        const request = new XMLHttpRequest();
        request.open('GET', self.location.origin === 'null' ? self.auditBase + 'heartbeat' : '/heartbeat', false);
        request.send();
      }
      return JSON.stringify({results: [{id: self.job.tests[0].id, got: self.job.code}], logs: []});
    }});
    self.auditBase = ${JSON.stringify(`http://127.0.0.1:`)} + ${server.address().port} + '/';`);
    if (request.url === '/heartbeat') { heartbeats++; return response.end('heard'); }
    if (request.url === '/fixture.js') return response.end(`const felix = { newId: () => crypto.randomUUID() };
      const listeners = new Set();
      const add = window.addEventListener.bind(window), remove = window.removeEventListener.bind(window);
      window.addEventListener = (name, fn, options) => { if (name === 'message') listeners.add(fn); add(name, fn, options); };
      window.removeEventListener = (name, fn, options) => { if (name === 'message') listeners.delete(fn); remove(name, fn, options); };
      const timeout = window.setTimeout.bind(window);
      window.setTimeout = (fn, ms) => timeout(fn, ms === 10000 ? 200 : ms);
      ${parent}
      window.audit = { run: runInSandbox, runner, listeners };`);
    response.setHeader('Content-Type', 'text/html');
    response.end('<!doctype html><title>Isolated runner fixture</title><script src="/fixture.js"></script>');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    const crashes = [];
    page.on('pageerror', (error) => crashes.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(() => audit.run({ language: 'python', entry: 'twice', code: 'forge', tests: [{ id: 'forge', args: [] }] }));
    await page.waitForTimeout(700);
    expect(heartbeats).toBe(0);
    const results = await page.evaluate(async () => {
      const jobs = ['first', 'second'].map((code) => audit.run({ language: 'python', entry: 'twice', code, tests: [{ id: code, args: [] }] }));
      return Promise.race([Promise.all(jobs), new Promise((resolve) => setTimeout(() => resolve('timed out'), 3000))]);
    });
    expect(results.map((result) => result.results[0].got)).toEqual(['first', 'second']);
    const malformed = await page.evaluate(() => audit.run({ language: 'javascript', entry: 'twice', code: "self.postMessage({results: 'broken', logs: {}}); function twice() { return 1; }", tests: [{ id: 'bad', args: [] }] }));
    expect(malformed.error).toContain('Check its return value');
    expect(malformed.results).toEqual([]);
    const recovered = await page.evaluate(() => audit.run({ language: 'javascript', entry: 'twice', code: 'function twice(value) { return value * 2; }', tests: [{ id: 'good', args: [3] }] }));
    expect(recovered.results[0].got).toBe(6);
    failStartup = true;
    await page.evaluate(() => { audit.runner.frame.remove(); audit.runner.frame = null; audit.runner.ready = null; });
    for (let turn = 0; turn < 3; turn++) {
      const result = await page.evaluate(() => audit.run({ language: 'javascript', entry: 'twice', code: '', tests: [] }));
      expect(result.error).toContain('did not start');
      expect(await page.evaluate(() => audit.listeners.size)).toBe(0);
      expect(await page.locator('iframe').count()).toBe(0);
    }
    expect(crashes).toEqual([]);
  } finally {
    await context.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
