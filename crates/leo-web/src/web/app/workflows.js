let workflowPage;
let workflowDir='';
let chosenScope={};
let sourcePage;
let generatedPreview;

function workflowOptions(list,current) { return list.map((t) => `<option value="${esc(t.id)}"${t.id===current?' selected':''}>${esc(t.name)}</option>`).join(''); }
function folderProfile(workflows,dir) {
  for (;;) { if (workflows.profiles[dir]) return {...workflows.profiles[dir]}; if (!dir) return {template:'lecture',context:'',vocabulary:[]}; dir=dir.includes('/')?dir.slice(0,dir.lastIndexOf('/')):''; }
}
async function showWorkflows() {
  const mine=++seq; state={view:'workflows',dir:''}; chrome({showBack:true}); document.title='Note workflows · leo'; app.innerHTML=skeleton(2);
  const [page,folders,calendar]=await Promise.all([api('/api/workflows'),api('/api/folders'),api('/api/calendar')]);
  if (mine!==seq) return;
  workflowPage={...page,folders,calendar}; drawWorkflows('');
}
function customRows(kind) {
  const list=workflowPage.workflows[kind];
  return list.map((item,i) => `<details class="workflow-custom" data-kind="${kind}" data-i="${i}"><summary>${esc(item.name)}</summary><label class="workflow-field">Name<input data-wf="${kind}" data-i="${i}" data-key="name" value="${esc(item.name)}" maxlength="100"></label><label class="workflow-field">Instructions<textarea data-wf="${kind}" data-i="${i}" data-key="prompt" rows="4" maxlength="12000">${esc(item.prompt)}</textarea></label><button class="btn danger" data-action="workflow-remove" data-kind="${kind}" data-i="${i}">Remove</button></details>`).join('');
}
function captureProfile() {
  if (!$('#workflow-template')) return;
  workflowPage.workflows.profiles[workflowDir]={template:$('#workflow-template').value,context:$('#workflow-context').value,vocabulary:$('#workflow-vocabulary').value.split('\n').map((s) => s.trim()).filter(Boolean)};
}
function captureCustom() { app.querySelectorAll('[data-wf]').forEach((input) => { const item=workflowPage.workflows[input.dataset.wf][Number(input.dataset.i)]; if (item) item[input.dataset.key]=input.value; }); }
function drawWorkflows(dir) {
  const opened=new Set([...app.querySelectorAll('.workflow-custom[open]')].map((x) => `${x.dataset.kind}:${x.dataset.i}`));
  const active=document.activeElement;
  const focus=active && app.contains(active) ? {id:active.id,wf:active.dataset.wf,i:active.dataset.i,key:active.dataset.key,start:active.selectionStart,end:active.selectionEnd} : null;
  workflowDir=dir;
  const p=folderProfile(workflowPage.workflows,dir); const formats=[...workflowPage.templates,...workflowPage.workflows.templates]; const c=workflowPage.calendar;
  app.innerHTML=`<div class="settings workflows"><div class="section-title">Note workflows</div>
    <section class="set-card"><header><h3>Folder defaults</h3></header><p class="hint">Subfolders inherit these choices. Leo keeps its explanations, gap-filling and merging of your points.</p>
      <label class="set-row"><span>Folder</span><select id="workflow-dir"><option value="">Default for all folders</option>${workflowPage.folders.map((f) => `<option value="${esc(f.name)}"${f.name===dir?' selected':''}>${esc(f.name)}</option>`).join('')}</select></label>
      <label class="set-row"><span>Note format</span><select id="workflow-template">${workflowOptions(formats,p.template || 'lecture')}</select></label>
      <label class="workflow-field">Context<textarea id="workflow-context" rows="4" maxlength="8000" placeholder="Course, project, participants or background">${esc(p.context)}</textarea></label>
      <label class="workflow-field">Vocabulary<textarea id="workflow-vocabulary" rows="3" placeholder="Names and technical terms, one per line">${esc(p.vocabulary.join('\n'))}</textarea></label><p class="hint">Terms guide note writing and speech providers that support hints. Parakeet uses its existing decoder.</p>
      <div class="buttons"><button class="btn plain" data-action="workflow-inherit">Use inherited defaults</button><button class="btn primary" data-action="workflow-save">Save workflows</button></div>
    </section>
    <section class="set-card"><header><h3>Your note formats</h3></header><p class="hint">Lecture, Meeting, Technical interview and Design review are built in.</p>${customRows('templates')}<button class="btn plain" data-action="workflow-add" data-kind="templates">Add format</button></section>
    <section class="set-card"><header><h3>Saved actions</h3></header><p class="hint">Reuse a prompt in Felix for one note, a folder, or a date range.</p>${customRows('recipes')}<button class="btn plain" data-action="workflow-add" data-kind="recipes">Add action</button><div class="buttons"><button class="btn primary" data-action="workflow-save">Save workflows</button></div></section>
    <section class="set-card"><header><h3>Google Calendar</h3></header><p>${c.connected?'Connected. '+(c.synced?'Last synced '+rel(c.synced)+'.':'Ready to sync.'):'Connect from the page on Leo’s computer using a Google Desktop app OAuth client.'}</p>
      <p class="hint">Enable the Google Calendar API in Google Cloud, create a Desktop app OAuth client, and add your account as a test user if the consent screen is in testing. Leo reads upcoming events from one calendar.</p>
      ${c.connected?`<div class="buttons"><button class="btn plain" data-action="calendar-sync">Sync now</button><button class="btn plain" data-action="calendar-reconnect">Reconnect</button><button class="btn danger" data-action="calendar-disconnect">Disconnect</button></div>`:`<label class="workflow-field">Client ID<input id="calendar-client" autocomplete="off" placeholder="…apps.googleusercontent.com"></label><label class="workflow-field">Client secret<input type="password" id="calendar-secret" autocomplete="off"></label><label class="workflow-field">Calendar ID<input id="calendar-id" placeholder="primary"></label><button class="btn primary" data-action="calendar-connect"${c.busy?' disabled':''}>${c.busy?'Waiting for Google sign-in…':'Connect Google Calendar'}</button>`}
      <p class="hint">Credentials stay on Leo’s computer. Upcoming events appear on the Record page.</p>
    </section></div>`;
  app.querySelectorAll('.workflow-custom').forEach((x) => { x.open=opened.has(`${x.dataset.kind}:${x.dataset.i}`); });
  if (focus) {
    const field=focus.id ? document.getElementById(focus.id) : [...app.querySelectorAll('[data-wf]')].find((x) => x.dataset.wf===focus.wf && x.dataset.i===focus.i && x.dataset.key===focus.key);
    if (field) { const details=field.closest('details'); if (details) details.open=true; field.focus({preventScroll:true}); if (focus.start!==null && typeof field.setSelectionRange==='function') field.setSelectionRange(focus.start,focus.end); }
  }
  $('#workflow-dir').addEventListener('change' ,(e) => { captureProfile(); captureCustom(); drawWorkflows(e.target.value); });
}
async function saveWorkflows() {
  captureCustom(); const dir=$('#workflow-dir').value;
  workflowPage.workflows.profiles[dir]={template:$('#workflow-template').value,context:$('#workflow-context').value,vocabulary:$('#workflow-vocabulary').value.split('\n').map((s) => s.trim()).filter(Boolean)};
  await api('/api/workflows',{method:'PUT',body:workflowPage.workflows}); toast('Workflows saved.');
}
async function inheritedWorkflows() {
  const mine=seq; const dir=$('#workflow-dir').value; captureCustom(); delete workflowPage.workflows.profiles[dir];
  drawWorkflows(dir); await api('/api/workflows',{method:'PUT',body:workflowPage.workflows}); if (mine===seq) toast('Inherited defaults restored.');
}
function addWorkflow(kind) {
  captureProfile(); captureCustom(); const dir=$('#workflow-dir').value;
  workflowPage.workflows[kind].push({id:crypto.randomUUID(),name:kind==='templates'?'New format':'New action',prompt:kind==='templates'?'Organize these notes with the following headings:':'Summarize these notes with source citations.'}); drawWorkflows(dir);
}
function removeWorkflow(el) { captureProfile(); captureCustom(); const dir=$('#workflow-dir').value; const list=workflowPage.workflows[el.dataset.kind]; const removed=list.splice(Number(el.dataset.i),1)[0]; for (const p of Object.values(workflowPage.workflows.profiles)) if (p.template===removed.id) p.template='lecture'; drawWorkflows(dir); }
async function connectCalendar() {
  const mine=seq; let polls=0;
  const button=$('[data-action="calendar-connect"]'); button.disabled=true;
  const popup=window.open('about:blank','leo-calendar','popup,width=620,height=760');
  try {
    const made=await api('/api/calendar/connect',{method:'POST',body:{client_id:$('#calendar-client').value.trim(),client_secret:$('#calendar-secret').value.trim(),calendar_id:$('#calendar-id').value.trim()}});
    if (popup) { popup.opener=null; popup.location=made.url; } else window.location.assign(made.url);
    toast('Complete Google sign-in, then return here.');
    const poll=async () => { const c=await api('/api/calendar'); if (mine!==seq || state.view!=='workflows') return; if (c.busy && ++polls<160) { setTimeout(() => poll().catch(fail),2000); return; } captureProfile(); captureCustom(); workflowPage.calendar=c; drawWorkflows($('#workflow-dir').value); if (!c.connected) toast('Sign-in did not finish. Try connecting again.',{bad:true}); };
    setTimeout(() => poll().catch(fail),2000);
  } catch(e) { if (popup) popup.close(); button.disabled=false; throw e; }
}

