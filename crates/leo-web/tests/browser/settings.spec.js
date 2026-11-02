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

test('setup groups permissions with live checkmarks, independent grants and a separate model step', async ({ browser }, testInfo) => {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const sheets = read('app/sheets.js').split('const QUICK_PLACES')[0];
  const fixture = `const app=document.querySelector('#app'), $=selector=>document.querySelector(selector);
    let seq=0,state={view:'folder'};
    const LEAVE_MS=180, ICON={check:'<svg aria-hidden="true" viewBox="0 0 24 24"><path d="m5 12 4 4 10-10" fill="none" stroke="currentColor"/></svg>'};
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    window.errors=[]; const fail=e=>window.errors.push(e.message);
    let native, pending=null, rejectStep=null;
    const calls=[];
    async function api(url,options={}) {
      if(url==='/api/speech') return {models:[]};
      if(url!=='/api/app') throw new Error(url);
      const change=options.body;
      if(change) {
        calls.push(change);
        if(change.action==='finish_step') {
          if(pending) await pending;
          if(change.step===rejectStep) throw new Error('Setup could not be saved. Try again.');
          native.preferences.onboarding[change.step]=true;
        }
      }
      return structuredClone(native);
    }
    ${sheets}
    ${read('app/app-settings.js')}
    ${read('app/onboarding.js')}
    document.addEventListener('click',event=>{
      const el=event.target.closest('[data-action]'); if(!el) return;
      if(el.dataset.action==='app-command') appChange({action:el.dataset.appAction}).catch(fail);
      if(el.dataset.action==='onboarding-next') finishOnboarding(el).catch(fail);
    });
    window.audit={
      start(platform='macos',flags=[false,false,false]) {
        closeSheet(true); calls.length=0; rejectStep=null;
        native={platform,preferences:{onboarding:flags},permissions:{microphone:'not_requested',accessibility:false},updates:{}};
        desktopApp=structuredClone(native); maybeOnboarding(true);
      },
      grant:async (permission,value)=>{native.permissions[permission]=value; await pollAppStatus(appPollGeneration);},
      calls:()=>calls, flags:()=>native.preferences.onboarding,
      reject:step=>{rejectStep=step;},
      hold:()=>{pending=new Promise(resolve=>window.release=()=>{pending=null;resolve();});},
      replace:()=>{closeSheet(true);sheet('<h3>Another sheet</h3>');}
    }; audit.start();`;
  const server = http.createServer((req,res) => {
    if(req.url==='/fixture.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(fixture);}
    if(req.url==='/style.css') {res.setHeader('Content-Type','text/css; charset=utf-8');return res.end(css);}
    res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.goto('http://127.0.0.1:'+server.address().port);
    for(const [theme,order] of [['light',['microphone','accessibility']],['dark',['accessibility','microphone']]]) {
      await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;audit.start();},theme);
      await expect(page.getByRole('button',{name:'Allow microphone',exact:true})).toBeVisible();
      await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toBeVisible();
      const next = page.getByRole('button',{name:'Continue',exact:true});
      await expect(next).toBeDisabled();
      for(const permission of order) {
        const row = page.locator('[data-onboarding-permission="'+permission+'"]');
        const button = row.getByRole('button');
        const descriptionWidth = (await row.locator('.grow').boundingBox()).width;
        expect((await button.boundingBox()).height).toBeLessThanOrEqual(40);
        await button.click();
        await expect(row.locator('[role="status"]')).not.toContainText('Allowed');
        await page.evaluate(permission=>audit.grant(permission,permission==='microphone'?'allowed':true),permission);
        await expect(row.locator('[role="status"]')).toContainText('Allowed');
        await expect(row.locator('[role="status"] svg')).toHaveCount(1);
        await expect(button).toBeHidden();
        expect(Math.abs((await row.locator('.grow').boundingBox()).width-descriptionWidth)).toBeLessThanOrEqual(1);
        if(permission===order[0]) {
          await expect(next).toBeDisabled();
          await page.screenshot({path:testInfo.outputPath('permissions-'+theme+'.png')});
        }
      }
      await expect(next).toBeEnabled();
      await next.click();
      await expect(page.getByRole('heading',{name:'Choose a speech model'})).toBeVisible();
      expect(await page.evaluate(()=>audit.flags())).toEqual([true,true,false]);
      await page.getByRole('button',{name:'Skip for now'}).click();
      await expect(page.locator('.scrim')).toHaveCount(0);
      expect(await page.evaluate(()=>audit.flags())).toEqual([true,true,true]);
    }
    await page.evaluate(()=>audit.start('macos',[true,false,false]));
    await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toBeVisible();
    await page.getByRole('button',{name:'Skip for now'}).click();
    await expect(page.getByRole('heading',{name:'Choose a speech model'})).toBeVisible();
    await page.evaluate(()=>{audit.start();audit.reject(1);});
    await page.getByRole('button',{name:'Skip for now'}).click();
    await expect.poll(()=>page.evaluate(()=>window.errors.length)).toBe(1);
    await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toBeVisible();
    await page.evaluate(()=>audit.reject(null));
    await page.getByRole('button',{name:'Skip for now'}).click();
    await expect(page.getByRole('heading',{name:'Choose a speech model'})).toBeVisible();
    await page.evaluate(()=>{audit.start();audit.hold();});
    await page.getByRole('button',{name:'Skip for now'}).click();
    await page.evaluate(()=>{audit.replace();release();});
    await expect(page.getByRole('heading',{name:'Another sheet'})).toBeVisible();
    await page.setViewportSize({width:390,height:844});
    await page.evaluate(()=>audit.start('windows'));
    await expect(page.getByRole('button',{name:'Allow Accessibility',exact:true})).toHaveCount(0);
    await page.evaluate(()=>audit.grant('microphone','allowed'));
    await expect(page.getByRole('button',{name:'Continue',exact:true})).toBeEnabled();
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
    expect(await page.evaluate(()=>window.errors)).toEqual(['Setup could not be saved. Try again.']);
  } finally {await context.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});
