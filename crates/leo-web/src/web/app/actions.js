const actions = {
  'app-command': (el) => appChange({action:el.dataset.appAction}),
  'health-refresh': loadHealth,
  'backup-setup': setupBackup,
  'onboarding-next': finishOnboarding,
  'onboarding-model': onboardingModel,
  'onboarding-skip': skipOnboarding,
  'onboarding-back': onboardingBack,
  'onboarding-pick': pickOnboardingModel,
  'onboarding-models': showOnboardingModels,
  'dictation-record': () => dictationChange({action:'record_shortcut'}),
  'dictation-resume': () => dictationChange({action:'resume_shortcut'}),
  'dictation-use-shortcut': useDictationShortcut,
  'dictation-clear': () => dictationChange({action:'clear'}),
  'dictation-copy': (el) => dictationChange({action:'copy',id:Number(el.dataset.id)}),
  'speech-filter': filterSpeech,
  'speech-model': changeSpeechModel,
  'speech-delete': (el) => { closeSheet(); return speechChange({ action: el.dataset.verb || 'delete', id: el.dataset.model }); },
  back,
  home: () => go('#/'),
  interviews: () => go('#/interviews'),
  'side-fold': foldSide,
  'toggle-picker': togglePicker,
  'ask-selection': () => askAboutSelection(),
  calendar: () => {
    closeSheet();
    go('#/calendar');
  },
  boards: () => {
    closeSheet();
    go('#/boards');
  },
  'kanban-new-board': () => kanbanNewBoard(),
  'kanban-board-menu': () => kanbanBoardMenu(),
  'kanban-column-menu': (el) => kanbanColumnMenu(el.dataset.col),
  'kanban-add-card': (el) => kanbanComposer(el.dataset.col),
  'kanban-add-column': () => kanbanAddColumn(),
  'kanban-open-card': (el) => kanbanCardPanel(el.dataset.card),
  'cal-pick': (el) => {
    cal.selected = el.dataset.day;
    if (el.dataset.to) calSetView(el.dataset.to);
    calGo();
  },
  'cal-prev': () => calStep(-1),
  'cal-next': () => calStep(1),
  'cal-today': () => {
    cal.selected = S.dayKey(new Date());
    calGo();
  },
  'cal-view': (el) => calSetView(el.dataset.view),
  'cal-mini': (el) => {
    const at = S.fromKey(cal.selected);
    const m = new Date(at.getFullYear(), at.getMonth() + Number(el.dataset.step), 1);
    cal.selected = S.dayKey(m);
    calGo();
  },
  'cal-open': (el) => calOpenPeek(el.dataset.kind, el.dataset.id, el.dataset.on || null),
  'cal-peek-close': () => calClosePeek(),
  'cal-retry': () => {
    cal.polls = 0;
    return api('/api/calendar/refresh', { method: 'POST' }).catch(() => null).then(() => loadSchedule(seq));
  },
  'cal-done': () => {
    cal.showDone = !cal.showDone;
    try {
      localStorage.setItem(CAL_DONE_KEY, cal.showDone ? '0' : '1');
    } catch (e) {
      cal.showDone = Boolean(cal.showDone);
    }
    drawCalendar();
  },
  'task-new': (el) => newTaskSheet(el.dataset.day || null),
  'task-open': (el) => taskSheet(el.dataset.id, el.dataset.on || null),
  'task-open-here': (el) => openTaskAnywhere(el.dataset.id, el.dataset.on || null),
  'task-done': (el) => {
    const id = el.dataset.id;
    const on = el.dataset.on || null;
    const task = calFind(id);
    const done = el.dataset.set ? el.dataset.set === '1' : !(task && taskDoneOn(task, on));
    if (el.dataset.set) closeSheet();
    return setTaskDone(id, done, on).then(() => {
      for (const box of document.querySelectorAll(`.cal-check[data-id="${CSS.escape(id)}"]${on ? `[data-on="${CSS.escape(on)}"]` : ''}`)) box.checked = done;
    });
  },
  'task-date': (el) => dateSheet(el.dataset.id),
  'task-google': (el) => addToGoogle(el.dataset.id),
  'task-delete': (el) => deleteTask(el.dataset.id),
  'task-promote': (el) => promoteTask(el.dataset.line),
  'note-tasks': () => noteTasksSheet(),
  'tasks-fold': () => toggleTasksWindow(),
  'source-close': closeSource,
  'source-file': (el) => openSource(el.dataset.id, el.dataset.name),
  'set-model': (el) => changeSetting({ set: 'model', task: el.dataset.task, value: el.dataset.model }),
  'set-effort': (el) => changeSetting({ set: 'effort', value: el.dataset.effort === 'default' ? '' : el.dataset.effort }),
  'side-twist': (el) => twistFolder(el.dataset.dir, el),
  'side-collapse': () => collapseFolders(),
  'side-notes': (el) => loadSideNotes(el.dataset.dir, Number(el.dataset.offset || 0), true),
  'open-title': (el) => openTitle(el.dataset.title || ''),
  'toggle-search': () => toggleQuickLeo(),
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
  'rec-recover': (el) => recorder.recover(el.dataset.id).catch(fail),
  close: closeSheet,
  'open-folder': openFolder,
  'open-note': (el) => go(noteHash(el.dataset.id)),
  'pin-card': pinCard,
  'library-view': changeLibraryView,
  'theme-pick': (el) => pickTheme(el),
  'segment-pick': (el) => pickSegment(el),
  'felix-pick': (el) => pickFelix(el),
  'width-pick': (el) => pickWidth(el),
  'usage': () => showUsage(),
  'quick-leo': () => quickLeo(),
  'quick-run': (el) => quickGo(Number(el.dataset.at)),
  'quick-done': (el) => quickDone(el),
  'library-show-all': () => changeLibrary('show', 'all'),
  'tasks-restore-go': () => tasksRestoreGo().catch(fail),
  new: (el) => {
    closeSheet();
    go(el.dataset.dir ? `#/new/${enc(el.dataset.dir)}` : '#/new');
  },
  'new-folder': newFolder,
  'folder-options': folderOptions,
  'folder-rename': renameFolderSheet,
  'folder-rename-save': renameFolder,
  'folder-delete': deleteFolderAsk,
  'folder-delete-now': deleteFolderNow,
  'folder-root': () => { const from = state.dir; closeSheet(); return dropFolder(from, ''); },
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
  'storage-more': storageMore,
  'storage-open-chat': storageOpenChat,
  'storage-go': () => storageGo().catch(fail),
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
  'use-plan': (el) => changeSetting({ set: 'provider', task: 'writing', value: el.dataset.provider }),
  'note-map': (el) => go(`#/map/${enc(el.dataset.id)}`),
  'note-picture': () => choosePictures(),
  'view-original': (el) => {
    el.focus({ preventScroll: true });
    return viewOriginal(el.dataset.id, el.dataset.name);
  },
  'viewer-zoom': (el) => setViewerZoom(Number(el.dataset.step) === 0 ? 1 : state.viewerZoom * (Number(el.dataset.step) > 0 ? 1.5 : 1 / 1.5)),
  record: (el) => {
    closeSheet();
    go(el.dataset.dir ? `#/record/${enc(el.dataset.dir)}` : '#/record');
  },
  'rec-start': () => recorder.begin().catch(fail),
  'rec-pause': () => recorder.pause().catch(fail),
  'rec-stop': () => recorder.stop().then(checkActivity).catch(fail),
  'activity-open': (el) => (el.dataset.href.startsWith('#combine/') ? reviewCombine(el.dataset.href.slice('#combine/'.length)).catch(fail) : go(el.dataset.href)),
  'activity-fold': () => {
    activity.folded = !activity.folded;
    drawActivity();
  },
  'rec-rejoin': () => recorder.rejoin().catch(fail),
  'rec-point': () => recorder.point(),
  'rec-dismiss': () => recorder.dismiss(),
  'rec-discard': (el) => discardRecordingAsk(el),
  'rec-discard-now': (el) => discardRecordingNow(el),
  'rec-again': () => recorder.again(),
  upload: (el) => uploadSheet(undefined, el.dataset.dir, { add: el.dataset.add === 'yes' }).catch(fail),
  'upload-go': () => uploadGo().catch(fail),
  'upload-drop': (el) => {
    picked.splice(Number(el.dataset.i), 1);
    drawPicked();
  },
  chat: async () => {
    if (chat.isOpen()) return chat.toggle(false);
    const session = interviewPage;
    if (session && interviewHere(session)) {
      await saveInterviewWorkspace(session);
      if (interviewHere(session)) return chat.openInterview(session.item.id, () => interviewHere(session));
      return;
    }
    chat.toggle(true);
  },
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
