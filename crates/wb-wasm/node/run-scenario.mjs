// Runs a scenario (or continues a save) in the WebAssembly engine under Node and prints
// the state hash — the WASM side of scripts/determinism-check.sh.
// Usage: node crates/wb-wasm/node/run-scenario.mjs --content build/content.wbpack
//          (--scenario scenarios/empty.yaml | --load war.wbsave) [--ticks 10000] [--save-at N]
// `--ticks` is the tick count to reach from war start, as in `wb run`.
// Build first: pnpm --filter @warboard/wb-wasm build:wasm; wb content build.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { Engine, initSync } from '../pkg/wb_wasm.js';

const { values } = parseArgs({
  options: {
    scenario: { type: 'string' },
    load: { type: 'string' },
    content: { type: 'string', default: 'build/content.wbpack' },
    ticks: { type: 'string', default: '10000' },
    'save-at': { type: 'string' },
  },
});
if (!values.scenario === !values.load) {
  console.error('usage: run-scenario.mjs --content <wbpack> (--scenario <yaml> | --load <wbsave>) [--ticks N] [--save-at N]');
  process.exit(2);
}
const whole = (name, v) => {
  const n = Number(v);
  if (!Number.isInteger(n) || n < 0 || n > 0xffffffff) {
    console.error(`--${name} must be a whole number from 0 to 4294967295, got ${v}`);
    process.exit(2);
  }
  return n;
};
const ticks = whole('ticks', values.ticks);
const saveAt = values['save-at'] === undefined ? null : whole('save-at', values['save-at']);

const pkgDir = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'pkg');
initSync({ module: readFileSync(resolve(pkgDir, 'wb_wasm_bg.wasm')) });

const content = readFileSync(values.content);
let engine = values.scenario
  ? Engine.create(readFileSync(values.scenario, 'utf8'), content)
  : Engine.load(readFileSync(values.load), content);
const label = values.scenario ?? values.load;
const started = performance.now();
const startTick = engine.tick();
const runTo = (t) => {
  while (engine.tick() < t) engine.run_ticks(Math.min(t - engine.tick(), 100_000));
};
if (saveAt !== null) {
  runTo(saveAt);
  const bytes = engine.save(label, '[]', 0);
  engine.free();
  engine = Engine.load(bytes, content);
}
runTo(ticks);
const elapsed = performance.now() - started;
const ran = Math.max(engine.tick() - startTick, 1);

console.error(
  `${label}: ${engine.tick()} ticks, sim time ${(engine.sim_time_ms() / 1000).toFixed(1)} s, ` +
    `${((elapsed * 1000) / ran).toFixed(2)} µs per tick (wasm)`,
);
console.log(engine.state_hash());
engine.free();