async function savedActions() {
  const mine=seq;
  const [page,folders,notes]=await Promise.all([api('/api/workflows'),api('/api/folders'),api('/api/search?q=&brief=true')]);
  if (mine!==seq) return;
  const recipes=[...page.recipes,...page.workflows.recipes];
  const current=state.session && state.session.note;
  const box=sheet(`<h3>Saved actions</h3><p>Choose which notes Felix can retrieve for this action.</p>
    <label class="workflow-field">Folder<select id="scope-folder"><option value="">All folders</option>${folders.map((f) => `<option value="${esc(f.name)}"${f.name===(chosenScope.folder || state.dir)?' selected':''}>${esc(f.name)}</option>`).join('')}</select></label>
    <div class="scope-dates"><label class="workflow-field">From<input type="date" id="scope-from" value="${esc(chosenScope.from || '')}"></label><label class="workflow-field">Through<input type="date" id="scope-to" value="${esc(chosenScope.to || '')}"></label></div>
    <details><summary>Choose individual notes (optional)</summary><div class="scope-notes">${notes.slice(0,100).map((n) => `<label><input type="checkbox" name="scope-note" value="${esc(n.id)}"${chosenScope.ids && chosenScope.ids.includes(n.id)?' checked':''}>${esc(n.title)}</label>`).join('')}</div></details>
    ${current?`<button class="btn plain" id="scope-this-note">Use only this note</button>`:''}
    <label class="workflow-field">Action<select id="recipe">${workflowOptions(recipes,'revision')}</select></label>
    <div class="buttons"><button class="btn plain" id="scope-apply">Set scope for chat</button><button class="btn primary" id="recipe-run">Run action</button></div>`);
  const scope=() => ({folder:$('#scope-folder',box).value || null,from:$('#scope-from',box).value || null,to:$('#scope-to',box).value || null,ids:[...box.querySelectorAll('[name="scope-note"]:checked')].map((x) => x.value)});
  const apply=() => { chosenScope=scope(); chat.setScope(chosenScope); closeSheet(); };
  $('#scope-apply',box).addEventListener('click',() => { apply(); chat.toggle(true); });
  $('#recipe-run',box).addEventListener('click',() => { const recipe=recipes.find((r) => r.id===$('#recipe',box).value); apply(); chat.ask(recipe.prompt); });
  const only=$('#scope-this-note',box);
  if (only) only.addEventListener('click',() => { box.querySelectorAll('[name="scope-note"]').forEach((x) => { x.checked=x.value===current.id; }); $('#scope-folder',box).value=''; });
}

