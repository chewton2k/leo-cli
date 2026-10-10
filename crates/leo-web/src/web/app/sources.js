let sourcePage;
let generatedPreview;

async function showRecordingSources(query='') {
  const mine=seq;
  const s=await ready(); if (!s || !s.note.id) return;
  const page=await api(`/api/notes/${enc(s.note.id)}/recording?q=${enc(query)}`);
  if (mine!==seq) return;
  if (!page.sources.length) return toast('This note has no retained recording transcript. New recordings keep their sources.');
  sourcePage={...page,note:s.note.id}; generatedPreview=null; drawRecordingSources(query);
}
function sourceClock(secs) { return window.leoRecording.clock(secs); }
function drawRecordingSources(query='') {
  const active=document.activeElement;
  const focus=active && active.closest('.source-sheet') ? {id:active.id,passage:active.dataset.passage,point:active.dataset.point,i:active.dataset.i,start:active.selectionStart,end:active.selectionEnd} : null;
  query=query || ($('#source-query') && $('#source-query').value) || '';
  const typedWants=$('#regenerate-wants') ? $('#regenerate-wants').value : null;
  const page=sourcePage;
  const box=sheet(`<h3>Recording sources</h3><p class="hint">Review Parakeet’s words and your original points. Corrections are used when you regenerate.</p>
    <label class="source-field">Find in transcript<input id="source-query" value="${esc(query)}" placeholder="Word, name or phrase"></label>
    <div id="source-passages">${page.sources.map((source,i) => `<section class="recording-source"><h4>${esc(new Date(source.started).toLocaleString())}</h4>
      ${source.passages.map((p,j) => `<details class="source-passage" data-text="${esc(p.text.toLowerCase())}"><summary>${sourceClock(p.start_secs)}–${sourceClock(p.end_secs)} · ${esc(p.speaker || 'Recording')} · ${esc(p.text.slice(0,110))}</summary><textarea data-passage="${i}" data-i="${j}" rows="5">${esc(p.text)}</textarea></details>`).join('')}
      <h4>Your original points</h4>${source.points.map((p,j) => `<label class="source-field">${sourceClock(p.at_secs)}<textarea data-point="${i}" data-i="${j}" rows="2">${esc(p.text)}</textarea></label>`).join('')}
      <details><summary>Recording diagnostics</summary>${source.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('')}<ul class="source-trace">${source.trace.map((t) => `<li>${esc(new Date(t.at).toLocaleTimeString())} · ${esc(t.stage)} · ${esc(t.detail)}</li>`).join('')}</ul></details>
      <button class="btn plain" data-action="source-save" data-i="${i}">Save corrections</button></section>`).join('')}</div>
    <label class="source-field">What you want from the notes<textarea id="regenerate-wants" rows="2" maxlength="4000" placeholder="For example: focus on what will be on the exam, keep it short">${esc(page.sources.map((x) => x.wants || '').filter(Boolean).join('\n'))}</textarea></label>
    <div class="buttons"><button class="btn plain" data-action="close">Close</button><button class="btn primary" data-action="source-regenerate">Preview regenerated note</button></div>`);
  $('.sheet',box).classList.add('source-sheet');
  if (typedWants!==null) $('#regenerate-wants',box).value=typedWants;
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
    const preview=await api(`/api/notes/${enc(page.note)}/regenerate`,{method:'POST',body:{wants:$('#regenerate-wants').value.trim()}});
    const previous=await api(`/api/notes/${enc(page.note)}`);
    if (mine!==seq || sourcePage!==page || !$('#regenerate-wants')) return;
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
