// Queries region packs with the WebAssembly build under Node and prints the same lines as
// `wb geo query` / `wb geo los`, so scripts/determinism-check.sh can compare them.
// Pack files are read with fs.readSync, the Node stand-in for the browser's OPFS handles.
// Usage:
//   node crates/wb-wasm/node/geo-query.mjs [--packs build/packs/world,build/packs/europe] --probes geodata/probes.txt
//   node crates/wb-wasm/node/geo-query.mjs --los "lat,lon,alt" "lat,lon,alt" [--k 1.3333] [--step 100]
import { closeSync, fstatSync, openSync, readFileSync, readSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { GeoQuery, initSync } from '../pkg/wb_wasm.js';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    packs: { type: 'string', default: 'build/packs/world,build/packs/europe' },
    probes: { type: 'string' },
    at: { type: 'string', multiple: true },
    los: { type: 'boolean' },
    info: { type: 'boolean' },
    k: { type: 'string', default: String(4 / 3) },
    step: { type: 'string', default: '100' },
  },
});

const pkgDir = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'pkg');
initSync({ module: readFileSync(resolve(pkgDir, 'wb_wasm_bg.wasm')) });

const opened = [];
function packFile(path) {
  const fd = openSync(path, 'r');
  opened.push(fd);
  return {
    name: path,
    size: () => fstatSync(fd).size,
    readAt: (buf, at) => readSync(fd, buf, 0, buf.length, at),
  };
}

const geo = new GeoQuery();
for (const dir of values.packs.split(',')) {
  const id = basename(dir);
  geo.add_pack(
    id,
    packFile(join(dir, `dem-${id}.wbdem`)),
    packFile(join(dir, `terrain-${id}.wbh3`)),
    packFile(join(dir, `territory-${id}.bin`)),
  );
}

const nums = (s, n) => {
  const v = s.split(',').map(Number);
  if (v.length !== n || v.some((x) => !Number.isFinite(x))) throw new Error(`expected ${n} numbers, got "${s}"`);
  return v;
};

if (values.info) console.log(geo.info());
if (values.los) {
  const [a, b] = positionals;
  if (!a || !b) throw new Error('--los needs two "lat,lon,alt" arguments');
  const [la, lo, al] = nums(a, 3);
  const [lb, lob, alb] = nums(b, 3);
  console.log(geo.los(la, lo, al, lb, lob, alb, Number(values.k), Number(values.step)));
}
for (const at of values.at ?? []) {
  const [lat, lon] = nums(at, 2);
  console.log(geo.query(lat, lon, ''));
}
if (values.probes) {
  for (const raw of readFileSync(values.probes, 'utf8').split('\n')) {
    const line = raw.split('#')[0].trim();
    if (!line) continue;
    const [lat, lon, ...label] = line.split(/\s+/);
    console.log(geo.query(Number(lat), Number(lon), label.join(' ')));
  }
}
geo.free();
for (const fd of opened) closeSync(fd);
