const actions = {
  back,
  home: () => go('#/'),
  'side-fold': foldSide,
  'toggle-picker': togglePicker,
  'set-model': (el) => changeSetting({ set: 'model', task: el.dataset.task, value: el.dataset.model }),
  'set-effort': (el) => changeSetting({ set: 'effort', value: el.dataset.effort === 'default' ? '' : el.dataset.effort }),
  'side-twist': (el) => twistFolder(el.dataset.dir),
  'open-title': (el) => openTitle(el.dataset.title || ''),
  'side-search': searchEverywhere,
  'toggle-search': () => {
    if ($('#search').hidden) {
      openSearch('');
      $('#search-input').focus();
    } else {
      closeSearch();
      if (state.view === 'search') back();
    }
  },
  menu,
  'calendar-add': () => addCalendar().catch(fail),
  'calendar-remove': (el) => removeCalendar(el).catch(fail),
  'calendar-refresh': () => refreshCalendar().catch(fail),
  'calendar-more': () => {
    state.calendarAdding = true;
    drawSettings(state.settings);
    const box = $('#calendar-link');
    if (box) box.focus();
  },
  'calendar-cancel': () => {
    state.calendarAdding = false;
    drawSettings(state.settings);
  },
  'note-sources': () => showRecordingSources(window.getSelection().toString().trim().slice(0,200)).catch(fail),
  'source-save': () => saveSource().catch(fail),
  'source-regenerate': () => regenerateSource().catch(fail),
  'source-back': () => drawRecordingSources(),
  'source-apply': () => applyGenerated().catch(fail),
  'rec-finish': () => recorder.finishAvailable().catch(fail),
  'rec-snapshot': () => recorder.snapshot().catch(fail),
  'rec-recover': (el) => recorder.recover(el.dataset.id).catch(fail),
  close: closeSheet,
  'open-folder': (el) => {
    closeSheet();
    go(folderHash(el.dataset.dir));
  },
  'open-note': (el) => go(noteHash(el.dataset.id)),
  'pin-card': pinCard,
  new: (el) => {
    closeSheet();
    go(el.dataset.dir ? `#/new/${enc(el.dataset.dir)}` : '#/new');
  },
  'new-folder': newFolder,
  'create-folder': createFolder,
  map: () => {
    closeSheet();
    go('#/map');
  },
  settings: () => {
    closeSheet();
    go('#/settings');
  },
  storage: () => go('#/settings/storage'),
  'storage-act': storageAsk,
  'storage-go': () => storageGo().catch(fail),
  'session-end': sessionAsk,
  'session-end-others': sessionAsk,
  'session-end-now': () => sessionEndNow().catch(fail),
  'session-new-link': newLinkAsk,
  'session-new-link-now': () => newLinkNow().catch(fail),
  'copy-link': () => copyLink(),
  'keep-now': () => {
    const next = state.keepPending;
    state.keepPending = null;
    closeSheet();
    if (next) saveKeep(next).catch(fail);
  },
  'keep-cancel': () => closeSheet(),
  'set-key': (el) => saveKey(el.dataset.account),
  'remove-key': confirmRemoveKey,
  'remove-key-now': (el) => {
    closeSheet();
    return changeSetting({ set: 'key', account: el.dataset.account, value: null });
  },
  'test-ai': testAi,
  'note-map': (el) => go(`#/map/${enc(el.dataset.id)}`),
  'note-picture': () => choosePictures(),
  'view-original': (el) => viewOriginal(el.dataset.id, el.dataset.name),
  'viewer-zoom': (el) => setViewerZoom(Number(el.dataset.step) === 0 ? 1 : state.viewerZoom * (Number(el.dataset.step) > 0 ? 1.5 : 1 / 1.5)),
  record: (el) => {
    closeSheet();
    go(el.dataset.dir ? `#/record/${enc(el.dataset.dir)}` : '#/record');
  },
  'rec-start': () => recorder.begin().catch(fail),
  'rec-pause': () => recorder.pause().catch(fail),
  'rec-stop': () => recorder.stop().then(checkActivity).catch(fail),
  'activity-open': (el) => go(el.dataset.href),
  'activity-fold': () => {
    activity.folded = !activity.folded;
    drawActivity();
  },
  'rec-rejoin': () => recorder.rejoin().catch(fail),
  'rec-point': () => recorder.point(),
  'rec-again': () => recorder.again(),
  upload: () => uploadSheet().catch(fail),
  'upload-go': () => uploadGo().catch(fail),
  'upload-drop': (el) => {
    picked.splice(Number(el.dataset.i), 1);
    drawPicked();
  },
  chat: () => chat.toggle(),
  'map-build': () => mapBuild(false),
  'map-build-now': () => mapBuild(true),
  'map-rebuild': mapRebuildAsk,
  'map-rebuild-now': () => mapRebuild().catch(fail),
  'map-select': (el) => mapView && mapView.select(el.dataset.id, { center: true }),
  'map-pick': (el) => mapView && mapPick(el.dataset.id),
  'map-clear': () => mapView && mapView.select(null),
  'map-fit': () => mapView && mapView.fit(),
  'map-tidy': () => {
    if (!mapView) return;
    const { before, after } = mapView.tidy();
    toast(after < before ? `Tidied up: ${before - after} fewer crossing ${before - after === 1 ? 'line' : 'lines'}.` : 'Tidied up.');
  },
  'map-zoom-in': () => mapView && mapView.zoomBy(1.35),
  'map-zoom-out': () => mapView && mapView.zoomBy(1 / 1.35),
  'map-across': () => mapView && mapView.setOptions({ crossOnly: !mapView.options().crossOnly }),
  'map-ideas': () => mapView && mapView.setOptions({ ideas: !mapView.options().ideas }),
  'map-unfocus': () => {
    if (!mapView) return;
    mapView.setOptions({ focus: null });
    const chosen = mapView.selected();
    if (chosen) mapPanel(mapView.graph.byId.get(chosen));
  },
  'map-focus': (el) => {
    if (!mapView) return;
    const on = mapView.options().focus === el.dataset.id;
    mapView.setOptions({ focus: on ? null : el.dataset.id, depth: 2 });
    mapView.select(el.dataset.id);
  },
  'map-class': (el) => {
    if (!mapView) return;
    const hidden = mapView.options().hidden;
    if (hidden.has(el.dataset.top)) hidden.delete(el.dataset.top);
    else hidden.add(el.dataset.top);
    mapView.setOptions({ hidden });
  },
  'map-sheet': () => mapSheetTo(mapSheet === 'open' ? 'peek' : 'open'),
  trash: () => {
    closeSheet();
    go('#/trash');
  },
  refresh: () => {
    closeSheet();
    saving.retry();
    route();
  },
  move: () => moveSheet(),
  'move-to': (el) => moveTo(el.dataset.dir),
  share,
  delete: confirmDelete,
  'delete-now': deleteNow,
  restore: (el) => restore(el.dataset.id, false),
  'folder-select': () => {
    state.selecting = !state.selecting;
    state.picked = new Set();
    drawFolder();
  },
  'folder-pick': (el) => {
    const key = el.dataset.key;
    const focused = document.activeElement === el;
    if (state.picked.has(key)) state.picked.delete(key);
    else state.picked.add(key);
    drawFolder();
    if (focused) {
      const again = app.querySelector(`[data-key="${CSS.escape(key)}"]`);
      if (again) again.focus();
    }
  },
  'folder-trash': folderTrashAsk,
  'folder-trash-now': () => folderTrashNow().catch(fail),
  'trash-select': () => {
    state.selecting = !state.selecting;
    state.picked = new Set();
    drawTrash();
  },
  'trash-empty': () => trashAsk([], true),
  'trash-forget': (el) => trashAsk([el.dataset.id], false),
  'trash-delete-picked': () => state.picked.size && trashAsk([...state.picked], false),
  'trash-delete-now': () => trashDeleteNow().catch(fail),
  'trash-restore-picked': () => trashRestorePicked().catch(fail),
};