async function showRecordingSources(query='') {
  const mine=seq;
  const s=await ready(); if (!s || !s.note.id) return;
  const [page,workflows]=await Promise.all([api(`/api/notes/${enc(s.note.id)}/recording?q=${enc(query)}`),api('/api/workflows')]);
  if (mine!==seq) return;
  if (!page.sources.length) return toast('This note has no retained recording transcript. New recordings keep their sources.');
  sourcePage={...page,note:s.note.id,workflows}; generatedPreview=null; drawRecordingSources(query);
}
function sourceClock(secs) { return window.leoRecording.clock(secs); }
function drawRecordingSources(query='') {
  const active=document.activeElement;
  const focus=active && active.closest('.source-sheet') ? {id:active.id,passage:active.dataset.passage,point:active.dataset.point,i:active.dataset.i,start:active.selectionStart,end:active.selectionEnd} : null;
  query=query || ($('#source-query') && $('#source-query').value) || '';
  const page=sourcePage;
  const box=sheet(`<h3>Recording sources</h3><p class="hint">Review Parakeet’s words and your original points. Corrections are used when you regenerate.</p>
    <label class="workflow-field">Find in transcript<input id="source-query" value="${esc(query)}" placeholder="Word, name or phrase"></label>
    <div id="source-passages">${page.sources.map((source,i) => `<section class="recording-source"><h4>${esc(new Date(source.started).toLocaleString())}</h4>
      ${source.passages.map((p,j) => `<details class="source-passage" data-text="${esc(p.text.toLowerCase())}"><summary>${sourceClock(p.start_secs)}–${sourceClock(p.end_secs)} · ${esc(p.speaker || 'Recording')} · ${esc(p.text.slice(0,110))}</summary><textarea data-passage="${i}" data-i="${j}" rows="5">${esc(p.text)}</textarea></details>`).join('')}
      <h4>Your original points</h4>${source.points.map((p,j) => `<label class="workflow-field">${sourceClock(p.at_secs)}<textarea data-point="${i}" data-i="${j}" rows="2">${esc(p.text)}</textarea></label>`).join('')}
      <details><summary>Recording diagnostics</summary>${source.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('')}<ul class="source-trace">${source.trace.map((t) => `<li>${esc(new Date(t.at).toLocaleTimeString())} · ${esc(t.stage)} · ${esc(t.detail)}</li>`).join('')}</ul></details>
      <button class="btn plain" data-action="source-save" data-i="${i}">Save corrections</button></section>`).join('')}</div>
    <label class="workflow-field">Regenerate format<select id="regenerate-template">${workflowOptions([...page.workflows.templates,...page.workflows.workflows.templates],page.sources[0].template || 'lecture')}</select></label>
    <div class="buttons"><button class="btn plain" data-action="close">Close</button><button class="btn primary" data-action="source-regenerate">Preview regenerated note</button></div>`);
  $('.sheet',box).classList.add('source-sheet');
  const filter=() => { const q=$('#source-query',box).value.toLowerCase().trim(); box.querySelectorAll('.source-passage').forEach((p) => { p.hidden=q && !p.dataset.text.includes(q); if (q && !p.hidden) p.open=true; }); };
  $('#source-query',box).addEventListener('input',filter); filter();
  if (focus) {
    const field=focus.id ? document.getElementById(focus.id) : [...box.querySelectorAll('[data-passage],[data-point]')].find((x) => x.dataset.passage===focus.passage && x.dataset.point===focus.point && x.dataset.i===focus.i);
    if (field) { const details=field.closest('details'); if (details) details.open=true; field.focus({preventScroll:true}); if (focus.start!==null && typeof field.setSelectionRange==='function') field.setSelectionRange(focus.start,focus.end); }
  }
}
async function saveSource(index) {
  const page=sourcePage; const mine=seq;
  const source=page.sources[index];
  document.querySelectorAll(`[data-passage="${index}"]`).forEach((x) => { source.passages[Number(x.dataset.i)].text=x.value; });
  document.querySelectorAll(`[data-point="${index}"]`).forEach((x) => { source.points[Number(x.dataset.i)].text=x.value; });
  await api(`/api/notes/${enc(sourcePage.note)}/recording`,{method:'PUT',body:{source:source.id,base:sourcePage.versions[source.id],points:source.points,passages:source.passages}});
  const updated=await api(`/api/notes/${enc(page.note)}/recording`);
  if (mine!==seq || sourcePage!==page || !$('#source-query')) return;
  sourcePage={...page,...updated}; drawRecordingSources(); toast('Corrections saved.');
}
async function regenerateSource() {
  if (document.querySelectorAll('[data-passage]').length) {
    const dirty=sourcePage.sources.some((s,i) => [...document.querySelectorAll(`[data-passage="${i}"]`)].some((x) => x.value!==s.passages[Number(x.dataset.i)].text) || [...document.querySelectorAll(`[data-point="${i}"]`)].some((x) => x.value!==s.points[Number(x.dataset.i)].text));
    if (dirty) throw new Error('Save your transcript corrections before regenerating.');
  }
  const page=sourcePage; const mine=seq;
  const button=$('[data-action="source-regenerate"]'); button.disabled=true; button.textContent='Generating preview…';
  try {
    const preview=await api(`/api/notes/${enc(page.note)}/regenerate`,{method:'POST',body:{template:$('#regenerate-template').value}});
    const previous=await api(`/api/notes/${enc(page.note)}`);
    if (mine!==seq || sourcePage!==page || !$('#regenerate-template')) return;
    generatedPreview={...preview,note:page.note,previous};
    const box=sheet(`<h3>Preview regenerated note</h3><p class="hint">Your current note is kept until you apply this preview.</p><label><input type="checkbox" id="regenerate-title" checked>Keep the current title</label><div class="prose regenerate-preview">${md.render(preview.body)}</div><div class="buttons"><button class="btn plain" data-action="source-back">Back</button><button class="btn primary" data-action="source-apply">Apply to note</button></div>`); $('.sheet',box).classList.add('source-sheet');
  } catch(e) { button.disabled=false; button.textContent='Preview regenerated note'; throw e; }
}
async function applyGenerated() {
  const p=generatedPreview; const mine=seq;
  const note=await api(`/api/notes/${enc(p.note)}`,{method:'PATCH',body:{body:p.body,title:$('#regenerate-title').checked?p.previous.title:p.title,base:p.base}});
  if (mine!==seq) return;
  closeSheet(); await showNote(p.note);
  toast('Regenerated note saved.',{action:'Undo',run:async () => { try { await api(`/api/notes/${enc(p.note)}`,{method:'PATCH',body:{title:p.previous.title,body:p.previous.body,base:note.version}}); await showNote(p.note); toast('Previous note restored.'); } catch(e) { fail(e); } }});
}
