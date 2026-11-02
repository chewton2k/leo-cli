const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

async function sidebarFixture(browser, run) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const core = read('app/core.js');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const drag = read('app/drag.js').split("document.addEventListener('dragend'")[0];
  const open = read('app/actions.js').split("'open-folder':")[1].split("'open-note':")[0].trim().replace(/,$/, '');
  const fixture = `const $=selector=>document.querySelector(selector);
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    ${icons}
    const chat={button:()=>ICON.chat}, felix={NOTE_DRAG:'application/x-leo-note'};
    let seq=0,state={view:'folder',dir:''};
    const folders=[{name:'Course',notes:2},{name:'Course/Week 1',notes:1},{name:'Course/Week 1/Lab',notes:0},{name:'Personal',notes:1},...Array.from({length:50},(_,i)=>({name:'Reading '+i,notes:0}))];
    const folderLabel=dir=>dir.split('/').pop()||'Top level', folderHash=dir=>dir?'#/f/'+encodeURIComponent(dir):'#/', noteHash=id=>'#/n/'+encodeURIComponent(id);
    const closeSheet=()=>{}, go=hash=>{location.hash=hash;}, fail=error=>window.errors.push(error.message);
    const slideHeight=(el,from,to,done)=>{if(done)done();};
    const notes={
      '':[{id:'root-note',title:'Welcome',directory:''}],
      Course:[{id:'course-note',title:'Course overview',directory:'Course'},{id:'escaped-note',title:'<script> stays text',directory:'Course'}],
      'Course/Week 1':[{id:'week-note',title:'Lecture 1',directory:'Course/Week 1'}],
      Personal:Array.from({length:105},(_,i)=>({id:'personal-'+i,title:'Personal note '+i,directory:'Personal'}))
    };
    let pending=null, broken=null;
    async function api(url,options={}) {
      if(url==='/api/folders')return structuredClone(folders);
      const query=new URL(url,location.origin).searchParams, dir=query.get('dir'), offset=Number(query.get('offset')||0);
      const all=notes[dir]||[], result={items:structuredClone(all.slice(offset,offset+100)),total:all.length};
      if(pending && dir==='Course') {
        const wait=pending;pending=null;
        await new Promise((resolve,reject)=>{
          const abort=()=>reject(new Error('Timed out'));
          options.signal?.addEventListener('abort',abort,{once:true});
          wait.then(()=>{options.signal?.removeEventListener('abort',abort);resolve();});
        });
      }
      if(broken===dir)throw new Error('Unavailable');
      return result;
    }
    ${read('app/side.js')}
    ${drag}
    const folderClick=${open};
    document.addEventListener('click',event=>{
      const el=event.target.closest('[data-action]');if(!el)return;
      const action=el.dataset.action;
      if(action==='open-folder') folderClick(el);
      else if(action==='side-twist') twistFolder(el.dataset.dir,el);
      else if(action==='side-fold') foldSide();
      else if(action==='side-notes') loadSideNotes(el.dataset.dir,Number(el.dataset.offset||0),true);
      else if(action==='open-note') go(noteHash(el.dataset.id));
      else if(action==='home') go('#/');
    });
    window.errors=[];
    window.audit={draw:drawSide,side,refresh:loadSideFolders,note:(id,dir)=>{seq++;state={view:'note',dir,session:{note:{id}}};drawSide();},dragging,drop:el=>dirOf(dropTargetOf(el)),stop:endDrag,rename:title=>{notes.Course[0].title=title;},route:(dir)=>{seq++;state={view:'folder',dir};drawSide();},mac:on=>{document.documentElement.dataset.app=on?'mac':'';},broken:dir=>{broken=dir;},hold:()=>{pending=new Promise(resolve=>window.release=()=>{pending=null;resolve();});}};
    loadSideFolders().catch(fail);`;
  const server = http.createServer((req,res) => {
    if(req.url==='/fixture.js') {res.setHeader('Content-Type','text/javascript; charset=utf-8');return res.end(fixture);}
    if(req.url==='/style.css') {res.setHeader('Content-Type','text/css; charset=utf-8');return res.end(css);}
    res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body><aside id="side" class="side"></aside><header class="bar"><div class="bar-row">Leo</div></header><main><textarea id="draft">Keep my draft 文字</textarea></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const context = await browser.newContext();

  try {await run(context,'http://127.0.0.1:'+server.address().port);}
  finally {await context.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
}

test('sidebar separates icon actions from the explorer and keeps them reachable in both themes', async ({ browser }, testInfo) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror',error=>errors.push(error.message));
    for(const theme of ['light','dark']) {
      await page.setViewportSize({width:1280,height:640});
      await page.goto(url);
      await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      const rail = page.locator('.side-rail'), explorer = page.locator('.side-explorer');
      await expect(rail).toBeVisible();
      await expect(explorer).toBeVisible();
      const railBox = await rail.boundingBox(), explorerBox = await explorer.boundingBox();
      expect(railBox.width).toBeLessThanOrEqual(60);
      expect(railBox.x+railBox.width).toBeLessThanOrEqual(explorerBox.x+1);
      const search = await rail.getByRole('button',{name:'Search',exact:true}).boundingBox();
      const quick = await rail.getByRole('button',{name:'Quick Leo',exact:true}).boundingBox();
      expect(quick.y-search.y-search.height).toBeGreaterThanOrEqual(6);
      await expect(rail.locator('kbd')).toHaveCount(0);
      for(const label of ['New note','Notes','Ask Felix','Search','Quick Leo','Record','Note from a file','Calendar','Interview practice','Knowledge graph','Trash','Storage','Settings']) {
        const button = rail.getByRole('button',{name:label,exact:true});
        await expect(button).toBeInViewport();
        await expect(button).toHaveAttribute('title',/.+/);
        await expect(button.locator('.side-label')).toBeHidden();
      }
      const settings = rail.getByRole('button',{name:'Settings',exact:true});
      const before = await settings.boundingBox();
      await page.locator('.side-tree').evaluate(el=>el.scrollTop=el.scrollHeight);
      expect(await settings.boundingBox()).toEqual(before);
      await expect(page.locator('.side-top [data-dir=""]')).toBeInViewport();
      await page.locator('.side-tree').evaluate(el=>el.scrollTop=0);
      const course=page.locator('.side-tree .side-row[data-dir="Course"]');
      if(await course.getAttribute('aria-expanded')==='false') await course.click();
      const week=page.locator('.side-tree .side-row[data-dir="Course/Week 1"]');
      if(await week.getAttribute('aria-expanded')==='false') await week.click();
      await expect(page.locator('.side-note[data-id="week-note"]')).toBeVisible();
      await page.screenshot({path:testInfo.outputPath('sidebar-'+theme+'.png')});
      await page.getByRole('button',{name:'Hide folders',exact:true}).click();
      await expect(explorer).toBeHidden();
      await expect(settings).toBeInViewport();
      await page.reload();
      await expect(explorer).toBeHidden();
      await page.getByRole('button',{name:'Show folders',exact:true}).click();
      await expect(explorer).toBeVisible();
      await page.evaluate(()=>audit.mac(true));
      await page.getByRole('button',{name:'Hide folders',exact:true}).click();
      await page.setViewportSize({width:1000,height:640});
      await page.evaluate(()=>document.body.classList.add('on-note'));
      expect((await page.locator('.bar-row').boundingBox()).x).toBeGreaterThanOrEqual(84);
      await page.getByRole('button',{name:'Show folders',exact:true}).click();
      expect((await page.locator('.side-rail-main').boundingBox()).y).toBeGreaterThanOrEqual(50);
      await page.setViewportSize({width:1280,height:420});
      await expect(settings).toBeInViewport();
      const railMain=page.locator('.side-rail-main');
      await railMain.evaluate(el=>el.scrollTop=el.scrollHeight);
      const scroll=await railMain.evaluate(el=>el.scrollTop);
      expect(scroll).toBeGreaterThan(0);
      await page.evaluate(()=>audit.draw());
      expect(await railMain.evaluate(el=>el.scrollTop)).toBe(scroll);
      await page.setViewportSize({width:390,height:844});
      await expect(page.locator('#side')).toBeHidden();
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
    }
    expect(errors).toEqual([]);
  });
});

