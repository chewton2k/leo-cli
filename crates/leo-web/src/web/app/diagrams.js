const MERMAID_URL = '/vendor/mermaid-11.4.1.js';
const DIAGRAMS_KEPT = 60;
const diagrams = { loading: null, seq: 0, scheme: '', drawn: new Map(), timer: 0 };

function diagramScheme() {
  return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'default';
}

function diagramTheme(dark) {
  const css = getComputedStyle(document.documentElement);
  const token = (name, fallback) => css.getPropertyValue(name).trim() || fallback;
  const accent = token('--accent', '#4f46e5');
  const soft = token('--accent-soft', '#eceafd');
  const text = token('--text', '#1c1c1a');
  const muted = token('--muted', '#6b6a65');
  const surface = token('--surface', '#ffffff');
  const surface2 = token('--surface-2', '#f0efec');
  const line = token('--line', '#e6e5e1');
  const tones = ['--accent', '--tone-doc', '--tone-sheet', '--tone-slides', '--tone-image', '--tone-pdf', '--tone-text'].map((t) => token(t, accent));
  const pies = Object.fromEntries(tones.map((tone, i) => [`pie${i + 1}`, tone]));
  return {
    darkMode: dark,
    background: surface,
    fontFamily: getComputedStyle(document.body).fontFamily,
    fontSize: '14px',
    primaryColor: soft,
    primaryBorderColor: accent,
    primaryTextColor: text,
    secondaryColor: surface2,
    secondaryBorderColor: line,
    secondaryTextColor: text,
    tertiaryColor: surface,
    tertiaryBorderColor: line,
    tertiaryTextColor: text,
    lineColor: muted,
    textColor: text,
    mainBkg: soft,
    nodeBorder: accent,
    clusterBkg: surface2,
    clusterBorder: line,
    edgeLabelBackground: surface,
    titleColor: text,
    noteBkgColor: surface2,
    noteTextColor: text,
    noteBorderColor: line,
    actorBkg: soft,
    actorBorder: accent,
    actorTextColor: text,
    signalColor: muted,
    signalTextColor: text,
    pieTitleTextSize: '17px',
    pieSectionTextColor: dark ? '#141414' : '#ffffff',
    pieSectionTextSize: '13px',
    pieLegendTextColor: text,
    pieStrokeColor: surface,
    pieOuterStrokeColor: line,
    pieOpacity: '0.9',
    ...pies,
  };
}

function loadMermaid() {
  if (window.mermaid) return Promise.resolve(window.mermaid);
  if (!diagrams.loading) {
    diagrams.loading = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = MERMAID_URL;
      script.onload = () => (window.mermaid ? resolve(window.mermaid) : reject(new Error('the diagram drawer did not start')));
      script.onerror = () => {
        diagrams.loading = null;
        reject(new Error('the diagram drawer could not be loaded'));
      };
      document.head.appendChild(script);
    });
  }
  return diagrams.loading;
}

const waitingDiagrams = () =>
  [...document.querySelectorAll('figure.diagram:not([data-drawn])')].filter((fig) => !fig.closest('.msg.pending'));
const diagramSource = (fig) => {
  const code = fig.querySelector('.diagram-src code');
  return code ? code.textContent : '';
};

function showDiagram(fig, svg) {
  const art = document.createElement('div');
  art.className = 'diagram-art';
  art.innerHTML = svg;
  fig.appendChild(art);
  fig.dataset.drawn = 'yes';
}

function failDiagram(fig, problem) {
  fig.dataset.drawn = 'failed';
  const note = document.createElement('figcaption');
  note.className = 'diagram-error';
  note.textContent = `This diagram could not be drawn: ${problem}`;
  fig.appendChild(note);
}

function keepDiagram(key, svg) {
  diagrams.drawn.delete(key);
  diagrams.drawn.set(key, svg);
  while (diagrams.drawn.size > DIAGRAMS_KEPT) diagrams.drawn.delete(diagrams.drawn.keys().next().value);
}

function drawKnownDiagrams() {
  const scheme = diagramScheme();
  for (const fig of waitingDiagrams()) {
    const svg = diagrams.drawn.get(`${scheme}\n${diagramSource(fig)}`);
    if (svg) showDiagram(fig, svg);
  }
}

async function drawDiagrams() {
  if (!waitingDiagrams().length) return;
  let mermaid;
  try {
    mermaid = await loadMermaid();
  } catch (e) {
    for (const fig of waitingDiagrams()) failDiagram(fig, e.message);
    return;
  }
  const scheme = diagramScheme();
  if (diagrams.scheme !== scheme) {
    mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: 'base', themeVariables: diagramTheme(scheme === 'dark') });
    diagrams.scheme = scheme;
  }
  for (const fig of waitingDiagrams()) {
    const source = diagramSource(fig);
    const key = `${scheme}\n${source}`;
    fig.dataset.drawn = 'working';
    const id = `leo-diagram-${++diagrams.seq}`;
    try {
      const { svg } = await mermaid.render(id, source);
      keepDiagram(key, svg);
      if (fig.isConnected) showDiagram(fig, svg);
    } catch (e) {
      const stray = document.getElementById(`d${id}`);
      if (stray) stray.remove();
      if (fig.isConnected) failDiagram(fig, String((e && e.message) || e).split('\n')[0].slice(0, 160));
    }
  }
}

function watchDiagrams() {
  drawKnownDiagrams();
  clearTimeout(diagrams.timer);
  diagrams.timer = setTimeout(() => drawDiagrams().catch(() => {}), 60);
}

new MutationObserver(watchDiagrams).observe(document.body, { childList: true, subtree: true });
if (window.matchMedia) {
  const scheme = window.matchMedia('(prefers-color-scheme: dark)');
  if (scheme.addEventListener) {
    scheme.addEventListener('change', () => {
      for (const fig of document.querySelectorAll('figure.diagram[data-drawn]')) {
        fig.removeAttribute('data-drawn');
        for (const old of fig.querySelectorAll('.diagram-art, .diagram-error')) old.remove();
      }
      watchDiagrams();
    });
  }
}
