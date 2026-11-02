const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

// The Boards page with its real CSS, icons, sheets and boards.js over an in-memory API
// that behaves like routes/boards.rs (order by position, same-name columns across boards).
async function boardsFixture(browser, run, { width = 1280, height = 800 } = {}) {
  const web = path.resolve(__dirname, '../../src/web');
  const read = name => fs.readFileSync(path.join(web, name), 'utf8');
  const css = read('index.html').match(/<style>([\s\S]*?)<\/style>/)[1];
  const core = read('app/core.js');
  const icons = core.slice(core.indexOf('const svg ='), core.indexOf('const recorder ='));
  const sheets = read('app/sheets.js');
  const sheetParts = sheets.slice(sheets.indexOf('function sheet('), sheets.indexOf('const QUICK_PLACES'));
  const fixture = `const $=(selector,root=document)=>root.querySelector(selector);
    const esc=text=>String(text).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));
    const enc=encodeURIComponent, app=$('#app'), LEAVE_MS=150;
    ${icons}
    class Locked extends Error {}
    let seq=0,state={view:null};
    const chrome=()=>{}, skeleton=n=>'<div class="skeleton"></div>'.repeat(n);
    const empty=(icon,title,text)=>'<div class="empty">'+icon+'<h3>'+esc(title)+'</h3><p>'+esc(text)+'</p></div>';
    const stillMotion=()=>false, drawStorage=()=>{};
    window.toasts=[];
    function toast(message,{action,run,bad}={}){window.toasts.push({message,bad:Boolean(bad)});document.querySelector('.toast')?.remove();const el=document.createElement('div');el.className='toast';el.innerHTML='<span>'+esc(message)+'</span>'+(action?'<button>'+esc(action)+'</button>':'');if(action)el.querySelector('button').onclick=()=>{el.remove();run();};document.body.appendChild(el);}
    const fail=error=>window.errors.push(error.message);
    function go(hash,{replace=false}={}){if(location.hash===hash)return route();if(replace){history.replaceState(history.state,'',hash);route();}else location.hash=hash;}
    ${sheetParts}
    const db={boards:[
      {id:'bwork00001',name:'Work',columns:[{id:'ktodo00001',name:'To do'},{id:'kdoing0001',name:'In progress'},{id:'kdone00001',name:'Done'}],created_at:'2026-01-01T00:00:00Z'},
      {id:'bhome00001',name:'Home',columns:[{id:'khtodo0001',name:'To do'},{id:'khdoing001',name:'In progress'}],created_at:'2026-01-02T00:00:00Z'}],
      cards:[
        {id:'cessay0001',board:'bwork00001',column:'ktodo00001',title:'Write essay',subheading:'History · due soon',description:'Outline first',priority:'high'},
        {id:'cread00001',board:'bwork00001',column:'ktodo00001',title:'Read chapter 3',subheading:'',description:'',priority:''},
        {id:'cemail0001',board:'bwork00001',column:'kdoing0001',title:'Email tutor <b>now</b>',subheading:'',description:'',priority:'low'}]};
    let made=0;
    const ranked=board=>db.cards.filter(c=>c.board===board).map((c,i)=>({...c,rank:i+1}));
    const place=(card,after)=>{db.cards=db.cards.filter(c=>c.id!==card.id);const same=db.cards.filter(c=>c.board===card.board&&c.column===card.column);let at;if(after===undefined)at=same.length?db.cards.indexOf(same[same.length-1])+1:db.cards.length;else if(after===null)at=same.length?db.cards.indexOf(same[0]):db.cards.length;else at=db.cards.findIndex(c=>c.id===after)+1;db.cards.splice(at,0,card);};
    window.calls=[];
    window.pending=0;
    async function api(url,options={}){window.pending++;try{return await fakeApi(url,options);}finally{window.pending--;}}
    async function fakeApi(url,{method='GET',body}={}){
      window.calls.push(method+' '+url);
      await new Promise(r=>setTimeout(r,5));
      let m;
      if(url==='/api/boards'&&method==='GET')return {boards:db.boards.map(b=>({...b,cards:db.cards.filter(c=>c.board===b.id).length}))};
      if(url==='/api/boards'&&method==='POST'){const b={id:'bnew'+(++made)+'00000',name:body.name||'New board',columns:[{id:'kn'+made+'a000000',name:'To do'},{id:'kn'+made+'b000000',name:'In progress'},{id:'kn'+made+'c000000',name:'Done'}]};db.boards.push(b);return structuredClone(b);}
      if(url==='/api/boards/restore'){db.boards.push(body.board);db.cards.push(...body.cards);return {board:body.board,cards:body.cards};}
      if((m=url.match(/^\\/api\\/boards\\/([^/]+)$/))){
        const b=db.boards.find(x=>x.id===m[1]);if(!b){const e=new Error('That board is not there any more.');e.status=404;throw e;}
        if(method==='GET')return {board:structuredClone(b),cards:ranked(b.id)};
        if(method==='PATCH'){if(body.name!==undefined)b.name=body.name;if(body.columns){const before=b.columns;b.columns=body.columns.map((c,i)=>({id:c.id||'kadd'+(++made)+'0000000',name:c.name}));const kept=new Set(b.columns.map(c=>c.id));for(const c of db.cards)if(c.board===b.id&&!kept.has(c.column))c.column=body.move_to||b.columns[0].id;}return structuredClone(b);}
        if(method==='DELETE'){const cards=ranked(b.id);db.boards=db.boards.filter(x=>x!==b);db.cards=db.cards.filter(c=>c.board!==b.id);return {board:b,cards};}
      }
      if((m=url.match(/^\\/api\\/boards\\/([^/]+)\\/cards$/))){const card={id:body.id||'cmade'+(++made)+'00000',board:m[1],column:body.column||db.boards.find(b=>b.id===m[1]).columns[0].id,title:body.title,subheading:body.subheading||'',description:body.description||'',priority:body.priority||''};place(card,undefined);return {...card,rank:0};}
      if((m=url.match(/^\\/api\\/cards\\/([^/]+)\\/move$/))){const card=db.cards.find(c=>c.id===m[1]);const from=db.boards.find(b=>b.id===card.board);if(body.board&&body.board!==card.board){const to=db.boards.find(b=>b.id===body.board);const name=from.columns.find(c=>c.id===card.column)?.name;card.board=to.id;card.column=body.column||(to.columns.find(c=>c.name===name)||to.columns[0]).id;}else if(body.column)card.column=body.column;place(card,'after' in body?body.after:undefined);return {...card,rank:0};}
      if((m=url.match(/^\\/api\\/cards\\/([^/]+)$/))){const card=db.cards.find(c=>c.id===m[1]);if(method==='DELETE'){db.cards=db.cards.filter(c=>c!==card);return {...card,rank:0};}Object.assign(card,body);return {...card,rank:0};}
      throw new Error('No fake for '+method+' '+url);
    }
    ${read('app/boards.js')}
    const actions={'kanban-new-board':()=>kanbanNewBoard(),'kanban-board-menu':()=>kanbanBoardMenu(),'kanban-column-menu':el=>kanbanColumnMenu(el.dataset.col),'kanban-add-card':el=>kanbanComposer(el.dataset.col),'kanban-add-column':()=>kanbanAddColumn(),'kanban-open-card':el=>kanbanCardSheet(el.dataset.card),close:()=>closeSheet()};
    document.addEventListener('click',e=>{const el=e.target.closest('[data-action]');if(!el||!actions[el.dataset.action])return;e.preventDefault();Promise.resolve(actions[el.dataset.action](el)).catch(fail);});
    document.addEventListener('keydown',e=>{if(e.key==='Escape'&&$('.scrim'))closeSheet();});
    async function route(){closeSheet(true);const m=location.hash.match(/^#\\/boards\\/?(.*)$/);if(!m){leaveBoards();state={view:'other'};app.innerHTML='<p id="elsewhere">Elsewhere</p>';return;}await showBoards(decodeURIComponent(m[1]));}
    window.addEventListener('hashchange',()=>route().catch(fail));
    window.errors=[];window.db=db;
    if(!location.hash)history.replaceState(null,'','#/boards');
    route().catch(fail);`;
  const server = http.createServer((req, res) => {
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); return res.end(fixture); }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); return res.end(css); }
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body class="on-boards"><header class="bar" id="bar"><div class="bar-row">Leo</div></header><main id="app"></main><script src="/fixture.js"></script></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const context = await browser.newContext({ viewport: { width, height } });
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto('http://127.0.0.1:' + server.address().port + '/#/boards');
    await expect(page.locator('.kanban-col')).toHaveCount(3);
    await run(page);
    expect(errors).toEqual([]);
    expect(await page.evaluate(() => window.errors)).toEqual([]);
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
}

