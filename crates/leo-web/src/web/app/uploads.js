const UPLOAD_ACCEPT = '.pdf,.docx,.pptx,.txt,.md,image/*';
const UPLOAD_MOST = 90 * 1024 * 1024;
let picked = [];
let uploadPoll = 0;

const size = (n) => (n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);

async function shrink(file) {
  if (!/^image\//.test(file.type) || file.type === 'image/gif') return { name: file.name, type: file.type, blob: file };
  try {
    const bitmap = await createImageBitmap(file);
    const scale = Math.min(1, 2000 / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.86));
    if (!blob) throw new Error('no blob');
    return { name: file.name.replace(/\.[^.]+$/, '') + '.jpg', type: 'image/jpeg', blob };
  } catch (e) {
    return { name: file.name, type: file.type, blob: file };
  }
}

function base64(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(',')[1] || '');
    reader.onerror = () => reject(new Error(`${blob.name || 'A file'} could not be read.`));
    reader.readAsDataURL(blob);
  });
}

async function pictureFor(file) {
  if (file.type === 'image/png' && file.size <= 3 * 1024 * 1024) return { name: file.name, type: file.type, blob: file };
  return shrink(file);
}

function altFor(name) {
  const stem = String(name || '').replace(/\.[^.]+$/, '').replace(/[[\]()]/g, ' ').replace(/\s+/g, ' ').trim();
  return !stem || /^image$/i.test(stem) ? 'Pasted picture' : stem;
}

const pastedName = (file, i) => (file.name && !/^image\.\w+$/i.test(file.name) ? file.name : `pasted-${i + 1}.${(file.type.split('/')[1] || 'png').replace('jpeg', 'jpg')}`);
const pastedFiles = (files, from = 0) => files.map((f, i) => (pastedName(f, from + i) === f.name ? f : new File([f], pastedName(f, from + i), { type: f.type })));

