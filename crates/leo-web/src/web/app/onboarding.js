function nextOnboarding(preferences) {
  const done = preferences.onboarding || [false, false, false];
  return !done[0] || !done[1] ? 0 : !done[2] ? 2 : -1;
}
let onboardingShown = false;
let onboardingGeneration = 0;
let onboardingSpeech = null;
let onboardingChoice = '';
let onboardingFinishing = false;
function onboardingHere(step) { return document.querySelector(`[data-onboarding-step="${step}"]`); }
function maybeOnboarding(force = false) {
  if (!desktopApp || !desktopApp.preferences || !force && onboardingShown || document.querySelector('.scrim')) return;
  const step = nextOnboarding(desktopApp.preferences);
  if (step < 0) return;
  onboardingShown = true;
  const permissions = step === 0;
  const title = permissions ? 'Allow access' : 'Choose a speech model';
  const description = permissions ? 'Give Leo access for dictation and recording. Each permission is checked here as soon as it is allowed.' : 'Dictation and recording share one local model. Pick a model to download; you can change it later in Settings.';
  const row = (action, name, why, label) => `<div class="set-row onboarding-permission" data-onboarding-permission="${action}"><div class="grow"><strong>${name}</strong><p class="hint">${why}</p></div><div class="onboarding-permission-action"><button class="btn primary sm" data-action="app-command" data-app-action="${action}">${label}</button><span role="status" class="set-status" hidden></span></div></div>`;
  const controls = permissions ? `<section class="set-card">${row('microphone', 'Microphone', 'Leo opens your microphone only while you capture speech.', 'Allow microphone')}${desktopApp.platform === 'windows' ? '' : row('accessibility', 'Accessibility', 'Paste dictated words into the app you are using. Without access, you can paste from the clipboard yourself.', 'Allow Accessibility')}</section>` : '<div id="onboarding-models"><p class="hint">Opening the model library…</p></div>';
  sheet(`<div data-onboarding-step="${step}"><p class="hint">Set up Leo · ${permissions ? 1 : 2} of 2</p><h3>${title}</h3><p>${description}</p>${controls}<p class="hint">You can skip this and finish setup in Health.</p><div class="buttons"><button class="btn plain" data-action="onboarding-next" data-step="${step}">Skip for now</button>${permissions ? `<button class="btn primary" id="onboarding-continue" data-action="onboarding-next" data-step="${step}" disabled>Continue</button>` : ''}</div></div>`);
  updateOnboardingPermissions();
  if (step === 2) loadOnboardingModels(++onboardingGeneration);
}
function updateOnboardingPermissions() {
  if (!desktopApp) return;
  const microphone = desktopApp.permissions?.microphone === 'allowed';
  const accessibility = desktopApp.platform === 'windows' || desktopApp.permissions?.accessibility === true;
  for (const [permission, allowed] of [['microphone', microphone], ['accessibility', accessibility]]) {
    const row = document.querySelector(`[data-onboarding-permission="${permission}"]`);
    if (!row) continue;
    row.querySelector('button').hidden = allowed;
    const status = row.querySelector('[role="status"]');
    status.hidden = !allowed;
    status.classList.toggle('ok', allowed);
    if (allowed && !status.firstChild) status.innerHTML = `${ICON.check} Allowed`;
  }
  const next = $('#onboarding-continue');
  if (next) next.disabled = !microphone || !accessibility || onboardingFinishing;
}
async function finishOnboarding(el) {
  if (onboardingFinishing) return;
  const generation = ++onboardingGeneration, step = Number(el.dataset.step);
  onboardingFinishing = true;
  updateOnboardingPermissions();
  try {
    for (const index of step < 2 ? [0, 1] : [2]) {
      await appChange({ action: 'finish_step', step: index });
      if (generation !== onboardingGeneration || !onboardingHere(step)) return;
    }
    closeSheet(true);
    maybeOnboarding(true);
  } finally { onboardingFinishing = false; updateOnboardingPermissions(); }
}
async function loadOnboardingModels(generation) {
  try {
    const view = await api('/api/speech');
    if (generation !== onboardingGeneration || !onboardingHere(2)) return;
    onboardingSpeech = view;
    if (!onboardingChoice) onboardingChoice = view.selected?.id || view.models[0]?.id || '';
    const box = $('#onboarding-models');
    if (box && !box.querySelector('select')) box.innerHTML = `<label class="field">Speech model<span class="select-control"><select id="onboarding-model">${view.models.map((model) => settingsOption(model.id, model.name + ' · ' + speechBytes(model.size), onboardingChoice)).join('')}</select>${ICON.chevron}</span></label><div class="set-row" id="onboarding-model-progress"></div>`;
    drawOnboardingProgress();
    if (view.models.some((model) => model.progress?.state === 'downloading')) setTimeout(() => loadOnboardingModels(generation), 700);
  } catch (e) { if (generation === onboardingGeneration && onboardingHere(2)) { const box = $('#onboarding-models'); if (box) box.textContent = 'Models could not be opened. Skip for now and try Speech models in Settings.'; } }
}
function drawOnboardingProgress() {
  const model = onboardingSpeech?.models.find((model) => model.id === onboardingChoice);
  const box = $('#onboarding-model-progress');
  if (!model || !box) return;
  const progress = model.progress;
  box.innerHTML = progress?.state === 'downloading' ? `<span class="grow hint">${speechBytes(progress.bytes)} of ${speechBytes(progress.total)}</span><button class="btn plain" data-action="onboarding-model" data-verb="cancel">Cancel</button>` : `<span class="grow hint">${model.languages.length === 1 ? esc(speechLanguage(model.languages[0])) : model.languages.length + ' languages'} · ${speechBytes(model.size)}</span><button class="btn primary" data-action="onboarding-model" data-verb="${model.verified ? 'use' : 'download'}">${model.verified ? 'Use and finish' : 'Download'}</button>${progress?.error ? `<p class="set-note warn">${esc(progress.error)}</p>` : ''}`;
}
async function onboardingModel(el) {
  const model = onboardingSpeech?.models.find((model) => model.id === onboardingChoice);
  if (!model) return;
  const generation = ++onboardingGeneration;
  const verb = el.dataset.verb;
  const view = await api('/api/speech', { method: 'POST', body: { action: verb, id: model.id, language: model.languages.length === 1 ? model.languages[0] : 'auto' } });
  if (generation !== onboardingGeneration || !onboardingHere(2)) return;
  onboardingSpeech = view;
  if (verb === 'use') { await finishOnboarding({ dataset: { step: '2' } }); return; }
  drawOnboardingProgress();
  if (verb === 'download') loadOnboardingModels(generation);
}