async function settled(page) {
  await expect.poll(() => page.evaluate(() => window.pending)).toBe(0);
}
const titles = (page, column) => page.locator(`.kanban-cards[data-col="${column}"] .kanban-card-title`).allTextContents();
const saved = (page, board) => page.evaluate(id => window.db.cards.filter(c => c.board === id).map(c => `${c.column}:${c.title}`), board);

async function drag(page, from, to, { below = false, left = false } = {}) {
  const a = await from.boundingBox();
  const b = await to.boundingBox();
  await page.mouse.move(a.x + a.width / 2, a.y + Math.min(14, a.height / 2));
  await page.mouse.down();
  await page.mouse.move(a.x + a.width / 2 + 10, a.y + 20, { steps: 3 });
  const y = below ? b.y + b.height - 4 : b.y + Math.min(8, b.height / 4);
  const x = left ? b.x + 8 : b.x + b.width / 2;
  await page.mouse.move(x, y, { steps: 8 });
  await page.mouse.move(x, y);
  await page.mouse.up();
  await settled(page);
}

test('a board shows columns of cards, and cards drag between columns and onto another board', async ({ browser }, testInfo) => {
  await boardsFixture(browser, async page => {
    for (const theme of ['light', 'dark']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      await page.screenshot({ path: testInfo.outputPath(`boards-${theme}.png`) });
    }
    await page.evaluate(() => { document.documentElement.dataset.theme = 'light'; });
    await expect(page.locator('.kanban-tab.on')).toContainText('Work');
    await expect(page.locator('.kanban-col-name').first()).toHaveValue('To do');
    expect(await titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3']);
    await expect(page.locator('.kanban-card-sub').first()).toHaveText('History · due soon');
    await expect(page.locator('[data-card="cemail0001"] .kanban-card-title')).toHaveText('Email tutor <b>now</b>');
    await expect(page.locator('[data-card="cessay0001"] .kanban-priority.high')).toBeVisible();
    await expect(page.locator('[data-count-col="ktodo00001"]')).toHaveText('2');

    // Into another column, below the card already there.
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('[data-card="cemail0001"]'), { below: true });
    await expect.poll(() => saved(page, 'bwork00001')).toEqual(['ktodo00001:Write essay', 'kdoing0001:Email tutor <b>now</b>', 'kdoing0001:Read chapter 3']);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);
    await expect(page.locator('[data-count-col="kdoing0001"]')).toHaveText('2');
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
    await expect(page.locator('.scrim')).toHaveCount(0);

    // Into an empty column.
    await drag(page, page.locator('[data-card="cessay0001"]'), page.locator('.kanban-cards[data-col="kdone00001"]'));
    await expect.poll(() => titles(page, 'kdone00001')).toEqual(['Write essay']);

    // Escape puts a lifted card back where it was.
    await expect(page.locator('[data-card="cread00001"]')).toBeVisible();
    const card = await page.locator('[data-card="cread00001"]').boundingBox();
    await page.mouse.move(card.x + 20, card.y + 10);
    await page.mouse.down();
    await page.mouse.move(card.x - 300, card.y + 10, { steps: 6 });
    await expect(page.locator('.kanban-ghost')).toHaveCount(1);
    await page.keyboard.press('Escape');
    await page.mouse.up();
    await expect(page.locator('.kanban-ghost')).toHaveCount(0);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);

    // Onto the Home tab: the card keeps its In progress column there.
    await drag(page, page.locator('[data-card="cread00001"]'), page.locator('.kanban-tab[data-kanban-drop="bhome00001"]'));
    await expect(page.locator('.toast')).toContainText('Moved “Read chapter 3” to Home');
    await expect.poll(() => saved(page, 'bhome00001')).toEqual(['khdoing001:Read chapter 3']);
    await expect(page.locator('[data-card="cread00001"]')).toHaveCount(0);
    await expect(page.locator('[data-count-board="bhome00001"]')).toHaveText('1');
    await page.locator('.toast button').click();
    await expect(page.locator('[data-card="cread00001"]')).toHaveCount(1);
    expect(await titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Read chapter 3']);
  });
});

