const NOTE_DRAG = felix.NOTE_DRAG;
const FOLDER_DRAG = 'application/x-leo-folder';
const FILE_VIEWS = ['folder', 'search', 'note'];
const dragging = { note: null, folder: null, filesTimer: 0 };

const carries = (e, type) => Boolean(e.dataTransfer && [...e.dataTransfer.types].includes(type));
const parentOf = (path) => path.split('/').slice(0, -1).join('/');
const dropTargetOf = (el) => (el && el.closest ? el.closest('[data-action="open-folder"][data-dir], [data-action="home"], [data-drop-dir]') : null);
const dirOf = (target) => (Object.hasOwn(target.dataset, 'dropDir') ? target.dataset.dropDir : target.dataset.action === 'home' ? '' : target.dataset.dir);
const noteCardOf = (el) => (el && el.closest ? el.closest('.card[data-id][data-action="open-note"]') : null);
const combinesWith = (e) => {
  const card = dragging.note && carries(e, NOTE_DRAG) ? noteCardOf(e.target) : null;
  return card && card.dataset.id !== dragging.note.id ? card : null;
};

function accepts(e, target) {
  const into = dirOf(target);
  if (carries(e, NOTE_DRAG)) return !dragging.note || dragging.note.dir !== into;
  if (carries(e, FOLDER_DRAG)) {
    const from = dragging.folder;
    return from !== null && into !== from && !into.startsWith(`${from}/`) && parentOf(from) !== into;
  }
  return carries(e, 'Files');
}

function markDrop(target) {
  for (const el of document.querySelectorAll('.drop-ok')) if (el !== target) el.classList.remove('drop-ok');
  if (target) target.classList.add('drop-ok');
}

function filesOver(on) {
  clearTimeout(dragging.filesTimer);
  document.body.classList.toggle('files-over', on);
  if (on) dragging.filesTimer = setTimeout(() => document.body.classList.remove('files-over'), 160);
}

function endDrag() {
  dragging.note = null;
  dragging.folder = null;
  document.body.classList.remove('dragging');
  markDrop(null);
  filesOver(false);
}

document.addEventListener('dragstart', (e) => {
  const from = e.target.closest ? e.target : null;
  const card = from && from.closest('.card[data-id][draggable="true"]');
  const folder = !card && from && from.closest('[data-action="open-folder"][data-dir][draggable="true"]');
  if (card) {
    dragging.note = { id: card.dataset.id, title: card.dataset.title || 'Untitled', dir: card.dataset.from || '' };
    e.dataTransfer.setData(NOTE_DRAG, JSON.stringify(dragging.note));
    e.dataTransfer.setData('text/plain', dragging.note.title);
    e.dataTransfer.effectAllowed = 'copyMove';
  } else if (folder) {
    dragging.folder = folder.dataset.dir;
    e.dataTransfer.setData(FOLDER_DRAG, dragging.folder);
    e.dataTransfer.setData('text/plain', folderLabel(dragging.folder));
    e.dataTransfer.effectAllowed = 'move';
  } else {
    return;
  }
  document.body.classList.add('dragging');
});

document.addEventListener('dragend', endDrag);

document.addEventListener('dragover', (e) => {
  if (e.defaultPrevented) return;
  const onto = combinesWith(e);
  if (onto) {
    markDrop(onto);
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    return;
  }
  const target = dropTargetOf(e.target);
  const ok = target && accepts(e, target);
  markDrop(ok ? target : null);
  if (ok) {
    e.preventDefault();
    e.dataTransfer.dropEffect = carries(e, 'Files') ? 'copy' : 'move';
    return;
  }
  if (carries(e, 'Files') && FILE_VIEWS.includes(state.view) && !$('.scrim')) {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    filesOver(state.view !== 'note');
  }
});

