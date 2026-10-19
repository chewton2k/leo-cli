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
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'j') {
    e.preventDefault();
    if ($('.scrim:not(.scrim-gone) .quick-leo')) closeSheet();
    else if (state.view !== 'locked') quickLeo();
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && state.view === 'note') {
    e.preventDefault();
    flush().catch(fail);
    return;
  }
  if (e.key === 'Escape') {
    if ($('.scrim')) return closeSheet();
    if (source.note && sourcePane.contains(e.target)) return closeSource();
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

const unsaved = () => saving.unsaved() || Boolean(interviewPage && interviewHere(interviewPage) && (interviewPage.generation !== interviewPage.saved || interviewPage.name != null));
let websiteLeaveSent = false;
let websiteBackupShown = 0;

function saveWebsiteWork() {
  saving.retry().catch(() => {});
  if (interviewPage && interviewHere(interviewPage)) saveInterviewWorkspace(interviewPage).catch(() => {});
}

function leaveWebsite() {
  saveWebsiteWork();
  if (websiteLeaveSent) return;
  websiteLeaveSent = true;
  const body = new Blob(['{}'], { type: 'application/json' });
  try {
    if (navigator.sendBeacon('/api/backup', body)) return;
  } catch (_) {}
  fetch('/api/backup', { method: 'POST', credentials: 'same-origin', body, keepalive: true }).catch(() => {});
}

async function checkWebsiteBackup() {
  try {
    const backup = await api('/api/backup');
    if (backup.state === 'failed' && backup.at !== websiteBackupShown) {
      websiteBackupShown = backup.at;
      toast(backup.message, { bad: true });
    }
  } catch (_) {}
}

window.addEventListener('pagehide', leaveWebsite);
window.addEventListener('pageshow', () => { websiteLeaveSent = false; });

window.addEventListener('beforeunload', (e) => {
  if (!unsaved()) return;
  saveWebsiteWork();
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
    leaveWebsite();
    return;
  }
  websiteLeaveSent = false;
  checkWebsiteBackup();
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

const activity = { tasks: [], timer: 0, folded: false, route: '', seen: new Map() };

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
    dock().appendChild(box);
  }
  const shown = activity.tasks.filter((t) => !hiddenHere(t));
  box.hidden = !shown.length;
  if (!shown.length) return;
  box.classList.toggle('folded', activity.folded);
  const now = Date.now();
  const rows = shown
    .map((t) => {
      const seen = activity.seen.get(t.label) || { since: now, step: `${t.done}/${t.total}`, stepSince: now, steps: [] };
      if (seen.step !== `${t.done}/${t.total}`) {
        seen.steps = [...seen.steps, (now - seen.stepSince) / 1000].slice(-6);
        seen.step = `${t.done}/${t.total}`;
        seen.stepSince = now;
      }
      activity.seen.set(t.label, seen);
      const typical = seen.steps.length ? seen.steps.reduce((a, b) => a + b, 0) / seen.steps.length : 20;
      const creep = 1 - Math.exp(-((now - seen.stepSince) / 1000) / Math.max(3, typical));
      const share = t.total ? Math.max(4, Math.min(99, Math.round(((Math.min(t.done, t.total) + (t.done < t.total ? 0.9 * creep : 0)) / t.total) * 100))) : 0;
      const took = Math.round((now - seen.since) / 1000);
      const count = t.ready ? '' : `${t.kind === 'combine' ? ` · ${t.done}%` : t.total ? ` · ${t.done}/${t.total}` : ''} · ${took >= 60 ? `${Math.floor(took / 60)}:${String(took % 60).padStart(2, '0')}` : `${took} s`}`;
      return `<button class="activity-row" data-action="activity-open" data-href="${esc(t.href)}">
        <span class="activity-label">${esc(t.label)}</span>
        <span class="activity-step">${esc(t.step)}${count}</span>
        ${t.ready ? '' : `<span class="activity-bar${t.total ? '' : ' busy'}"><i style="width:${share}%"></i></span>`}
      </button>`;
    })
    .join('');
  const settled = shown.every((t) => t.ready);
  box.innerHTML = `<button class="activity-head" data-action="activity-fold" aria-expanded="${!activity.folded}">${settled ? '' : '<span class="activity-spin"></span>'}<span class="grow">${settled ? (shown.length === 1 ? 'Ready in the background' : `${shown.length} ready in the background`) : shown.length === 1 ? 'Working in the background' : `${shown.length} things in the background`}</span><span class="activity-fold">${activity.folded ? 'Show' : 'Hide'}</span></button>${activity.folded ? '' : rows}`;
}