test('cards are added, opened, edited, moved by keyboard and deleted with undo', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    await page.locator('[data-action="kanban-add-card"][data-col="ktodo00001"]').click();
    const composer = page.locator('.kanban-composer textarea');
    await expect(composer).toBeFocused();
    await composer.fill('Plan the trip');
    await composer.press('Enter');
    await composer.fill('Pack bags');
    await composer.press('Enter');
    await expect.poll(() => titles(page, 'ktodo00001')).toEqual(['Write essay', 'Read chapter 3', 'Plan the trip', 'Pack bags']);
    await expect(composer).toBeFocused();
    await composer.press('Escape');
    await expect(page.locator('.kanban-composer')).toHaveCount(0);
    await expect(page.locator('[data-count-col="ktodo00001"]')).toHaveText('4');

    await page.locator('[data-card="cessay0001"]').click();
    const sheet = page.locator('.scrim .kanban-sheet');
    await expect(sheet.locator('#kanban-card-title')).toHaveValue('Write essay');
    await expect(sheet.locator('#kanban-card-description')).toHaveValue('Outline first');
    await sheet.locator('#kanban-card-sub').fill('History · draft 2');
    await sheet.locator('#kanban-card-description').fill('Outline first\nThen write');
    await sheet.locator('[data-priority="medium"]').click();
    await expect(sheet.locator('[data-priority="medium"]')).toHaveAttribute('aria-pressed', 'true');
    await sheet.locator('#kanban-card-column').selectOption({ label: 'In progress' });
    await expect.poll(() => page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001'))).toMatchObject({ column: 'kdoing0001', subheading: 'History · draft 2', description: 'Outline first\nThen write', priority: 'medium' });
    await page.keyboard.press('Escape');
    await expect(page.locator('.scrim')).toHaveCount(0);
    await expect(page.locator('[data-card="cessay0001"] .kanban-card-sub')).toHaveText('History · draft 2');
    await expect(page.locator('[data-card="cessay0001"] .kanban-priority.medium')).toBeVisible();

    // Alt + arrows move the focused card.
    await page.locator('[data-card="cessay0001"]').focus();
    await page.keyboard.press('Alt+ArrowRight');
    await expect.poll(() => titles(page, 'kdone00001')).toEqual(['Write essay']);
    await expect(page.locator('[data-card="cessay0001"]')).toBeFocused();
    await page.keyboard.press('Alt+ArrowLeft');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Email tutor <b>now</b>', 'Write essay']);
    await page.keyboard.press('Alt+ArrowUp');
    await expect.poll(() => titles(page, 'kdoing0001')).toEqual(['Write essay', 'Email tutor <b>now</b>']);
    await page.keyboard.press('Enter');
    await expect(page.locator('.scrim .kanban-sheet')).toBeVisible();

    await page.locator('.scrim [data-kanban="delete"]').click();
    await expect(page.locator('[data-card="cessay0001"]')).toHaveCount(0);
    await expect(page.locator('.toast')).toContainText('Deleted “Write essay”');
    await page.locator('.toast button').click();
    await expect(page.locator('[data-card="cessay0001"]')).toHaveCount(1);
    expect(await page.evaluate(() => window.db.cards.find(c => c.id === 'cessay0001').description)).toBe('Outline first\nThen write');
  });
});