test('clicking parent folder labels toggles descendants without navigating or disturbing the draft', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page = await context.newPage({viewport:{width:1280,height:800}});
    await page.goto(url);
    const row = dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    await expect(row('Course')).toBeVisible();
    await row('Course').click();
    await expect(row('Course/Week 1')).toBeVisible();
    await expect(row('Course')).toHaveAttribute('aria-expanded','true');
    await expect(page.locator('.side-rail [data-action="new"]')).toHaveAttribute('data-dir','Course');
    await expect(page.locator('.side-rail [data-action="record"]')).toHaveAttribute('data-dir','Course');
    await expect(page.locator('.side-rail [data-action="upload"]')).toHaveAttribute('data-dir','Course');
    expect(new URL(page.url()).hash).toBe('');
    await expect(row('Course')).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(row('Course/Week 1')).toHaveCount(0);
    await expect(row('Course')).toHaveAttribute('aria-expanded','false');
    await page.keyboard.press('Space');
    await expect(row('Course/Week 1')).toBeVisible();
    await row('Course/Week 1').click();
    await expect(row('Course/Week 1/Lab')).toBeVisible();
    await page.reload();
    await expect(row('Course/Week 1/Lab')).toBeVisible();
    await page.evaluate(()=>audit.route('Course/Week 1/Lab'));
    await row('Course').click();
    await expect(row('Course/Week 1')).toHaveCount(0);
    await page.locator('[data-action="side-twist"][data-dir="Course"]').click();
    await expect(row('Course/Week 1/Lab')).toBeVisible();
    await row('Personal').click();
    await page.locator('.side-note[data-id="personal-0"]').click();
    await expect(page).toHaveURL(/#\/n\/personal-0$/);
    await expect(page.locator('#draft')).toHaveValue('Keep my draft 文字');
  });
});

