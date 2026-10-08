let mapView = null;
let mapPoll = 0;
let mapSheet = 'peek';
const leoGraph = window.leoGraph;
const plural = (n, word, many = `${word}s`) => `${n} ${n === 1 ? word : many}`;
const darkScheme = () => Boolean(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
const className = (n) => (n.top ? n.top : 'Unfiled');
const mapWide = () => window.innerWidth >= 900;

function leaveMap() {
  clearTimeout(mapPoll);
  if (mapView) mapView.destroy();
  mapView = null;
  document.body.classList.remove('on-map');
}

async function showMap(focus) {
  const mine = ++seq;
  leaveMap();
  state = { view: 'map', dir: '', focus, status: null };
  chrome({ showBack: true });
  $('#crumbs').innerHTML = '<span class="sep">/</span><button data-action="map-clear">Map of ideas</button>';
  document.title = 'Map of ideas · leo';
  app.innerHTML = skeleton(3);
  const data = await api('/api/graph');
  if (mine !== seq) return;
  drawMap(data, focus ? `n:${focus}` : null);
}

function drawMap(data, select, keep) {
  state.status = data.status;
  if (!data.graph.nodes.some((n) => n.kind === 'note')) {
    app.innerHTML = empty(ICON.map, 'Nothing to map yet', 'Write a few notes, and the map shows how they connect.');
    return;
  }
  document.body.classList.add('on-map');
  app.innerHTML = `<section class="map" id="map">
    <canvas id="map-canvas" role="img" aria-label="Map of how your notes connect. The panel lists the same connections."></canvas>
    <div class="map-tools">
      <label class="map-search">${ICON.search}<input id="map-find" type="search" placeholder="Find a note or idea" autocomplete="off" enterkeyhint="go"></label>
      <div class="map-found" id="map-found"></div>
      <div class="map-status" id="map-status"></div>
      <div class="map-chips" id="map-chips"></div>
    </div>
    <div class="map-zoom">
      <button data-action="map-zoom-in" aria-label="Zoom in">${ICON.plus}</button>
      <button data-action="map-zoom-out" aria-label="Zoom out">${ICON.minus}</button>
      <button data-action="map-fit" aria-label="Fit the whole map">${ICON.fit}</button>
    </div>
    <aside class="map-panel ${mapSheet}" id="map-panel" aria-live="polite"></aside>
  </section>`;
  $('#map').style.top = `${Math.round($('#bar').getBoundingClientRect().bottom)}px`;
  mapView = leoGraph.create($('#map-canvas'), data.graph, {
    onSelect: mapPanel,
    onOpen: (id) => go(noteHash(id.slice(2))),
    onChange: mapChips,
    insets: mapInsets,
  });
  if (keep) mapView.setOptions(keep);
  mapChips(mapView.options());
  mapStatus(data.status);
  const input = $('#map-find');
  input.addEventListener('input', () => mapFound(input.value));
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      input.value = '';
      mapFound('');
      input.blur();
    }
    if (e.key !== 'Enter') return;
    const first = leoGraph.find(mapView.graph, input.value)[0];
    if (first) mapPick(first.id);
  });
  if (select && mapView.graph.byId.has(select)) mapView.select(select, { center: true });
  else mapPanel(null);
  if (data.status.state === 'building') pollMap();
}

function mapInsets() {
  const map = $('#map');
  if (!map) return { top: 0, bottom: 0, right: 0 };
  const box = map.getBoundingClientRect();
  const top = $('.map-tools').getBoundingClientRect().bottom - box.top + 10;
  const panel = $('#map-panel');
  if (mapWide()) return { top, bottom: 0, right: box.right - panel.getBoundingClientRect().left + 12 };
  const tall = panel.classList.contains('open') ? Math.min(panel.scrollHeight, window.innerHeight * 0.5) : 96;
  return { top, bottom: tall + 10, right: 0 };
}

