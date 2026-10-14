# Runner tools

Vendored helpers for running interview code in the browser sandbox, beside
Python (Pyodide, downloaded on first use):

- `typescript.min.js`: sucrase removes TypeScript types on the page; the result
  runs as JavaScript in the sandbox.
- `sql/sql-wasm.js` and `sql/sql-wasm.wasm`: SQLite (sql.js) for SQL problems,
  loaded by the sandboxed runner.
- `wasi-shim.min.js`: an in-memory WASI system (@bjorn3/browser_wasi_shim) that
  the Go compiler and linker run on in the sandbox.

Install the pinned dependencies and rebuild the vendored files and licences:

```sh
pnpm --dir scripts/runner-tools install --frozen-lockfile --ignore-scripts
pnpm --dir scripts/runner-tools run build
```

The generated files in `crates/leo-web/src/web/vendor` are committed so Leo needs
no package manager or CDN for them at runtime.
