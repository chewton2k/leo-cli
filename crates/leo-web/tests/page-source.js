const fs = require('node:fs');
const path = require('node:path');

const web = path.resolve(__dirname, '../src/web');
const read = (name) => fs.readFileSync(path.join(web, name), 'utf8');

function pageCss() {
  const dir = path.join(web, 'styles');
  return fs
    .readdirSync(dir)
    .filter((name) => name.endsWith('.css'))
    .sort()
    .map((name) => fs.readFileSync(path.join(dir, name), 'utf8'))
    .join('');
}

function pageHtml() {
  return read('page-head.html') + pageCss() + read('page-tail.html');
}

module.exports = { pageCss, pageHtml };
