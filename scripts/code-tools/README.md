# Coding workspace tools

This builds the local syntax and indentation helpers used by Leo’s interview
textarea. Code is parsed as data and never executed. The supported languages are
C++, Rust, Java, Python, SQL, JavaScript, TypeScript and Go; aliases are handled in
`code-tools.js`. CodeMirror commands handle newline indentation, selection indent/outdent,
and bracket pairing/type-over. Enter retains manual indentation in incomplete
fragments and blank lines while keeping syntax-aware continuations and exits
from multiline delimiters. The textarea adapter applies a minimal text change
so browser Undo, wrapping and the visible scroll position survive each edit.

Install the pinned dependencies and rebuild the vendored bundle and licences:

```sh
pnpm --dir scripts/code-tools install --frozen-lockfile --ignore-scripts
pnpm --dir scripts/code-tools run build
node --test crates/leo-web/tests/interview.test.js
```

The generated files in `crates/leo-web/src/web/vendor` are committed so Leo needs
no package manager, CDN or network service at runtime. Only coding workspaces load
the bundle. Preserve the size limit, escaping, literal protection and selection
mapping when updating these helpers.
