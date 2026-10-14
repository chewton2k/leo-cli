(function () {
  'use strict';
  const ENTRY = /^[A-Za-z_$][\w$]{0,79}$/;
  const send = (message) => parent.postMessage(message, '*');

  function judge() {
    const plain = (value) => {
      const seen = new WeakSet();
      const walk = (v) => {
        if (v === undefined) return null;
        if (typeof v === 'number' && !Number.isFinite(v)) return String(v);
        if (typeof v === 'bigint') return Number(v);
        if (v instanceof Set) return [...v].map(walk);
        if (v instanceof Map) return Object.fromEntries([...v].map(([k, x]) => [String(k), walk(x)]));
        if (ArrayBuffer.isView(v)) return Array.from(v);
        if (v && typeof v === 'object') {
          if (seen.has(v)) return '[circular]';
          seen.add(v);
          if (Array.isArray(v)) return v.map(walk);
          return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x)]));
        }
        return v;
      };
      return walk(value);
    };
    const show = (args) => args.map((a) => {
      try { return typeof a === 'string' ? a : JSON.stringify(plain(a)); } catch (e) { return String(a); }
    }).join(' ').slice(0, 300);
    const HARNESS = `
import json, sys, io, time
job = json.loads(job_json)
logs = io.StringIO()
def plain(v):
    if isinstance(v, (list, tuple)): return [plain(x) for x in v]
    if isinstance(v, (set, frozenset)): return sorted((plain(x) for x in v), key=repr)
    if isinstance(v, dict): return {str(k): plain(x) for k, x in v.items()}
    if isinstance(v, float) and (v != v or v in (float('inf'), float('-inf'))): return str(v)
    if v is None or isinstance(v, (bool, int, float, str)): return v
    return repr(v)
out = {"results": []}
ns = {"__name__": "__main__"}
exec("from typing import *\\nimport collections, heapq, math, itertools, functools, bisect, re, string\\nfrom collections import *", ns)
old = sys.stdout
sys.stdout = logs
try:
    exec(compile(job["code"], "solution.py", "exec"), ns)
except Exception as e:
    out["error"] = "Your code did not run: %s: %s" % (type(e).__name__, e)
else:
    fn = ns.get(job["entry"])
    solution = ns.get("Solution")
    if not callable(fn) and callable(solution) and hasattr(solution, job["entry"]):
        fn = getattr(solution(), job["entry"])
    if not callable(fn):
        out["error"] = "Define a function named %s." % job["entry"]
    else:
        for t in job["tests"]:
            started = time.perf_counter()
            try:
                got = fn(*json.loads(json.dumps(t["args"])))
                out["results"].append({"id": t["id"], "got": plain(got), "ms": round((time.perf_counter() - started) * 1000)})
            except Exception as e:
                out["results"].append({"id": t["id"], "error": ("%s: %s" % (type(e).__name__, e))[:300], "ms": round((time.perf_counter() - started) * 1000)})
finally:
    sys.stdout = old
out["logs"] = [line[:300] for line in logs.getvalue().splitlines()[:40]]
json.dumps(out)
`;
    let py = null;
    async function python(base) {
      if (!py) {
        importScripts(`${base}pyodide/pyodide.js`);
        py = await loadPyodide({ indexURL: `${base}pyodide/` });
      }
      return py;
    }
    let lite = null;
    function sqlite(base) {
      if (!lite) {
        importScripts(`${base}sql/sql-wasm.js`);
        lite = initSqlJs({ locateFile: (file) => `${base}sql/${file}` });
      }
      return lite;
    }
    self.onmessage = async (e) => {
      const job = e.data;
      const logs = [];
      if (job.language === 'sql') {
        let SQL;
        try {
          SQL = await sqlite(job.base);
        } catch (err) {
          self.postMessage({ error: 'SQLite could not start in this browser.', results: [], logs });
          return;
        }
        const results = [];
        for (const test of job.tests || []) {
          const started = performance.now();
          const db = new SQL.Database();
          try {
            db.exec(String((test.args || [])[0] || ''));
            const out = db.exec(String(job.code || ''));
            const last = out[out.length - 1];
            results.push({ id: test.id, got: last ? last.values.map((row) => row.map(plain)) : [], ms: Math.round(performance.now() - started) });
          } catch (err) {
            results.push({ id: test.id, error: String(err && err.message ? err.message : err).slice(0, 300), ms: Math.round(performance.now() - started) });
          } finally {
            db.close();
          }
        }
        self.postMessage({ results, logs });
        return;
      }
      if (job.language === 'python') {
        try {
          const ready = await python(job.base);
          self.postMessage({ loaded: true });
          ready.globals.set('job_json', JSON.stringify({ code: job.code, entry: job.entry, tests: job.tests }));
          self.postMessage(JSON.parse(ready.runPython(HARNESS)));
        } catch (err) {
          self.postMessage({ error: py ? `Your code did not run: ${err && err.message ? err.message.split('\n').slice(-2).join(' ') : err}` : 'Python could not start in this browser.', results: [], logs });
        }
        return;
      }
      if (!/^[A-Za-z_$][\w$]{0,79}$/.test(job.entry || '')) { self.postMessage({ error: 'This problem has no function to call.', results: [], logs }); return; }
      const log = (...args) => { if (logs.length < 40) logs.push(show(args)); };
      const fake = { log, info: log, warn: log, error: log, debug: log };
      let call;
      try {
        const made = new Function('console', `"use strict";\n${job.code}\n;return [typeof ${job.entry} === 'function' ? ${job.entry} : undefined, typeof Solution === 'function' ? Solution : undefined];`)(fake);
        if (made[0]) call = made[0];
        else if (made[1] && typeof made[1].prototype[job.entry] === 'function') call = (...args) => new made[1]()[job.entry](...args);
      } catch (err) {
        self.postMessage({ error: `Your code did not run: ${err && err.message ? err.message : err}`, results: [], logs });
        return;
      }
      if (!call) { self.postMessage({ error: `Define a function named ${job.entry}.`, results: [], logs }); return; }
      const results = [];
      for (const test of job.tests || []) {
        const started = performance.now();
        try {
          const got = call(...JSON.parse(JSON.stringify(test.args || [])));
          results.push({ id: test.id, got: plain(got), ms: Math.round(performance.now() - started) });
        } catch (err) {
          results.push({ id: test.id, error: String(err && err.message ? err.message : err).slice(0, 300), ms: Math.round(performance.now() - started) });
        }
      }
      self.postMessage({ results, logs });
    };
  }

  const source = `(${judge.toString()})();`;

  const start = () => new Worker(URL.createObjectURL(new Blob([source], { type: 'text/javascript' })));
  let snake = null;
  let snakeLoaded = false;

  function program() {
    let memory = null;
    let out = '';
    const decode = new TextDecoder();
    const view = () => new DataView(memory.buffer);
    const calls = {
      fd_write(fd, iovs, count, written) {
        const v = view();
        let total = 0;
        for (let i = 0; i < count; i++) {
          const at = v.getUint32(iovs + i * 8, true);
          const length = v.getUint32(iovs + i * 8 + 4, true);
          if (fd === 1 && out.length < 2000000) out += decode.decode(new Uint8Array(memory.buffer, at, length), { stream: true });
          total += length;
        }
        v.setUint32(written, total, true);
        return 0;
      },
      fd_read(fd, iovs, count, read) { view().setUint32(read, 0, true); return 0; },
      fd_close() { return 0; },
      fd_seek() { return 70; },
      fd_fdstat_get(fd, at) {
        if (fd > 2) return 8;
        const v = view();
        for (let i = 0; i < 24; i++) v.setUint8(at + i, 0);
        v.setUint8(at, 2);
        return 0;
      },
      fd_prestat_get() { return 8; },
      fd_prestat_dir_name() { return 8; },
      args_sizes_get(count, size) { view().setUint32(count, 0, true); view().setUint32(size, 0, true); return 0; },
      args_get() { return 0; },
      environ_sizes_get(count, size) { view().setUint32(count, 0, true); view().setUint32(size, 0, true); return 0; },
      environ_get() { return 0; },
      clock_res_get(id, at) { view().setBigUint64(at, 1000n, true); return 0; },
      clock_time_get(id, precision, at) { view().setBigUint64(at, BigInt(Math.round((performance.timeOrigin + performance.now()) * 1e6)), true); return 0; },
      random_get(at, length) {
        for (let i = 0; i < length; i += 65536) crypto.getRandomValues(new Uint8Array(memory.buffer, at + i, Math.min(65536, length - i)));
        return 0;
      },
      sched_yield() { return 0; },
      proc_exit(code) { throw { exit: code }; },
    };
    const wasi = new Proxy(calls, { get: (all, name) => all[name] || (() => 52) });
    self.onmessage = async (e) => {
      try {
        const made = await WebAssembly.instantiate(await WebAssembly.compile(e.data.wasm), { wasi_snapshot_preview1: wasi });
        memory = made.exports.memory;
        made.exports._start();
        self.postMessage({ stdout: out });
      } catch (err) {
        if (err && err.exit !== undefined) self.postMessage({ stdout: out, ...(err.exit ? { error: `Your program stopped with exit code ${err.exit}.` } : {}) });
        else self.postMessage({ stdout: out, error: `Your program crashed: ${String(err && err.message ? err.message : err).slice(0, 300)}` });
      }
    };
  }

  const programSource = `(${program.toString()})();`;
  const compilerSource = (bundle, base) => `${bundle.split('import.meta.url').join(JSON.stringify(`${base}clang/bundle.js`)).replace(/\nexport\s*\{[^}]*\};?\s*$/, '\n')}
const decode = new TextDecoder();
self.onmessage = async (e) => {
  let errors = '';
  try {
    const out = await runClang(['clang++', '-O1', '-std=c++20', '-fno-exceptions', '-w', 'main.cpp', '-o', 'main.wasm'], e.data.files, {
      stdout: () => {},
      stderr: (bytes) => { if (bytes && errors.length < 20000) errors += decode.decode(bytes); },
      fetchProgress: ({ doneLength, totalLength }) => self.postMessage({ progress: totalLength ? doneLength / totalLength : 0 }),
    });
    const wasm = out['main.wasm'];
    self.postMessage({ wasm }, [wasm.buffer]);
  } catch (err) {
    self.postMessage({ compile: errors || String(err && err.message ? err.message : err) });
  }
};
self.postMessage({ loaded: true });
`;
  function gopher() {
    const { WASI, File, OpenFile, ConsoleStdout, PreopenDirectory, Directory } = self.leoWasi;
    let tools = null;
    let root = null;
    let config = '';
    const encode = (text) => new TextEncoder().encode(text);
    function tree(files) {
      const top = new Map();
      for (const [name, bytes] of Object.entries(files)) {
        const parts = name.split('/');
        let here = top;
        for (const part of parts.slice(0, -1)) {
          if (!here.has(part)) here.set(part, new Directory(new Map()));
          here = here.get(part).contents;
        }
        here.set(parts[parts.length - 1], new File(new Uint8Array(bytes), { readonly: true }));
      }
      return top;
    }
    async function tool(name, args, work, log) {
      const out = (line) => { if (log.text.length < 20000) log.text += `${line}\n`; };
      const wasi = new WASI([name, ...args], ['GOROOT=/go', 'TMPDIR=/work', 'HOME=/work'], [new OpenFile(new File([])), ConsoleStdout.lineBuffered(out), ConsoleStdout.lineBuffered(out), root, work], { debug: false });
      const made = await WebAssembly.instantiate(tools[name], { wasi_snapshot_preview1: wasi.wasiImport });
      try { return wasi.start(made); } catch (err) { log.text += String(err && err.message ? err.message : err); return 1; }
    }
    self.onmessage = async (e) => {
      const job = e.data || {};
      try {
        if (job.files) {
          const files = job.files;
          config = new TextDecoder().decode(files.importcfg);
          tools = { compile: await WebAssembly.compile(files['compile.wasm']), link: await WebAssembly.compile(files['link.wasm']) };
          const std = {};
          for (const [name, bytes] of Object.entries(files)) if (name.startsWith('std/')) std[name] = bytes;
          root = new PreopenDirectory('/go', tree(std));
        }
        if (!tools) { self.postMessage({ missing: true }); return; }
        const work = new PreopenDirectory('/work', [
          ...Object.entries(job.sources || {}).map(([name, text]) => [name, new File(encode(text))]),
          ['importcfg', new File(encode(config))],
          ['importcfg.link', new File(encode(`${config}packagefile main=/work/main.a\n`))],
        ]);
        const log = { text: '' };
        const names = Object.keys(job.sources || {}).map((name) => `/work/${name}`);
        if (await tool('compile', ['-p', 'main', '-complete', '-o', '/work/main.a', '-importcfg', '/work/importcfg', '-pack', ...names], work, log) !== 0) { self.postMessage({ compile: log.text }); return; }
        if (await tool('link', ['-o', '/work/prog.wasm', '-importcfg', '/work/importcfg.link', '-buildmode=exe', '/work/main.a'], work, log) !== 0) { self.postMessage({ compile: log.text || 'The program could not be linked.' }); return; }
        const wasm = work.dir.contents.get('prog.wasm').data;
        const copy = wasm.slice().buffer;
        self.postMessage({ wasm: copy }, [copy]);
      } catch (err) {
        self.postMessage({ error: `Go could not build this: ${String(err && err.message ? err.message : err).slice(0, 300)}` });
      }
    };
  }

  let goWorker = null;
  let goBusy = false;

  function runGo(job, done) {
    const limit = Math.min(Math.max(Number(job.limit) || 4000, 500), 10000);
    if (!goWorker) {
      if (typeof job.shim !== 'string' || !job.shim || !job.files) return done({ missing: true, stdout: '' });
      try {
        goWorker = new Worker(URL.createObjectURL(new Blob([`${job.shim}\n(${gopher.toString()})();`], { type: 'text/javascript' })));
      } catch (e) {
        return done({ error: 'This browser could not start Go.', stdout: '' });
      }
    }
    if (goBusy) return done({ error: 'Go is still building the last run. Try again in a moment.', stdout: '' });
    goBusy = true;
    const worker = goWorker;
    const first = Boolean(job.files);
    const timer = setTimeout(() => { worker.terminate(); goWorker = null; goBusy = false; done({ error: 'Building took too long. Try Run again.', stdout: '' }); }, first ? 240000 : 60000);
    worker.onerror = (e) => { e.preventDefault(); clearTimeout(timer); worker.terminate(); goWorker = null; goBusy = false; done({ error: 'Go could not start in this browser.', stdout: '' }); };
    worker.onmessage = (e) => {
      clearTimeout(timer);
      goBusy = false;
      const data = e.data || {};
      if (data.missing) { worker.terminate(); goWorker = null; return done({ missing: true, stdout: '' }); }
      if (data.compile !== undefined) return done({ compile: String(data.compile), stdout: '' });
      if (!data.wasm) return done({ error: data.error || 'Go could not build this.', stdout: '' });
      let running;
      try {
        running = new Worker(URL.createObjectURL(new Blob([programSource], { type: 'text/javascript' })));
      } catch (err) {
        return done({ error: 'This browser could not run the program.', stdout: '' });
      }
      const stopped = setTimeout(() => { running.terminate(); done({ stopped: true, error: `Stopped after ${limit / 1000} seconds. Check for an endless loop.`, stdout: '' }); }, limit);
      running.onmessage = (r) => { clearTimeout(stopped); running.terminate(); done(r.data || { stdout: '' }); };
      running.onerror = (r) => { r.preventDefault(); clearTimeout(stopped); running.terminate(); done({ error: `Your program crashed: ${r.message || 'unknown error'}`, stdout: '' }); };
      running.postMessage({ wasm: data.wasm }, [data.wasm]);
    };
    const transfer = first ? Object.values(job.files).filter((b) => b instanceof ArrayBuffer) : [];
    worker.postMessage({ sources: job.sources || {}, ...(first ? { files: job.files } : {}) }, transfer);
  }

  let compiler = null;
  let compiled = false;

  function runCpp(job, done) {
    const limit = Math.min(Math.max(Number(job.limit) || 4000, 500), 10000);
    if (!compiler) {
      if (typeof job.bundle !== 'string' || !job.bundle) return done({ error: 'The C++ compiler is not loaded yet. Try Run again.', stdout: '' });
      try {
        compiler = new Worker(URL.createObjectURL(new Blob([compilerSource(job.bundle, String(job.base || ''))], { type: 'text/javascript' })));
      } catch (e) {
        return done({ error: 'This browser could not start the C++ compiler.', stdout: '' });
      }
    }
    const worker = compiler;
    let timer = 0;
    const arm = (ms, message) => { clearTimeout(timer); timer = setTimeout(() => finish({ error: message, stdout: '' }, true), ms); };
    const finish = (result, broken) => {
      clearTimeout(timer);
      worker.onmessage = null;
      worker.onerror = null;
      if (broken) { worker.terminate(); compiler = null; }
      done(result);
    };
    arm(compiled ? 60000 : 240000, compiled ? 'Compiling took too long. Try Run again.' : 'The C++ compiler took too long to start. Try Run again.');
    worker.onerror = (e) => { e.preventDefault(); finish({ error: 'The C++ compiler could not start in this browser.', stdout: '' }, true); };
    worker.onmessage = (e) => {
      const data = e.data || {};
      if (data.loaded) return;
      if (typeof data.progress === 'number') { send({ kind: 'progress', run: job.run, fraction: data.progress }); return; }
      compiled = true;
      if (data.compile !== undefined) return finish({ compile: String(data.compile), stdout: '' });
      clearTimeout(timer);
      worker.onmessage = null;
      worker.onerror = null;
      let running;
      try {
        running = new Worker(URL.createObjectURL(new Blob([programSource], { type: 'text/javascript' })));
      } catch (err) {
        return done({ error: 'This browser could not run the program.', stdout: '' });
      }
      const stopped = setTimeout(() => { running.terminate(); done({ stopped: true, error: `Stopped after ${limit / 1000} seconds. Check for an endless loop.`, stdout: '' }); }, limit);
      running.onmessage = (r) => { clearTimeout(stopped); running.terminate(); done(r.data || { stdout: '' }); };
      running.onerror = (r) => { r.preventDefault(); clearTimeout(stopped); running.terminate(); done({ error: `Your program crashed: ${r.message || 'unknown error'}`, stdout: '' }); };
      running.postMessage({ wasm: data.wasm }, [data.wasm.buffer]);
    };
    worker.postMessage({ files: job.files || {} });
  }

  function run(job) {
    const done = (result) => send({ kind: 'done', run: job.run, ...result });
    if (job.language === 'cpp') return runCpp(job, done);
    if (job.language === 'go') return runGo(job, done);
    if (!ENTRY.test(job.entry || '')) return done({ error: 'This problem has no function to call.', results: [], logs: [] });
    const python = job.language === 'python';
    let worker;
    try {
      worker = python ? (snake = snake || start()) : start();
    } catch (e) {
      return done({ error: 'This browser could not start the code runner.', results: [], logs: [] });
    }
    const limit = Math.min(Math.max(Number(job.limit) || 4000, 500), 10000);
    let timer = 0;
    const stop = (result) => {
      clearTimeout(timer);
      if (!python || result.stopped) { worker.terminate(); if (python) { snake = null; snakeLoaded = false; } }
      worker.onmessage = null;
      worker.onerror = null;
      done(result);
    };
    const arm = (ms, message) => { clearTimeout(timer); timer = setTimeout(() => stop({ stopped: true, error: message, results: [], logs: [] }), ms); };
    if (python && !snakeLoaded) arm(60000, 'Python took too long to start. Try Run again.');
    else arm(limit, `Stopped after ${limit / 1000} seconds. Check for an endless loop.`);
    worker.onmessage = (e) => {
      if (e.data && e.data.loaded) { snakeLoaded = true; arm(limit, `Stopped after ${limit / 1000} seconds. Check for an endless loop.`); return; }
      stop(e.data || {});
    };
    worker.onerror = (e) => { stop({ stopped: true, error: `Your code did not run: ${e.message || 'unknown error'}`, results: [], logs: [] }); e.preventDefault(); };
    worker.postMessage({ code: String(job.code || ''), entry: job.entry, tests: job.tests || [], language: python ? 'python' : job.language === 'sql' ? 'sql' : 'javascript', base: String(job.base || '') });
  }

  window.addEventListener('message', (e) => {
    if (e.source !== parent || !e.data || e.data.kind !== 'run') return;
    run(e.data);
  });
  send({ kind: 'ready' });
})();
