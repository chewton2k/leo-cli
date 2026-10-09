const KATEX = '/vendor/katex-0.16.11/';
const MATH_KEPT = 300;
const math = { loading: null, drawn: new Map(), timer: 0 };

function loadKatex() {
  if (window.katex) return Promise.resolve(window.katex);
  if (!math.loading) {
    math.loading = new Promise((resolve, reject) => {
      const sheet = document.createElement('link');
      sheet.rel = 'stylesheet';
      sheet.href = `${KATEX}katex.min.css`;
      document.head.appendChild(sheet);
      const script = document.createElement('script');
      script.src = `${KATEX}katex.min.js`;
      script.onload = () => (window.katex ? resolve(window.katex) : reject(new Error('the formula drawer did not start')));
      script.onerror = () => {
        math.loading = null;
        reject(new Error('the formula drawer could not be loaded'));
      };
      document.head.appendChild(script);
    });
  }
  return math.loading;
}

const waitingMath = () => [...document.querySelectorAll('.math:not([data-drawn])')].filter((el) => !el.closest('.msg.pending, .editing'));
const mathKey = (el) => `${el.classList.contains('math-block') ? 'block' : 'inline'}\n${el.dataset.tex || ''}`;

function placeMath(el, html) {
  el.innerHTML = html;
  el.dataset.drawn = 'yes';
}

function drawKnownMath() {
  for (const el of waitingMath()) {
    const html = math.drawn.get(mathKey(el));
    if (html) placeMath(el, html);
  }
}

async function drawMath() {
  if (!waitingMath().length) return;
  let katex;
  try {
    katex = await loadKatex();
  } catch (e) {
    for (const el of waitingMath()) el.dataset.drawn = 'failed';
    return;
  }
  for (const el of waitingMath()) {
    const key = mathKey(el);
    try {
      const html = katex.renderToString(el.dataset.tex || '', {
        displayMode: el.classList.contains('math-block'),
        throwOnError: false,
        trust: false,
        strict: 'ignore',
        output: 'htmlAndMathml',
      });
      math.drawn.set(key, html);
      if (math.drawn.size > MATH_KEPT) math.drawn.delete(math.drawn.keys().next().value);
      placeMath(el, html);
    } catch (e) {
      el.dataset.drawn = 'failed';
    }
  }
}

new MutationObserver(() => {
  drawKnownMath();
  clearTimeout(math.timer);
  math.timer = setTimeout(() => drawMath().catch(() => {}), 30);
}).observe(document.body, { childList: true, subtree: true });
