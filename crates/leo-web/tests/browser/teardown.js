const fs = require('node:fs');

module.exports = async () => {
  const home = process.env.LEO_BROWSER_HOME;
  if (home && /leo-browser-/.test(home)) fs.rmSync(home, { recursive: true, force: true });
};
