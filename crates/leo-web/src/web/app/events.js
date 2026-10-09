document.addEventListener('click', (e) => {
  const el = e.target.closest('[data-action]');
  if (!el) return;
  const act = actions[el.dataset.action];
  if (!act) return;
  e.preventDefault();
  Promise.resolve(act(el)).catch(fail);
});

document.addEventListener('keydown', (e) => {
  const typing = e.target.closest('input, textarea, select, [contenteditable]');
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    searchEverywhere();
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && state.view === 'note') {
    e.preventDefault();
    flush().catch(fail);
    return;
  }
  if (e.key === 'Escape') {
    if ($('.scrim')) return closeSheet();
    if (typing) return e.target.blur();
    if ((state.view === 'folder' || state.view === 'trash') && state.selecting) {
      state.selecting = false;
      state.picked = new Set();
      return state.view === 'folder' ? drawFolder() : drawTrash();
    }
    if (state.view === 'map' && mapView && mapView.selected()) return mapView.select(null);
    if (state.view !== 'folder' || state.dir) return back();
  }
  if ((e.key === 'Enter' || e.key === ' ') && e.target.matches('.pick-card')) {
    e.preventDefault();
    e.target.click();
    return;
  }
  if (e.key === 'Enter' && e.target.matches('.card')) {
    e.preventDefault();
    go(noteHash(e.target.dataset.id));
    return;
  }
  if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === '/') {
    e.preventDefault();
    openSearch('');
    $('#search-input').focus();
  } else if (e.key === 'n' && state.view === 'folder') {
    go(state.dir ? `#/new/${enc(state.dir)}` : '#/new');
  }
});

const unsaved = () => saving.unsaved();

window.addEventListener('beforeunload', (e) => {
  if (!unsaved()) return;
  flush().catch(() => {});
  e.preventDefault();
});

window.addEventListener('hashchange', () => {
  const leaving = state.session;
  if (leaving) {
    leaving.doc.stop();
    flush(leaving).catch(fail);
    leaving.doc.destroy();
  }
  route();
});

const busy = () => {
  const active = document.activeElement;
  return unsaved() || Boolean(active && active !== document.body && app.contains(active));
};

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') {
    if (state.session) flush().catch(() => {});
    return;
  }
  if (!['folder', 'note', 'search', 'trash'].includes(state.view)) return;
  if (state.view === 'note' && (busy() || !state.session.note.id)) return;
  render().catch(fail);
});

window.addEventListener(
  'scroll',
  () => $('#bar').classList.toggle('scrolled', window.scrollY > 4),
  { passive: true }
);

async function route() {
  closeSheet();
  window.scrollTo(0, 0);
  await render();
}

const activity = { tasks: [], timer: 0, folded: false, route: '' };

function hiddenHere(task) {
  if (task.kind === 'recording') return activity.route === 'record';
  if (task.kind === 'map') return activity.route === 'map';
  return Boolean($('.upload-working'));
}

function drawActivity() {
  let box = $('#activity');
  if (!box) {
    box = document.createElement('aside');
    box.id = 'activity';
    box.className = 'activity';
    box.setAttribute('aria-label', 'Working in the background');
    box.setAttribute('aria-live', 'polite');
    document.body.appendChild(box);
  }
  const shown = activity.tasks.filter((t) => !hiddenHere(t));
  box.hidden = !shown.length;
  if (!shown.length) return;
  box.classList.toggle('folded', activity.folded);
  const rows = shown
    .map((t) => {
      const share = t.total ? Math.max(4, Math.round((t.done / t.total) * 100)) : 0;
      const count = t.total ? ` · ${t.done}/${t.total}` : '';
      return `<button class="activity-row" data-action="activity-open" data-href="${esc(t.href)}">
        <span class="activity-label">${esc(t.label)}</span>
        <span class="activity-step">${esc(t.step)}${count}</span>
        <span class="activity-bar${t.total ? '' : ' busy'}"><i style="width:${share}%"></i></span>
      </button>`;
    })
    .join('');
  box.innerHTML = `<button class="activity-head" data-action="activity-fold" aria-expanded="${!activity.folded}"><span class="activity-spin"></span><span class="grow">${shown.length === 1 ? 'Working in the background' : `${shown.length} things in the background`}</span><span class="activity-fold">${activity.folded ? 'Show' : 'Hide'}</span></button>${activity.folded ? '' : rows}`;
}

async function checkActivity() {
  clearTimeout(activity.timer);
  try {
    activity.tasks = (await api('/api/activity')).tasks;
  } catch (e) {
    if (e instanceof Locked) return;
  }
  drawActivity();
  if (activity.tasks.length) activity.timer = setTimeout(checkActivity, 1500);
}

async function render() {
  const hash = decodeURI(location.hash || '#/');
  const [, kind, rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [null, '', ''];
  const arg = decodeURIComponent(rest);
  if (kind !== 'search') closeSearch();
  if (kind !== 'map') leaveMap();
  if (kind !== 'n') chat.setContext(null);
  if (kind !== 'record') recorder.leave();
  activity.route = kind;
  drawActivity();
  try {
    if (kind === 'f') await showFolder(arg);
    else if (kind === 'n') await showNote(arg.replace(/\/edit$/, ''));
    else if (kind === 'new') await newNote(arg);
    else if (kind === 'search') await showSearch(arg);
    else if (kind === 'trash') await showTrash();
    else if (kind === 'map') await showMap(arg);
    else if (kind === 'settings') await (arg === 'storage' ? showStorage() : showSettings());
    else if (kind === 'record') await showRecord(arg);
    else if (kind === 'drafts') showDrafts();
    else if (kind === 'draft') await showDraft(arg);
    else await showFolder('');
    loadSideFolders();
  } catch (e) {
    if (e.status === 404 && hash.startsWith('#/n/')) {
      toast('That note is not there any more.', { bad: true });
      go('#/', { replace: true });
    } else if (e instanceof Offline) {
      app.innerHTML = empty(ICON.cloud, "Can't reach leo", 'Check that `leo serve` is still running on your computer, then pull down or tap Refresh in the menu.');
    } else {
      fail(e);
    }
  }
}

window.addEventListener('online', () => saving.retry());
route();
checkActivity();

document.addEventListener('paste', (e) => {
  if (e.defaultPrevented) return;
  const files = [...((e.clipboardData && e.clipboardData.files) || [])];
  if (!files.length) return;
  if ($('#upload-list')) {
    e.preventDefault();
    picked.push(...pastedFiles(files, picked.length));
    drawPicked();
    return;
  }
  if ($('.scrim') || e.target.closest('#chat')) return;
  if (state.view === 'note' && state.session && files.some((f) => /^image\//.test(f.type))) {
    e.preventDefault();
    addPictures(state.session, files);
    return;
  }
  if (!['folder', 'search', 'drafts'].includes(state.view)) return;
  e.preventDefault();
  uploadSheet(pastedFiles(files));
});