test('explorer loads direct notes, escapes titles, pages large folders and recovers errors', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    const row=dir=>page.locator('.side-tree .side-row[data-dir="'+dir+'"]');
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await row('Course').click();
    await expect(page.locator('.side-note[data-id="course-note"]')).toBeVisible();
    await expect(page.locator('.side-note[data-id="escaped-note"]')).toHaveText('<script> stays text');
    await expect(page.locator('.side-note script')).toHaveCount(0);
    await row('Course/Week 1').click();
    await expect(page.locator('.side-note[data-id="week-note"]')).toBeVisible();
    await row('Personal').click();
    await expect(page.locator('.side-note[data-id^="personal-"]')).toHaveCount(100);
    await page.locator('[data-action="side-notes"][data-dir="Personal"]').filter({hasText:'Next'}).click();
    await expect(page.locator('.side-note[data-id="personal-104"]')).toBeVisible();
    await page.evaluate(()=>audit.route('Personal'));
    await expect(page.locator('.side-note[data-id="personal-104"]')).toBeVisible();
    await expect(page.locator('.side-note[data-id^="personal-"]')).toHaveCount(5);
    await page.evaluate(()=>audit.note('personal-104','Personal'));
    await expect(page.locator('.side-note[data-id="personal-104"]')).toHaveAttribute('aria-current','page');
    await page.evaluate(()=>audit.refresh());
    await expect(page.locator('.side-note[data-id="personal-104"]')).toBeVisible();
    await page.locator('[data-action="side-notes"][data-dir="Personal"]').filter({hasText:'Previous'}).click();
    await expect(page.locator('.side-note[data-id="personal-0"]')).toBeVisible();
    await page.evaluate(()=>{audit.broken('Course/Week 1/Lab');});
    await row('Course/Week 1/Lab').click();
    const group=page.locator('[data-kids="Course/Week 1/Lab"]');
    await expect(group).toContainText('Notes could not be opened');
    await page.evaluate(()=>audit.broken(null));
    await group.getByRole('button',{name:'Try again'}).click();
    await expect(group).toContainText('Empty folder');
    await page.evaluate(()=>{audit.side.notes.delete('Course');audit.hold();audit.draw();});
    await row('Course').click();
    await page.evaluate(()=>{audit.route('Personal');release();});
    await expect(row('Course/Week 1')).toHaveCount(0);
    await expect(page.locator('#draft')).toHaveValue('Keep my draft 文字');
  });
});

test('explorer notes keep drag data and refuse stale reads with bounded requests and cache', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    const note=page.locator('.side-note[data-id="course-note"]');
    await expect(note).toBeVisible();
    const transfer=await page.evaluateHandle(()=>new DataTransfer());
    await note.dispatchEvent('dragstart',{dataTransfer:transfer});
    const payload=await transfer.evaluate(data=>data.getData('application/x-leo-note'));
    expect(payload).not.toBe('');
    expect(JSON.parse(payload)).toEqual({id:'course-note',title:'Course overview',dir:'Course'});
    expect(await page.locator('.side-row[data-dir="Personal"]').evaluate(el=>audit.drop(el))).toBe('Personal');
    await page.evaluate(()=>audit.stop());
    await page.evaluate(()=>{audit.rename('Old response');audit.side.notes.delete('Course');audit.hold();audit.draw();});
    await page.evaluate(()=>{audit.rename('Fresh response');audit.route('Course');});
    await expect(note).toHaveText('Fresh response');
    await page.evaluate(()=>release());
    await expect(note).toHaveText('Fresh response');
    await note.focus();
    await page.evaluate(()=>{audit.rename('Renamed while focused');return audit.refresh();});
    await expect(note).toHaveText('Renamed while focused');
    await expect(note).toBeFocused();
    const limits=await page.evaluate(()=>{
      for(let i=0;i<100;i++) audit.side.folders.push({name:'Bounded '+i,notes:0});
      for(let i=0;i<100;i++) loadSideNotes('Bounded '+i);
      return {cache:audit.side.notes.size,active:audit.side.reading,queued:audit.side.pending.length};
    });
    expect(limits.cache).toBeLessThanOrEqual(64);
    expect(limits.active).toBeLessThanOrEqual(4);
    expect(limits.queued).toBeLessThanOrEqual(64);
  });
});

test('a stalled explorer read times out and offers a working retry', async ({ browser }) => {
  await sidebarFixture(browser,async(context,url)=>{
    const page=await context.newPage();
    await page.goto(url);
    await expect(page.locator('.side-note[data-id="root-note"]')).toBeVisible();
    await page.clock.install();
    await page.evaluate(()=>audit.hold());
    await page.locator('.side-tree .side-row[data-dir="Course"]').click();
    await page.clock.fastForward(15000);
    const group=page.locator('[data-kids="Course"]');
    await expect(group).toContainText('Notes could not be opened');
    await page.evaluate(()=>audit.hold());
    await group.getByRole('button',{name:'Try again'}).click();
    await expect(group).toContainText('Opening notes');
    await page.evaluate(()=>release());
    await expect(page.locator('.side-note[data-id="course-note"]')).toBeVisible();
  });
});