function mapChips(opts) {
  const chips = $('#map-chips');
  if (!chips || !mapView) return;
  const g = mapView.graph;
  const dark = darkScheme();
  const toggle = (action, on, label, title) =>
    `<button class="map-chip toggle${on ? ' on' : ''}" data-action="${action}" aria-pressed="${on}" title="${esc(title)}">${label}</button>`;
  const classes =
    g.folders.length > 1
      ? g.folders
          .map((top) => {
            const off = opts.hidden.has(top);
            const color = leoGraph.colorOf(g, top, dark) || 'var(--faint)';
            return `<button class="map-chip${off ? ' off' : ''}" data-action="map-class" data-top="${esc(top)}" aria-pressed="${!off}"><i style="background:${color}"></i>${esc(top || 'Unfiled')}</button>`;
          })
          .join('')
      : '';
  chips.innerHTML =
    toggle('map-across', opts.crossOnly, `Across classes${leoGraph.counts(g).across ? ` · ${leoGraph.counts(g).across}` : ''}`, 'Show only connections between different classes') +
    (g.nodes.some((n) => n.kind === 'concept') ? toggle('map-ideas', opts.ideas, 'Ideas', 'Show the shared ideas as their own dots') : '') +
    (opts.focus ? `<button class="map-chip toggle on" data-action="map-unfocus">Focused · show all</button>` : '') +
    classes;
}

function mapStatus(status) {
  state.status = status;
  const box = $('#map-status');
  if (!box || !mapView) return;
  const c = leoGraph.counts(mapView.graph);
  const building = status.state === 'building';
  const summary = c.connections ? `${plural(c.notes, 'note')} · ${plural(c.connections, 'connection')}` : plural(c.notes, 'note');
  let button = '';
  if (building) button = `<button class="btn primary sm" disabled>${status.total ? `Connecting ${status.done}/${status.total}…` : 'Starting…'}</button>`;
  else if (status.read === 0) button = '<button class="btn primary sm" data-action="map-build">Connect notes</button>';
  else if (status.stale > 0 || status.requests > 0) button = `<button class="btn primary sm" data-action="map-build">Update${status.stale ? ` · ${status.stale} changed` : ''}</button>`;
  const bar = building && status.total ? `<span class="map-progress"><i style="width:${Math.round((status.done / status.total) * 100)}%"></i></span>` : '';
  const again = !building && status.read > 0 ? '<button class="btn plain sm map-rebuild" data-action="map-rebuild" title="Read and connect every note again">Rebuild</button>' : '';
  box.innerHTML = `<span class="map-summary">${summary}</span>${button}${again}${bar}`;
}

function mapFound(query) {
  const found = leoGraph.find(mapView.graph, query);
  const dark = darkScheme();
  $('#map-found').innerHTML = found
    .map((n) => {
      const dot = n.kind === 'note' ? `<i class="dot" style="background:${leoGraph.colorOf(mapView.graph, n.top, dark) || 'var(--faint)'}"></i>` : '<i class="dot idea"></i>';
      return `<button class="map-row" data-action="map-pick" data-id="${esc(n.id)}">${dot}<span class="grow">${esc(n.label)}<span class="sub">${n.kind === 'note' ? esc(className(n)) : `Idea in ${plural(n.degree, 'note')}`}</span></span></button>`;
    })
    .join('');
}

function mapPick(id) {
  $('#map-find').value = '';
  $('#map-found').innerHTML = '';
  $('#map-find').blur();
  mapView.select(id, { center: true });
}

function mapSheetTo(next) {
  mapSheet = next;
  const panel = $('#map-panel');
  if (!panel) return;
  panel.classList.toggle('peek', next === 'peek');
  panel.classList.toggle('open', next === 'open');
}

const dotFor = (n) => `<i class="dot" style="background:${leoGraph.colorOf(mapView.graph, n.top, darkScheme()) || 'var(--faint)'}"></i>`;

