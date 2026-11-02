// Boards (routes/boards.rs): Jira-style task boards. Cards have no dates, so they are
// kept apart from calendar tasks. A board has columns (To do, In progress, Done…) that
// people rename, add, delete and drag into order; cards are dragged between columns,
// and onto another board's tab to move them there.
const KANBAN_KEY = 'leo-board';
const KANBAN_MOST_COLUMNS = 12;
const KANBAN_HOLD_MS = 300;
const KANBAN_EDGE = 56;
const KANBAN_SAVE_MS = 600;
const KANBAN_PRIORITIES = [['', 'None'], ['low', 'Low'], ['medium', 'Medium'], ['high', 'High']];
const KANBAN_ICON = {
  pencil: svg('<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/>'),
  lines: svg('<path d="M5 7h14M5 12h14M5 17h9"/>'),
  high: svg('<path d="M6 13l6-6 6 6M6 19l6-6 6 6"/>'),
  medium: svg('<path d="M6 9.5h12M6 14.5h12"/>'),
  low: svg('<path d="M6 9l6 6 6-6"/>'),
};
const kanban = { boards: [], board: null, cards: [], loaded: false, drag: null, suppressClick: false };

function kanbanRemembered() {
  try { return localStorage.getItem(KANBAN_KEY) || ''; } catch (e) { return ''; }
}

function kanbanRemember(id) {
  try { localStorage.setItem(KANBAN_KEY, id); } catch (e) {}
}

async function showBoards(arg) {
  const mine = ++seq;
  state = { view: 'boards', dir: '' };
  chrome({ showBack: true });
  document.title = 'Boards · leo';
  if (!kanban.loaded || !app.querySelector('.kanban-page')) app.innerHTML = `<div class="kanban-page">${skeleton(3)}</div>`;
  const { boards } = await api('/api/boards');
  if (mine !== seq) return;
  kanban.boards = boards || [];
  kanban.loaded = true;
  if (!kanban.boards.length) {
    kanban.board = null;
    kanban.cards = [];
    drawKanban();
    return;
  }
  const wanted = arg || kanbanRemembered();
  const id = kanban.boards.some((b) => b.id === wanted) ? wanted : kanban.boards[0].id;
  if (arg && arg !== id) history.replaceState(history.state, '', `#/boards/${enc(id)}`);
  const data = await api(`/api/boards/${enc(id)}`);
  if (mine !== seq) return;
  kanban.board = data.board;
  kanban.cards = data.cards || [];
  kanbanRemember(id);
  document.title = `${data.board.name} · leo`;
  drawKanban();
}

// Fetch the open board again after a change, so ranks renumbered by leo are known here.
async function kanbanRefresh() {
  const board = kanban.board;
  if (!board || state.view !== 'boards') return;
  const mine = seq;
  const data = await api(`/api/boards/${enc(board.id)}`);
  if (mine !== seq || kanban.board?.id !== board.id || kanban.drag) return;
  kanban.board = data.board;
  kanban.cards = data.cards || [];
  drawKanban();
}

const kanbanCardsIn = (column) => kanban.cards.filter((c) => c.column === column);
const kanbanCard = (id) => kanban.cards.find((c) => c.id === id);
const kanbanColumn = (id) => kanban.board?.columns.find((c) => c.id === id);

// A sheet holds the card it opened; the board may have been fetched again since.
function kanbanUpdate(card, saved) {
  Object.assign(card, saved);
  const live = kanbanCard(card.id);
  if (live && live !== card) Object.assign(live, saved);
  kanbanRedrawCard(card);
}

function kanbanPriority(priority) {
  if (!priority) return '';
  const label = KANBAN_PRIORITIES.find(([id]) => id === priority)?.[1] || priority;
  return `<span class="kanban-priority ${esc(priority)}" title="${esc(label)} priority" aria-label="${esc(label)} priority">${KANBAN_ICON[priority] || ''}</span>`;
}

function kanbanCardHtml(card) {
  const foot = card.priority || card.description
    ? `<div class="kanban-card-foot">${kanbanPriority(card.priority)}${card.description ? `<span class="kanban-has-text" title="Has a description" aria-label="Has a description">${KANBAN_ICON.lines}</span>` : ''}</div>`
    : '';
  return `<article class="kanban-card" tabindex="0" data-card="${esc(card.id)}" data-action="kanban-open-card" aria-roledescription="card" aria-label="${esc(card.title)}"><div class="kanban-card-title">${esc(card.title)}</div>${card.subheading ? `<div class="kanban-card-sub">${esc(card.subheading)}</div>` : ''}${foot}</article>`;
}

