const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');

test('mermaid sources cannot override themeCSS, and the vendored defaults stay locked', () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/diagrams.js'), 'utf8');
  const vendored = fs.readFileSync(path.join(__dirname, '../src/web/vendor/mermaid.min.js'), 'utf8');
  const defaults = JSON.parse(/secure:(\[[^\]]*\])/.exec(vendored)[1]);
  const secure = JSON.parse(/secure: (\[[^\]]*\])/.exec(source)[1].replace(/'/g, '"'));
  for (const key of [...defaults, 'themeCSS']) assert.ok(secure.includes(key), key);
});