function connectionRow(c) {
  return `<button class="map-row" data-action="map-select" data-id="${esc(c.node.id)}">${dotFor(c.node)}<span class="grow"><span class="row-head">${esc(c.node.label)}<em class="badge${c.cross ? ' across' : ''}">${esc(c.label)}</em></span>${
    c.edge.why ? `<span class="sub">${esc(c.edge.why)}</span>` : c.edge.kind === 'link' ? '<span class="sub">A link you wrote</span>' : ''
  }<span class="sub class">${esc(className(c.node))}${c.linked && c.edge.kind !== 'link' ? ' · you linked these' : ''}</span></span></button>`;
}

function mapPanel(node) {
  const panel = $('#map-panel');
  if (!panel || !mapView) return;
  const g = mapView.graph;
  const status = state.status || {};
  const grip = '<button class="map-grip" data-action="map-sheet" aria-label="Show more or less"><i></i></button>';
  if (!node) {
    mapSheetTo(mapSheet === 'open' && !mapWide() ? 'peek' : mapSheet);
    const c = leoGraph.counts(g);
    if (!c.connections) {
      const first = status.read === 0;
      panel.innerHTML = `${grip}<div class="panel-head"><h3>Connect your notes</h3></div>
        <p class="hint">${first
          ? 'leo can read your notes with your AI and connect the ones worth studying together, especially across classes: the same method in two courses, an idea one class builds on, two approaches that contrast. Each connection says why.'
          : 'No connections yet. Update reads any notes you changed and connects them.'}</p>
        ${status.state === 'building' ? '' : `<div class="map-actions"><button class="btn primary sm" data-action="map-build">${first ? 'Connect notes' : 'Update'}</button></div>`}
        <p class="hint small">Lines you see already are the [[links]] you wrote.</p>`;
      return;
    }
    const top = leoGraph.strongest(g, 12);
    panel.innerHTML = `${grip}<div class="panel-head"><h3>Strongest connections</h3><span class="sub">${plural(c.across, 'connection')} across classes · tap one to explore</span></div>
      <div class="map-list">${top
        .map(
          (x) => `<button class="map-row" data-action="map-select" data-id="${esc(x.a.id)}">${dotFor(x.a)}<span class="grow"><span class="row-head">${esc(x.a.label)} <span class="arrow">↔</span> ${esc(x.b.label)}<em class="badge${x.cross ? ' across' : ''}">${esc(leoGraph.relationFrom(x.edge, x.a.id))}</em></span>${
            x.edge.why ? `<span class="sub">${esc(x.edge.why)}</span>` : ''
          }<span class="sub class">${esc(className(x.a))} · ${esc(className(x.b))}</span></span>${dotFor(x.b)}</button>`
        )
        .join('')}</div>`;
    return;
  }
  mapSheetTo('open');
  chat.setContext(node.kind === 'note' ? { id: node.id.slice(2), title: node.label } : null);
  const close = `<button class="icon-btn map-close" data-action="map-clear" aria-label="Close">${ICON.close}</button>`;
  if (node.kind === 'note') {
    const id = node.id.slice(2);
    const all = leoGraph.connections(g, node.id);
    const across = all.filter((x) => x.cross);
    const inside = all.filter((x) => !x.cross);
    const focused = mapView.options().focus === node.id;
    const ideas = node.concepts.length
      ? `<h4>Ideas</h4><div class="map-ideas">${node.concepts
          .map((c) => {
            const cid = `c:${c.toLowerCase()}`;
            return g.byId.has(cid)
              ? `<button class="chip accent" data-action="map-select" data-id="${esc(cid)}">${esc(c)}</button>`
              : `<span class="chip">${esc(c)}</span>`;
          })
          .join('')}</div>`
      : '';
    const empty = !all.length
      ? `<p class="hint">${status.read === 0 || status.stale ? 'Not connected yet. Connect notes reads it.' : 'No strong connections to other notes yet.'}</p>`
      : '';
    panel.innerHTML = `${grip}${close}<div class="panel-head"><span class="sub class">${dotFor(node)}${esc(className(node))}</span><h3>${esc(node.label)}</h3>${
      node.summary ? `<p class="summary">${esc(node.summary)}</p>` : ''
    }<div class="map-actions"><button class="btn primary sm" data-action="open-note" data-id="${esc(id)}">Open note</button><button class="btn plain sm" data-action="map-focus" data-id="${esc(node.id)}">${focused ? 'Show all' : 'Focus'}</button></div></div>
      ${across.length ? `<h4>Across classes · ${across.length}</h4><div class="map-list">${across.map(connectionRow).join('')}</div>` : ''}
      ${inside.length ? `<h4>In ${esc(className(node))} · ${inside.length}</h4><div class="map-list">${inside.map(connectionRow).join('')}</div>` : ''}
      ${empty}${ideas}`;
    return;
  }
  const notes = leoGraph.conceptNotes(g, node.id);
  panel.innerHTML = `${grip}${close}<div class="panel-head"><span class="sub class"><i class="dot idea"></i>Idea</span><h3>${esc(node.label)}</h3><span class="sub">In ${plural(notes.length, 'note')} across ${plural(new Set(notes.map((n) => n.top)).size, 'class', 'classes')}</span></div>
    <div class="map-list">${notes
      .map((n) => `<button class="map-row" data-action="map-select" data-id="${esc(n.id)}">${dotFor(n)}<span class="grow">${esc(n.label)}${n.summary ? `<span class="sub">${esc(n.summary)}</span>` : ''}<span class="sub class">${esc(className(n))}</span></span></button>`)
      .join('')}</div>`;
}