function kanbanColumnHtml(column) {
  const cards = kanbanCardsIn(column.id);
  return `<section class="kanban-col" data-col="${esc(column.id)}" aria-label="${esc(column.name)}">
    <header class="kanban-col-head" title="Drag to move this column">
      <input class="kanban-col-name" data-kanban-name="${esc(column.id)}" value="${esc(column.name)}" maxlength="80" aria-label="Column name" spellcheck="false" autocomplete="off">
      <span class="kanban-count" data-count-col="${esc(column.id)}">${cards.length}</span>
      <button class="icon-btn kanban-col-more" data-action="kanban-column-menu" data-col="${esc(column.id)}" aria-label="Options for ${esc(column.name)}" title="Column options">${ICON.more}</button>
    </header>
    <div class="kanban-cards" data-col="${esc(column.id)}">${cards.map(kanbanCardHtml).join('')}</div>
    <button class="kanban-add-card" data-action="kanban-add-card" data-col="${esc(column.id)}">${ICON.plus}<span>Add card</span></button>
  </section>`;
}

function kanbanTabsHtml() {
  const here = kanban.board?.id;
  return kanban.boards.map((b) => {
    const count = b.id === here ? kanban.cards.length : b.cards || 0;
    return `<a class="kanban-tab${b.id === here ? ' on' : ''}" href="#/boards/${enc(b.id)}" data-kanban-drop="${esc(b.id)}"${b.id === here ? ' aria-current="page"' : ''}><span class="kanban-tab-name">${esc(b.name)}</span><span class="kanban-count" data-count-board="${esc(b.id)}">${count}</span></a>`;
  }).join('') + `<button class="kanban-tab kanban-tab-new" data-action="kanban-new-board">${ICON.plus}<span>New board</span></button>`;
}

function drawKanban() {
  if (state.view !== 'boards') return;
  const board = kanban.board;
  if (!board) {
    app.innerHTML = `<div class="kanban-page kanban-none">${empty(ICON.board, 'Keep track of your work', 'A board has columns like To do, In progress and Done. Add a card for each task and drag it along as the work moves.')}<button class="btn primary" data-action="kanban-new-board">${ICON.plus}<span>New board</span></button></div>`;
    return;
  }
  const row = app.querySelector('.kanban-cols');
  const scrolled = row ? row.scrollLeft : 0;
  const active = app.contains(document.activeElement) ? document.activeElement : null;
  const focus = active?.dataset.card ? `[data-card="${CSS.escape(active.dataset.card)}"]` : active?.dataset.kanbanName ? `[data-kanban-name="${CSS.escape(active.dataset.kanbanName)}"]` : null;
  app.innerHTML = `<div class="kanban-page">
    <header class="kanban-head">
      <nav class="kanban-tabs" aria-label="Boards">${kanbanTabsHtml()}</nav>
      <div class="kanban-title-row">
        <input class="kanban-title" data-kanban-name="board" value="${esc(board.name)}" maxlength="80" aria-label="Board name" spellcheck="false" autocomplete="off">
        <button class="icon-btn" data-action="kanban-board-menu" aria-label="Board options" title="Board options">${ICON.more}</button>
      </div>
    </header>
    <div class="kanban-cols">${board.columns.map(kanbanColumnHtml).join('')}${board.columns.length < KANBAN_MOST_COLUMNS ? `<button class="kanban-add-col" data-action="kanban-add-column">${ICON.plus}<span>Add column</span></button>` : ''}</div>
    <p class="hint kanban-hint">Drag cards between columns or onto another board. Drag a column by its name bar to reorder. With a card selected, ${ON_MAC ? '⌥' : 'Alt'} and the arrow keys move it.</p>
  </div>`;
  const next = app.querySelector('.kanban-cols');
  if (next) next.scrollLeft = scrolled;
  if (focus) app.querySelector(focus)?.focus({ preventScroll: true });
}

function kanbanCounts() {
  for (const el of app.querySelectorAll('[data-count-col]')) el.textContent = String(kanbanCardsIn(el.dataset.countCol).length);
  const here = app.querySelector(`[data-count-board="${CSS.escape(kanban.board?.id || '')}"]`);
  if (here) here.textContent = String(kanban.cards.length);
}

function kanbanRedrawCard(card) {
  const el = app.querySelector(`.kanban-card[data-card="${CSS.escape(card.id)}"]`);
  if (!el) return;
  const focused = document.activeElement === el;
  el.outerHTML = kanbanCardHtml(card);
  if (focused) app.querySelector(`.kanban-card[data-card="${CSS.escape(card.id)}"]`)?.focus({ preventScroll: true });
}

function kanbanSyncTab() {
  const tab = kanban.boards.find((b) => b.id === kanban.board?.id);
  if (tab) { tab.name = kanban.board.name; tab.columns = kanban.board.columns; tab.cards = kanban.cards.length; }
  const nav = app.querySelector('.kanban-tabs');
  if (nav) nav.innerHTML = kanbanTabsHtml();
}

