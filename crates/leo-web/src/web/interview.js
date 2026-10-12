(function (root) {
  'use strict';
  const PRESETS = [
    { id: 'coding', label: 'Coding', about: 'Algorithms, clear code and edge cases', skills: ['Approach', 'Correctness', 'Complexity', 'Testing', 'Communication'] },
    { id: 'systems', label: 'System design', about: 'Requirements, architecture and trade-offs', skills: ['Requirements', 'Architecture', 'Scale', 'Reliability', 'Trade-offs'] },
    { id: 'design', label: 'Product & design', about: 'Users, whiteboard challenges and critique', skills: ['User needs', 'Ideation', 'Craft', 'Rationale', 'Communication'] },
    { id: 'marketing', label: 'Marketing', about: 'Audiences, campaigns and experiments', skills: ['Audience', 'Strategy', 'Experiments', 'Metrics', 'Communication'] },
    { id: 'business', label: 'Business & economics', about: 'Cases, assumptions and recommendations', skills: ['Structure', 'Analysis', 'Assumptions', 'Economics', 'Recommendation'] },
    { id: 'quant', label: 'Quant', about: 'Probability, estimation and rigorous reasoning', skills: ['Probability', 'Quantitative reasoning', 'Assumptions', 'Checks', 'Communication'] },
    { id: 'custom', label: 'Your own framework', about: 'Choose your role and assessment criteria', skills: ['Reasoning', 'Domain knowledge', 'Communication'] },
  ];
  const MOST_SHAPES = 100;
  const MOST_POINTS = 4000;
  const MOST_ANSWER = 32000;
  const escape = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const preset = (id) => PRESETS.find((p) => p.id === id) || PRESETS[0];
  const framework = (id) => ({ title: `${preset(id).label} practice`, track: preset(id).id, company: '', role: '', language: id === 'coding' ? 'Python' : '', style: 'coach', date: '', weekly_minutes: 120, skills: [...preset(id).skills], wants: '' });
  const point = (x, y) => [Math.round(Math.max(0, Math.min(1000, x))), Math.round(Math.max(0, Math.min(1000, y)))];
  function validBoard(board) {
    return Array.isArray(board) && board.length <= MOST_SHAPES && board.every((s) => s && typeof s === 'object') && board.reduce((n, s) => n + (Array.isArray(s.points) ? s.points.length : MOST_POINTS + 1), 0) <= MOST_POINTS
      && board.reduce((n, s) => n + (typeof s.text === 'string' ? [...s.text].length : 4001), 0) <= 4000
      && board.every((s) => ['pen', 'box', 'arrow', 'text'].includes(s.kind) && typeof s.text === 'string' && [...s.text].length <= 300
        && Array.isArray(s.points) && s.points.length > 0 && s.points.length <= 128 && s.points.every((p) => Array.isArray(p) && p.length === 2 && p.every((n) => Number.isFinite(n) && n >= 0 && n <= 1000)));
  }
  function boardSvg(board) {
    if (!validBoard(board)) return '';
    const shapes = board.map((s) => {
      const [x, y] = s.points[0];
      const [endX, endY] = s.points[s.points.length - 1];
      if (s.kind === 'text') return `<text x="${x}" y="${y}" fill="#19191b" stroke="none" font-family="system-ui, sans-serif" font-size="22">${escape(s.text)}</text>`;
      if (s.kind === 'box') return `<rect x="${Math.min(x, endX)}" y="${Math.min(y, endY)}" width="${Math.abs(endX - x)}" height="${Math.abs(endY - y)}" rx="8"/>`;
      if (s.kind === 'arrow') {
        const angle = Math.atan2(endY - y, endX - x);
        const a = point(endX - 18 * Math.cos(angle - Math.PI / 6), endY - 18 * Math.sin(angle - Math.PI / 6));
        const b = point(endX - 18 * Math.cos(angle + Math.PI / 6), endY - 18 * Math.sin(angle + Math.PI / 6));
        return `<path d="M${x},${y} L${endX},${endY} M${a.join(',')} L${endX},${endY} L${b.join(',')}"/>`;
      }
      return `<polyline points="${s.points.map((p) => p.join(',')).join(' ')}"/>`;
    }).join('');
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 700" role="img" aria-label="Interview whiteboard"><rect width="1000" height="700" fill="white"/><g fill="none" stroke="#19191b" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">${shapes}</g></svg>`;
  }
  function progress(attempts) {
    const grouped = new Map();
    for (const attempt of (attempts || []).slice(-100)) {
      if (!attempt.feedback) continue;
      for (const score of attempt.feedback.scores || []) {
        if (!Number.isFinite(score.value) || score.value < 0 || score.value > 4) continue;
        const values = grouped.get(score.skill) || [];
        values.push(score.value);
        grouped.set(score.skill, values);
      }
    }
    return [...grouped].map(([skill, values]) => ({ skill, n: values.length, value: values.slice(-5).reduce((a, b) => a + b, 0) / Math.min(5, values.length) })).sort((a, b) => a.value - b.value);
  }
  function agenda(slots, events) {
    const tasks = (slots || []).slice(0, 60).map((s) => ({ ...s, kind: 'practice', day: s.date, at: s.time || '' }));
    const calendar = (events || []).slice(0, 200).filter((e) => Number.isFinite(Date.parse(e.start))).map((e) => {
      const d = new Date(e.start);
      const pad = (n) => String(n).padStart(2, '0');
      return { ...e, kind: 'event', day: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`, at: `${pad(d.getHours())}:${pad(d.getMinutes())}` };
    });
    return [...tasks, ...calendar].sort((a, b) => a.day.localeCompare(b.day) || (a.at || '24:00').localeCompare(b.at || '24:00'));
  }
  const draftKey = (id, challenge) => `${id}:${challenge}`;
  function keepDraft(storage, id, challenge, workspace) {
    if (!validBoard(workspace.board) || [...workspace.answer].length > MOST_ANSWER) return false;
    try {
      const drafts = JSON.parse(storage.getItem('leo-interview-drafts') || '{}');
      const key = draftKey(id, challenge);
      delete drafts[key];
      drafts[key] = workspace;
      storage.setItem('leo-interview-drafts', JSON.stringify(Object.fromEntries(Object.entries(drafts).slice(-8))));
      return true;
    } catch (e) { return false; }
  }
  function readDraft(storage, id, challenge) {
    try {
      const draft = JSON.parse(storage.getItem('leo-interview-drafts') || '{}')[draftKey(id, challenge)];
      return draft && typeof draft.answer === 'string' && [...draft.answer].length <= MOST_ANSWER && validBoard(draft.board) ? draft : null;
    } catch (e) { return null; }
  }
  function clearDraft(storage, id, challenge) {
    try {
      const drafts = JSON.parse(storage.getItem('leo-interview-drafts') || '{}');
      delete drafts[draftKey(id, challenge)];
      storage.setItem('leo-interview-drafts', JSON.stringify(drafts));
    } catch (e) {}
  }
  root.leoInterview = { PRESETS, preset, framework, point, validBoard, boardSvg, progress, agenda, keepDraft, readDraft, clearDraft, MOST_SHAPES, MOST_POINTS, MOST_ANSWER };
})(typeof window !== 'undefined' ? window : globalThis);
