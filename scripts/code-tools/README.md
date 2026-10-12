# Coding workspace tools

This builds the local syntax and indentation helpers used by Leo’s interview
textarea. Code is parsed as data and never executed. The supported languages are
C++, Rust, Java, Python, SQL, JavaScript, TypeScript and Go; aliases are handled in
`code-tools.js`. Indent code adjusts indentation, rather than rewriting syntax.

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
