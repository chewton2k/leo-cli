(function (root) {
  'use strict';

  const STEP = 32;
  const BIG_STEP = 96;
  let active = null;

  function create({ handle, key, measure, limits, apply, enabled, storage = null, onChange = () => {} }) {
    let preferred = null;
    let drag = null;
    let tap = null;
    try {
      storage ??= root.localStorage;
      const saved = Number(storage.getItem(key));
      if (Number.isFinite(saved) && saved > 0) preferred = saved;
    } catch (e) {}

    function keep() {
      try {
        if (preferred === null) storage.removeItem(key);
        else storage.setItem(key, String(preferred));
      } catch (e) {}
    }

    function fit() {
      if (!enabled()) return stop();
      const [min, max] = limits();
      apply(preferred === null ? null : Math.round(Math.min(max, Math.max(min, preferred))));
      const width = measure();
      if (width < min || width > max) apply(Math.round(Math.min(max, Math.max(min, width))));
      handle.setAttribute('aria-valuemin', String(min));
      handle.setAttribute('aria-valuemax', String(max));
      handle.setAttribute('aria-valuenow', String(Math.round(measure())));
    }

    function change(value) {
      const [min, max] = limits();
      preferred = value === null ? null : Math.round(Math.min(max, Math.max(min, value)));
      fit();
      onChange();
    }

    function stop(event) {
      if (!drag || (event?.pointerId !== undefined && event.pointerId !== drag.id)) return;
      const id = drag.id;
      const tapped = event?.type === 'pointerup' && Math.abs(event.clientX - drag.x) < 4;
      tap = tapped ? { at: event.timeStamp, x: event.clientX } : null;
      drag = null;
      if (handle.hasPointerCapture(id)) handle.releasePointerCapture(id);
      if (active === stop) {
        active = null;
        document.body.classList.remove('pane-resizing');
      }
      keep();
    }

    handle.addEventListener('pointerdown', (event) => {
      if (!enabled() || event.button !== 0) return;
      event.preventDefault();
      active?.();
      handle.focus({ preventScroll: true });
      if (tap && event.timeStamp - tap.at < 400 && Math.abs(event.clientX - tap.x) < 6) {
        tap = null;
        change(null);
        keep();
        return;
      }
      drag = { id: event.pointerId, x: event.clientX, width: measure() };
      active = stop;
      handle.setPointerCapture(event.pointerId);
      document.body.classList.add('pane-resizing');
    });
    handle.addEventListener('pointermove', (event) => {
      if (!drag || event.pointerId !== drag.id) return;
      if (!enabled() || !(event.buttons & 1)) return stop(event);
      if (!handle.hasPointerCapture(event.pointerId)) return stop(event);
      change(drag.width + event.clientX - drag.x);
    });
    for (const name of ['pointerup', 'pointercancel', 'lostpointercapture']) handle.addEventListener(name, stop);
    root.addEventListener('blur', stop);
    root.addEventListener('resize', fit);
    handle.addEventListener('keydown', (event) => {
      if (!enabled()) return;
      const step = event.shiftKey ? BIG_STEP : STEP;
      if (event.key === 'ArrowLeft') change(measure() - step);
      else if (event.key === 'ArrowRight') change(measure() + step);
      else if (event.key === 'Home') change(null);
      else if (event.key === 'End') change(limits()[1]);
      else return;
      event.preventDefault();
      keep();
    });
    fit();
    return { fit, stop };
  }

  root.leoPaneResize = { create };
})(typeof window === 'undefined' ? globalThis : window);
