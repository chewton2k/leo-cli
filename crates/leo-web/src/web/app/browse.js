let pageArrival = null;

function stopLibraryArrival() {
  if (pageArrival) pageArrival.cancel();
  pageArrival = null;
}

function arriveOnPage(element = app) {
  stopLibraryArrival();
  if (stillMotion() || typeof app.animate !== 'function') return;
  const from = element === app ? 'translateY(4px)' : 'none';
  const run = element.animate([{ opacity: .45, transform: from }, { opacity: 1, transform: 'none' }], { duration: MOTION_MS, easing: EASE_OUT });
  pageArrival = run;
  run.onfinish = () => { if (pageArrival === run) pageArrival = null; };
}
