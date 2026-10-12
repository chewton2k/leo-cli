import { build } from 'esbuild';
import { readFile, writeFile, access } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const outfile = resolve(here, '../../crates/leo-web/src/web/vendor/code-tools.min.js');
const result = await build({ entryPoints: [resolve(here, 'code-tools.js')], outfile, bundle: true, minify: true, format: 'iife', target: 'es2022', legalComments: 'none', metafile: true });
const packages = new Map();
for (const input of Object.keys(result.metafile.inputs)) {
  if (!input.includes('node_modules')) continue;
  let folder = dirname(resolve(input));
  for (let depth = 0; depth < 8; depth++, folder = dirname(folder)) {
    let text;
    try {
      await access(resolve(folder, 'package.json'));
      text = await readFile(resolve(folder, 'package.json'), 'utf8');
    } catch (e) {
      if (e.code !== 'ENOENT' || depth === 7) throw e;
      continue;
    }
    const pkg = JSON.parse(text);
    if (!packages.has(pkg.name)) packages.set(pkg.name, `${pkg.name} ${pkg.version}\n${await readFile(resolve(folder, 'LICENSE'), 'utf8')}`);
    break;
  }
}
await writeFile(resolve(here, '../../crates/leo-web/src/web/vendor/code-tools.LICENSE'), [...packages.values()].join('\n\n'));
console.log(`Code tools: ${(await readFile(outfile)).length} bytes, ${packages.size} dependency licences`);
