const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

test('speech model downloads keep drafts and ignore late progress after cancel', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const source = fs.readFileSync(path.join(web, 'app/settings.js'), 'utf8');
  const helpers = source.slice(source.indexOf('function speechCards('));
  const nav = fs.readFileSync(path.join(web, 'app/settings-navigation.js'), 'utf8');
  const segments = nav.slice(nav.indexOf('function segmented('), nav.indexOf('function pickSegment('));
  const option = source.slice(source.indexOf('function settingsOption('), source.indexOf('const effortName'));
  const css = fs.readFileSync(path.join(web, 'index.html'), 'utf8').match(/<style>([\s\S]*?)<\/style>/)[1];
  const initial = () => ({ selected: { id: 'local', language: 'en' }, ready: true, disk_bytes: 5,
    models: [{ id: 'local', name: 'Local <b>speech</b>', description: 'Fast, accurate <live> English transcription', streaming: true, speed: 13.955, accuracy: 96.01, accuracy_measured: true, benchmark_language: 'en', languages: ['en'], size: 731357568, bytes: 5, downloaded: true, verified: true, in_use: true, live: true, progress: null },
      { id: 'next', name: 'Another model', recommended: true, languages: ['en'], size: 100, bytes: 0, downloaded: false, verified: false, in_use: false, live: false, progress: null }] });
  let view = initial();
  let held = null;
  let oldView = null;
  const fixture = `const app = document.querySelector('#app'), $ = selector => document.querySelector(selector);
const plural = (n, word, many = word + 's') => n + ' ' + (n === 1 ? word : many);
    let seq = 1, state = {view:'settings',speech:${JSON.stringify(view)}};
    const ICON={chevron:''}, fail=error => {window.errors.push(error.message)}, toast=()=>{}, sheet=()=>{};
    const esc = text => String(text).replace(/[&<>"']/g, ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    async function api(url, options={}) { const response=await fetch(url,{method:options.method || 'GET',headers:{'Content-Type':'application/json'},body:options.body ? JSON.stringify(options.body) : undefined}); const data=await response.json(); if(!response.ok) throw new Error(data.error); return data; }
    window.errors=[];
    ${fs.readFileSync(path.join(web, 'app/refresh.js'), 'utf8')}
    const knobStretch=()=>{};
    ${segments}
    ${option}
    ${helpers}
    $('#speech-library').innerHTML=speechCards(state.speech);
    app.addEventListener('click',event=>{const filter=event.target.closest('[data-action="speech-filter"]');if(filter) return filterSpeech(filter);const button=event.target.closest('[data-action="speech-model"]');if(button) changeSpeechModel(button).catch(fail);});
    window.audit={progress:()=>state.speech.models[1].progress};`;
  const server = http.createServer(async (request, response) => {
    if (request.url === '/api/speech') {
      response.setHeader('Content-Type', 'application/json');
      if (request.method === 'POST') {
        let raw = '';
        for await (const bytes of request) raw += bytes;
        const body = JSON.parse(raw);
        if (body.action === 'download') view.models[1].progress = { state: 'downloading', bytes: 20, total: 100, error: null };
        if (body.action === 'cancel') view.models[1].progress = { state: 'cancelled', bytes: 20, total: 100, error: null };
        return response.end(JSON.stringify(view));
      }
      held = response;
      oldView = JSON.stringify(view);
      return;
    }
    if (request.url === '/fixture.js') { response.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return response.end(fixture); }
    if (request.url === '/style.css') { response.setHeader('Content-Type', 'text/css; charset=utf-8'); return response.end(css); }
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.end('<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body><main id="app"><div class="settings"><label>Draft<input id="draft" class="set-input"></label><div id="speech-library"></div></div></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    for (const theme of ['light', 'dark']) {
      view = initial();
      held = null;
      oldView = null;
      const page = await browser.newPage({ colorScheme: theme });
      await page.goto(`http://127.0.0.1:${server.address().port}`);
      await expect(page.getByText('Local <b>speech</b>', { exact: true }).first()).toBeVisible();
      const clipped = () => page.locator('.model-filters .segment').evaluateAll(items => items.filter(item => item.querySelector('span').scrollWidth > item.querySelector('span').clientWidth).length);
      expect(await clipped()).toBe(0);
      await page.setViewportSize({ width: 480, height: 700 });
      expect(await clipped()).toBe(0);
      await page.locator('.model-filters [data-filter="english"]').click();
      await expect(page.locator('.model-filters [data-filter="english"]')).toHaveAttribute('aria-pressed', 'true');
      await expect(page.locator('.model-filters [data-filter="recommended"]')).toHaveAttribute('aria-pressed', 'false');
      expect(await page.locator('.model-filters .segmented').evaluate(box => box.style.getPropertyValue('--at'))).toBe('1');
      await page.locator('.model-filters [data-filter="recommended"]').click();
      await page.setViewportSize({ width: 900, height: 420 });
      await page.evaluate(() => { for (let i = 0; i < 20; i++) state.speech.models.push({ id: 'extra' + i, name: 'Extra ' + i, languages: ['en'], size: 100, bytes: 0, downloaded: false, verified: false, in_use: false, live: false, progress: null }); });
      await page.locator('.model-filters [data-filter="all"]').click();
      await page.locator('.model-filters').evaluate(box => box.scrollIntoView({ block: 'start' }));
      const anchored = () => page.locator('.model-filters').evaluate(box => Math.round(box.getBoundingClientRect().top));
      const where = await anchored();
      expect(await page.evaluate(() => scrollY)).toBeGreaterThan(0);
      await page.locator('.model-filters [data-filter="english"]').click();
      expect(Math.abs(await anchored() - where)).toBeLessThan(2);
      await page.locator('.model-filters [data-filter="recommended"]').click();
      expect(Math.abs(await anchored() - where)).toBeLessThan(2);
      await page.evaluate(() => { state.speech.models.splice(2); });
      await page.locator('.model-filters [data-filter="recommended"]').click();
      await page.setViewportSize({ width: 1280, height: 720 });
      expect(await page.locator('#speech-library b').count()).toBe(0);
      const local = page.locator('[data-model-row="local"]');
      for (const detail of ['Fast, accurate <live> English transcription', 'English only', 'Streaming', '697 MB', '96.0%', '14.0× real time']) await expect(local).toContainText(detail);
      await expect(local.getByRole('meter', {name:'Accuracy 96.0%'})).toHaveAttribute('value', '96.01');
      if (process.env.LEO_MODEL_PREVIEW) await page.screenshot({path:process.env.LEO_MODEL_PREVIEW+'-'+theme+'.png',fullPage:true});
      await page.getByRole('button', { name: 'Download', exact: true }).click();
      await expect(page.getByText('20%', { exact: true })).toBeVisible();
      await page.locator('#draft').fill('Keep typing 文字');
      await page.locator('#draft').press('End');
      await expect.poll(() => Boolean(held)).toBe(true);
      await page.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(page.getByRole('button', { name: 'Download', exact: true })).toBeVisible();
      await page.locator('#draft').focus();
      const response = page.waitForResponse(r => r.url().endsWith('/api/speech') && r.request().method() === 'GET');
      held.end(oldView);
      await (await response).finished();
      await expect.poll(() => page.evaluate(() => window.audit.progress().state)).toBe('cancelled');
      await expect(page.locator('#draft')).toBeFocused();
      await expect(page.locator('#draft')).toHaveValue('Keep typing 文字');
      expect(await page.locator('#draft').evaluate(el => el.selectionStart)).toBe('Keep typing 文字'.length);
      expect(await page.evaluate(() => window.errors)).toEqual([]);
      await page.close();
    }
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});
