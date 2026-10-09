(function (root) {
  'use strict';

  const LIGHT = ['#5b5bd6', '#0f8f6b', '#d1661a', '#c93c7a', '#1186a8', '#8250df', '#5e8f12', '#c2402f', '#9a6b00', '#3f6fd8'];
  const DARK = ['#9b98ff', '#40d4a4', '#f5a35c', '#f585b6', '#55c8ec', '#bb98ff', '#a4d65a', '#ff8a7a', '#e6bf5a', '#86a8ff'];
  const KIND_LABEL = { 'same idea': 'Same idea', 'same method': 'Same method', 'builds on': 'Builds on', applies: 'Applies', contrasts: 'Contrasts' };
  const KIND_BACK = { 'builds on': 'Built on by', applies: 'Applied by' };
  const FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", Inter, system-ui, sans-serif';

  const topFolder = (dir) => (dir || '').split('/')[0];
  const byLabel = (a, b) => a.label.localeCompare(b.label);

  function prepare(data) {
    const nodes = (data.nodes || []).map((n) => ({ ...n, top: n.kind === 'note' ? topFolder(n.folder) : null, degree: 0 }));
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const edges = (data.edges || [])
      .filter((e) => byId.has(e.a) && byId.has(e.b) && e.a !== e.b)
      .map((e, i) => ({ ...e, index: i, strength: e.strength || 1 }));
    const adjacent = new Map(nodes.map((n) => [n.id, []]));
    for (const e of edges) {
      adjacent.get(e.a).push({ id: e.b, edge: e });
      adjacent.get(e.b).push({ id: e.a, edge: e });
      if (e.kind === 'covers') {
        byId.get(e.b).degree += 1;
      } else {
        byId.get(e.a).degree += 1;
        byId.get(e.b).degree += 1;
      }
    }
    const folders = [...new Set(nodes.filter((n) => n.kind === 'note').map((n) => n.top))].sort(
      (a, b) => (a === '') - (b === '') || a.localeCompare(b)
    );
    return { nodes, edges, byId, adjacent, folders };
  }

  function colorOf(g, top, dark) {
    if (!top) return null;
    const named = g.folders.filter((f) => f !== '');
    const palette = dark ? DARK : LIGHT;
    return palette[named.indexOf(top) % palette.length];
  }

  function isCross(g, e) {
    const a = g.byId.get(e.a);
    const b = g.byId.get(e.b);
    return a.kind === 'note' && b.kind === 'note' && a.top !== b.top;
  }

  function counts(g) {
    const related = g.edges.filter((e) => e.kind === 'related');
    return {
      notes: g.nodes.filter((n) => n.kind === 'note').length,
      ideas: g.nodes.filter((n) => n.kind === 'concept').length,
      connections: related.length,
      across: related.filter((e) => isCross(g, e)).length,
    };
  }

  function relationFrom(edge, fromId) {
    if (edge.kind === 'link') return 'Linked';
    const kind = edge.relation || 'same idea';
    if (edge.b === fromId && KIND_BACK[kind]) return KIND_BACK[kind];
    return KIND_LABEL[kind] || 'Related';
  }

  function connections(g, noteId) {
    const byNote = new Map();
    for (const x of g.adjacent.get(noteId) || []) {
      if (x.edge.kind === 'covers') continue;
      const seen = byNote.get(x.id);
      const linked = x.edge.kind === 'link' || Boolean(seen && seen.linked);
      if (seen && seen.edge.kind === 'related' && x.edge.kind === 'link') {
        seen.linked = true;
        continue;
      }
      byNote.set(x.id, { node: g.byId.get(x.id), edge: x.edge, cross: isCross(g, x.edge), label: relationFrom(x.edge, noteId), linked });
    }
    return [...byNote.values()].sort((a, b) => b.cross - a.cross || b.edge.strength - a.edge.strength || byLabel(a.node, b.node));
  }

  function conceptNotes(g, conceptId) {
    return (g.adjacent.get(conceptId) || []).map((x) => g.byId.get(x.id)).filter((n) => n.kind === 'note').sort(byLabel);
  }

  function strongest(g, most = 12) {
    return g.edges
      .filter((e) => e.kind === 'related')
      .map((e) => ({ edge: e, a: g.byId.get(e.a), b: g.byId.get(e.b), cross: isCross(g, e) }))
      .sort((x, y) => y.cross - x.cross || y.edge.strength - x.edge.strength || byLabel(x.a, y.a))
      .slice(0, most);
  }

  const MAP_MOST = 400;

  function busiest(g, notes, usable, most) {
    if (notes.size <= most) return { notes, of: notes.size };
    const degree = new Map();
    for (const e of g.edges) {
      if (!usable(e) || e.kind === 'covers' || !notes.has(e.a) || !notes.has(e.b)) continue;
      degree.set(e.a, (degree.get(e.a) || 0) + 1);
      degree.set(e.b, (degree.get(e.b) || 0) + 1);
    }
    const kept = [...notes]
      .sort((a, b) => (degree.get(b) || 0) - (degree.get(a) || 0) || g.byId.get(a).label.localeCompare(g.byId.get(b).label))
      .slice(0, most);
    return { notes: new Set(kept), of: notes.size };
  }

  function visible(g, { hidden = new Set(), ideas = false, crossOnly = false, focus = null, depth = 1, most = MAP_MOST } = {}) {
    let notes = new Set(g.nodes.filter((n) => n.kind === 'note' && !hidden.has(n.top)).map((n) => n.id));
    const usable = (e) => {
      if (e.kind === 'covers') return ideas;
      if (crossOnly && e.kind === 'related' && !isCross(g, e)) return false;
      return true;
    };
    const shown = new Set();
    let limited = null;
    if (focus && g.byId.has(focus)) {
      const near = new Set([focus]);
      let frontier = [focus];
      for (let step = 0; step < depth; step++) {
        const next = [];
        for (const id of frontier) {
          for (const x of g.adjacent.get(id)) {
            if (!usable(x.edge) || near.has(x.id)) continue;
            if (g.byId.get(x.id).kind === 'note' && !notes.has(x.id)) continue;
            near.add(x.id);
            next.push(x.id);
          }
        }
        frontier = next;
      }
      for (const id of near) if (g.byId.get(id).kind === 'note' || ideas) shown.add(id);
    } else {
      const top = busiest(g, notes, usable, most);
      if (top.notes.size < top.of) limited = { shown: top.notes.size, of: top.of };
      notes = top.notes;
      for (const id of notes) shown.add(id);
      if (ideas) {
        for (const n of g.nodes) {
          if (n.kind === 'concept' && g.adjacent.get(n.id).some((x) => notes.has(x.id))) shown.add(n.id);
        }
      }
    }
    return { nodes: shown, edges: g.edges.filter((e) => usable(e) && shown.has(e.a) && shown.has(e.b)), limited };
  }

  function find(g, query, most = 8) {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return g.nodes
      .filter((n) => n.label.toLowerCase().includes(q))
      .sort(
        (a, b) =>
          b.label.toLowerCase().startsWith(q) - a.label.toLowerCase().startsWith(q) ||
          (a.kind === 'concept') - (b.kind === 'concept') ||
          b.degree - a.degree ||
          byLabel(a, b)
      )
      .slice(0, most);
  }

  function radiusOf(n) {
    return n.kind === 'note' ? Math.min(18, 4.5 + 2.3 * Math.sqrt(n.degree)) : Math.min(9, 3 + 1.1 * Math.sqrt(n.degree));
  }

  function createSim(g) {
    const pos = new Map();
    const classes = Math.max(1, g.folders.length);
    const spread = 70 + 26 * Math.sqrt(g.nodes.length);
    const seen = new Map();
    for (const n of g.nodes) {
      if (n.kind !== 'note') continue;
      const c = g.folders.indexOf(n.top);
      const angle = (c / classes) * Math.PI * 2;
      const cx = classes > 1 ? Math.cos(angle) * spread * 0.55 : 0;
      const cy = classes > 1 ? Math.sin(angle) * spread * 0.55 : 0;
      const i = seen.get(n.top) || 0;
      seen.set(n.top, i + 1);
      const r = 12 * Math.sqrt(i + 0.5);
      const a = i * Math.PI * (3 - Math.sqrt(5));
      pos.set(n.id, { x: cx + r * Math.cos(a), y: cy + r * Math.sin(a), vx: 0, vy: 0, fx: null, fy: null });
    }
    let k = 0;
    for (const n of g.nodes) {
      if (n.kind === 'note') continue;
      const around = g.adjacent.get(n.id).map((x) => pos.get(x.id)).filter(Boolean);
      const cx = around.reduce((s, p) => s + p.x, 0) / Math.max(around.length, 1);
      const cy = around.reduce((s, p) => s + p.y, 0) / Math.max(around.length, 1);
      k += 1;
      pos.set(n.id, { x: cx + Math.cos(k * 2.4) * 14, y: cy + Math.sin(k * 2.4) * 14, vx: 0, vy: 0, fx: null, fy: null });
    }
    return { g, pos, alpha: 1, alphaTarget: 0, alphaMin: 0.002, alphaDecay: 0.0228, velocityDecay: 0.42 };
  }

  function quad(points) {
    let x0 = Infinity;
    let y0 = Infinity;
    let x1 = -Infinity;
    let y1 = -Infinity;
    for (const p of points) {
      x0 = Math.min(x0, p.x);
      y0 = Math.min(y0, p.y);
      x1 = Math.max(x1, p.x);
      y1 = Math.max(y1, p.y);
    }
    const make = (x, y, s) => ({ x, y, s, mass: 0, cx: 0, cy: 0, kids: null, item: null, extra: null });
    const top = make(x0, y0, Math.max(x1 - x0, y1 - y0, 1) + 1);
    function add(cell, p) {
      cell.mass += p.m;
      cell.cx += p.x * p.m;
      cell.cy += p.y * p.m;
    }
    function insert(cell, p, depth) {
      if (!cell.kids && !cell.item && !cell.extra) {
        cell.item = p;
      } else if (!cell.kids && depth > 32) {
        cell.extra = (cell.extra || []).concat(p);
      } else {
        if (!cell.kids) {
          const old = cell.item;
          cell.item = null;
          const h = cell.s / 2;
          cell.kids = [make(cell.x, cell.y, h), make(cell.x + h, cell.y, h), make(cell.x, cell.y + h, h), make(cell.x + h, cell.y + h, h)];
          place(cell, old, depth);
        }
        place(cell, p, depth);
      }
      add(cell, p);
    }
    function place(cell, p, depth) {
      const h = cell.s / 2;
      insert(cell.kids[(p.x >= cell.x + h ? 1 : 0) + (p.y >= cell.y + h ? 2 : 0)], p, depth + 1);
    }
    for (const p of points) insert(top, p, 0);
    (function finish(cell) {
      if (cell.mass) {
        cell.cx /= cell.mass;
        cell.cy /= cell.mass;
      }
      if (cell.kids) cell.kids.forEach(finish);
    })(top);
    return top;
  }

  function repel(cell, q, alpha) {
    if (!cell.mass) return;
    const dx = cell.cx - q.p.x;
    const dy = cell.cy - q.p.y;
    const d2 = dx * dx + dy * dy;
    const leaf = !cell.kids;
    if (!leaf && (cell.s * cell.s) / Math.max(d2, 1e-6) >= 0.81) {
      for (const kid of cell.kids) repel(kid, q, alpha);
      return;
    }
    const mass = cell.mass - (leaf && (cell.item === q || (cell.extra && cell.extra.includes(q))) ? q.m : 0);
    if (mass <= 0 || d2 > 640000) return;
    const w = (mass * -26 * alpha) / Math.max(d2, 64);
    q.p.vx += dx * w;
    q.p.vy += dy * w;
  }

  function tick(sim, vis) {
    const { g, pos } = sim;
    sim.alpha += (sim.alphaTarget - sim.alpha) * sim.alphaDecay;
    const alpha = sim.alpha;
    const points = [...vis.nodes].map((id) => {
      const n = g.byId.get(id);
      const p = pos.get(id);
      return { x: p.x, y: p.y, m: n.kind === 'note' ? 6 + n.degree : 2.5, p, n };
    });
    if (points.length > 1) {
      const tree = quad(points);
      for (const q of points) repel(tree, q, alpha);
    }
    const degree = new Map();
    for (const e of vis.edges) {
      degree.set(e.a, (degree.get(e.a) || 0) + 1);
      degree.set(e.b, (degree.get(e.b) || 0) + 1);
    }
    for (const e of vis.edges) {
      const s = pos.get(e.a);
      const t = pos.get(e.b);
      const dx = t.x + t.vx - s.x - s.vx || 1e-3;
      const dy = t.y + t.vy - s.y - s.vy || 1e-3;
      const l = Math.sqrt(dx * dx + dy * dy);
      const distance = e.kind === 'covers' ? 34 : e.kind === 'link' ? 70 : 92 - e.strength * 14;
      const base = e.kind === 'covers' ? 0.5 : e.kind === 'link' ? 0.8 : 0.45 + e.strength * 0.2;
      const da = degree.get(e.a);
      const db = degree.get(e.b);
      const f = ((l - distance) / l) * alpha * (base / Math.min(da, db));
      const bias = da / (da + db);
      t.vx -= dx * f * bias;
      t.vy -= dy * f * bias;
      s.vx += dx * f * (1 - bias);
      s.vy += dy * f * (1 - bias);
    }
    const centres = new Map();
    for (const q of points) {
      if (q.n.kind !== 'note') continue;
      const c = centres.get(q.n.top) || { x: 0, y: 0, k: 0 };
      c.x += q.p.x;
      c.y += q.p.y;
      c.k += 1;
      centres.set(q.n.top, c);
    }
    for (const q of points) {
      const pull = degree.has(q.n.id) ? 0.035 : 0.16;
      q.p.vx -= q.p.x * pull * alpha;
      q.p.vy -= q.p.y * pull * alpha;
      if (q.n.kind === 'note' && centres.size > 1) {
        const c = centres.get(q.n.top);
        q.p.vx += (c.x / c.k - q.p.x) * 0.05 * alpha;
        q.p.vy += (c.y / c.k - q.p.y) * 0.05 * alpha;
      }
    }
    if (points.length <= 600) {
      for (let i = 0; i < points.length; i++) {
        const a = points[i];
        const ra = radiusOf(a.n) + 7;
        for (let j = i + 1; j < points.length; j++) {
          const b = points[j];
          const min = ra + radiusOf(b.n) + 7;
          const dx = b.p.x - a.p.x;
          const dy = b.p.y - a.p.y;
          const d2 = dx * dx + dy * dy;
          if (d2 >= min * min || d2 === 0) continue;
          const d = Math.sqrt(d2);
          const push = ((min - d) / d) * 0.35;
          b.p.vx += dx * push;
          b.p.vy += dy * push;
          a.p.vx -= dx * push;
          a.p.vy -= dy * push;
        }
      }
    }
    for (const q of points) {
      const p = q.p;
      if (p.fx !== null) {
        p.x = p.fx;
        p.y = p.fy;
        p.vx = 0;
        p.vy = 0;
        continue;
      }
      p.vx *= 1 - sim.velocityDecay;
      p.vy *= 1 - sim.velocityDecay;
      p.x += p.vx;
      p.y += p.vy;
    }
  }

  function bounds(sim, ids) {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const id of ids) {
      const p = sim.pos.get(id);
      if (!p) continue;
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x);
      maxY = Math.max(maxY, p.y);
    }
    if (minX === Infinity) return { minX: -1, minY: -1, maxX: 1, maxY: 1 };
    return { minX, minY, maxX, maxY };
  }

  function wrap(ctx, text, width, most) {
    const words = text.split(/\s+/).filter(Boolean);
    const lines = [];
    let line = '';
    let used = 0;
    for (const w of words) {
      const next = line ? `${line} ${w}` : w;
      if (ctx.measureText(next).width > width && line) {
        lines.push(line);
        line = w;
        if (lines.length === most) break;
      } else {
        line = next;
      }
      used += 1;
    }
    if (lines.length < most && line) lines.push(line);
    if (used < words.length) lines[lines.length - 1] = lines[lines.length - 1].replace(/\s*\S*$/, '') + '…';
    return lines;
  }

  function create(canvas, data, { onSelect = () => {}, onOpen = () => {}, onChange = () => {}, insets = () => ({ top: 0, bottom: 0, right: 0 }) } = {}) {
    const g = prepare(data);
    const sim = createSim(g);
    const opts = { hidden: new Set(), ideas: false, crossOnly: false, focus: null, depth: 1 };
    let vis = visible(g, opts);
    let selected = null;
    let hovered = null;
    let hoverEdge = null;
    let mouse = null;
    let frame = 0;
    let colors = {};
    let dark = false;
    let width = 0;
    let height = 0;
    let touched = false;
    const cam = { x: 0, y: 0, k: 1 };
    const goal = { x: 0, y: 0, k: 1 };
    const fade = new Map(g.nodes.map((n) => [n.id, 1]));
    const ctx = canvas.getContext('2d');
    const media = root.matchMedia ? root.matchMedia('(prefers-color-scheme: dark)') : null;
    const ranked = [...g.nodes].sort((a, b) => b.degree - a.degree);
    const prominent = new Set(ranked.slice(0, Math.max(6, Math.ceil(ranked.length * 0.12))).map((n) => n.id));

    function readColors() {
      const css = getComputedStyle(document.documentElement);
      const v = (name) => css.getPropertyValue(name).trim();
      colors = { text: v('--text'), muted: v('--muted'), faint: v('--faint'), line: v('--line'), accent: v('--accent'), surface: v('--surface'), bg: v('--bg') };
      dark = Boolean(media && media.matches);
    }

    function size() {
      const rect = canvas.getBoundingClientRect();
      const ratio = root.devicePixelRatio || 1;
      width = rect.width;
      height = rect.height;
      canvas.width = Math.max(1, Math.round(width * ratio));
      canvas.height = Math.max(1, Math.round(height * ratio));
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    }

    const toScreen = (p) => ({ x: (p.x - cam.x) * cam.k + width / 2, y: (p.y - cam.y) * cam.k + height / 2 });
    const toWorld = (x, y) => ({ x: (x - width / 2) / cam.k + cam.x, y: (y - height / 2) / cam.k + cam.y });
    const nodeSize = (n) => Math.max(2.2, radiusOf(n) * Math.pow(cam.k, 0.6));

    function region() {
      const ins = insets();
      let x1 = width - (ins.right || 0);
      let y0 = ins.top || 0;
      let y1 = height - (ins.bottom || 0);
      if (x1 < 160) x1 = width;
      if (y1 - y0 < 140) {
        y0 = 0;
        y1 = height;
      }
      return { x0: 0, x1, y0, y1 };
    }

    function aimAt(x, y, k, now) {
      const r = region();
      goal.k = k;
      goal.x = x - ((r.x0 + r.x1) / 2 - width / 2) / k;
      goal.y = y - ((r.y0 + r.y1) / 2 - height / 2) / k;
      if (now) Object.assign(cam, goal);
    }

    function fit(now) {
      const b = bounds(sim, vis.nodes);
      const r = region();
      const k = Math.min((r.x1 - r.x0 - 120) / Math.max(b.maxX - b.minX, 1), (r.y1 - r.y0 - 90) / Math.max(b.maxY - b.minY, 1));
      aimAt((b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2, Math.max(0.12, Math.min(k, 1.8)), now);
    }

    function focusSet() {
      const centre = hovered || selected;
      if (!centre || !vis.nodes.has(centre)) return null;
      const near = new Set([centre]);
      for (const e of vis.edges) {
        if (e.a === centre) near.add(e.b);
        if (e.b === centre) near.add(e.a);
      }
      return { centre, near };
    }

    function edgeStyle(e) {
      if (e.kind === 'covers') return { color: colors.faint, width: 0.8, alpha: 0.4, dash: [2, 3] };
      if (e.kind === 'link') return { color: colors.muted, width: 1.2, alpha: 0.5, dash: [] };
      if (isCross(g, e)) return { color: colors.accent, width: 0.9 + e.strength * 0.55, alpha: 0.45 + e.strength * 0.12, dash: [] };
      return { color: colors.muted, width: 0.6 + e.strength * 0.4, alpha: 0.26 + e.strength * 0.07, dash: [] };
    }

    function animate() {
      let busy = false;
      const f = focusSet();
      for (const n of g.nodes) {
        const target = !f || f.near.has(n.id) ? 1 : 0.13;
        const now = fade.get(n.id);
        const next = now + (target - now) * 0.22;
        const settled = Math.abs(next - target) <= 0.004;
        if (!settled) busy = true;
        fade.set(n.id, settled ? target : next);
      }
      for (const key of ['x', 'y', 'k']) {
        const d = goal[key] - cam[key];
        if (Math.abs(d) > (key === 'k' ? 0.0005 : 0.05)) {
          cam[key] += d * 0.2;
          busy = true;
        } else {
          cam[key] = goal[key];
        }
      }
      return busy;
    }

    function draw() {
      ctx.clearRect(0, 0, width, height);
      const f = focusSet();
      const zoomWidth = Math.max(0.7, Math.min(1.4, Math.sqrt(cam.k)));
      for (const e of vis.edges) {
        const s = toScreen(sim.pos.get(e.a));
        const t = toScreen(sim.pos.get(e.b));
        const style = edgeStyle(e);
        const lit = f && (e.a === f.centre || e.b === f.centre);
        const hot = hoverEdge === e;
        let alpha = style.alpha * Math.min(fade.get(e.a), fade.get(e.b));
        if (f) alpha = lit ? Math.min(1, style.alpha + 0.4) : style.alpha * 0.1;
        if (hot) alpha = 1;
        ctx.globalAlpha = alpha;
        ctx.strokeStyle = style.color;
        ctx.lineWidth = style.width * (hot || lit ? 1.6 : 1) * zoomWidth;
        ctx.setLineDash(style.dash);
        ctx.beginPath();
        ctx.moveTo(s.x, s.y);
        ctx.lineTo(t.x, t.y);
        ctx.stroke();
      }
      ctx.setLineDash([]);
      const labels = [];
      for (const n of g.nodes) {
        if (!vis.nodes.has(n.id)) continue;
        const p = toScreen(sim.pos.get(n.id));
        const r = nodeSize(n);
        if (p.x < -60 || p.y < -60 || p.x > width + 60 || p.y > height + 60) continue;
        const colour = n.kind === 'note' ? colorOf(g, n.top, dark) || colors.faint : colors.accent;
        if (n.id === selected || n.id === hovered) {
          ctx.globalAlpha = 0.18;
          ctx.fillStyle = colour;
          ctx.beginPath();
          ctx.arc(p.x, p.y, r + 7, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.globalAlpha = fade.get(n.id);
        ctx.beginPath();
        ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
        if (n.kind === 'note') {
          ctx.fillStyle = colour;
          ctx.fill();
          ctx.lineWidth = 1.5;
          ctx.strokeStyle = colors.bg;
          ctx.stroke();
        } else {
          ctx.fillStyle = colors.bg;
          ctx.fill();
          ctx.lineWidth = 1.6;
          ctx.strokeStyle = colour;
          ctx.stroke();
        }
        if (n.id === selected) {
          ctx.globalAlpha = 1;
          ctx.lineWidth = 2;
          ctx.strokeStyle = colors.text;
          ctx.beginPath();
          ctx.arc(p.x, p.y, r + 3.5, 0, Math.PI * 2);
          ctx.stroke();
        }
        const zoom = prominent.has(n.id) ? (cam.k - 0.35) / 0.35 : (cam.k - 0.75) / 0.4;
        let show = Math.max(0, Math.min(1, zoom));
        if (f) show = f.near.has(n.id) ? 1 : 0;
        if (n.id === selected || n.id === hovered) show = 1;
        if (show > 0.02) labels.push({ n, p, r, show });
      }
      ctx.font = `500 12px ${FONT}`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      ctx.lineJoin = 'round';
      const pinned = (n) => n.id === selected || n.id === hovered;
      labels.sort((a, b) => pinned(b.n) - pinned(a.n) || b.n.degree - a.n.degree);
      const taken = [];
      for (const { n, p, r, show } of labels) {
        const text = n.label.length > 34 && !pinned(n) ? n.label.slice(0, 33) + '…' : n.label;
        const w = ctx.measureText(text).width;
        const box = { x: p.x - w / 2 - 2, y: p.y + r + 4, w: w + 4, h: 15 };
        if (!pinned(n) && taken.some((t) => box.x < t.x + t.w && t.x < box.x + box.w && box.y < t.y + t.h && t.y < box.y + box.h)) continue;
        taken.push(box);
        ctx.globalAlpha = show;
        ctx.lineWidth = 3.5;
        ctx.strokeStyle = colors.bg;
        ctx.strokeText(text, p.x, box.y);
        ctx.fillStyle = n.kind === 'note' ? colors.text : colors.muted;
        ctx.fillText(text, p.x, box.y);
      }
      ctx.globalAlpha = 1;
      tooltip();
    }

    function tooltip() {
      if (!mouse) return;
      let title = '';
      let body = '';
      if (hovered) {
        const n = g.byId.get(hovered);
        if (n.kind !== 'note' || !n.summary) return;
        title = n.label;
        body = n.summary;
      } else if (hoverEdge && hoverEdge.kind !== 'covers') {
        const a = g.byId.get(hoverEdge.a);
        const b = g.byId.get(hoverEdge.b);
        title = `${relationFrom(hoverEdge, hoverEdge.a)} · ${a.label} ↔ ${b.label}`;
        body = hoverEdge.why || (hoverEdge.kind === 'link' ? 'A link you wrote between these notes.' : '');
      } else {
        return;
      }
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      ctx.font = `600 12.5px ${FONT}`;
      const heads = wrap(ctx, title, 260, 2);
      const headWidth = Math.max(...heads.map((l) => ctx.measureText(l).width));
      ctx.font = `12.5px ${FONT}`;
      const lines = body ? wrap(ctx, body, 260, 4) : [];
      const bodyWidth = lines.length ? Math.max(...lines.map((l) => ctx.measureText(l).width)) : 0;
      const w = Math.min(284, Math.max(headWidth, bodyWidth) + 24);
      const h = 16 + heads.length * 17 + lines.length * 17 + (lines.length ? 4 : 0);
      let x = mouse.x + 14;
      let y = mouse.y + 14;
      if (x + w > width - 8) x = mouse.x - w - 14;
      if (y + h > height - 8) y = mouse.y - h - 14;
      ctx.globalAlpha = 0.97;
      ctx.fillStyle = colors.surface;
      ctx.strokeStyle = colors.line;
      ctx.lineWidth = 1;
      ctx.beginPath();
      if (ctx.roundRect) ctx.roundRect(x, y, w, h, 10);
      else ctx.rect(x, y, w, h);
      ctx.fill();
      ctx.stroke();
      ctx.globalAlpha = 1;
      ctx.font = `600 12.5px ${FONT}`;
      ctx.fillStyle = colors.text;
      heads.forEach((l, i) => ctx.fillText(l, x + 12, y + 8 + i * 17));
      ctx.font = `12.5px ${FONT}`;
      ctx.fillStyle = colors.muted;
      lines.forEach((l, i) => ctx.fillText(l, x + 12, y + 12 + (heads.length + i) * 17));
    }

    function loop() {
      frame = 0;
      const moving = sim.alpha > sim.alphaMin || sim.alphaTarget > 0;
      if (moving) {
        tick(sim, vis);
        if (!touched && sim.alpha > 0.3) fit(true);
      }
      const busy = animate();
      draw();
      if (moving || busy) frame = root.requestAnimationFrame(loop);
    }

    function wake(alpha) {
      if (alpha !== undefined) sim.alpha = Math.max(sim.alpha, alpha);
      if (!frame) frame = root.requestAnimationFrame(loop);
    }

    function nodeAt(x, y) {
      let best = null;
      let bestD = Infinity;
      for (const id of vis.nodes) {
        const p = toScreen(sim.pos.get(id));
        const d = Math.hypot(p.x - x, p.y - y);
        if (d < nodeSize(g.byId.get(id)) + 8 && d < bestD) {
          best = id;
          bestD = d;
        }
      }
      return best;
    }

    function edgeAt(x, y) {
      let best = null;
      let bestD = 6;
      for (const e of vis.edges) {
        if (e.kind === 'covers') continue;
        const s = toScreen(sim.pos.get(e.a));
        const t = toScreen(sim.pos.get(e.b));
        const dx = t.x - s.x;
        const dy = t.y - s.y;
        const u = Math.max(0, Math.min(1, ((x - s.x) * dx + (y - s.y) * dy) / (dx * dx + dy * dy || 1)));
        const d = Math.hypot(s.x + u * dx - x, s.y + u * dy - y);
        if (d < bestD) {
          best = e;
          bestD = d;
        }
      }
      return best;
    }

    const pointers = new Map();
    let gesture = null;

    function local(e) {
      const rect = canvas.getBoundingClientRect();
      return { x: e.clientX - rect.left, y: e.clientY - rect.top };
    }

    function hover(at) {
      const node = nodeAt(at.x, at.y);
      const edge = node ? null : edgeAt(at.x, at.y);
      mouse = at;
      hovered = node;
      hoverEdge = edge;
      canvas.style.cursor = node || edge ? 'pointer' : 'grab';
      wake();
    }

    function down(e) {
      canvas.setPointerCapture(e.pointerId);
      const at = local(e);
      pointers.set(e.pointerId, at);
      if (e.pointerType !== 'mouse') {
        hovered = null;
        hoverEdge = null;
        mouse = null;
      }
      if (pointers.size === 2) {
        const [p, q] = [...pointers.values()];
        gesture = { kind: 'pinch', d: Math.hypot(p.x - q.x, p.y - q.y), k: cam.k, moved: true };
        touched = true;
        return;
      }
      const node = nodeAt(at.x, at.y);
      gesture = { kind: node ? 'drag' : 'pan', node, start: at, last: at, moved: false };
    }

    function move(e) {
      const at = local(e);
      if (!pointers.has(e.pointerId)) {
        if (e.pointerType === 'mouse') hover(at);
        return;
      }
      pointers.set(e.pointerId, at);
      if (!gesture) return;
      if (gesture.kind === 'pinch' && pointers.size === 2) {
        const [p, q] = [...pointers.values()];
        const mid = { x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 };
        const before = toWorld(mid.x, mid.y);
        cam.k = Math.max(0.08, Math.min(6, gesture.k * (Math.hypot(p.x - q.x, p.y - q.y) / Math.max(gesture.d, 1))));
        const after = toWorld(mid.x, mid.y);
        cam.x += before.x - after.x;
        cam.y += before.y - after.y;
        Object.assign(goal, cam);
        wake();
        return;
      }
      if (Math.hypot(at.x - gesture.start.x, at.y - gesture.start.y) > 5) gesture.moved = true;
      if (!gesture.moved) return;
      touched = true;
      if (gesture.kind === 'drag') {
        const p = sim.pos.get(gesture.node);
        const w = toWorld(at.x, at.y);
        p.fx = w.x;
        p.fy = w.y;
        sim.alphaTarget = 0.25;
        canvas.style.cursor = 'grabbing';
        wake(0.25);
      } else if (gesture.kind === 'pan') {
        cam.x -= (at.x - gesture.last.x) / cam.k;
        cam.y -= (at.y - gesture.last.y) / cam.k;
        Object.assign(goal, cam);
        mouse = null;
        canvas.style.cursor = 'grabbing';
        wake();
      }
      gesture.last = at;
    }

    function up(e) {
      if (!pointers.has(e.pointerId)) return;
      pointers.delete(e.pointerId);
      const done = gesture;
      if (pointers.size > 0) {
        const [rest] = [...pointers.values()];
        gesture = { kind: 'pan', start: rest, last: rest, moved: true };
        return;
      }
      gesture = null;
      canvas.style.cursor = 'grab';
      if (!done) return;
      if (done.kind === 'drag' && done.moved) {
        const p = sim.pos.get(done.node);
        p.fx = null;
        p.fy = null;
        sim.alphaTarget = 0;
        wake();
        return;
      }
      if (!done.moved && done.kind !== 'pinch') select(done.node || null);
    }

    function leave() {
      if (!hovered && !hoverEdge && !mouse) return;
      hovered = null;
      hoverEdge = null;
      mouse = null;
      wake();
    }

    function wheel(e) {
      e.preventDefault();
      touched = true;
      const at = local(e);
      const k = Math.max(0.08, Math.min(6, goal.k * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0018))));
      const before = toWorld(at.x, at.y);
      goal.x = before.x - (at.x - width / 2) / k;
      goal.y = before.y - (at.y - height / 2) / k;
      goal.k = k;
      wake();
    }

    function dbl(e) {
      const at = local(e);
      const id = nodeAt(at.x, at.y);
      if (id && g.byId.get(id).kind === 'note') onOpen(id);
    }

    function select(id, { center = false } = {}) {
      selected = id && g.byId.has(id) ? id : null;
      if (selected && !vis.nodes.has(selected)) {
        const concept = g.byId.get(selected).kind === 'concept';
        setOptions({ hidden: new Set(), focus: null, crossOnly: false, ideas: opts.ideas || concept });
      }
      onSelect(selected ? g.byId.get(selected) : null);
      const p = selected ? sim.pos.get(selected) : null;
      if (p) {
        const s = toScreen(p);
        const r = region();
        const off = s.x < r.x0 + 30 || s.x > r.x1 - 30 || s.y < r.y0 + 30 || s.y > r.y1 - 30;
        if (center || off) {
          touched = true;
          aimAt(p.x, p.y, center ? Math.max(goal.k, 1.15) : goal.k);
        }
      }
      wake();
    }

    function setOptions(change) {
      const before = new Set(vis.nodes);
      Object.assign(opts, change);
      if (opts.focus && !g.byId.has(opts.focus)) opts.focus = null;
      vis = visible(g, opts);
      if (selected && !vis.nodes.has(selected)) {
        selected = null;
        onSelect(null);
      }
      if (hovered && !vis.nodes.has(hovered)) hovered = null;
      if (hoverEdge && !vis.edges.includes(hoverEdge)) hoverEdge = null;
      for (const id of vis.nodes) {
        if (before.has(id)) continue;
        const anchor = g.adjacent.get(id).find((x) => before.has(x.id));
        if (!anchor) continue;
        const p = sim.pos.get(id);
        const q = sim.pos.get(anchor.id);
        p.x = q.x + (Math.random() - 0.5) * 30;
        p.y = q.y + (Math.random() - 0.5) * 30;
      }
      const changed = before.size !== vis.nodes.size || [...vis.nodes].some((id) => !before.has(id));
      onChange({ ...opts, hidden: new Set(opts.hidden) });
      if (changed) {
        touched = false;
        wake(0.6);
      } else {
        wake();
      }
    }

    function zoomBy(factor) {
      const r = region();
      const cx = (r.x0 + r.x1) / 2;
      const cy = (r.y0 + r.y1) / 2;
      const before = toWorld(cx, cy);
      const k = Math.max(0.08, Math.min(6, goal.k * factor));
      goal.x = before.x - (cx - width / 2) / k;
      goal.y = before.y - (cy - height / 2) / k;
      goal.k = k;
      touched = true;
      wake();
    }

    const resize = () => {
      size();
      wake();
    };
    const scheme = () => {
      readColors();
      wake();
    };

    canvas.addEventListener('pointerdown', down);
    canvas.addEventListener('pointermove', move);
    canvas.addEventListener('pointerup', up);
    canvas.addEventListener('pointercancel', up);
    canvas.addEventListener('pointerleave', leave);
    canvas.addEventListener('wheel', wheel, { passive: false });
    canvas.addEventListener('dblclick', dbl);
    root.addEventListener('resize', resize);
    const watcher = typeof root.ResizeObserver === 'function' ? new root.ResizeObserver(resize) : null;
    if (watcher) watcher.observe(canvas);
    if (media && media.addEventListener) media.addEventListener('change', scheme);

    readColors();
    size();
    const warm = g.nodes.length > 800 ? 60 : 220;
    for (let i = 0; i < warm; i++) tick(sim, vis);
    fit(true);
    wake();

    return {
      graph: g,
      select,
      selected: () => selected,
      setOptions,
      options: () => ({ ...opts, hidden: new Set(opts.hidden) }),
      limited: () => vis.limited || null,
      fit: () => {
        touched = true;
        fit(false);
        wake();
      },
      zoomBy,
      visible: () => vis,
      positions: () => sim.pos,
      destroy() {
        if (frame) root.cancelAnimationFrame(frame);
        frame = 0;
        canvas.removeEventListener('pointerdown', down);
        canvas.removeEventListener('pointermove', move);
        canvas.removeEventListener('pointerup', up);
        canvas.removeEventListener('pointercancel', up);
        canvas.removeEventListener('pointerleave', leave);
        canvas.removeEventListener('wheel', wheel);
        canvas.removeEventListener('dblclick', dbl);
        root.removeEventListener('resize', resize);
        if (watcher) watcher.disconnect();
        if (media && media.removeEventListener) media.removeEventListener('change', scheme);
      },
    };
  }

  root.leoGraph = { MAP_MOST, prepare, colorOf, isCross, counts, connections, conceptNotes, strongest, visible, find, createSim, tick, bounds, radiusOf, relationFrom, create, topFolder };
})(typeof window !== 'undefined' ? window : globalThis);
