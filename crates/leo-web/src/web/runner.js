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

  function run(job) {
    const done = (result) => send({ kind: 'done', run: job.run, ...result });
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