async function kanbanNewBoard() {
  const box = sheet(`<h3>New board</h3>
    <input class="set-input" id="kanban-board-name" maxlength="80" placeholder="Like Coursework or Job search" aria-label="Board name" autocomplete="off">
    <p class="hint">It starts with To do, In progress and Done. You can rename, add and reorder columns.</p>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="kanban-board-make">Make board</button></div>`);
  const name = $('#kanban-board-name', box);
  const make = $('#kanban-board-make', box);
  const save = async () => {
    if (make.disabled) return;
    make.disabled = true;
    try {
      const board = await api('/api/boards', { method: 'POST', body: { name: name.value.trim() } });
      closeSheet();
      kanbanRemember(board.id);
      go(`#/boards/${enc(board.id)}`);
    } catch (e) {
      make.disabled = false;
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  };
  make.addEventListener('click', () => save().catch(fail));
  name.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); save().catch(fail); }
  });
  name.focus();
}

async function kanbanSaveBoard(change) {
  const board = kanban.board;
  const saved = await api(`/api/boards/${enc(board.id)}`, { method: 'PATCH', body: change });
  if (kanban.board?.id !== saved.id) return saved;
  kanban.board = saved;
  kanbanSyncTab();
  return saved;
}

async function kanbanRename(input) {
  const key = input.dataset.kanbanName;
  const board = kanban.board;
  if (!board) return;
  const text = input.value.replace(/\s+/g, ' ').trim();
  const column = key === 'board' ? null : kanbanColumn(key);
  const was = column ? column.name : board.name;
  if (!text || text === was) { input.value = was; return; }
  try {
    if (column) {
      await kanbanSaveBoard({ columns: board.columns.map((c) => ({ id: c.id, name: c.id === key ? text : c.name })) });
      const box = input.closest('.kanban-col');
      if (box) box.setAttribute('aria-label', text);
    } else {
      const saved = await kanbanSaveBoard({ name: text });
      document.title = `${saved.name} · leo`;
    }
  } catch (e) {
    input.value = was;
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
}

document.addEventListener('change', (e) => {
  const input = e.target.closest?.('[data-kanban-name]');
  if (input && state.view === 'boards') kanbanRename(input).catch(fail);
});

async function kanbanAddColumn() {
  const board = kanban.board;
  if (!board || board.columns.length >= KANBAN_MOST_COLUMNS) return;
  const before = new Set(board.columns.map((c) => c.id));
  await kanbanSaveBoard({ columns: [...board.columns, { name: 'New column' }] });
  drawKanban();
  const made = kanban.board.columns.find((c) => !before.has(c.id));
  const input = made && app.querySelector(`[data-kanban-name="${CSS.escape(made.id)}"]`);
  if (input) {
    input.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    input.focus();
    input.select();
  }
}

async function kanbanSaveColumns(columns, moveTo = null) {
  try {
    await kanbanSaveBoard({ columns: columns.map((c) => ({ id: c.id, name: c.name })), ...(moveTo ? { move_to: moveTo } : {}) });
    await kanbanRefresh();
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
    drawKanban();
  }
}

function kanbanColumnMenu(id) {
  const board = kanban.board;
  const column = kanbanColumn(id);
  if (!column) return;
  const at = board.columns.indexOf(column);
  const count = kanbanCardsIn(id).length;
  const others = board.columns.filter((c) => c.id !== id);
  const box = sheet(`<h3>${esc(column.name)}</h3>
    <p class="hint">${count === 1 ? '1 card' : `${count} cards`}</p>
    <div class="panel kanban-menu">
      <button type="button" class="list-row" data-kanban="rename">${KANBAN_ICON.pencil}<span class="grow">Rename</span></button>
      <button type="button" class="list-row" data-kanban="left"${at === 0 ? ' disabled' : ''}>${ICON.back}<span class="grow">Move left</span></button>
      <button type="button" class="list-row" data-kanban="right"${at === board.columns.length - 1 ? ' disabled' : ''}>${ICON.chevron}<span class="grow">Move right</span></button>
    </div>
    ${others.length ? `<div class="kanban-delete-col">
      ${count ? `<div class="set-row"><span class="set-label">Move its cards to</span><label class="select-control"><select id="kanban-move-to" aria-label="Move its cards to">${others.map((c) => `<option value="${esc(c.id)}">${esc(c.name)}</option>`).join('')}</select>${ICON.chevron}</label></div>` : ''}
      <div class="buttons"><button type="button" class="btn plain danger" data-kanban="delete">${ICON.trash}<span>Delete column</span></button></div>
    </div>` : '<p class="hint">A board keeps at least one column.</p>'}`);
  box.addEventListener('click', (e) => {
    const button = e.target.closest('[data-kanban]');
    if (!button || button.disabled) return;
    const what = button.dataset.kanban;
    if (what === 'rename') {
      closeSheet(true);
      const input = app.querySelector(`[data-kanban-name="${CSS.escape(id)}"]`);
      input?.focus();
      input?.select();
      return;
    }
    const columns = [...board.columns];
    if (what === 'left' || what === 'right') {
      const to = at + (what === 'left' ? -1 : 1);
      [columns[at], columns[to]] = [columns[to], columns[at]];
      closeSheet();
      kanbanSaveColumns(columns).catch(fail);
    } else if (what === 'delete') {
      const target = $('#kanban-move-to', box)?.value || null;
      closeSheet();
      kanbanSaveColumns(columns.filter((c) => c.id !== id), target).then(() => {
        toast(count ? `Deleted “${column.name}” and moved its cards to “${kanbanColumn(target)?.name || 'another column'}”` : `Deleted “${column.name}”`);
      }).catch(fail);
    }
  });
}

function kanbanBoardMenu() {
  const board = kanban.board;
  if (!board) return;
  const count = kanban.cards.length;
  const box = sheet(`<h3>${esc(board.name)}</h3>
    <p class="hint">${count === 1 ? '1 card' : `${count} cards`} · ${board.columns.length === 1 ? '1 column' : `${board.columns.length} columns`}</p>
    <div class="panel kanban-menu">
      <button type="button" class="list-row" data-kanban="rename">${KANBAN_ICON.pencil}<span class="grow">Rename board</span></button>
      <button type="button" class="list-row" data-kanban="column"${board.columns.length >= KANBAN_MOST_COLUMNS ? ' disabled' : ''}>${ICON.plus}<span class="grow">Add column</span></button>
    </div>
    <div class="buttons"><button type="button" class="btn plain danger" data-kanban="delete">${ICON.trash}<span>Delete board</span></button></div>`);
  box.addEventListener('click', (e) => {
    const button = e.target.closest('[data-kanban]');
    if (!button || button.disabled) return;
    const what = button.dataset.kanban;
    if (what === 'rename') {
      closeSheet(true);
      const input = app.querySelector('[data-kanban-name="board"]');
      input?.focus();
      input?.select();
    } else if (what === 'column') {
      closeSheet(true);
      kanbanAddColumn().catch(fail);
    } else if (what === 'delete') {
      if (button.dataset.sure !== '1') {
        button.dataset.sure = '1';
        button.querySelector('span').textContent = count ? `Delete the board and its ${count === 1 ? 'card' : `${count} cards`}` : 'Delete the board';
        return;
      }
      closeSheet();
      kanbanDeleteBoard(board).catch(fail);
    }
  });
}

async function kanbanDeleteBoard(board) {
  const gone = await api(`/api/boards/${enc(board.id)}`, { method: 'DELETE' });
  toast(`Deleted “${board.name}”`, {
    action: 'Undo',
    run: () => api('/api/boards/restore', { method: 'POST', body: gone }).then((back) => go(`#/boards/${enc(back.board.id)}`)).catch(fail),
  });
  const left = kanban.boards.filter((b) => b.id !== board.id);
  go(left.length ? `#/boards/${enc(left[0].id)}` : '#/boards', { replace: true });
}

function kanbanComposer(columnId) {
  const column = app.querySelector(`.kanban-col[data-col="${CSS.escape(columnId)}"]`);
  const button = column?.querySelector('.kanban-add-card');
  if (!button) return;
  const form = document.createElement('div');
  form.className = 'kanban-composer';
  form.innerHTML = `<textarea class="set-input" rows="2" maxlength="300" placeholder="What needs doing?" aria-label="New card title"></textarea><div class="kanban-composer-row"><button type="button" class="btn sm primary" data-kanban="add">Add card</button><button type="button" class="btn sm plain" data-kanban="cancel">Cancel</button></div>`;
  button.replaceWith(form);
  const text = form.querySelector('textarea');
  // Cards save one after another, so typing the next one while the last saves loses nothing.
  let queue = Promise.resolve();
  let waiting = 0;
  const close = () => {
    if (!form.isConnected) return;
    form.replaceWith(button);
    button.focus({ preventScroll: true });
  };
  const post = async (title) => {
    try {
      const card = await api(`/api/boards/${enc(kanban.board.id)}/cards`, { method: 'POST', body: { title, column: columnId } });
      if (kanban.board?.id !== card.board || !column.isConnected) return;
      kanban.cards.push(card);
      column.querySelector('.kanban-cards').insertAdjacentHTML('beforeend', kanbanCardHtml(card));
      kanbanCounts();
    } catch (e) {
      if (e instanceof Locked) throw e;
      if (form.isConnected && !text.value.trim()) text.value = title;
      toast(e.message, { bad: true });
    }
  };
  const add = () => {
    const title = text.value.replace(/\s+/g, ' ').trim();
    if (!title) { text.focus(); return Promise.resolve(); }
    text.value = '';
    text.focus();
    waiting++;
    queue = queue.then(() => post(title)).finally(() => { waiting--; });
    return queue;
  };
  form.addEventListener('click', (e) => {
    const what = e.target.closest('[data-kanban]')?.dataset.kanban;
    if (what === 'add') add().catch(fail);
    else if (what === 'cancel') close();
  });
  text.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); add().catch(fail); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
  });
  form.addEventListener('focusout', (e) => {
    if (!form.contains(e.relatedTarget) && !text.value.trim() && !waiting) setTimeout(() => { if (!form.contains(document.activeElement)) close(); }, 0);
  });
  text.focus();
}

