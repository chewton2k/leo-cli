const askFloat = document.createElement('button');
askFloat.type = 'button';
askFloat.className = 'float-act ask-float';
askFloat.innerHTML = `${ICON.chat}<span>Ask Felix</span>`;
askFloat.hidden = true;
askFloat.addEventListener('mousedown', (e) => e.preventDefault());
askFloat.addEventListener('click', () => askAboutSelection());
document.body.appendChild(askFloat);

function hideAskFloat() {
  askFloat.hidden = true;
}

function placeAskFloat(has) {
  const s = state.view === 'note' ? state.session : null;
  const point = has && s && s.doc ? s.doc.selectionBox() : null;
  const doc = $('#doc');
  if (!point || !doc) return hideAskFloat();
  askFloat.hidden = false;
  const width = askFloat.offsetWidth;
  const area = doc.getBoundingClientRect();
  const left = Math.max(area.left, Math.min(point.x - width / 2, area.right - width));
  askFloat.style.left = `${Math.max(8, left)}px`;
  askFloat.style.top = `${Math.min(point.y + 6, window.innerHeight - askFloat.offsetHeight - 12)}px`;
}

function askAboutSelection() {
  const s = state.view === 'note' ? state.session : null;
  const picked = s && s.doc ? s.doc.selection() : null;
  hideAskFloat();
  if (!picked) return toast('Select some text in the note first.');
  if (!s.note.id) return toast('Felix can read this note once it has been saved.');
  chat.attachSelection({ note: s.note.id, title: cleanTitle(s.title.textContent) || s.note.title || 'Untitled', ...picked });
}

document.addEventListener('mousedown', (e) => {
  if (e.target.closest && e.target.closest('[data-keep-selection]')) e.preventDefault();
});

document.addEventListener('keydown', (e) => {
  if (state.view !== 'note' || !(e.metaKey || e.ctrlKey) || !e.shiftKey || e.altKey || e.key.toLowerCase() !== 'e') return;
  e.preventDefault();
  askAboutSelection();
});

window.addEventListener('scroll', hideAskFloat, { passive: true, capture: true });

function requestId() {
  if (window.crypto && typeof window.crypto.randomUUID === 'function') return window.crypto.randomUUID();
  return `r${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`;
}

function addSheet({ text }) {
  const open = state.view === 'note' && state.session && state.session.note.id ? state.session : null;
  const caret = open && open.doc ? open.doc.caretLine() : null;
  const pick = { target: open ? { id: open.note.id, title: cleanTitle(open.title.textContent) || open.note.title || 'Untitled' } : null, where: 'end', searching: !open, found: [], query: '' };
  const request = requestId();
  const box = sheet(`<h3>Add to a note</h3>
    <div class="add-target" id="add-target"></div>
    <div class="add-preview prose">${md.render(text)}</div>
    <div class="add-where" id="add-where" role="radiogroup" aria-label="Where to add it"></div>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" id="add-go">Add</button></div>`);
  const targetBox = $('#add-target', box);
  const whereBox = $('#add-where', box);
  const go = $('#add-go', box);
  let looking = 0;

  const canCursor = () => Boolean(open && pick.target && pick.target.id === open.note.id && caret !== null);

  function drawWhere() {
    if (!canCursor()) pick.where = 'end';
    const chip = (id, label) => `<button type="button" class="chip${pick.where === id ? ' on' : ''}" role="radio" aria-checked="${pick.where === id}" data-where="${id}">${label}</button>`;
    whereBox.innerHTML = chip('end', 'End of note') + (canCursor() ? chip('cursor', 'At the cursor') : '');
  }

  function drawTarget() {
    if (!pick.searching && pick.target) {
      targetBox.innerHTML = `<div class="set-row"><span class="set-label">Into</span><span class="set-value"><span class="add-title">${esc(pick.target.title)}</span><button type="button" class="btn plain sm" id="add-change">Change</button></span></div>`;
      $('#add-change', box).addEventListener('click', () => {
        pick.searching = true;
        drawTarget();
      });
    } else {
      const rows = pick.found.map((n, i) => `<button type="button" class="list-row add-found" data-i="${i}">${ICON.note}<span class="grow">${esc(n.title || 'Untitled')}${n.directory ? `<span class="sub">${esc(folderLabel(n.directory))}</span>` : ''}</span></button>`).join('');
      targetBox.innerHTML = `<input class="set-input" id="add-search" type="search" placeholder="Find the note to add it to" autocomplete="off" value="${esc(pick.query)}">
        <div class="add-results">${rows || `<p class="hint">${pick.query.trim() ? 'No notes match.' : 'Type part of a note’s title.'}</p>`}</div>`;
      const input = $('#add-search', box);
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
      input.addEventListener('input', () => {
        pick.query = input.value;
        const mine = ++looking;
        setTimeout(async () => {
          if (mine !== looking) return;
          const query = pick.query.trim();
          let found = [];
          if (query) {
            try {
              found = await api(`/api/search?q=${enc(query)}&brief=true`);
            } catch (e) {
              found = [];
            }
          }
          if (mine !== looking) return;
          pick.found = (found || []).slice(0, 8);
          drawTarget();
        }, 180);
      });
      for (const row of targetBox.querySelectorAll('.add-found')) {
        row.addEventListener('click', () => {
          const n = pick.found[Number(row.dataset.i)];
          pick.target = { id: n.id, title: n.title || 'Untitled' };
          pick.searching = false;
          drawTarget();
          drawWhere();
        });
      }
    }
    go.disabled = !pick.target;
  }

  whereBox.addEventListener('click', (e) => {
    const chip = e.target.closest('[data-where]');
    if (!chip) return;
    pick.where = chip.dataset.where;
    drawWhere();
  });

  go.addEventListener('click', async () => {
    if (!pick.target || go.disabled) return;
    go.disabled = true;
    const target = pick.target;
    try {
      let base = null;
      const isOpen = state.view === 'note' && state.session && state.session.note.id === target.id;
      if (isOpen) {
        const s = await ready();
        base = s ? s.note.version : null;
      }
      if (!base) base = (await api(`/api/notes/${enc(target.id)}`)).version;
      const line = pick.where === 'cursor' && caret !== null ? caret + 1 : null;
      const done = await api(`/api/notes/${enc(target.id)}/insert`, { method: 'POST', body: { text, line, base, request } });
      closeSheet();
      if (state.view === 'note' && state.session && state.session.note.id === target.id) await showNote(target.id);
      toast(`Added to “${target.title}”`, {
        action: 'Undo',
        run: async () => {
          try {
            await api(`/api/notes/${enc(target.id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
            if (state.view === 'note' && state.session && state.session.note.id === target.id) await showNote(target.id);
            toast(`Took it out of “${target.title}”`);
          } catch (e) {
            toast(e.status === 409 ? 'That note has changed since, so leo left it as it is.' : e.message, { bad: true });
          }
        },
      });
    } catch (e) {
      go.disabled = false;
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  });

  drawTarget();
  drawWhere();
}
