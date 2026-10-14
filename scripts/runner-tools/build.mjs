import { build } from 'esbuild';
import { readFile, writeFile, copyFile, access, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const vendor = resolve(here, '../../crates/leo-web/src/web/vendor');
const outfile = resolve(vendor, 'typescript.min.js');
const result = await build({ entryPoints: [resolve(here, 'typescript.js')], outfile, bundle: true, minify: true, format: 'iife', target: 'es2022', legalComments: 'none', metafile: true });
const shim = await build({ entryPoints: [resolve(here, 'wasi.js')], outfile: resolve(vendor, 'wasi-shim.min.js'), bundle: true, minify: true, format: 'iife', target: 'es2022', legalComments: 'none', metafile: true });

async function licence(folder) {
  for (let depth = 0; depth < 8; depth++, folder = dirname(folder)) {
    try {
      await access(resolve(folder, 'package.json'));
    } catch (e) {
      if (e.code !== 'ENOENT' || depth === 7) throw e;
      continue;
    }
    const pkg = JSON.parse(await readFile(resolve(folder, 'package.json'), 'utf8'));
    const file = ['LICENSE', 'LICENSE-MIT'].find((name) => existsSync(resolve(folder, name)));
    return [pkg.name, `${pkg.name} ${pkg.version}\n${await readFile(resolve(folder, file), 'utf8')}`];
  }
}

const packages = new Map();
for (const input of [...Object.keys(result.metafile.inputs), ...Object.keys(shim.metafile.inputs)]) {
  if (!input.includes('node_modules')) continue;
  const [name, text] = await licence(dirname(resolve(input)));
  if (!packages.has(name)) packages.set(name, text);
}
const sql = resolve(here, 'node_modules/sql.js/dist');
await mkdir(resolve(vendor, 'sql'), { recursive: true });
for (const file of ['sql-wasm.js', 'sql-wasm.wasm']) await copyFile(resolve(sql, file), resolve(vendor, 'sql', file));
const [name, text] = await licence(sql);
packages.set(name, text);
await writeFile(resolve(vendor, 'runner-tools.LICENSE'), [...packages.values()].join('\n\n'));
console.log(`Runner tools: TypeScript ${(await readFile(outfile)).length} bytes, WASI ${(await readFile(resolve(vendor, 'wasi-shim.min.js'))).length} bytes, SQL ${(await readFile(resolve(vendor, 'sql/sql-wasm.wasm'))).length} bytes, ${packages.size} licences`);