async function checkActivity() {
  clearTimeout(activity.timer);
  try {
    activity.tasks = (await api('/api/activity')).tasks;
    for (const label of [...activity.seen.keys()]) if (!activity.tasks.some((t) => t.label === label)) activity.seen.delete(label);
  } catch (e) {
    if (e instanceof Locked) return;
  }
  drawActivity();
  for (const t of activity.tasks) if (t.kind === 'combine' && !t.ready) watchCombine(t.href.slice('#combine/'.length));
  if (activity.tasks.some((t) => !t.ready)) activity.timer = setTimeout(checkActivity, 1000);
}

function closedHeight(box, summary) {
  const outer = getComputedStyle(box);
  const inner = getComputedStyle(summary);
  return summary.getBoundingClientRect().height
    + parseFloat(inner.marginTop) + parseFloat(inner.marginBottom)
    + parseFloat(outer.paddingTop) + parseFloat(outer.paddingBottom)
    + parseFloat(outer.borderTopWidth) + parseFloat(outer.borderBottomWidth);
}

document.addEventListener('click', (e) => {
  if (e.defaultPrevented || e.button !== 0 || stillMotion()) return;
  const summary = e.target.closest ? e.target.closest('summary') : null;
  const box = summary && summary.parentElement;
  if (!box || box.tagName !== 'DETAILS' || box.querySelector(':scope > summary') !== summary) return;
  const control = e.target.closest('a, button, input, select, textarea, label');
  if (control && control !== summary && summary.contains(control)) return;
  e.preventDefault();
  const from = box.getBoundingClientRect().height;
  stopSlide(box);
  if (box.open && !box.classList.contains('closing')) {
    box.classList.add('closing');
    slideHeight(box, from, closedHeight(box, summary), () => {
      box.classList.remove('closing');
      box.open = false;
    });
  } else {
    box.classList.remove('closing');
    box.open = true;
    slideHeight(box, from, box.getBoundingClientRect().height);
  }
});

async function render() {
  hideAskFloat();
  interviewPage?.codeResize?.disconnect();
  clearTimeout(cal.timer);
  if (!/^#\/n\//.test(location.hash)) closeSource();
  document.body.classList.toggle('on-calendar', /^#\/calendar/.test(location.hash));
  document.body.classList.toggle('on-interviews', /^#\/interviews(?:\/|$)/.test(location.hash));
  const hash = decodeURI(location.hash || '#/');
  const [, kind, rest = ''] = location.hash.match(/^#\/([a-z]*)\/?(.*)$/) || [null, '', ''];
  const arg = decodeURIComponent(rest);
  stopLibraryArrival();
  document.body.classList.toggle('on-library', !kind || kind === 'f');
  document.body.classList.toggle('on-note', kind === 'n' || kind === 'new');
  if (library.observer) library.observer.disconnect();
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
    else if (kind === 'calendar') await showCalendar();
    else if (kind === 'interviews') await showInterviews(arg);
    else if (kind === 'settings') await (arg === 'storage' ? showStorage() : showSettings());
    else if (kind === 'record') await showRecord(arg);
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
window.addEventListener('beforeprint', () => {
  for (const fold of document.querySelectorAll('details.callout:not([open])')) {
    fold.dataset.printed = '1';
    fold.open = true;
  }
});
window.addEventListener('afterprint', () => {
  for (const fold of document.querySelectorAll('details.callout[data-printed]')) {
    fold.open = false;
    delete fold.dataset.printed;
  }
});
route();
checkActivity();
checkWebsiteBackup();

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
  if (!['folder', 'search'].includes(state.view)) return;
  e.preventDefault();
  uploadSheet(pastedFiles(files));
});
