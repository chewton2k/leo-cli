(function (root) {
  'use strict';

  const SEND_EVERY = 1000;
  const WANTS_SAVE_AFTER = 900;
  const POLL_EVERY = 1000;
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

  const fedByBrowser = (source) => source === 'browser' || source === 'tab' || source === 'call';

  const QUIET = 0.2;
  const WAVE_STEP_MS = 60;
  const SERVER_STEP_MS = 250;
  const MOST_WAVE = 400;
  const BAR = 3;
  const GAP = 2.5;

  function ease(current, target) {
    const rate = target > current ? 0.55 : 0.16;
    return current + (target - current) * rate;
  }

  function spread(from, to, steps) {
    return Array.from({ length: steps }, (_, i) => from + ((to - from) * (i + 1)) / steps);
  }

  function freshLevels(levels, start, fedUpTo) {
    const end = start + levels.length;
    if (fedUpTo >= end) return { fresh: [], fedUpTo };
    const from = Math.max(fedUpTo, start);
    return { fresh: levels.slice(from - start), fedUpTo: end };
  }

  function sourceFor(kind, { local, canShare }) {
    if (kind === 'microphone') return 'browser';
    if (local) return 'screen';
    return canShare ? 'tab' : null;
  }

  function loudness(rms) {
    if (!(rms > 0)) return 0;
    const db = 20 * Math.log10(rms);
    return Math.max(0, Math.min(1, (db + 54) / 48));
  }

  function hearing(levels, stepMs) {
    if (!levels.length) return 'waiting';
    let quietMs = 0;
    for (let i = levels.length - 1; i >= 0 && levels[i] < QUIET; i--) quietMs += stepMs;
    if (quietMs < 1500) return 'sound';
    return quietMs < 6000 ? 'quiet' : 'silent';
  }

  function hearingWords(state, source, paused) {
    if (paused) return 'Paused: nothing is being recorded';
    if (state === 'sound') return 'Hearing sound';
    if (state === 'waiting' || state === 'quiet') return 'Listening…';
    if (source === 'tab') return 'No sound for a while: is the tab playing, with “Share tab audio” on?';
    if (source === 'screen') return 'No sound for a while: is something playing on the computer?';
    return 'No sound for a while: is the right microphone chosen, and not muted?';
  }

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

  const MEETING_WANTS = 'Meeting notes: what was discussed, the decisions made, open questions, and action items with owners and due dates when they were said.';
  const SOON_MS = 15 * 60 * 1000;

  function eventNow(events, now = Date.now()) {
    return (events || []).findIndex((e) => Date.parse(e.start) - SOON_MS <= now && now <= Date.parse(e.end));
  }

  function whenOf(e) {
    const start = new Date(e.start);
    const end = new Date(e.end);
    const day = start.toDateString() === new Date().toDateString() ? 'Today' : start.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
    const time = (d) => d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
    return `${day} ${time(start)}–${time(end)}`;
  }

  function create(deps) {
    const { api, esc, toast, go, noteHash, felix, icons, noteReady } = deps;
    const s = {
      view: null,
      overview: null,
      mine: false,
      stream: null,
      micStream: null,
      seq: 0,
      pending: 0,
      config: null,
      calendar: null,
      event: null,
      eventChosen: false,
      context: null,
      node: null,
      held: [],
      wave: [],
      queue: [],
      smooth: 0,
      lastStep: 0,
      lastFed: 0,
      fedUpTo: 0,
      samples: null,
      analyser: null,
      raf: 0,
      heard: '',
      sending: false,
      lost: 0,
      deviceLost:false,
      showHeard: (() => {
        try {
          return root.localStorage.getItem('leo-rec-heard') === 'open';
        } catch (e) {
          return false;
        }
      })(),
      wantsDraft: null,
      wantsState: '',
      wantsTimer: 0,
      poll: 0,
      send: 0,
      wake: null,
      container: null,
      follow: true,
    };

    const audioQueue = root.leoAudioQueue && root.leoAudioQueue.pick();
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
      s.stream = kind === 'tab' || kind === 'call' ? await shareSound() : await microphone();
      if (kind === 'call') { try { s.micStream = await microphone(); } catch(e) { closeMic(); throw e; } }
      s.context = new root.AudioContext({ sinkId: { type: 'none' } });
      await s.context.audioWorklet.addModule('/recorder.js');
      const source = s.context.createMediaStreamSource(s.stream);
      s.node = new root.AudioWorkletNode(s.context, 'leo-recorder', { channelCount: kind === 'call' ? 2 : 1, channelCountMode:'explicit', processorOptions:{dual:kind === 'call'} });
      s.analyser = s.context.createAnalyser();
      s.analyser.fftSize = 2048;
      s.samples = new Float32Array(s.analyser.fftSize);
      source.connect(s.analyser);
      s.node.port.onmessage = (event) => {
        if (s.view && s.view.state === 'paused') return;
        s.held.push(new Int16Array(event.data.samples));

      };
      if (kind === 'call') {
        const merger = s.context.createChannelMerger(2);
        const mic = s.context.createMediaStreamSource(s.micStream);
        mic.connect(merger,0,0); source.connect(merger,0,1); merger.connect(s.node);
      } else source.connect(s.node);
      if (s.context.state === 'suspended') await s.context.resume();
      s.deviceLost=false;
      if (s.micStream) s.micStream.getAudioTracks().forEach((track) => { track.onended=() => { s.deviceLost=true; note('The microphone disconnected. Reconnect it to keep capturing your side of the call.'); }; });
      s.stream.getTracks().forEach((track) => {
        track.onended = () => {
          if (!live(s.view) || !s.mine) return;
          if (kind === 'tab' || kind === 'call') {
            note('Sharing stopped, so the recording stopped and is being saved.');
            stop().catch(() => {});
          } else if (track.kind === 'audio') {
            s.deviceLost=true; note('The microphone was disconnected. What was recorded is kept; stop to save it.');
          }
        };
      });
    }

    function closeMic() {
      if (s.node) s.node.port.onmessage = null;
      if (s.stream) s.stream.getTracks().forEach((t) => { t.onended=null; t.stop(); });
      if (s.micStream) s.micStream.getTracks().forEach((t) => { t.onended=null; t.stop(); });
      s.micStream=null;
      if (s.context) s.context.close().catch(() => {});
      s.stream = null;
      s.context = null;
      s.node = null;
      s.analyser = null;
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

    async function sendChunk(item) {
      const controller=new AbortController(); const timer=setTimeout(() => controller.abort(),15000);
      try {
        const response=await root.fetch(`/api/record/${item.id}/audio?seq=${item.seq}`,{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/octet-stream'},body:item.bytes,signal:controller.signal});
        if (!response.ok) throw new Error('Audio is kept on this device until Leo reconnects.');
        return (await response.json()).next_seq;
      } finally { clearTimeout(timer); }
    }
    async function flush(all=false) {
      if (s.sending || !s.view) return false;
      s.sending=true;
      try {
        if (!audioQueue) throw new Error('Recovery audio storage is unavailable.');
        if (s.held.length) {
          const chunks=s.held.splice(0);
          const saved=await root.leoAudioQueue.spool(audioQueue,s.view.id,s.seq,chunks,bytesOf);
          s.seq=saved.next;
          if (saved.error) { s.held=saved.remaining.concat(s.held); closeMic(); note('Recovery storage could not be written. Capture is stopped; the audio already kept is available. Free space, then reconnect.'); throw saved.error; }
        }
        s.pending=await audioQueue.count(s.view.id);
        await root.leoAudioQueue.drain(audioQueue,s.view.id,sendChunk,all?Infinity:3);
        s.pending=await audioQueue.count(s.view.id); return s.pending===0;
      } catch(e) { note(e.message || 'Audio is kept on this device until Leo reconnects.'); return false; }
      finally { s.sending=false; }
    }

    function detach() {
      clearInterval(s.send);
      s.send = 0;
      closeMic();
      letSleep();
      s.mine = false;
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
      feedFromServer(view);
      if (!live(view) && s.mine) {
        if (view.state !== 'writing' || !s.held.length) detach();
      }
      if (view.state === 'done' && before && before.state !== 'done') {
        detach();
        if (s.container && s.container.isConnected) go(noteHash(view.note), { replace: true });
        else if (noteReady) noteReady();
        toast('Your recording is now a note.', { action: 'Open', run: () => go(noteHash(view.note)) });
      }
      draw();
      pill();
    }

    async function start(source, directory, title, profile) {
      if (fedByBrowser(source)) await openMic(source);
      let made;
      try {
        made = await api('/api/record', { method: 'POST', body: { source, directory, title, profile } });
      } catch (e) {
        closeMic();
        throw e;
      }
      s.view = { id: made.id, source, state: 'starting', secs: 0, step: 'Starting', steps: null, transcript: '', warnings: [], points: [], note: null, error: null };
      s.lost = 0; s.seq=0; s.pending=0; s.wantsDraft = null; s.wantsState = '';
      s.follow = true;
      Object.assign(s, { wave: [], queue: [], smooth: 0, lastStep: 0, lastFed: 0, fedUpTo: 0 });
      if (fedByBrowser(source)) attach();
      draw();
      pill();
      schedule();
    }

    async function rejoin() {
      if (!s.view || !fedByBrowser(s.view.source)) return;
      const queued=audioQueue ? await audioQueue.list(s.view.id) : [];
      s.seq=Math.max(s.view.next_seq || 0,...queued.map((c) => c.seq+1));
      await flush();
      closeMic(); await openMic(s.view.source);
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
        if (s.context) await s.context.suspend();
        if (s.node) {
          const original=s.node.port.onmessage;
          await new Promise((resolve) => {
            const timer=setTimeout(resolve,1500);
            s.node.port.onmessage=(event) => { if (event.data.flushed) { clearTimeout(timer); resolve(); } else if (original) original(event); };
            s.node.port.postMessage({flush:true});
          });
          s.node.port.onmessage=null;
        }
        while (s.sending) await new Promise((resolve) => setTimeout(resolve, 50));
        if (!await flush(true)) throw new Error('Audio is still waiting on this device. Reconnect to Leo, then stop again.');
      } else if (audioQueue && (await audioQueue.list(s.view.id)).length && !await flush()) {
        throw new Error('Recovery audio is waiting on this device. Reconnect before saving.');
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

    function heardNow() {
      if (!s.view || s.view.state === 'paused') return 0;
      if (s.mine && s.analyser) {
        s.analyser.getFloatTimeDomainData(s.samples);
        let sum = 0;
        for (const v of s.samples) sum += v * v;
        return loudness(Math.sqrt(sum / s.samples.length));
      }
      if (s.queue.length > 40) s.queue.splice(0, s.queue.length - 40);
      return s.queue.length ? s.queue.shift() : s.smooth * 0.85;
    }

    function feedFromServer(view) {
      if (s.mine || !view || !Array.isArray(view.levels)) return;
      const { fresh, fedUpTo } = freshLevels(view.levels.map(loudness), view.levels_start || 0, s.fedUpTo);
      s.fedUpTo = fedUpTo;
      const per = Math.round(SERVER_STEP_MS / WAVE_STEP_MS);
      for (const level of fresh) {
        s.queue.push(...spread(s.lastFed, level, per));
        s.lastFed = level;
      }
    }

    function advance(now) {
      if (!s.lastStep || now - s.lastStep > 2000) s.lastStep = now;
      while (now - s.lastStep >= WAVE_STEP_MS) {
        s.smooth = ease(s.smooth, heardNow());
        s.wave.push(s.smooth);
        s.lastStep += WAVE_STEP_MS;
      }
      if (s.wave.length > MOST_WAVE) s.wave.splice(0, s.wave.length - MOST_WAVE);
    }

    function drawWave(now) {
      s.raf = 0;
      const canvas = root.document.getElementById('rec-wave');
      if (!canvas || !s.view || !live(s.view)) return;
      advance(now);
      const paused = s.view.state === 'paused';
      const ratio = root.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
        canvas.width = Math.round(width * ratio);
        canvas.height = Math.round(height * ratio);
      }
      const ctx = canvas.getContext('2d');
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      ctx.clearRect(0, 0, width, height);
      const css = root.getComputedStyle(canvas);
      const on = css.getPropertyValue('--wave-on').trim() || '#d0342c';
      const off = css.getPropertyValue('--wave-off').trim() || '#c9c8c3';
      const pitch = BAR + GAP;
      const glide = paused ? 0 : Math.min(1, (now - s.lastStep) / WAVE_STEP_MS);
      const count = Math.ceil(width / pitch) + 2;
      for (let i = 0; i < count; i++) {
        const level = s.wave[s.wave.length - 1 - i] || 0;
        const x = width - BAR - (i + glide) * pitch;
        if (x < -BAR) break;
        const h = Math.max(3, level * (height - 6));
        ctx.fillStyle = paused || level < 0.06 ? off : on;
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(x, (height - h) / 2, BAR, h, BAR / 2);
        else ctx.rect(x, (height - h) / 2, BAR, h);
        ctx.fill();
      }
      const state = hearing(s.wave.slice(-200), WAVE_STEP_MS);
      const words = hearingWords(state, s.view.source, paused);
      const label = root.document.getElementById('rec-hear');
      if (label && s.heard !== `${state}:${words}`) {
        s.heard = `${state}:${words}`;
        label.textContent = words;
        label.className = `rec-hear ${paused ? 'paused' : state}`;
      }
      s.raf = root.requestAnimationFrame(drawWave);
    }

    function startWave() {
      if (!s.raf && root.requestAnimationFrame) s.raf = root.requestAnimationFrame(drawWave);
    }

    function paintWaveNow() {
      if (!root.requestAnimationFrame) return;
      if (s.raf && root.cancelAnimationFrame) root.cancelAnimationFrame(s.raf);
      s.raf = 0;
      drawWave(root.performance ? root.performance.now() : Date.now());
    }

    const KINDS = {
      microphone: { title: 'Microphone', about: 'A lecture, a meeting, or your own voice' },
      call: { title: 'Call', about: 'Your microphone and the shared tab, on separate tracks' },
      screen: { title: 'Screen', about: 'A video, a call, or anything playing' },
    };

    function screenNote(local) {
      if (local) return 'Records what this computer plays';
      return canShareSound(env()) ? 'Pick the tab or screen and turn on “Share tab audio”' : 'Needs Chrome or Edge on a computer';
    }

    function kindChoices(local) {
      const canShare = canShareSound(env());
      return `<div class="rec-kinds" role="radiogroup" aria-label="What to record">${['microphone', 'screen', 'call']
        .map((kind, i) => {
          const off = kind === 'call' ? !canShare : !sourceFor(kind, { local, canShare });
          return `<label class="rec-kind${off ? ' off' : ''}"><input type="radio" name="rec-kind" value="${kind}"${i === 0 ? ' checked' : ''}${off ? ' disabled' : ''}>
            <span class="rec-kind-body"><span class="rec-kind-icon">${kind === 'microphone' ? icons.mic : icons.screen}</span><b>${KINDS[kind].title}</b><span class="sub">${esc(kind === 'screen' ? screenNote(local) : KINDS[kind].about)}</span></span></label>`;
        })
        .join('')}</div>`;
    }

    function drawIdle(folders, here) {
      const ov = s.overview || {};
      const options = ['', ...folders].map((f) => `<option value="${esc(f)}"${f === here ? ' selected' : ''}>${esc(f || 'All notes (top level)')}</option>`).join('');
      const upcoming = (s.calendar && s.calendar.events) || [];
      const picked = s.event !== null ? upcoming[s.event] : null;
      const pending=ov.pending || [];
      const ready = micReady(env());
      const insecure = ready !== 'ok' && !ov.local ? `<p class="rec-warn">${esc(micProblem(ready))}</p>` : '';
      return `<div class="rec rec-idle">
        <div class="section-title">Record</div>
        <section class="set-card rec-card">
          <div class="rec-intro">${felix.felix(46, 'idle')}<p>Lectures and meetings become notes. leo writes down what is said as you go, then turns it into notes when you stop.</p></div>
          ${kindChoices(ov.local)}
          <label class="set-row"><span class="set-label">Folder</span><select id="rec-dir">${options}</select></label>
          <label class="set-row"><span class="set-label">Title</span><input id="rec-title" class="rec-title-input" placeholder="Optional; the AI names it otherwise" autocomplete="off"></label>
          ${upcoming.length ? `<label class="set-row"><span class="set-label">Calendar event</span><select id="rec-event"><option value="">None</option>${upcoming.map((e, i) => `<option value="${i}"${i === s.event ? ' selected' : ''}>${esc(whenOf(e))} · ${esc(e.title)}</option>`).join('')}</select></label>
          ${picked ? `<p class="hint rec-event-note">The AI gets this event’s description, place and who was invited, so names and topics come out right.</p>` : ''}` : ''}
          <label class="source-field">What do you want from the notes?<textarea id="rec-wants" rows="2" maxlength="4000" placeholder="For example: focus on what will be on the exam, keep it short, explain every formula"></textarea></label>
          ${s.calendar && !s.calendar.connected ? `<p class="hint">Want recordings named after your meetings and lectures? <a href="#/settings">Connect a calendar in Settings.</a></p>` : ''}
          ${pending.map((p) => `<button class="list-row" data-action="rec-recover" data-id="${esc(p.id)}">Recover: ${esc(p.title || 'Interrupted recording')}</button>`).join('')}
          ${insecure}
          <div class="rec-start">
            <button class="rec-go" data-action="rec-start" aria-label="Start recording" ${ov.available === false ? 'disabled' : ''}><span class="rec-go-dot"></span></button>
            <span class="rec-start-label">Start recording</span>
          </div>
        </section>
        <p class="hint rec-tip">On a phone, keep this page open while it records; the screen stays on. Points you jot while recording are woven into the notes.</p>
      </div>`;
    }

    function drawLive() {
      const v = s.view;
      const paused = v.state === 'paused';
      const orphan = fedByBrowser(v.source) && !s.mine && live(v);
      const transcript = v.transcript ? esc(v.transcript) : `<span class="hint">${paused ? 'Paused.' : 'Listening… words appear here after a few seconds.'}</span>`;
      const points = v.points.length ? `<ul class="rec-points">${v.points.map(([at, text]) => `<li><span class="rec-at">${clock(at)}</span><span>${esc(text)}</span></li>`).join('')}</ul>` : '';
      const warnings = v.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('') + (s.lost ? `<p class="rec-warn">${clock(s.lost / RATE)} of audio could not reach leo and was dropped.</p>` : '');
      const kind = v.source === 'call' ? 'Call · You + Others' : v.source === 'browser' || v.source === 'microphone' ? 'Microphone' : 'Screen';
      const controls = orphan
        ? `<p class="rec-warn">This recording lost its microphone when the page reloaded.</p><div class="rec-controls"><button class="btn plain" data-action="rec-rejoin">Keep recording here</button><button class="btn primary" data-action="rec-stop">Stop and save</button></div>`
        : `<div class="rec-buttons">
            <button class="rec-round plain" data-action="rec-pause" aria-label="${paused ? 'Resume' : 'Pause'}">${paused ? '<svg viewBox="0 0 24 24"><path d="M8 5l12 7-12 7z" fill="currentColor"/></svg>' : '<svg viewBox="0 0 24 24"><rect x="6" y="5" width="4" height="14" rx="1" fill="currentColor"/><rect x="14" y="5" width="4" height="14" rx="1" fill="currentColor"/></svg>'}<span>${paused ? 'Resume' : 'Pause'}</span></button>
            <button class="rec-round stop rec-stop" data-action="rec-stop" aria-label="Stop and save"><span class="rec-square"></span><span>Stop and save</span></button>
          </div>`;
      return `<div class="rec rec-live${paused ? ' paused' : ''}">
        <div class="section-title">Recording</div>
        <section class="set-card rec-live-card">
          <div class="rec-live-top"><span class="rec-state"><span class="rec-dot"></span><span id="rec-word">${esc(stateWord(v))}</span></span><span class="rec-meta">${kind}</span></div>
          <div class="rec-clock"><span id="rec-time">${clock(v.secs)}</span></div>
          ${orphan ? '' : `<div class="rec-wave-box"><canvas class="rec-wave" id="rec-wave" aria-hidden="true"></canvas></div><p class="rec-hear" id="rec-hear" role="status">${esc(hearingWords(hearing(s.wave, WAVE_STEP_MS), v.source, paused))}</p>`}
          ${controls}
          ${s.deviceLost?'<button class="btn plain" data-action="rec-rejoin">Reconnect audio</button>':''}
          ${warnings}
          ${s.pending ? `<p class="hint">${s.pending} audio chunks kept on this device, waiting to reach Leo.</p>` : ''}
        </section>
        <section class="set-card rec-notepad">
          <header><h3>Your notes</h3></header>
          <p class="hint">Type what matters, a line at a time. Your words go into the note exactly as you write them, and leo fills in the rest from the recording.</p>
          ${points}
          <div class="rec-point"><textarea id="rec-point-text" rows="4" placeholder="Type a point and press Enter"></textarea><button class="btn plain" data-action="rec-point">Add</button></div>
          <button class="btn plain rec-slide" data-action="rec-snapshot">Capture a slide</button>
        </section>
        <section class="set-card">
          <header><h3>What you want from the notes</h3></header>
          <p class="hint">The AI follows this when it writes the note after you stop. You can change it until then.</p>
          <textarea id="rec-wants-live" class="rec-wants" rows="2" maxlength="4000" placeholder="For example: focus on what will be on the exam, keep it short">${esc(s.wantsDraft !== null ? s.wantsDraft : v.wants || '')}</textarea>
          <p class="hint rec-wants-state" id="rec-wants-state">${esc(s.wantsState)}</p>
        </section>
        <details class="set-card rec-heard" id="rec-heard"${s.showHeard ? ' open' : ''}>
          <summary>Show what’s being heard</summary>
          <div class="rec-transcript" id="rec-transcript" aria-live="polite">${transcript}</div>
        </details>
      </div>`;
    }

    function drawWriting() {
      const v = s.view;
      const bar = v.steps ? (v.steps[0] / Math.max(1, v.steps[1])) * 100 : 0;
      return `<div class="rec rec-writing">
        <div class="section-title">Recording</div>
        <section class="set-card rec-done-card">${felix.felix(72, 'idle think')}<h3>${esc(stateWord(v) || 'Writing the notes')}</h3>
          <div class="upload-bar"><i style="width:${Math.max(6, bar)}%"></i></div>
          ${v.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('')}
          <button class="btn plain" data-action="rec-finish">Finish with available transcript</button>
          <p class="hint">${clock(v.secs)} recorded. You can leave this page; the note appears in its folder when it is ready.</p>
        </section>
      </div>`;
    }

    function drawFailed() {
      const v = s.view;
      return `<div class="rec rec-writing">
        <div class="section-title">Recording</div>
        <section class="set-card rec-done-card">${felix.felix(72, 'droop')}<h3>The recording could not become a note</h3>
          <p class="upload-error">${esc(v.error || 'Something went wrong.')}</p>
          ${v.warnings.map((w) => `<p class="rec-warn">${esc(w)}</p>`).join('')}
          <div class="buttons"><button class="btn plain" data-action="settings">Settings</button><button class="btn primary" data-action="rec-again">Record again</button></div>
        </section>
      </div>`;
    }

    let folders = [];
    let here = '';

    const IDLE_FIELDS = ['#rec-dir', '#rec-title', '#rec-wants'];

    function chooseEvent(box, index) {
      const events = (s.calendar && s.calendar.events) || [];
      const before = s.event !== null ? events[s.event] : null;
      const e = index !== null ? events[index] : null;
      s.event = e ? index : null;
      const title = box.querySelector('#rec-title');
      const wants = box.querySelector('#rec-wants');
      if (title && (!title.value.trim() || (before && title.value === before.title))) title.value = e ? e.title : '';
      if (wants && e && !wants.value.trim()) wants.value = MEETING_WANTS;
      else if (wants && !e && wants.value === MEETING_WANTS) wants.value = '';
    }

    function idleForm(box) {
      if (!box.querySelector('.rec-idle')) return null;
      const picked = box.querySelector('input[name="rec-kind"]:checked');
      const active = root.document.activeElement;
      return {
        kind: picked ? picked.value : null,
        values: IDLE_FIELDS.map((at) => { const field = box.querySelector(at); return field ? field.value : null; }),
        focused: active && box.contains(active) && active.id ? { id: active.id, start: active.selectionStart, end: active.selectionEnd } : null,
      };
    }

    function restoreIdleForm(box, form) {
      if (!box.querySelector('.rec-idle')) return;
      const kind = form.kind && box.querySelector(`input[name="rec-kind"][value="${form.kind}"]`);
      if (kind && !kind.disabled) kind.checked = true;
      IDLE_FIELDS.forEach((at, i) => {
        const field = box.querySelector(at);
        const value = form.values[i];
        if (!field || value === null) return;
        if (field.tagName === 'SELECT' && ![...field.options].some((o) => o.value === value)) return;
        field.value = value;
      });
      const field = form.focused && root.document.getElementById(form.focused.id);
      if (field && box.contains(field)) {
        field.focus({ preventScroll: true });
        if (typeof form.focused.start === 'number' && typeof field.setSelectionRange === 'function') field.setSelectionRange(form.focused.start, form.focused.end);
      }
    }

    function draw() {
      const box = s.container;
      if (!box || !box.isConnected) return;
      const v = s.view;
      const typed = box.querySelector('#rec-point-text');
      const keep = typed ? { value: typed.value, focused: root.document.activeElement === typed, start: typed.selectionStart, end: typed.selectionEnd } : null;
      const wantsBox = box.querySelector('#rec-wants-live');
      const wantsKeep = wantsBox && root.document.activeElement === wantsBox ? { start: wantsBox.selectionStart, end: wantsBox.selectionEnd } : null;
      const old = box.querySelector('#rec-transcript');
      if (old) s.follow = old.scrollTop + old.clientHeight >= old.scrollHeight - 24;
      const form = idleForm(box);
      if (!v || v.state === 'done') box.innerHTML = drawIdle(folders, here);
      else if (live(v)) {
        box.innerHTML = drawLive();
        s.heard = '';
        paintWaveNow();
      }
      else if (v.state === 'writing') box.innerHTML = drawWriting();
      else box.innerHTML = drawFailed();
      const heard = box.querySelector('#rec-heard');
      if (heard) {
        heard.addEventListener('toggle', () => {
          s.showHeard = heard.open;
          try {
            root.localStorage.setItem('leo-rec-heard', heard.open ? 'open' : 'shut');
          } catch (e) {}
          const text = heard.querySelector('#rec-transcript');
          if (heard.open && text) text.scrollTop = text.scrollHeight;
        });
      }
      const wantsArea = box.querySelector('#rec-wants-live');
      if (wantsArea) {
        if (wantsKeep) {
          wantsArea.focus();
          wantsArea.setSelectionRange(wantsKeep.start, wantsKeep.end);
        }
        wantsArea.addEventListener('input', () => {
          s.wantsDraft = wantsArea.value;
          s.wantsState = '';
          clearTimeout(s.wantsTimer);
          s.wantsTimer = setTimeout(saveWants, WANTS_SAVE_AFTER);
        });
        wantsArea.addEventListener('blur', () => {
          if (s.wantsDraft === null) return;
          clearTimeout(s.wantsTimer);
          saveWants();
        });
      }
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
      if (form) restoreIdleForm(box, form);
      const event=box.querySelector('#rec-event');
      if (event) {
        event.addEventListener('change', () => {
          s.eventChosen = true;
          chooseEvent(box, event.value === '' ? null : Number(event.value));
          draw();
        });
      }
      const dir=box.querySelector('#rec-dir');
      const text = box.querySelector('#rec-transcript');
      if (text && s.follow) text.scrollTop = text.scrollHeight;
    }

    async function saveWants() {
      if (s.wantsDraft === null || !s.view || !live(s.view)) return;
      const text = s.wantsDraft;
      const id = s.view.id;
      try {
        const view = await api(`/api/record/${id}/wants`, { method: 'POST', body: { text } });
        if (!s.view || s.view.id !== id) return;
        if (s.wantsDraft === text) s.wantsDraft = null;
        s.wantsState = 'Saved. The note will follow this.';
        accept(view);
      } catch (e) {
        s.wantsState = '';
        toast(e.message, { bad: true });
      }
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
        s.calendar = await api('/api/calendar').catch(() => null);
        if (!s.eventChosen && s.container && s.container.querySelector('.rec-idle')) {
          const now = eventNow(s.calendar && s.calendar.events);
          if (now >= 0) chooseEvent(s.container, now);
        }
        draw();
        pill();
      },
      leave() {
        s.container = null;
        pill();
      },
      async begin() {
        const box = s.container;
        const picked = box.querySelector('input[name="rec-kind"]:checked');
        const kind = picked ? picked.value : 'microphone';
        const source = kind === 'call' ? 'call' : sourceFor(kind, { local: Boolean(s.overview && s.overview.local), canShare: canShareSound(env()) });
        if (!source) throw Object.assign(new Error(sharingProblem('unsupported')), { shown: true });
        const button = box.querySelector('[data-action="rec-start"]');
        if (button) button.disabled = true;
        try {
          const dir=box.querySelector('#rec-dir').value;
          const events = (s.calendar && s.calendar.events) || [];
          const picked = s.event !== null ? events[s.event] : null;
          await start(source, dir, box.querySelector('#rec-title').value.trim(), { context: picked ? picked.context : '', wants: box.querySelector('#rec-wants').value.trim() });
        } finally {
          if (button && button.isConnected) button.disabled = false;
        }
      },
      pause: () => setPaused(!(s.view && s.view.state === 'paused')),
      stop,
      async finishAvailable() { if (s.view) { await api(`/api/record/${s.view.id}/finish`,{method:'POST'}); note('Queued transcription is skipped; any unfinished audio is kept for retry.'); } },
      rejoin,
      async recover(id) {
        if (audioQueue) await root.leoAudioQueue.drain(audioQueue,id,sendChunk);
        await api(`/api/record/${id}/recover`,{method:'POST'}); await refresh();
      },
      async snapshot() {
        if (!s.view || !live(s.view)) return;
        let stream=s.stream && s.stream.getVideoTracks().length ? s.stream : null;
        const temporary=!stream;
        if (!stream) stream=await root.navigator.mediaDevices.getDisplayMedia({video:true,audio:false});
        const at=s.view.secs;
        try {
          const video=root.document.createElement('video'); video.srcObject=stream; video.muted=true; await video.play();
          const canvas=root.document.createElement('canvas');
          const scale=Math.min(1,1600/video.videoWidth); canvas.width=video.videoWidth*scale; canvas.height=video.videoHeight*scale;
          canvas.getContext('2d').drawImage(video,0,0,canvas.width,canvas.height);
          const data=canvas.toDataURL('image/png').split(',')[1];
          accept(await api(`/api/record/${s.view.id}/snapshot`,{method:'POST',body:{data,at}}));
        } finally { if (temporary) stream.getTracks().forEach((t) => t.stop()); }
      },
      point: submitPoint,
      again() {
        s.view = null;
        draw();
      },
      active: () => live(s.view),
    };
  }

  root.leoRecording = { eventNow, whenOf, MEETING_WANTS, ease, spread, freshLevels, sourceFor, create, clock, join, bytesOf, trimHeld, micReady, micProblem, stateWord, canShareSound, sharingProblem, fedByBrowser, loudness, hearing, hearingWords, QUIET };
})(typeof window !== 'undefined' ? window : globalThis);