document.addEventListener('drop', (e) => {
  const onto = combinesWith(e);
  const target = dropTargetOf(e.target);
  const ok = target && accepts(e, target);
  const note = dragging.note;
  const folder = dragging.folder;
  endDrag();
  if (e.defaultPrevented) return;
  if (note && onto) {
    e.preventDefault();
    combineSheet(note, { id: onto.dataset.id, title: onto.dataset.title || 'Untitled' });
    return;
  }
  if (note || folder) {
    if (!ok) return;
    e.preventDefault();
    const into = dirOf(target);
    if (note && note.dir !== into) dropNote(note, into).catch(fail);
    else if (folder !== null && folder !== into) dropFolder(folder, into).catch(fail);
    return;
  }
  const files = e.dataTransfer && e.dataTransfer.files;
  if (!files || !files.length || $('.scrim')) return;
  if (target) {
    e.preventDefault();
    uploadSheet(files, dirOf(target)).catch(fail);
  } else if (FILE_VIEWS.includes(state.view)) {
    e.preventDefault();
    uploadSheet(files).catch(fail);
  }
});

function combineSheet(from, into) {
  const box = sheet(`<div class="combine">
    <h3>Combine these notes?</h3>
    <p>Both notes go into “${esc(into.title)}”, each whole under its own title, the older one first. Any recording transcripts come along too, so you can read both.</p>
    <p class="hint">“${esc(from.title)}” goes to the trash, and Undo brings both back as they were.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" id="combine-go">Combine</button></div>
  </div>`);
  $('#combine-go', box).addEventListener('click', () => combineNow(from, into).catch(fail));
}

async function combineNow(from, into) {
  const go = $('#combine-go');
  if (go) go.disabled = true;
  let saved;
  try {
    saved = await api(`/api/notes/${enc(into.id)}/combine`, { method: 'POST', body: { with: from.id } });
  } catch (e) {
    const open = $('.scrim .combine');
    if (open) open.innerHTML = `<h3>The notes could not be combined</h3><p class="upload-error">${esc(e.message)}</p><div class="buttons"><button class="btn plain" data-action="close">Close</button></div>`;
    return;
  }
  closeSheet();
  await afterCombine(into.id);
  toast(`Combined into “${into.title}”`, {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(into.id)}`, { method: 'PATCH', body: { body: saved.before, base: saved.note.version } });
        await api('/api/trash/restore', { method: 'POST', body: { ids: [saved.removed] } });
        if (saved.sources?.length) await api(`/api/notes/${enc(into.id)}/uncombine`, { method: 'POST', body: { sources: saved.sources } });
        await afterCombine(into.id);
        toast('Both notes are back as they were.');
      } catch (e) {
        fail(e);
      }
    },
  });
}

async function afterCombine(id) {
  if (state.view === 'note' && state.session && state.session.note.id === id && !saving.unsaved()) await showNote(id);
  else await showLatest();
  loadSideFolders();
}

async function moveNote(id, dir) {
  return api(`/api/notes/${enc(id)}/move`, { method: 'POST', body: { directory: dir } });
}

async function dropNote(note, into) {
  await moveNote(note.id, into);
  await afterMove();
  toast(`Moved “${note.title}” to ${folderLabel(into)}`, {
    action: 'Undo',
    run: () => moveNote(note.id, note.dir).then(() => afterMove()).catch(fail),
  });
}

async function dropFolder(from, into) {
  let moved;
  try {
    moved = await api('/api/dirs/move', { method: 'POST', body: { from, into } });
  } catch (e) {
    if (e.status !== 409) throw e;
    toast(`${folderLabel(into)} already has a folder named ${folderLabel(from)}.`, { bad: true });
    return;
  }
  await afterMove(from, moved.path);
  toast(`Moved ${folderLabel(from)} into ${folderLabel(into)}`, {
    action: 'Undo',
    run: () =>
      api('/api/dirs/move', { method: 'POST', body: { from: moved.path, into: parentOf(from) } })
        .then(() => afterMove(moved.path, from))
        .catch(fail),
  });
}

async function afterMove(from, to) {
  const dir = state.dir || '';
  if (from && (state.view === 'folder' || state.view === 'note') && (dir === from || dir.startsWith(`${from}/`))) {
    const now = to + dir.slice(from.length);
    if (state.view === 'folder') return go(folderHash(now), { replace: true });
    state.dir = now;
  }
  if (state.view === 'note' && state.session && state.session.note.id && !saving.unsaved()) await showNote(state.session.note.id);
  else await showLatest();
  loadSideFolders();
}