test('columns are renamed, added, reordered and deleted, and boards are made and deleted', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const name = page.locator('[data-kanban-name="kdoing0001"]');
    await name.fill('Doing');
    await name.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns[1].name)).toBe('Doing');

    // Drag the Done column by its header to the front.
    await drag(page, page.locator('.kanban-col[data-col="kdone00001"] .kanban-col-name'), page.locator('.kanban-col[data-col="ktodo00001"] .kanban-col-head'), { left: true });
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns.map(c => c.name))).toEqual(['Done', 'To do', 'Doing']);

    await page.locator('[data-action="kanban-add-column"]').click();
    await expect(page.locator('.kanban-col')).toHaveCount(4);
    const fresh = page.locator('.kanban-col').last().locator('.kanban-col-name');
    await expect(fresh).toBeFocused();
    await fresh.fill('Waiting');
    await fresh.press('Enter');
    await expect.poll(() => page.evaluate(() => window.db.boards[0].columns.map(c => c.name))).toEqual(['Done', 'To do', 'Doing', 'Waiting']);

    // Deleting To do moves its two cards to the column picked.
    await page.locator('[data-action="kanban-column-menu"][data-col="ktodo00001"]').click();
    await page.locator('#kanban-move-to').selectOption({ label: 'Waiting' });
    await page.locator('.scrim [data-kanban="delete"]').click();
    await expect(page.locator('.kanban-col')).toHaveCount(3);
    await expect.poll(() => page.evaluate(() => window.db.cards.filter(c => c.board === 'bwork00001' && c.column !== 'kdoing0001').map(c => c.title))).toEqual(['Write essay', 'Read chapter 3']);

    await page.locator('[data-action="kanban-new-board"]').click();
    await page.locator('#kanban-board-name').fill('Job search');
    await page.locator('#kanban-board-name').press('Enter');
    await expect(page.locator('.kanban-tab.on')).toContainText('Job search');
    await expect(page.locator('.kanban-col-name')).toHaveCount(3);
    await expect(page.locator('.kanban-card')).toHaveCount(0);

    await page.locator('[data-action="kanban-board-menu"]').click();
    await page.locator('.scrim [data-kanban="delete"]').click();
    await expect(page.locator('.scrim [data-kanban="delete"]')).toContainText('Delete the board');
    await page.locator('.scrim [data-kanban="delete"]').click();
    await expect(page.locator('.kanban-tab.on')).toContainText('Work');
    await expect(page.locator('.kanban-tab')).toHaveCount(3);
    await page.locator('.toast button').click();
    await expect(page.locator('.kanban-tab.on')).toContainText('Job search');
  });
});

test('on a phone the columns scroll sideways inside the page, which never scrolls sideways itself', async ({ browser }) => {
  await boardsFixture(browser, async page => {
    const width = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(width, 'the page itself never scrolls sideways').toBeLessThanOrEqual(390);
    const row = page.locator('.kanban-cols');
    expect(await row.evaluate(el => el.scrollWidth > el.clientWidth)).toBe(true);
    const col = await page.locator('.kanban-col').first().boundingBox();
    expect(col.width).toBeLessThanOrEqual(330);
  }, { width: 390, height: 780 });
});