function kanbanCardSheet(id) {
  const card = kanbanCard(id);
  const board = kanban.board;
  if (!card || !board) return;
  const others = kanban.boards.filter((b) => b.id !== board.id);
  const box = sheet(`<div class="kanban-sheet">
    <textarea class="kanban-sheet-title" id="kanban-card-title" rows="1" maxlength="300" aria-label="Card title" spellcheck="true">${esc(card.title)}</textarea>
    <input class="set-input kanban-sheet-sub" id="kanban-card-sub" maxlength="200" placeholder="Add a subheading" aria-label="Subheading" autocomplete="off" value="${esc(card.subheading || '')}">
    <div class="set-row"><span class="set-label">Status</span><label class="select-control"><select id="kanban-card-column" aria-label="Status">${board.columns.map((c) => `<option value="${esc(c.id)}"${c.id === card.column ? ' selected' : ''}>${esc(c.name)}</option>`).join('')}</select>${ICON.chevron}</label></div>
    <div class="set-row"><span class="set-label">Priority</span><div class="library-views kanban-priorities" role="group" aria-label="Priority">${KANBAN_PRIORITIES.map(([value, label]) => `<button type="button" class="btn sm plain${(card.priority || '') === value ? ' on' : ''}" data-priority="${value}" aria-pressed="${(card.priority || '') === value}">${value ? KANBAN_ICON[value] : ''}<span>${esc(label)}</span></button>`).join('')}</div></div>
    ${others.length ? `<div class="set-row"><span class="set-label">Board</span><label class="select-control"><select id="kanban-card-board" aria-label="Board"><option value="${esc(board.id)}" selected>${esc(board.name)}</option>${others.map((b) => `<option value="${esc(b.id)}">${esc(b.name)}</option>`).join('')}</select>${ICON.chevron}</label></div>` : ''}
    <div class="set-row column"><label class="set-label" for="kanban-card-description">Description</label><textarea class="set-input kanban-description" id="kanban-card-description" rows="7" maxlength="8000" placeholder="Add details, steps or links…">${esc(card.description || '')}</textarea></div>
    <div class="buttons kanban-sheet-acts"><span class="hint grow" id="kanban-card-saved" role="status"></span><button type="button" class="btn plain danger" data-kanban="delete">${ICON.trash}<span>Delete</span></button><button type="button" class="btn primary" data-action="close">Done</button></div>
  </div>`);
  box.returnFocus = app.querySelector(`.kanban-card[data-card="${CSS.escape(id)}"]`);
  const title = $('#kanban-card-title', box);
  const sub = $('#kanban-card-sub', box);
  const description = $('#kanban-card-description', box);
  const status = $('#kanban-card-column', box);
  const saved = $('#kanban-card-saved', box);
  const fit = () => { title.style.height = 'auto'; title.style.height = `${title.scrollHeight}px`; };
  fit();
  const fields = { title, subheading: sub, description };
  const timers = {};
  const save = async (name) => {
    clearTimeout(timers[name]);
    timers[name] = 0;
    const field = fields[name];
    const value = name === 'description' ? field.value.trim() : field.value.replace(/\s+/g, ' ').trim();
    if (value === (card[name] || '')) return;
    if (name === 'title' && !value) { field.value = card.title; fit(); return; }
    try {
      const done = await api(`/api/cards/${enc(card.id)}`, { method: 'PATCH', body: { [name]: value } });
      kanbanUpdate(card, done);
      if (saved.isConnected) saved.textContent = 'Saved';
    } catch (e) {
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  };
  const flush = () => { for (const name of Object.keys(fields)) if (timers[name]) save(name).catch(fail); };
  for (const [name, field] of Object.entries(fields)) {
    field.addEventListener('input', () => {
      if (name === 'title') fit();
      if (saved.isConnected) saved.textContent = '';
      clearTimeout(timers[name]);
      timers[name] = setTimeout(() => save(name).catch(fail), KANBAN_SAVE_MS);
    });
    field.addEventListener('blur', () => { if (timers[name]) save(name).catch(fail); });
  }
  title.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); sub.focus(); }
  });
  // Escape closes the sheet before a blur could save, so save first.
  box.addEventListener('keydown', (e) => { if (e.key === 'Escape') flush(); }, true);
  box.addEventListener('pointerdown', (e) => { if (e.target === box) flush(); });
  status.addEventListener('change', () => {
    kanbanMove(card, { column: status.value }).catch(fail);
  });
  $('.kanban-priorities', box).addEventListener('click', async (e) => {
    const chip = e.target.closest('[data-priority]');
    if (!chip || chip.classList.contains('on')) return;
    try {
      const done = await api(`/api/cards/${enc(card.id)}`, { method: 'PATCH', body: { priority: chip.dataset.priority } });
      kanbanUpdate(card, done);
      for (const c of box.querySelectorAll('[data-priority]')) {
        c.classList.toggle('on', c === chip);
        c.setAttribute('aria-pressed', String(c === chip));
      }
    } catch (err) {
      if (err instanceof Locked) throw err;
      toast(err.message, { bad: true });
    }
  });
  $('#kanban-card-board', box)?.addEventListener('change', (e) => {
    flush();
    closeSheet();
    kanbanMoveToBoard(card, e.target.value).catch(fail);
  });
  box.querySelector('[data-kanban="delete"]').addEventListener('click', () => {
    flush();
    closeSheet();
    kanbanDeleteCard(card).catch(fail);
  });
  box.querySelector('[data-action="close"]').addEventListener('click', flush);
}

