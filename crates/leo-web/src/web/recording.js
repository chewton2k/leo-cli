(function (root) {
  'use strict';

  const SEND_EVERY = 1000;
  const POLL_EVERY = 1000;
  const MOST_HELD_SECS = 120;
  const RATE = 16000;

  function clock(secs) {
    const s = Math.max(0, Math.floor(secs || 0));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const pad = (n) => String(n).padStart(2, '0');
    return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
  }

  function join(chunks) {
    const total = chunks.reduce((n, c) => n + c.length, 0);
    const out = new Int16Array(total);
    let at = 0;
    for (const c of chunks) {
      out.set(c, at);
      at += c.length;
    }
    return out;
  }

  function bytesOf(samples) {
    const view = new DataView(new ArrayBuffer(samples.length * 2));
    samples.forEach((s, i) => view.setInt16(i * 2, s, true));
    return new Uint8Array(view.buffer);
  }

  function trimHeld(chunks, mostSamples) {
    let total = chunks.reduce((n, c) => n + c.length, 0);
    let dropped = 0;
    while (chunks.length > 1 && total > mostSamples) {
      const gone = chunks.shift();
      total -= gone.length;
      dropped += gone.length;
    }
    return dropped;
  }

  function micReady(env) {
    if (!env.isSecureContext) return 'insecure';
    if (!env.mediaDevices || typeof env.mediaDevices.getUserMedia !== 'function') return 'unsupported';
    if (typeof env.AudioWorkletNode !== 'function') return 'unsupported';
    return 'ok';
  }

  function micProblem(reason) {
    const name = typeof reason === 'string' ? reason : reason && reason.name;
    if (name === 'insecure') return 'A browser only lends its microphone to a secure page. Open the https link leo serve prints (not the --local one), or open http://127.0.0.1 on the computer itself.';
    if (name === 'unsupported') return 'This browser cannot record here. Try a current Safari, Chrome, Edge or Firefox.';
    if (name === 'NotAllowedError' || name === 'SecurityError') return 'The browser was not allowed to use the microphone. Allow it for this site (the icon by the address), then try again.';
    if (name === 'NotFoundError' || name === 'OverconstrainedError') return 'No microphone was found on this device.';
    if (name === 'NotReadableError' || name === 'AbortError') return 'The microphone is busy or switched off. Close other apps using it, then try again.';
    return 'The microphone could not be opened' + (reason && reason.message ? ` (${reason.message})` : '') + '.';
  }

  const fedByBrowser = (source) => source === 'browser' || source === 'tab';

  function canShareSound(env) {
    return Boolean(env.isSecureContext && env.mediaDevices && typeof env.mediaDevices.getDisplayMedia === 'function' && typeof env.AudioWorkletNode === 'function');
  }

  function sharingProblem(reason) {
    const name = typeof reason === 'string' ? reason : reason && reason.name;
    if (name === 'no-audio') return 'The share had no sound. Share again and turn on “Share tab audio” (or “Share system audio”) in the browser’s window.';
    if (name === 'unsupported') return 'This browser cannot share a tab’s sound. Use Chrome or Edge on a computer, or record the computer’s sound from the page open on that computer.';
    if (name === 'NotAllowedError' || name === 'AbortError') return 'Nothing was shared, so nothing is recording.';
    return 'The tab’s sound could not be shared' + (reason && reason.message ? ` (${reason.message})` : '') + '.';
  }

  function stateWord(view) {
    if (!view) return '';
    if (view.state === 'paused') return 'Paused';
    if (view.state === 'recording') return 'Recording';
    if (view.state === 'starting') return 'Starting';
    if (view.state === 'writing') return view.steps ? `${view.step} ${view.steps[0]}/${view.steps[1]}` : view.step;
    if (view.state === 'done') return 'Saved';
    if (view.state === 'failed') return 'Stopped';
    return view.state;
  }

  const live = (view) => Boolean(view) && ['starting', 'recording', 'paused'].includes(view.state);

  function create(deps) {
    const { api, esc, toast, go, noteHash, felix, icons } = deps;
    const s = {
      view: null,
      overview: null,
      mine: false,
      stream: null,
      context: null,
      node: null,
      held: [],
      level: 0,
      sending: false,
      lost: 0,
      poll: 0,
      send: 0,
      wake: null,
      container: null,
      follow: true,
    };

    const sourceLabel = { browser: 'This device’s microphone', tab: 'A tab’s or screen’s sound', microphone: 'Computer’s microphone', screen: 'Computer’s sound' };
    const env = () => ({ isSecureContext: root.isSecureContext, mediaDevices: root.navigator && root.navigator.mediaDevices, AudioWorkletNode: root.AudioWorkletNode });
    const shown = (message) => Object.assign(new Error(message), { shown: true });

    async function microphone() {
      const ready = micReady(env());
      if (ready !== 'ok') throw shown(micProblem(ready));
      try {
        return await root.navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 } });
      } catch (e) {
        throw shown(micProblem(e));
      }
    }

    async function shareSound() {
      if (!canShareSound(env())) throw shown(sharingProblem('unsupported'));
      let stream;
      try {
        stream = await root.navigator.mediaDevices.getDisplayMedia({
          video: true,
          audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
          systemAudio: 'include',
          selfBrowserSurface: 'exclude',
        });
      } catch (e) {
        throw shown(sharingProblem(e));
      }
      if (!stream.getAudioTracks().length) {
        stream.getTracks().forEach((t) => t.stop());
        throw shown(sharingProblem('no-audio'));
      }
      return stream;
    }

    async function openMic(kind = 'browser') {
      s.stream = kind === 'tab' ? await shareSound() : await microphone();
      s.context = new root.AudioContext({ sinkId: { type: 'none' } });
      await s.context.audioWorklet.addModule('/recorder.js');
      const source = s.context.createMediaStreamSource(s.stream);
      s.node = new root.AudioWorkletNode(s.context, 'leo-recorder');
      s.node.port.onmessage = (event) => {
        s.level = Math.max(event.data.peak, s.level * 0.7);
        if (s.view && s.view.state === 'paused') return;
        s.held.push(new Int16Array(event.data.samples));
        s.lost += trimHeld(s.held, MOST_HELD_SECS * RATE);
      };
      source.connect(s.node);
      if (s.context.state === 'suspended') await s.context.resume();
      s.stream.getTracks().forEach((track) => {
        track.onended = () => {
          if (!live(s.view) || !s.mine) return;
          if (kind === 'tab') {
            note('Sharing stopped, so the recording stopped and is being saved.');
            stop().catch(() => {});
          } else if (track.kind === 'audio') {
            note('The microphone was disconnected. What was recorded is kept; stop to save it.');
          }
        };
      });
    }

    function closeMic() {
      if (s.node) s.node.port.onmessage = null;
      if (s.stream) s.stream.getTracks().forEach((t) => t.stop());
      if (s.context) s.context.close().catch(() => {});
      s.stream = null;
      s.context = null;
      s.node = null;
      s.level = 0;
    }

    async function keepAwake() {
      try {
        if (root.navigator.wakeLock && !s.wake) s.wake = await root.navigator.wakeLock.request('screen');
      } catch (e) {
        s.wake = null;
      }
    }

    function letSleep() {
      if (s.wake) s.wake.release().catch(() => {});
      s.wake = null;
    }

    async function flush() {
      if (s.sending || !s.view || !s.held.length) return;
      s.sending = true;
      const chunks = s.held.splice(0);
      try {
        const response = await root.fetch(`/api/record/${s.view.id}/audio`, {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/octet-stream' },
          body: bytesOf(join(chunks)),
        });
        if (response.status === 404 || response.status === 409) {
          detach();
        } else if (!response.ok) {
          s.held.unshift(...chunks);
        }
      } catch (e) {
        s.held.unshift(...chunks);
        s.lost += trimHeld(s.held, MOST_HELD_SECS * RATE);
      } finally {
        s.sending = false;
      }
    }

    function detach() {
      clearInterval(s.send);
      s.send = 0;
      closeMic();
      letSleep();
      s.mine = false;
      s.held = [];
    }

    function attach() {
      s.mine = true;
      clearInterval(s.send);
      s.send = setInterval(() => flush(), SEND_EVERY);
      keepAwake();
    }

    function note(text) {
      if (!s.view) return;
      if (!s.view.warnings.includes(text)) s.view.warnings.push(text);
      draw();
    }

    async function pollNow() {
      if (!s.view) return;
      try {
        const view = await api(`/api/record/${s.view.id}`);
        accept(view);
      } catch (e) {
        if (e.status === 404) {
          detach();
          s.view = null;
          draw();
          return;
        }
      }
      schedule();
    }

    function schedule() {
      clearTimeout(s.poll);
      if (s.view && (live(s.view) || s.view.state === 'writing')) s.poll = setTimeout(pollNow, POLL_EVERY);
    }

    function accept(view) {
      const before = s.view;
      const warnings = before && before.id === view.id ? before.warnings.filter((w) => !view.warnings.includes(w)) : [];
      s.view = { ...view, warnings: [...view.warnings, ...warnings] };
      if (!live(view) && s.mine) {
        if (view.state !== 'writing' || !s.held.length) detach();
      }
      if (view.state === 'done' && before && before.state !== 'done') {
        detach();
        toast('Your recording is now a note.', { action: 'Open', run: () => go(noteHash(view.note)) });
        if (s.container && s.container.isConnected) go(noteHash(view.note), { replace: true });
      }
      draw();
      pill();
    }

    async function start(source, directory, title) {
      if (fedByBrowser(source)) await openMic(source);
      let made;
      try {
        made = await api('/api/record', { method: 'POST', body: { source, directory, title } });
      } catch (e) {
        closeMic();
        throw e;
      }
      s.view = { id: made.id, source, state: 'starting', secs: 0, step: 'Starting', steps: null, transcript: '', warnings: [], points: [], note: null, error: null };
      s.lost = 0;
      s.follow = true;
      if (fedByBrowser(source)) attach();
      draw();
      pill();
      schedule();
    }

    async function rejoin() {
      if (!s.view || !fedByBrowser(s.view.source)) return;
      await openMic(s.view.source);
      attach();
      draw();
    }

    async function setPaused(paused) {
      if (!s.view) return;
      if (paused) await flush();
      accept(await api(`/api/record/${s.view.id}/pause`, { method: 'POST', body: { paused } }));
    }

    async function stop() {
      if (!s.view) return;
      if (s.mine) {
        if (s.node) s.node.port.onmessage = null;
        while (s.sending) await new Promise((resolve) => setTimeout(resolve, 50));
        await flush();
      }
      const view = await api(`/api/record/${s.view.id}/stop`, { method: 'POST' });
      closeMic();
      letSleep();
      clearInterval(s.send);
      s.send = 0;
      s.mine = false;
      accept(view);
      schedule();
    }

    async function addPoint(text) {
      if (!s.view || !text.trim()) return;
      accept(await api(`/api/record/${s.view.id}/point`, { method: 'POST', body: { text } }));
    }

    function levelBars() {
      const n = 14;
      const lit = Math.round(Math.min(1, s.level * 3) * n);
      return Array.from({ length: n }, (_, i) => `<i class="${i < lit ? 'on' : ''}"></i>`).join('');
    }

    function sourceChoices(local) {
      const sources = ['browser', ...(canShareSound(env()) ? ['tab'] : []), ...(local ? ['microphone', 'screen'] : [])];
      if (sources.length === 1) return '';
      return `<div class="rec-sources" role="radiogroup" aria-label="What to record">${sources
        .map((id, i) => `<label class="rec-source"><input type="radio" name="rec-source" value="${id}"${i === 0 ? ' checked' : ''}><span>${esc(sourceLabel[id])}</span></label>`)
        .join('')}</div>`;
    }

    function drawIdle(folders, here) {
      const ov = s.overview || {};
      const options = ['', ...folders].map((f) => `<option value="${esc(f)}"${f === here ? ' selected' : ''}>${esc(f || 'All notes (top level)')}</option>`).join('');
      const ready = micReady({ isSecureContext: root.isSecureContext, mediaDevices: root.navigator && root.navigator.mediaDevices, AudioWorkletNode: root.AudioWorkletNode });
      const insecure = ready !== 'ok' && !ov.local ? `<p class="rec-warn">${esc(micProblem(ready))}</p>` : '';
      return `<div class="rec rec-idle">
        <div class="rec-hero">${felix.felix(64, 'idle')}<div><h2>Record</h2><p class="sub">Lectures and meetings become notes. leo transcribes as you go and writes them up when you stop.</p></div></div>
        ${sourceChoices(ov.local)}
        <label class="field">${icons.folder}<select id="rec-dir">${options}</select></label>
        <label class="field">${icons.note}<input id="rec-title" placeholder="Title (optional; the AI names it otherwise)" autocomplete="off"></label>
        ${insecure}
        <button class="rec-go" data-action="rec-start" ${ov.available === false ? 'disabled' : ''}><span class="rec-dot"></span><span>Start recording</span></button>
        <p class="hint rec-tip">On a phone, keep this page open while it records; the screen stays on. Points you jot while recording are woven into the notes.${canShareSound(env()) ? ' For a lecture video or call in another tab, choose “A tab’s or screen’s sound” and turn on “Share tab audio”.' : ''}</p>
      </div>`;
    }

    function drawLive() {
      const v = s.view;
      const paused = v.state === 'paused';
      const orphan = fedByBrowser(v.source) && !s.mine && live(v);
      const transcript = v.transcript ? esc(v.transcript) : `<span class="hint">${paused ? 'Paused.' : 'Listening… words appear here after a few seconds.'}</span>`;
      const points = v.points.length ? `<ul class="rec-points">${v.points.map(([at, text]) => `<li><span class="hint">${clock(at)}</span> ${esc(text)}</li>`).join('')}</ul>` : '';
      const warnings = v.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('') + (s.lost ? `<p class="rec-warn">${clock(s.lost / RATE)} of audio could not reach leo and was dropped.</p>` : '');
      const controls = orphan
        ? `<p class="rec-warn">This recording lost its microphone when the page reloaded.</p><div class="rec-controls"><button class="btn plain" data-action="rec-rejoin">Keep recording here</button><button class="btn primary" data-action="rec-stop">Stop and save</button></div>`
        : `<div class="rec-controls"><button class="btn plain" data-action="rec-pause">${paused ? 'Resume' : 'Pause'}</button><button class="btn primary rec-stop" data-action="rec-stop"><span class="rec-square"></span>Stop and save</button></div>`;
      return `<div class="rec rec-live${paused ? ' paused' : ''}">
        <div class="rec-clock"><span class="rec-dot"></span><span id="rec-time">${clock(v.secs)}</span><span class="rec-word" id="rec-word">${esc(stateWord(v))}</span></div>
        <div class="rec-meta">${esc(sourceLabel[v.source] || '')}${s.mine ? `<span class="rec-level" id="rec-level" aria-hidden="true">${levelBars()}</span>` : ''}</div>
        ${controls}
        ${warnings}
        <div class="rec-transcript" id="rec-transcript" aria-live="polite">${transcript}</div>
        <div class="rec-point"><textarea id="rec-point-text" rows="2" placeholder="Jot a point to weave into the notes"></textarea><button class="btn plain" data-action="rec-point">Add</button></div>
        ${points}
      </div>`;
    }

    function drawWriting() {
      const v = s.view;
      const bar = v.steps ? (v.steps[0] / Math.max(1, v.steps[1])) * 100 : 0;
      return `<div class="rec rec-writing">${felix.felix(72, 'idle think')}<h3>${esc(stateWord(v) || 'Writing the notes')}</h3>
        <div class="upload-bar"><i style="width:${Math.max(6, bar)}%"></i></div>
        ${v.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('')}
        <p class="hint">${clock(v.secs)} recorded. You can leave this page; the note appears in its folder when it is ready.</p></div>`;
    }

    function drawFailed() {
      const v = s.view;
      return `<div class="rec rec-writing">${felix.felix(72, 'droop')}<h3>The recording could not become a note</h3>
        <p class="upload-error">${esc(v.error || 'Something went wrong.')}</p>
        ${v.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('')}
        <div class="buttons"><button class="btn plain" data-action="settings">Settings</button><button class="btn primary" data-action="rec-again">Record again</button></div></div>`;
    }

    let folders = [];
    let here = '';

    function draw() {
      const box = s.container;
      if (!box || !box.isConnected) return;
      const v = s.view;
      const typed = box.querySelector('#rec-point-text');
      const keep = typed ? { value: typed.value, focused: root.document.activeElement === typed, start: typed.selectionStart, end: typed.selectionEnd } : null;
      const old = box.querySelector('#rec-transcript');
      if (old) s.follow = old.scrollTop + old.clientHeight >= old.scrollHeight - 24;
      if (!v || v.state === 'done') box.innerHTML = drawIdle(folders, here);
      else if (live(v)) box.innerHTML = drawLive();
      else if (v.state === 'writing') box.innerHTML = drawWriting();
      else box.innerHTML = drawFailed();
      const area = box.querySelector('#rec-point-text');
      if (area && keep) {
        area.value = keep.value;
        if (keep.focused) {
          area.focus();
          area.setSelectionRange(keep.start, keep.end);
        }
      }
      if (area) {
        area.addEventListener('keydown', (e) => {
          if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            submitPoint();
          }
        });
      }
      const text = box.querySelector('#rec-transcript');
      if (text && s.follow) text.scrollTop = text.scrollHeight;
    }

    function submitPoint() {
      const area = s.container && s.container.querySelector('#rec-point-text');
      if (!area || !area.value.trim()) return;
      const text = area.value;
      area.value = '';
      addPoint(text).catch((e) => {
        area.value = text;
        toast(e.message, { bad: true });
      });
    }

    function tick() {
      const level = root.document.getElementById('rec-level');
      if (level) level.innerHTML = levelBars();
      s.level *= 0.85;
    }
    setInterval(tick, 120);

    function pill() {
      let el = root.document.getElementById('rec-pill');
      const show = s.view && live(s.view) && !(s.container && s.container.isConnected);
      if (!show) {
        if (el) el.remove();
        return;
      }
      if (!el) {
        el = root.document.createElement('button');
        el.id = 'rec-pill';
        el.className = 'rec-pill';
        el.dataset.action = 'record';
        root.document.body.appendChild(el);
      }
      el.classList.toggle('paused', s.view.state === 'paused');
      el.innerHTML = `<span class="rec-dot"></span>${s.view.state === 'paused' ? 'Paused' : 'Recording'} ${clock(s.view.secs)}`;
    }

    async function refresh() {
      s.overview = await api('/api/record');
      const job = s.overview.job;
      if (job && (live(job) || job.state === 'writing')) {
        if (!s.view || s.view.id !== job.id) s.mine = false;
        accept(job);
        schedule();
      } else if (s.view && !live(s.view) && s.view.state !== 'writing') {
        s.view = null;
      }
    }

    root.addEventListener('beforeunload', (e) => {
      if (s.mine && live(s.view)) {
        e.preventDefault();
        e.returnValue = '';
      }
    });

    root.document.addEventListener('visibilitychange', () => {
      if (root.document.visibilityState === 'visible' && s.mine && live(s.view)) keepAwake();
    });

    return {
      async show(container, options) {
        s.container = container;
        folders = options.folders || [];
        here = options.dir || '';
        draw();
        pill();
        await refresh();
        draw();
        pill();
      },
      leave() {
        s.container = null;
        pill();
      },
      async begin() {
        const box = s.container;
        const picked = box.querySelector('input[name="rec-source"]:checked');
        const source = picked ? picked.value : 'browser';
        const button = box.querySelector('[data-action="rec-start"]');
        if (button) button.disabled = true;
        try {
          await start(source, box.querySelector('#rec-dir').value, box.querySelector('#rec-title').value.trim());
        } finally {
          if (button && button.isConnected) button.disabled = false;
        }
      },
      pause: () => setPaused(!(s.view && s.view.state === 'paused')),
      stop,
      rejoin,
      point: submitPoint,
      again() {
        s.view = null;
        draw();
      },
      active: () => live(s.view),
    };
  }

  root.leoRecording = { create, clock, join, bytesOf, trimHeld, micReady, micProblem, stateWord, canShareSound, sharingProblem, fedByBrowser };
})(typeof window !== 'undefined' ? window : globalThis);
