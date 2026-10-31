function nextOnboarding(preferences) {
  return (preferences.onboarding || [false, false, false]).findIndex((done) => !done);
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
  const titles = ['Your microphone', 'Type into other apps', 'Choose a speech model'];
  const descriptions = ['Leo opens the microphone only while you capture speech. Allow microphone access when your system asks.', 'Accessibility lets Leo paste your words into the app you are using. You can still dictate without it and paste from the clipboard yourself.', 'Dictation and recording share one local model. Pick a model to download; you can change it later in Settings.'];
  const action = step === 0 ? 'microphone' : 'accessibility';
  const controls = step < 2 ? `<button class="btn primary" data-action="app-command" data-app-action="${action}">${step === 0 ? 'Allow microphone' : 'Allow Accessibility'}</button><span class="hint" id="onboarding-permission-status"></span>` : '<div id="onboarding-models"><p class="hint">Opening the model library…</p></div>';
  sheet(`<div data-onboarding-step="${step}"><p class="hint">Set up Leo · ${step + 1} of 3</p><h3>${titles[step]}</h3><p>${descriptions[step]}</p><div class="buttons">${controls}</div><p class="hint">You can skip this and finish setup in Health.</p><div class="buttons"><button class="btn plain" data-action="onboarding-next" data-step="${step}">Skip for now</button>${step < 2 ? `<button class="btn primary" id="onboarding-continue" data-action="onboarding-next" data-step="${step}" disabled>Continue</button>` : ''}</div></div>`);
  updateOnboardingPermissions();
  if (step === 2) loadOnboardingModels(++onboardingGeneration);
}
function updateOnboardingPermissions() {
  if (!desktopApp) return;
  const microphone = Boolean(onboardingHere(0));
  const ready = microphone ? desktopApp.permissions?.microphone === 'allowed' : desktopApp.permissions?.accessibility === true;
  const status = $('#onboarding-permission-status');
  if (status) status.textContent = ready ? 'Allowed' : 'You can allow access or skip for now.';
  const next = $('#onboarding-continue');
  if (next) next.disabled = !ready;
}
async function finishOnboarding(el) {
  if (onboardingFinishing) return;
  const generation = ++onboardingGeneration, step = Number(el.dataset.step);
  onboardingFinishing = true;
  try {
    await appChange({ action: 'finish_step', step });
    if (generation !== onboardingGeneration || !onboardingHere(step)) return;
    closeSheet(true);
    maybeOnboarding(true);
  } finally { onboardingFinishing = false; }
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
