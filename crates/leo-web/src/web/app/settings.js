const TASK_TITLE = { writing: 'AI for writing', speech: 'AI for speech' };
const TASK_USE = {
  writing: 'Turns recordings into notes, answers @leo questions, powers Felix and the knowledge graph.',
  speech: 'Turns what was said into text while you record.',
};

async function showSettings() {
  const mine = ++seq;
  state = { view: 'settings', dir: '' };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Settings</button>';
  document.title = 'Settings · leo';
  app.innerHTML = skeleton(3);
  const page = await api('/api/settings');
  if (mine !== seq) return;
  drawSettings(page);
}

function settingsOption(value, label, current) {
  return `<option value="${esc(value)}"${value === current ? ' selected' : ''}>${esc(label)}</option>`;
}

function taskCard(t, secure) {
  const status = t.ready ? '<span class="set-status ok">Ready</span>' : '<span class="set-status missing">Not set up</span>';
  const providers = t.choices.map((c) => settingsOption(c.id, c.label, t.provider)).join('') + (t.custom ? settingsOption(t.provider, `${t.provider} (from config.toml)`, t.provider) : '');
  let model = '';
  if (t.fixed_model) {
    model = `<div class="set-row"><span class="set-label">Model</span><span class="set-value">${esc(t.model || 'Built in')} <span class="hint">free, runs on your computer</span></span></div>`;
  } else if (t.models.length) {
    const known = t.models.some((m) => m.id === t.model);
    const options = (known || !t.model ? '' : settingsOption(t.model, `${t.model} (not in the list)`, t.model)) + t.models.map((m) => settingsOption(m.id, `${m.id} — ${m.price}`, t.model)).join('');
    model = `<label class="set-row"><span class="set-label">Model</span><select data-set="model" data-task="${t.task}">${options}</select></label>`;
  }
  let key = '';
  if (t.key) {
    const has = t.key.stored;
    const form = secure
      ? `<div class="set-key-form"><input type="password" autocomplete="off" spellcheck="false" placeholder="${has ? 'Paste a new key to replace it' : `Paste your ${esc(t.key.name)} API key`}" data-key-input="${esc(t.key.account)}"><button class="btn primary sm" data-action="set-key" data-account="${esc(t.key.account)}">${has ? 'Replace' : 'Save key'}</button></div>`
      : '<p class="hint">To add a key, open the https link leo serve printed (not the Wi-Fi one), or use this page on your computer. Keys never cross Wi-Fi unencrypted.</p>';
    key = `<div class="set-row column"><div class="set-line"><span class="set-label">${esc(t.key.name)} key</span><span class="set-value">${has ? '<span class="set-status ok">Stored on your computer</span>' : '<span class="set-status missing">Not added</span>'}${has ? `<button class="btn plain sm" data-action="remove-key" data-account="${esc(t.key.account)}" data-name="${esc(t.key.name)}">Remove</button>` : ''}</span></div>${form}${t.key.ignored ? `<p class="hint">leo does not read $${esc(t.key.ignored)}; add the key here instead.</p>` : ''}</div>`;
  }
  const signin = t.signin ? `<p class="set-note${t.signin.installed ? '' : ' warn'}">${esc(t.signin.text)}</p>` : '';
  const usage = t.usage ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
  const note = t.note ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
  return `<section class="set-card" data-task-card="${t.task}">
    <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
    <p class="hint">${TASK_USE[t.task]}</p>
    <label class="set-row"><span class="set-label">Provider</span><select data-set="provider" data-task="${t.task}">${providers}</select></label>
    ${model}${key}${signin}${usage}${note}
    <div class="set-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test</button><span class="set-result" id="test-${t.task}"></span></div>
  </section>`;
}

function drawSettings(page) {
  state.settings = page;
  const backup = page.backup;
  app.innerHTML = `<div class="settings">
    <div class="section-title">Settings</div>
    ${page.tasks.map((t) => taskCard(t, page.secure)).join('')}
    <section class="set-card">
      <header><h3>Backup</h3>${backup.remote ? '<span class="set-status ok">On</span>' : '<span class="set-status missing">Not set up</span>'}</header>
      <p class="hint">${backup.remote ? `Your notes are copied to ${esc(backup.remote)}.` : 'Backup is not set up yet. Run :backup in leo on your computer to keep a copy on GitHub.'}</p>
      <label class="set-row"><span class="set-label">Push changes</span><select data-set="auto_push">${backup.options.map((o) => settingsOption(o.id, o.label, backup.auto_push)).join('')}</select></label>
    </section>
    <section class="set-card">
      <header><h3>Where things are</h3></header>
      <div class="set-row column"><span class="set-label">Notes</span><code class="set-path">${esc(page.paths.notes || '')}</code></div>
      <div class="set-row column"><span class="set-label">Settings file</span><code class="set-path">${esc(page.paths.config || '')}</code></div>
      <p class="hint">Changes here save straight away and are the same settings as :settings in leo. Keys are kept in a file only your account can read, and are never shown again.</p>
    </section>
    <section class="set-card">
      <header><h3>Advanced</h3></header>
      <button class="list-row set-advanced" data-action="storage">${ICON.folder}<span class="grow">Storage and data<span class="sub">See what leo keeps on this computer, how much space it takes, and delete what you no longer need</span></span>${ICON.chevron}</button>
    </section>
  </div>`;
}

async function changeSetting(change) {
  try {
    const done = await api('/api/settings', { method: 'POST', body: change });
    drawSettings(done.settings);
    toast(done.message);
  } catch (e) {
    if (state.settings) drawSettings(state.settings);
    throw e;
  }
}

app.addEventListener('change', (e) => {
  const el = e.target.closest('select[data-set]');
  if (!el || state.view !== 'settings') return;
  const change = { set: el.dataset.set, value: el.value };
  if (el.dataset.task) change.task = el.dataset.task;
  changeSetting(change).catch(fail);
});

app.addEventListener('keydown', (e) => {
  const input = e.target.closest('[data-key-input]');
  if (input && e.key === 'Enter') {
    e.preventDefault();
    saveKey(input.dataset.keyInput).catch(fail);
  }
});

async function saveKey(account) {
  const input = app.querySelector(`[data-key-input="${CSS.escape(account)}"]`);
  const value = input ? input.value.trim() : '';
  if (!value) return toast('Paste the key first.', { bad: true });
  input.value = '';
  await changeSetting({ set: 'key', account, value });
}

function confirmRemoveKey(el) {
  sheet(`<h3>Remove the ${esc(el.dataset.name)} key?</h3>
    <p>leo stops using ${esc(el.dataset.name)} until you add a key again.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn danger" data-action="remove-key-now" data-account="${esc(el.dataset.account)}">Remove</button></div>`);
}

async function testAi(el) {
  const out = $(`#test-${el.dataset.task}`);
  el.disabled = true;
  out.className = 'set-result';
  out.textContent = 'Asking…';
  try {
    const done = await api('/api/settings/test', { method: 'POST', body: { task: el.dataset.task } });
    out.className = 'set-result ok';
    out.textContent = done.message;
  } catch (e) {
    out.className = 'set-result bad';
    out.textContent = e.message;
  } finally {
    el.disabled = false;
  }
}
