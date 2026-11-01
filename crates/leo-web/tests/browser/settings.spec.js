const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

test('Settings sections keep drafts and selections across redraws in both themes', async ({ browser }) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const fixture = `const app=document.querySelector('#app'), $=selector=>document.querySelector(selector);
    let seq=0,state={view:'folder'};
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const ICON=new Proxy({}, {get:()=>''}), chrome=()=>{}, skeleton=()=>'', svg=()=>'', felix={felix:()=>''}, rel=()=>'', toast=()=>{}, fail=e=>{window.errors.push(e.message)};
    window.errors=[]; window.leoChat={usageRows:()=>[]};
    const page={tasks:[],backup:{remote:null,auto_push:'quit',options:[{id:'quit',label:'On quit'}]}};
    async function api(url) {
      if(url==='/api/settings') return structuredClone(page);
      if(url==='/api/calendar') return {calendars:[],events:[]};
      if(url==='/api/speech') return {ready:false,models:[],extra_files:[],disk_bytes:0};
      if(url==='/api/dictation') return {settings:{custom_words:[],shortcut:'Option+Space'},recent:[]};
      if(url==='/api/health') return {sections:[{title:'This app',checks:[{name:'Speech model',state:'missing',fix:'Choose a model.',section:'models'}]}]};
      if(url==='/api/app') return {preferences:{onboarding:[true,true,true],automatic_updates:true,install_on_quit:true},version:'1.2.3',updates:{state:'ready',version:'1.2.4'},paths:{}};
      if(url==='/api/storage') return {total:0,total_label:'0 B',areas:[]};
      if(url==='/api/keep') return {trash_choices:[],chat_choices:[]};
      throw new Error(url);
    }
    ${read('theme.js')}
    ${read('app/settings-navigation.js')}
    ${read('app/settings.js')}
    ${read('app/storage.js')}
    ${read('app/dictation.js')}
    ${read('app/app-settings.js')}
    ${read('app/onboarding.js')}
    const route=()=>showSettings(location.hash.split('/')[2] || 'general').catch(fail);
    window.addEventListener('hashchange',route); route();
    window.audit={redraw:()=>drawSettings(state.settings)};`;
  const server=http.createServer((req,res)=>{
    if(req.url==='/fixture.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(fixture);}
    if(req.url==='/style.css') {res.setHeader('Content-Type','text/css; charset=utf-8');return res.end(css);}
    res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body class="on-settings"><div id="crumbs"></div><div id="app-update-banner" hidden></div><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  try {
    for(const theme of ['light','dark']) {
      const page=await browser.newPage({colorScheme:theme,viewport:{width:900,height:850}});
      await page.goto('http://127.0.0.1:'+server.address().port+'/#/settings/backup');
      await expect(page.locator('#backup-repository')).toBeVisible();
      await expect(page.locator('.settings-nav a')).toHaveCount(9);
      for(const name of ['Leo','Your notes','This app']) await expect(page.getByText(name,{exact:true})).toBeVisible();
      await page.locator('#backup-repository').fill('Personal notes 文字');
      await page.locator('#backup-repository').evaluate(el=>{el.focus();el.setSelectionRange(4,8);});
      await page.evaluate(()=>window.audit.redraw());
      await expect(page.locator('#backup-repository')).toBeFocused();
      expect(await page.locator('#backup-repository').evaluate(el=>[el.value,el.selectionStart,el.selectionEnd])).toEqual(['Personal notes 文字',4,8]);
      await page.getByRole('link',{name:'Dictation',exact:true}).click();
      const words=page.locator('[data-dictation="custom_words"]');
      await words.fill('Marie Curie\n文字');
      await words.evaluate(el=>{el.focus();el.setSelectionRange(2,7);});
      await page.evaluate(()=>window.audit.redraw());
      await expect(words).toBeFocused();
      expect(await words.evaluate(el=>[el.selectionStart,el.selectionEnd])).toEqual([2,7]);
      await page.getByRole('link',{name:'Calendar',exact:true}).click();
      await expect(page.locator('#calendar-link')).toBeVisible();
      await page.getByRole('link',{name:'Health',exact:true}).click();
      await expect(page.getByText('Choose a model.',{exact:true})).toBeVisible();
      await page.getByRole('link',{name:'Updates & about',exact:true}).click();
      await expect(page.getByRole('heading',{name:'About Leo'})).toBeVisible();
      await page.getByRole('link',{name:'Storage',exact:true}).click();
      await expect(page.locator('.settings-nav a[aria-current="page"]')).toHaveAttribute('href','#/settings/storage');
      await expect(page.getByText('Storage and data',{exact:true})).toBeVisible();
      await page.setViewportSize({width:620,height:850});
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
      expect(await page.evaluate(()=>window.errors)).toEqual([]);
      await page.close();
    }
  } finally {server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});