function mapRebuildAsk() {
  const status = state.status || {};
  sheet(`<h3>Rebuild the map from scratch?</h3>
    <p>leo forgets what it found and reads and connects every note again with the AI you chose for writing. That is about ${plural(status.rebuild_requests || 1, 'request')}. Usually Update is enough: it only reads notes that are new or changed.</p>
    <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="map-rebuild-now">Rebuild</button></div>`);
}

async function mapRebuild() {
  closeSheet();
  mapStatus(await api('/api/graph/build?fresh=1', { method: 'POST' }));
  checkActivity();
  pollMap();
}

async function mapBuild(confirmed) {
  const status = state.status || {};
  if (!confirmed && status.read === 0) {
    sheet(`<h3>Connect your notes</h3>
      <p>leo sends your notes to the AI you chose for writing (with :settings in leo on your computer). It reads what each note teaches, then connects the notes worth studying together, especially across classes, and says why. That is about ${plural(status.requests || 1, 'request')}. After that, only notes you change are read again.</p>
      <div class="buttons"><button class="btn plain" data-action="close">Cancel</button><button class="btn primary" data-action="map-build-now">Connect notes</button></div>`);
    return;
  }
  closeSheet();
  mapStatus(await api('/api/graph/build', { method: 'POST' }));
  checkActivity();
  pollMap();
}

function pollMap() {
  clearTimeout(mapPoll);
  mapPoll = setTimeout(async () => {
    if (state.view !== 'map') return;
    const status = await api('/api/graph/status').catch(() => null);
    if (!status || state.view !== 'map') return;
    if (status.state === 'building') {
      mapStatus(status);
      pollMap();
      return;
    }
    if (status.state === 'failed') {
      mapStatus(status);
      mapPanel(mapView && mapView.selected() ? mapView.graph.byId.get(mapView.selected()) : null);
      toast(status.message || 'The map could not be built.', { bad: true });
      return;
    }
    const chosen = mapView ? mapView.selected() : null;
    const keep = mapView ? mapView.options() : null;
    const data = await api('/api/graph');
    if (state.view !== 'map') return;
    leaveMap();
    drawMap(data, chosen, keep);
    const c = mapView ? leoGraph.counts(mapView.graph) : { connections: 0, across: 0 };
    toast(status.message || `Connected: ${plural(c.connections, 'connection')}, ${c.across} across classes.`);
  }, 1200);
}