async function addPictures(s, files) {
  const list = [...files].filter((f) => /^image\//.test(f.type));
  if (!list.length || !s || !s.doc) return;
  toast(list.length === 1 ? 'Adding the picture…' : `Adding ${list.length} pictures…`);
  let added = 0;
  for (const [i, file] of list.entries()) {
    try {
      const ready = await pictureFor(file);
      const saved = await api('/api/images', { method: 'POST', body: { name: pastedName(ready, i), data: await base64(ready.blob) } });
      if (state.session !== s || !s.doc) return;
      s.doc.insert(`![${altFor(file.name)}](${saved.path})`);
      added += 1;
    } catch (e) {
      fail(e);
      return;
    }
  }
  toast(added === 1 ? 'Picture added' : `${added} pictures added`);
}

function choosePictures() {
  const s = state.session;
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = 'image/*';
  input.multiple = true;
  input.addEventListener('change', () => addPictures(s, input.files));
  input.click();
}

function drawPicked() {
  const list = $('#upload-list');
  if (!list) return;
  list.innerHTML = picked
    .map((f, i) => `<div class="upload-file">${/^image\//.test(f.type) ? ICON.image : ICON.note}<span class="grow">${esc(f.name)}<span class="sub">${size(f.size)}</span></span><button class="icon-btn" data-action="upload-drop" data-i="${i}" aria-label="Remove ${esc(f.name)}">${ICON.close}</button></div>`)
    .join('');
  $('#upload-go').disabled = !picked.length;
  $('#upload-pick-label').textContent = picked.length ? 'Add more files' : 'Choose files, or take photos';
}

async function uploadSheet(files, into) {
  const here = into !== undefined ? into : state.view === 'folder' ? state.dir : state.view === 'note' ? state.dir || '' : '';
  picked = files ? [...files] : [];
  let folders = [];
  try {
    folders = await api('/api/folders');
  } catch (e) {
    folders = [];
  }
  const options = [{ name: '' }, ...folders].map((d) => `<option value="${esc(d.name)}"${d.name === here ? ' selected' : ''}>${esc(d.name ? folderLabel(d.name) + (d.name.includes('/') ? ` (${d.name})` : '') : 'All notes (top level)')}</option>`).join('');
  sheet(`<h3>Make a note from a file</h3>
    <p>Slides, handouts, papers or worksheets as PDF, Word, PowerPoint or text, or photos and scans of pages. Your AI reads them and writes study notes.</p>
    <label class="upload-drop" id="upload-zone">${ICON.upload}<span id="upload-pick-label">Choose files, or take photos</span><input type="file" id="upload-input" multiple accept="${UPLOAD_ACCEPT}"></label>
    <div class="upload-list" id="upload-list"></div>
    <label class="field">${ICON.folder}<select id="upload-dir">${options}</select></label>
    <label class="field">${ICON.note}<input id="upload-title" placeholder="Title (optional; the AI names it otherwise)" autocomplete="off"></label>
    <label class="field upload-wants"><textarea id="upload-wants" rows="2" maxlength="2000" placeholder="What do you want from it? (optional) e.g. the interview problem, then the code, explained step by step"></textarea></label>
    <p class="hint upload-hint">You can also paste a picture here. Photos stay in the note, and pictures and diagrams in slides and documents are kept beside the text. Reading photos and scans needs an AI that can see images: OpenAI, Anthropic, Gemini, xAI, Claude Code or Codex.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" id="upload-go" data-action="upload-go" disabled>Make the note</button></div>`);
  $('#upload-input').addEventListener('change', (e) => {
    picked.push(...e.target.files);
    e.target.value = '';
    drawPicked();
  });
  const zone = $('#upload-zone');
  zone.addEventListener('dragover', (e) => {
    e.preventDefault();
    zone.classList.add('over');
  });
  zone.addEventListener('dragleave', () => zone.classList.remove('over'));
  zone.addEventListener('drop', (e) => {
    e.preventDefault();
    zone.classList.remove('over');
    picked.push(...e.dataTransfer.files);
    drawPicked();
  });
  drawPicked();
}

function uploadProgress(text, done, total) {
  const bar = total ? Math.round((done / total) * 100) : 8;
  const box = $('#upload-progress');
  if (!box) return;
  box.innerHTML = `<div class="upload-step">${esc(text)}</div><div class="upload-bar"><i style="width:${Math.max(6, bar)}%"></i></div>`;
}

async function uploadGo() {
  if (!picked.length) return;
  const dir = $('#upload-dir').value;
  const title = $('#upload-title').value.trim();
  const wants = $('#upload-wants').value.trim();
  const names = picked.map((f) => f.name);
  sheet(`<div class="upload-working">${felix.felix(72, 'idle think')}<h3>Making your note</h3><p>${esc(names.length === 1 ? names[0] : `${names.length} files`)}</p><div id="upload-progress"></div><p class="hint">This can take a minute for long files. You can close this; the note appears in the folder when it is ready.</p></div>`);
  uploadProgress('Preparing the files', 0, 0);
  const files = [];
  let total = 0;
  for (const file of picked) {
    const ready = await shrink(file);
    total += ready.blob.size;
    if (total > UPLOAD_MOST) {
      closeSheet();
      return toast('That is too much to upload at once; send fewer files.', { bad: true });
    }
    files.push({ name: ready.name, type: ready.type, data: await base64(ready.blob) });
  }
  uploadProgress('Uploading', 0, 0);
  let started;
  try {
    started = await api('/api/import', { method: 'POST', body: { directory: dir, title: title || null, wants, files } });
  } catch (e) {
    closeSheet();
    throw e;
  }
  picked = [];
  watchUpload(started.id, dir);
  checkActivity();
}

function watchUpload(id, dir) {
  clearTimeout(uploadPoll);
  uploadPoll = setTimeout(async () => {
    let job;
    try {
      job = await api(`/api/import/${enc(id)}`);
    } catch (e) {
      return watchUpload(id, dir);
    }
    if (job.state === 'working') {
      uploadProgress(job.step, job.done, job.total);
      return watchUpload(id, dir);
    }
    const open = $('.upload-working');
    if (job.state === 'failed') {
      if (open) {
        open.innerHTML = `${felix.felix(72, 'droop')}<h3>The note could not be made</h3><p class="upload-error">${esc(job.error || 'Something went wrong.')}</p><div class="buttons"><button class="btn plain" data-action="close">Close</button><button class="btn primary" data-action="upload">Try again</button></div>`;
      } else {
        toast(job.error || 'The upload failed.', { bad: true });
      }
      return;
    }
    if (open) {
      closeSheet();
      go(noteHash(job.note));
    } else {
      await showLatest().catch(() => {});
      toast('Your note from the upload is ready.', { action: 'Open', run: () => go(noteHash(job.note)) });
    }
  }, 900);
}

async function showLatest() {
  if ($('.scrim')) return;
  if (state.view === 'folder' && !state.selecting) await showFolder(state.dir);
  else if (state.view === 'search') await render();
}

async function drawOriginals(note) {
  if (!note.id) return;
  let files = [];
  try {
    files = await api(`/api/notes/${enc(note.id)}/originals`);
  } catch (e) {
    return;
  }
  const meta = $('.note-meta');
  if (!files.length || !meta || state.view !== 'note' || state.session.note.id !== note.id) return;
  const all = files.length > 1 ? `<a class="chip original all" href="/api/notes/${enc(note.id)}/originals.zip" download>${ICON.paperclip}Download all ${files.length}</a>` : '';
  meta.insertAdjacentHTML('beforeend', files.map((f) => {
    const look = viewKind(f.name) ? ` data-action="view-original" data-id="${esc(note.id)}" data-name="${esc(f.name)}" title="Open ${esc(f.name)}"` : '';
    return `<a class="chip original" href="/api/notes/${enc(note.id)}/originals/${enc(f.name)}" download="${esc(f.name)}"${look}>${ICON.paperclip}${esc(f.name)}</a>`;
  }).join('') + all);
}

function viewKind(name) {
  if (/\.pdf$/i.test(name)) return 'pdf';
  if (/\.(png|jpe?g|gif|webp)$/i.test(name)) return 'image';
  if (/\.(txt|md)$/i.test(name)) return 'text';
  return null;
}

async function viewOriginal(id, name) {
  const kind = viewKind(name);
  if (!kind) return;
  const url = `/api/notes/${enc(id)}/originals/${enc(name)}`;
  const zoom = kind === 'image'
    ? '<button class="btn sm plain" data-action="viewer-zoom" data-step="-1" aria-label="Zoom out">−</button><button class="btn sm plain" data-action="viewer-zoom" data-step="1" aria-label="Zoom in">+</button><button class="btn sm plain" data-action="viewer-zoom" data-step="0">Fit</button>'
    : '';
  const tab = kind === 'text' ? '' : `<a class="btn sm plain" href="${url}?view=1" target="_blank" rel="noopener">Open in a new tab</a>`;
  const body = kind === 'pdf'
    ? `<iframe class="viewer-frame" src="${url}?view=1" title="${esc(name)}"></iframe>`
    : kind === 'image'
      ? `<div class="viewer-pan"><img class="viewer-img" src="${url}?view=1" alt="${esc(name)}"></div>`
      : '<div class="viewer-text prose">Loading…</div>';
  const scrim = sheet(`<div class="viewer-head"><b title="${esc(name)}">${esc(name)}</b><span class="viewer-tools">${zoom}<a class="btn sm plain" href="${url}" download="${esc(name)}">Download</a>${tab}<button class="icon-btn" data-action="close" aria-label="Close">${ICON.close}</button></span></div>${body}`);
  scrim.querySelector('.sheet').classList.add('viewer');
  state.viewerZoom = 1;
  const img = scrim.querySelector('.viewer-img');
  if (img) img.addEventListener('dblclick', () => setViewerZoom(state.viewerZoom > 1 ? 1 : 2));
  if (kind === 'text') {
    const box = scrim.querySelector('.viewer-text');
    try {
      const response = await fetch(url, { credentials: 'same-origin' });
      if (!response.ok) throw new Error();
      const text = await response.text();
      if (!box.isConnected) return;
      box.innerHTML = /\.md$/i.test(name) ? md.render(text) : `<pre>${esc(text)}</pre>`;
    } catch (e) {
      box.textContent = 'This file could not be shown. Download it instead.';
    }
  }
}

function setViewerZoom(level) {
  state.viewerZoom = Math.min(4, Math.max(0.5, level));
  const img = $('.viewer-img');
  if (img) img.style.setProperty('--zoom', String(state.viewerZoom));
}
