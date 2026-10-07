(function (root) {
  'use strict';

  const LIGHT = ['#4f46e5', '#0f8a63', '#c2620a', '#c0306b', '#0b7f9c', '#7a3fd1', '#5b8a12', '#b8322a'];
  const DARK = ['#8b85ff', '#3fd09b', '#f0a04b', '#f27aa8', '#4cc6e6', '#b48cff', '#9ccf4a', '#ff7f73'];
  const SPRING = { covers: 62, related: 96, link: 84 };

  const topFolder = (dir) => (dir || '').split('/')[0];

  function prepare(data) {
    const nodes = (data.nodes || []).map((n) => ({ ...n, top: n.kind === 'note' ? topFolder(n.folder) : null }));
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const edges = (data.edges || []).filter((e) => byId.has(e.a) && byId.has(e.b) && e.a !== e.b);
    const adjacent = new Map(nodes.map((n) => [n.id, []]));
    for (const e of edges) {
      adjacent.get(e.a).push({ id: e.b, edge: e });
      adjacent.get(e.b).push({ id: e.a, edge: e });
    }
    for (const n of nodes) {
      n.degree = adjacent.get(n.id).length;
      if (n.kind === 'concept') {
        n.tops = [...new Set(adjacent.get(n.id).map((x) => byId.get(x.id)).filter((m) => m.kind === 'note').map((m) => m.top))].sort();
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

  function counts(g) {
    return {
      notes: g.nodes.filter((n) => n.kind === 'note').length,
      ideas: g.nodes.filter((n) => n.kind === 'concept').length,
      links: g.edges.filter((e) => e.kind === 'related').length,
    };
  }

  function neighbors(g, id) {
    const out = new Set([id]);
    for (const x of g.adjacent.get(id) || []) out.add(x.id);
    return out;
  }

  const byLabel = (a, b) => a.label.localeCompare(b.label);

  function connectedNotes(g, noteId) {
    const shared = new Map();
    for (const c of g.adjacent.get(noteId) || []) {
      const concept = g.byId.get(c.id);
      if (concept.kind === 'note') {
        const entry = shared.get(concept.id) || { node: concept, via: [], linked: false };
        entry.linked = true;
        shared.set(concept.id, entry);
        continue;
      }
      for (const n of g.adjacent.get(c.id)) {
        const other = g.byId.get(n.id);
        if (other.kind !== 'note' || other.id === noteId) continue;
        const entry = shared.get(other.id) || { node: other, via: [], linked: false };
        entry.via.push(concept.label);
        shared.set(other.id, entry);
      }
    }
    return [...shared.values()].sort(
      (a, b) => b.via.length + b.linked - (a.via.length + a.linked) || b.linked - a.linked || byLabel(a.node, b.node)
    );
  }

  function conceptDetail(g, conceptId) {
    const notes = [];
    const related = [];
    for (const x of g.adjacent.get(conceptId) || []) {
      const other = g.byId.get(x.id);
      if (other.kind === 'note') notes.push(other);
      else related.push({ node: other, why: x.edge.why || '' });
    }
    return { notes: notes.sort(byLabel), related: related.sort((a, b) => byLabel(a.node, b.node)) };
  }

  function bridges(g) {
    return g.edges
      .filter((e) => e.kind === 'related')
      .map((e) => {
        const a = g.byId.get(e.a);
        const b = g.byId.get(e.b);
        const cross = (a.tops || []).join() !== (b.tops || []).join();
        return { a, b, why: e.why || '', cross };
      })
      .sort((x, y) => y.cross - x.cross || x.a.label.localeCompare(y.a.label) || x.b.label.localeCompare(y.b.label));
  }

  function visible(g, hidden) {
    const out = new Set();
    for (const n of g.nodes) if (n.kind === 'note' && !hidden.has(n.top)) out.add(n.id);
    for (const n of g.nodes) {
      if (n.kind !== 'concept') continue;
      if (g.adjacent.get(n.id).some((x) => out.has(x.id) && g.byId.get(x.id).kind === 'note')) out.add(n.id);
    }
    return out;
  }

  function find(g, query, most = 8) {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return g.nodes
      .filter((n) => n.label.toLowerCase().includes(q))
      .sort((a, b) => b.label.toLowerCase().startsWith(q) - a.label.toLowerCase().startsWith(q) || b.degree - a.degree || byLabel(a, b))
      .slice(0, most);
  }

  function random(seed) {
    let s = seed >>> 0 || 1;
    return () => {
      s ^= s << 13;
      s ^= s >>> 17;
      s ^= s << 5;
      return ((s >>> 0) % 100000) / 100000;
    };
  }

  function createLayout(g) {
    const rand = random(g.nodes.length * 7919 + g.edges.length);
    const pos = new Map();
    const notes = g.nodes.filter((n) => n.kind === 'note');
    const radius = 40 + 22 * Math.sqrt(notes.length);
    const sectors = Math.max(1, g.folders.length);
    for (const n of notes) {
      const sector = g.folders.indexOf(n.top);
      const angle = ((sector + 0.15 + rand() * 0.7) / sectors) * Math.PI * 2;
      const r = radius * (0.35 + rand() * 0.65);
      pos.set(n.id, { x: Math.cos(angle) * r, y: Math.sin(angle) * r, vx: 0, vy: 0, fixed: false });
    }
    for (const n of g.nodes) {
      if (n.kind === 'note') continue;
      const around = g.adjacent.get(n.id).map((x) => pos.get(x.id)).filter(Boolean);
      const cx = around.length ? around.reduce((s, p) => s + p.x, 0) / around.length : 0;
      const cy = around.length ? around.reduce((s, p) => s + p.y, 0) / around.length : 0;
      pos.set(n.id, { x: cx + (rand() - 0.5) * 30, y: cy + (rand() - 0.5) * 30, vx: 0, vy: 0, fixed: false });
    }
    return { g, pos, alpha: 1 };
  }

  function step(sim, shown) {
    const { g, pos } = sim;
    const ids = [...shown];
    const pts = ids.map((id) => pos.get(id));
    const fx = new Float64Array(ids.length);
    const fy = new Float64Array(ids.length);
    const index = new Map(ids.map((id, i) => [id, i]));
    for (let i = 0; i < pts.length; i++) {
      for (let j = i + 1; j < pts.length; j++) {
        const dx = pts[i].x - pts[j].x;
        const dy = pts[i].y - pts[j].y;
        const d2 = Math.max(dx * dx + dy * dy, 36);
        if (d2 > 400000) continue;
        const d = Math.sqrt(d2);
        const f = 1400 / d2;
        fx[i] += (f * dx) / d;
        fy[i] += (f * dy) / d;
        fx[j] -= (f * dx) / d;
        fy[j] -= (f * dy) / d;
      }
    }
    for (const e of g.edges) {
      const i = index.get(e.a);
      const j = index.get(e.b);
      if (i === undefined || j === undefined) continue;
      const dx = pts[j].x - pts[i].x;
      const dy = pts[j].y - pts[i].y;
      const d = Math.max(Math.sqrt(dx * dx + dy * dy), 1);
      const f = (d - SPRING[e.kind]) * 0.05;
      fx[i] += (f * dx) / d;
      fy[i] += (f * dy) / d;
      fx[j] -= (f * dx) / d;
      fy[j] -= (f * dy) / d;
    }
    const centres = new Map();
    ids.forEach((id, i) => {
      const n = g.byId.get(id);
      if (n.kind !== 'note') return;
      const c = centres.get(n.top) || { x: 0, y: 0, k: 0 };
      c.x += pts[i].x;
      c.y += pts[i].y;
      c.k += 1;
      centres.set(n.top, c);
    });
    ids.forEach((id, i) => {
      const n = g.byId.get(id);
      fx[i] -= pts[i].x * 0.006;
      fy[i] -= pts[i].y * 0.006;
      if (n.kind === 'note') {
        const c = centres.get(n.top);
        fx[i] += (c.x / c.k - pts[i].x) * 0.012;
        fy[i] += (c.y / c.k - pts[i].y) * 0.012;
      }
    });
    let moved = 0;
    pts.forEach((p, i) => {
      if (p.fixed) {
        p.vx = 0;
        p.vy = 0;
        return;
      }
      p.vx = (p.vx + fx[i] * sim.alpha) * 0.6;
      p.vy = (p.vy + fy[i] * sim.alpha) * 0.6;
      const speed = Math.hypot(p.vx, p.vy);
      if (speed > 40) {
        p.vx *= 40 / speed;
        p.vy *= 40 / speed;
      }
      p.x += p.vx;
      p.y += p.vy;
      moved = Math.max(moved, speed);
    });
    sim.alpha = Math.max(sim.alpha * 0.985, 0);
    return moved;
  }

  function bounds(sim, shown) {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const id of shown) {
      const p = sim.pos.get(id);
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x);
      maxY = Math.max(maxY, p.y);
    }
    if (minX === Infinity) return { minX: -1, minY: -1, maxX: 1, maxY: 1 };
    return { minX, minY, maxX, maxY };
  }

  function radiusOf(n) {
    return n.kind === 'note' ? 5 + Math.min(8, Math.sqrt(n.degree) * 1.8) : 3.5 + Math.min(6, Math.sqrt(n.degree) * 1.4);
  }

  function shorten(text, most) {
    return text.length > most ? text.slice(0, most - 1) + '…' : text;
  }

  function create(canvas, data, { onSelect = () => {}, insets = () => ({ top: 0, bottom: 0, right: 0 }) } = {}) {
    const g = prepare(data);
    const sim = createLayout(g);
    const view = { g, sim, hidden: new Set(), selected: null, cam: { x: 0, y: 0, k: 1 }, touched: false };
    let shown = visible(g, view.hidden);
    let frame = 0;
    let colors = {};
    let dark = false;
    let width = 0;
    let height = 0;
    const ctx = canvas.getContext('2d');
    const media = root.matchMedia ? root.matchMedia('(prefers-color-scheme: dark)') : null;
    const important = new Set(
      [...g.nodes].sort((a, b) => b.degree - a.degree).slice(0, 14).map((n) => n.id)
    );

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

    const toScreen = (p) => ({ x: (p.x - view.cam.x) * view.cam.k + width / 2, y: (p.y - view.cam.y) * view.cam.k + height / 2 });
    const toWorld = (x, y) => ({ x: (x - width / 2) / view.cam.k + view.cam.x, y: (y - height / 2) / view.cam.k + view.cam.y });

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

    function aim(x, y) {
      const r = region();
      view.cam.x = x - ((r.x0 + r.x1) / 2 - width / 2) / view.cam.k;
      view.cam.y = y - ((r.y0 + r.y1) / 2 - height / 2) / view.cam.k;
    }

    function fit() {
      const b = bounds(sim, shown);
      const r = region();
      const k = Math.min((r.x1 - r.x0 - 170) / Math.max(b.maxX - b.minX, 1), (r.y1 - r.y0 - 50) / Math.max(b.maxY - b.minY, 1));
      view.cam.k = Math.max(0.15, Math.min(k, 2.2));
      aim((b.minX + b.maxX) / 2 - 40 / view.cam.k, (b.minY + b.maxY) / 2);
    }

    function draw() {
      ctx.clearRect(0, 0, width, height);
      const focus = view.selected ? neighbors(g, view.selected) : null;
      const lit = (id) => !focus || focus.has(id);
      const scale = Math.max(0.6, Math.min(1.6, Math.sqrt(view.cam.k)));
      for (const e of g.edges) {
        if (!shown.has(e.a) || !shown.has(e.b)) continue;
        const a = toScreen(sim.pos.get(e.a));
        const b = toScreen(sim.pos.get(e.b));
        const on = !focus || (focus.has(e.a) && focus.has(e.b) && (e.a === view.selected || e.b === view.selected));
        ctx.globalAlpha = on ? 1 : 0.12;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        if (e.kind === 'related') {
          ctx.strokeStyle = colors.accent;
          ctx.lineWidth = 1.6;
          ctx.setLineDash([5, 4]);
        } else if (e.kind === 'link') {
          ctx.strokeStyle = colors.muted;
          ctx.lineWidth = 1.6;
          ctx.setLineDash([]);
        } else {
          ctx.strokeStyle = focus && on ? colors.faint : colors.line;
          ctx.lineWidth = 1;
          ctx.setLineDash([]);
        }
        ctx.stroke();
      }
      ctx.setLineDash([]);
      const labels = [];
      for (const n of g.nodes) {
        if (!shown.has(n.id)) continue;
        const p = toScreen(sim.pos.get(n.id));
        if (p.x < -40 || p.y < -40 || p.x > width + 40 || p.y > height + 40) continue;
        const r = radiusOf(n) * scale;
        ctx.globalAlpha = lit(n.id) ? 1 : 0.15;
        ctx.beginPath();
        ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
        if (n.kind === 'note') {
          ctx.fillStyle = colorOf(g, n.top, dark) || colors.faint;
          ctx.fill();
        } else {
          ctx.fillStyle = colors.surface;
          ctx.fill();
          ctx.lineWidth = 1.8;
          ctx.strokeStyle = colors.accent;
          ctx.stroke();
        }
        if (n.id === view.selected) {
          ctx.beginPath();
          ctx.arc(p.x, p.y, r + 4, 0, Math.PI * 2);
          ctx.lineWidth = 2;
          ctx.strokeStyle = colors.text;
          ctx.stroke();
        }
        const wanted = n.id === view.selected || (focus && focus.has(n.id)) || (!focus && (view.cam.k > 1.15 || important.has(n.id)));
        if (wanted) labels.push({ n, p, r });
      }
      ctx.font = '12.5px -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif';
      ctx.textBaseline = 'middle';
      ctx.lineJoin = 'round';
      const taken = [];
      labels.sort((a, b) => (b.n.id === view.selected) - (a.n.id === view.selected) || b.n.degree - a.n.degree);
      for (const { n, p, r } of labels) {
        const text = shorten(n.label, n.id === view.selected ? 48 : 26);
        const w = ctx.measureText(text).width;
        const box = { x: p.x + r + 4, y: p.y - 8, w, h: 16 };
        if (n.id !== view.selected && taken.some((t) => box.x < t.x + t.w && t.x < box.x + box.w && box.y < t.y + t.h && t.y < box.y + box.h)) continue;
        taken.push(box);
        ctx.globalAlpha = 1;
        ctx.lineWidth = 4;
        ctx.strokeStyle = colors.bg;
        ctx.strokeText(text, box.x, p.y);
        ctx.fillStyle = n.kind === 'note' ? colors.text : colors.muted;
        ctx.fillText(text, box.x, p.y);
      }
      ctx.globalAlpha = 1;
    }

    function tick() {
      frame = 0;
      if (sim.alpha > 0.02) {
        step(sim, shown);
        if (!view.touched && sim.alpha > 0.5) fit();
      }
      draw();
      if (sim.alpha > 0.02) frame = root.requestAnimationFrame(tick);
    }

    function wake(alpha) {
      if (alpha !== undefined) sim.alpha = Math.max(sim.alpha, alpha);
      if (!frame) frame = root.requestAnimationFrame(tick);
    }

    function hit(x, y) {
      let best = null;
      let bestD = Infinity;
      const scale = Math.max(0.6, Math.min(1.6, Math.sqrt(view.cam.k)));
      for (const n of g.nodes) {
        if (!shown.has(n.id)) continue;
        const p = toScreen(sim.pos.get(n.id));
        const d = Math.hypot(p.x - x, p.y - y);
        if (d < radiusOf(n) * scale + 10 && d < bestD) {
          best = n;
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

    function down(e) {
      canvas.setPointerCapture(e.pointerId);
      const at = local(e);
      pointers.set(e.pointerId, at);
      view.touched = true;
      if (pointers.size === 2) {
        const [p, q] = [...pointers.values()];
        gesture = { kind: 'pinch', d: Math.hypot(p.x - q.x, p.y - q.y), k: view.cam.k, moved: true };
        return;
      }
      const node = hit(at.x, at.y);
      gesture = { kind: node ? 'drag' : 'pan', node, start: at, last: at, moved: false, at: Date.now() };
    }

    function move(e) {
      if (!pointers.has(e.pointerId) || !gesture) return;
      const at = local(e);
      pointers.set(e.pointerId, at);
      if (gesture.kind === 'pinch' && pointers.size === 2) {
        const [p, q] = [...pointers.values()];
        const mid = { x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 };
        const before = toWorld(mid.x, mid.y);
        view.cam.k = Math.max(0.1, Math.min(5, gesture.k * (Math.hypot(p.x - q.x, p.y - q.y) / Math.max(gesture.d, 1))));
        const after = toWorld(mid.x, mid.y);
        view.cam.x += before.x - after.x;
        view.cam.y += before.y - after.y;
        wake();
        return;
      }
      if (Math.hypot(at.x - gesture.start.x, at.y - gesture.start.y) > 6) gesture.moved = true;
      if (!gesture.moved) return;
      if (gesture.kind === 'drag') {
        const p = sim.pos.get(gesture.node.id);
        const w = toWorld(at.x, at.y);
        p.x = w.x;
        p.y = w.y;
        p.fixed = true;
        wake(0.25);
      } else if (gesture.kind === 'pan') {
        view.cam.x -= (at.x - gesture.last.x) / view.cam.k;
        view.cam.y -= (at.y - gesture.last.y) / view.cam.k;
        wake();
      }
      gesture.last = at;
    }

    function up(e) {
      if (!pointers.has(e.pointerId)) return;
      pointers.delete(e.pointerId);
      const g0 = gesture;
      if (pointers.size > 0) {
        const [rest] = [...pointers.values()];
        gesture = { kind: 'pan', start: rest, last: rest, moved: true };
        return;
      }
      gesture = null;
      if (!g0) return;
      if (g0.kind === 'drag' && g0.moved) {
        sim.pos.get(g0.node.id).fixed = false;
        return;
      }
      if (!g0.moved && g0.kind !== 'pinch') select(g0.node ? g0.node.id : null);
    }

    function wheel(e) {
      e.preventDefault();
      view.touched = true;
      const at = local(e);
      const before = toWorld(at.x, at.y);
      view.cam.k = Math.max(0.1, Math.min(5, view.cam.k * Math.exp(-e.deltaY * 0.0015)));
      const after = toWorld(at.x, at.y);
      view.cam.x += before.x - after.x;
      view.cam.y += before.y - after.y;
      wake();
    }

    function select(id, { center = false } = {}) {
      view.selected = id && g.byId.has(id) && shown.has(id) ? id : null;
      onSelect(view.selected ? g.byId.get(view.selected) : null);
      const p = view.selected ? sim.pos.get(view.selected) : null;
      if (p) {
        const s = toScreen(p);
        const r = region();
        const hidden = s.x < r.x0 + 20 || s.x > r.x1 - 120 || s.y < r.y0 + 20 || s.y > r.y1 - 20;
        if (center || hidden) {
          view.touched = true;
          if (center) view.cam.k = Math.max(view.cam.k, 1.1);
          aim(p.x + 50 / view.cam.k, p.y);
        }
      }
      wake();
    }

    function hide(top, off) {
      if (off) view.hidden.add(top);
      else view.hidden.delete(top);
      shown = visible(g, view.hidden);
      if (view.selected && !shown.has(view.selected)) select(null);
      wake(0.4);
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
    canvas.addEventListener('wheel', wheel, { passive: false });
    root.addEventListener('resize', resize);
    if (media && media.addEventListener) media.addEventListener('change', scheme);

    readColors();
    size();
    const warm = g.nodes.length > 600 ? 40 : 160;
    for (let i = 0; i < warm; i++) step(sim, shown);
    fit();
    wake();

    return {
      graph: g,
      select,
      hide,
      hidden: () => new Set(view.hidden),
      fit: () => {
        fit();
        wake();
      },
      selected: () => view.selected,
      positions: () => sim.pos,
      destroy() {
        if (frame) root.cancelAnimationFrame(frame);
        frame = 0;
        canvas.removeEventListener('pointerdown', down);
        canvas.removeEventListener('pointermove', move);
        canvas.removeEventListener('pointerup', up);
        canvas.removeEventListener('pointercancel', up);
        canvas.removeEventListener('wheel', wheel);
        root.removeEventListener('resize', resize);
        if (media && media.removeEventListener) media.removeEventListener('change', scheme);
      },
    };
  }

  root.leoGraph = { prepare, colorOf, counts, neighbors, connectedNotes, conceptDetail, bridges, visible, find, createLayout, step, bounds, create, topFolder };
})(typeof window !== 'undefined' ? window : globalThis);