// The card just before this one in its column, or null at the top.
function kanbanAbove(card) {
  const list = kanbanCardsIn(card.column);
  const at = list.findIndex((c) => c.id === card.id);
  return at > 0 ? list[at - 1].id : null;
}

async function kanbanMove(card, to) {
  try {
    const saved = await api(`/api/cards/${enc(card.id)}/move`, { method: 'POST', body: to });
    Object.assign(card, saved);
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
  await kanbanRefresh();
}

async function kanbanMoveToBoard(card, boardId) {
  const target = kanban.boards.find((b) => b.id === boardId);
  if (!target || boardId === card.board) return;
  const back = { board: card.board, column: card.column, after: kanbanAbove(card) };
  try {
    const saved = await api(`/api/cards/${enc(card.id)}/move`, { method: 'POST', body: { board: boardId } });
    kanban.cards = kanban.cards.filter((c) => c.id !== card.id);
    target.cards = (target.cards || 0) + 1;
    toast(`Moved “${card.title}” to ${target.name}`, {
      action: 'Undo',
      run: () => api(`/api/cards/${enc(saved.id)}/move`, { method: 'POST', body: back }).then(() => {
        target.cards = Math.max(0, (target.cards || 1) - 1);
        return kanbanRefresh();
      }).catch(fail),
    });
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
  await kanbanRefresh();
  kanbanSyncTab();
}

async function kanbanDeleteCard(card) {
  await api(`/api/cards/${enc(card.id)}`, { method: 'DELETE' });
  kanban.cards = kanban.cards.filter((c) => c.id !== card.id);
  drawKanban();
  toast(`Deleted “${card.title}”`, {
    action: 'Undo',
    run: () => api(`/api/boards/${enc(card.board)}/cards`, { method: 'POST', body: { id: card.id, column: card.column, title: card.title, subheading: card.subheading, description: card.description, priority: card.priority, rank: card.rank } }).then(kanbanRefresh).catch(fail),
  });
}

// Dragging. Mouse and pen lift past a few pixels; touch lifts after a short hold, so a
// swipe still scrolls the board. The real card (or column) moves through the page as
// the placeholder while a copy follows the pointer.
function kanbanLift() {
  const d = kanban.drag;
  if (!d || d.live || !d.item.isConnected) return;
  d.live = true;
  clearTimeout(d.timer);
  if (d.item.contains(document.activeElement) && document.activeElement.matches('input')) document.activeElement.blur();
  getSelection()?.removeAllRanges();
  document.addEventListener('touchmove', kanbanHoldStill, { passive: false });
  const rect = d.item.getBoundingClientRect();
  d.dx = d.startX - rect.left;
  d.dy = d.startY - rect.top;
  const ghost = d.item.cloneNode(true);
  for (const el of [ghost, ...ghost.querySelectorAll('*')]) {
    for (const { name } of [...el.attributes]) if (name === 'id' || name.startsWith('data-') || name === 'tabindex') el.removeAttribute(name);
  }
  ghost.classList.remove('kanban-placeholder');
  ghost.classList.add('kanban-ghost');
  ghost.setAttribute('aria-hidden', 'true');
  ghost.inert = true;
  ghost.style.width = `${rect.width}px`;
  if (d.kind === 'col') ghost.style.height = `${Math.min(rect.height, innerHeight * 0.7)}px`;
  document.body.appendChild(ghost);
  d.ghost = ghost;
  d.from = { parent: d.item.parentElement, next: d.item.nextElementSibling };
  d.item.classList.add('kanban-placeholder');
  document.body.classList.add('kanban-dragging');
  try { app.setPointerCapture(d.pointer); } catch (e) {}
  kanbanFollow();
  d.frame = requestAnimationFrame(kanbanEdges);
}

function kanbanFollow() {
  const d = kanban.drag;
  if (!d || !d.live) return;
  d.ghost.style.transform = `translate(${d.x - d.dx}px, ${d.y - d.dy}px)${d.kind === 'card' && !stillMotion() ? ' rotate(2deg)' : ''}`;
  const under = document.elementFromPoint(d.x, d.y);
  for (const tab of document.querySelectorAll('.kanban-tab.drop-on')) tab.classList.remove('drop-on');
  d.tab = null;
  if (d.kind === 'card') {
    const tab = under?.closest('[data-kanban-drop]');
    if (tab && tab.dataset.kanbanDrop !== kanban.board?.id) {
      tab.classList.add('drop-on');
      d.tab = tab.dataset.kanbanDrop;
      return;
    }
    const list = under?.closest('.kanban-col')?.querySelector('.kanban-cards');
    if (!list) return;
    const before = [...list.querySelectorAll('.kanban-card:not(.kanban-placeholder)')].find((c) => {
      const r = c.getBoundingClientRect();
      return d.y < r.top + r.height / 2;
    }) || null;
    if (d.item.parentElement !== list || d.item.nextElementSibling !== before) list.insertBefore(d.item, before);
  } else {
    const row = app.querySelector('.kanban-cols');
    if (!row) return;
    const before = [...row.querySelectorAll('.kanban-col:not(.kanban-placeholder)')].find((c) => {
      const r = c.getBoundingClientRect();
      return d.x < r.left + r.width / 2;
    }) || row.querySelector('.kanban-add-col');
    if (d.item.nextElementSibling !== before) row.insertBefore(d.item, before);
  }
}

// Near an edge the board scrolls sideways and the page up or down, so far columns are reachable.
function kanbanEdges() {
  const d = kanban.drag;
  if (!d || !d.live) return;
  const row = app.querySelector('.kanban-cols');
  let moved = false;
  if (row) {
    const r = row.getBoundingClientRect();
    const step = d.x < r.left + KANBAN_EDGE ? -14 : d.x > r.right - KANBAN_EDGE ? 14 : 0;
    const before = row.scrollLeft;
    if (step) row.scrollLeft += step;
    moved = row.scrollLeft !== before;
  }
  const top = ($('#bar')?.getBoundingClientRect().bottom || 0) + KANBAN_EDGE;
  const v = d.y < top ? -14 : d.y > innerHeight - KANBAN_EDGE ? 14 : 0;
  if (v) {
    const before = scrollY;
    scrollBy(0, v);
    moved = moved || scrollY !== before;
  }
  if (moved) kanbanFollow();
  d.frame = requestAnimationFrame(kanbanEdges);
}

function kanbanEnd() {
  const d = kanban.drag;
  if (!d) return null;
  kanban.drag = null;
  clearTimeout(d.timer);
  cancelAnimationFrame(d.frame);
  document.removeEventListener('touchmove', kanbanHoldStill);
  if (!d.live) return null;
  d.ghost?.remove();
  d.item.classList.remove('kanban-placeholder');
  document.body.classList.remove('kanban-dragging');
  for (const tab of document.querySelectorAll('.kanban-tab.drop-on')) tab.classList.remove('drop-on');
  try { if (app.hasPointerCapture(d.pointer)) app.releasePointerCapture(d.pointer); } catch (e) {}
  kanban.suppressClick = true;
  setTimeout(() => { kanban.suppressClick = false; }, 0);
  return d;
}

function kanbanCancel() {
  const d = kanbanEnd();
  if (d && d.from.parent.isConnected) d.from.parent.insertBefore(d.item, d.from.next && d.from.next.parentElement === d.from.parent ? d.from.next : null);
}

async function kanbanDrop() {
  const d = kanbanEnd();
  if (!d || !kanban.board) return;
  if (d.kind === 'col') {
    const order = [...app.querySelectorAll('.kanban-col')].map((c) => c.dataset.col);
    if (order.join('\n') === kanban.board.columns.map((c) => c.id).join('\n')) return;
    return kanbanSaveColumns(order.map(kanbanColumn).filter(Boolean));
  }
  const card = kanbanCard(d.id);
  if (!card) return;
  if (d.tab) {
    d.item.remove();
    return kanbanMoveToBoard(card, d.tab);
  }
  const column = d.item.parentElement?.dataset.col;
  if (!column) return kanbanRefresh();
  const after = d.item.previousElementSibling?.dataset.card || null;
  if (column === card.column && after === kanbanAbove(card)) return;
  card.column = column;
  kanbanCounts();
  d.item.focus({ preventScroll: true });
  return kanbanMove(card, { column, after });
}

// Keep the moved card focused after keyboard moves and redraws.
async function kanbanKeyMove(card, key) {
  const columns = kanban.board.columns;
  const list = kanbanCardsIn(card.column);
  const at = list.findIndex((c) => c.id === card.id);
  let to = null;
  if (key === 'ArrowUp' && at > 0) to = { column: card.column, after: at > 1 ? list[at - 2].id : null };
  else if (key === 'ArrowDown' && at < list.length - 1) to = { column: card.column, after: list[at + 1].id };
  else if (key === 'ArrowLeft' || key === 'ArrowRight') {
    const c = columns.findIndex((col) => col.id === card.column) + (key === 'ArrowLeft' ? -1 : 1);
    if (c >= 0 && c < columns.length) to = { column: columns[c].id };
  }
  if (!to) return;
  await kanbanMove(card, to);
  app.querySelector(`.kanban-card[data-card="${CSS.escape(card.id)}"]`)?.focus();
}

document.addEventListener('pointerdown', (e) => {
  if (state.view !== 'boards' || e.button !== 0 || kanban.drag || !e.isPrimary) return;
  const card = e.target.closest('.kanban-card');
  const head = card ? null : e.target.closest('.kanban-col-head');
  if (!card && !head) return;
  // A column also drags by its name, unless that name is being edited; a click still edits it.
  const onName = Boolean(head) && e.target.matches('.kanban-col-name') && document.activeElement !== e.target;
  if (!onName && e.target.closest('input, textarea, select, button, a')) return;
  const item = card || head.closest('.kanban-col');
  const touch = e.pointerType === 'touch';
  kanban.drag = { kind: card ? 'card' : 'col', item, id: card ? card.dataset.card : item.dataset.col, pointer: e.pointerId, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, live: false, timer: 0, touch };
  if (touch) kanban.drag.timer = setTimeout(kanbanLift, KANBAN_HOLD_MS);
  else if (!onName) {
    // No text selection while dragging; the card still takes focus.
    e.preventDefault();
    card?.focus({ preventScroll: true });
  }
});

document.addEventListener('pointermove', (e) => {
  const d = kanban.drag;
  if (!d || e.pointerId !== d.pointer) return;
  d.x = e.clientX;
  d.y = e.clientY;
  if (!d.live) {
    const far = Math.hypot(d.x - d.startX, d.y - d.startY);
    if (d.touch) { if (far > 8) kanbanEnd(); return; }
    if (far < 5) return;
    if (!e.buttons) { kanbanEnd(); return; }
    kanbanLift();
    return;
  }
  if (!e.buttons && e.pointerType !== 'touch') { kanbanDrop().catch(fail); return; }
  kanbanFollow();
});

document.addEventListener('pointerup', (e) => {
  const d = kanban.drag;
  if (!d || e.pointerId !== d.pointer) return;
  if (d.live) kanbanDrop().catch(fail);
  else kanbanEnd();
});

document.addEventListener('pointercancel', (e) => {
  const d = kanban.drag;
  if (!d || e.pointerId !== d.pointer) return;
  if (d.live) kanbanCancel();
  else kanbanEnd();
});

app.addEventListener('lostpointercapture', (e) => {
  if (kanban.drag?.live && e.pointerId === kanban.drag.pointer) kanbanDrop().catch(fail);
});

// A held touch is a drag: stop the page from scrolling under it. Added only while a drag
// is live, so scrolling everywhere else stays on the fast path.
function kanbanHoldStill(e) {
  if (kanban.drag?.live && e.cancelable) e.preventDefault();
}

document.addEventListener('contextmenu', (e) => {
  if (kanban.drag) e.preventDefault();
});

// A drop ends with a click on the card; it must not open it.
document.addEventListener('click', (e) => {
  if (!kanban.suppressClick) return;
  e.preventDefault();
  e.stopImmediatePropagation();
}, true);

document.addEventListener('keydown', (e) => {
  if (state.view !== 'boards') return;
  if (kanban.drag?.live && e.key === 'Escape') {
    e.preventDefault();
    e.stopImmediatePropagation();
    kanbanCancel();
    return;
  }
  const name = e.target.closest?.('[data-kanban-name]');
  if (name) {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); name.blur(); }
    else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopImmediatePropagation();
      const column = kanbanColumn(name.dataset.kanbanName);
      name.value = column ? column.name : kanban.board?.name || '';
      name.blur();
    }
    return;
  }
  const el = e.target.closest?.('.kanban-card');
  if (!el || e.target !== el) return;
  const card = kanbanCard(el.dataset.card);
  if (!card) return;
  if ((e.key === 'Enter' || e.key === ' ') && !e.altKey && !e.metaKey && !e.ctrlKey) {
    e.preventDefault();
    kanbanCardSheet(card.id);
  } else if (e.altKey && !e.metaKey && !e.ctrlKey && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) {
    e.preventDefault();
    kanbanKeyMove(card, e.key).catch(fail);
  }
});

// Leaving the page mid-drag drops nothing and leaves nothing behind.
function leaveBoards() {
  if (kanban.drag?.live) kanbanCancel();
  else kanbanEnd();
}
