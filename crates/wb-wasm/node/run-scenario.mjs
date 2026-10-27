// Runs a scenario in the WebAssembly engine under Node and prints the state hash.
// Usage: node crates/wb-wasm/node/run-scenario.mjs --scenario scenarios/empty.yaml [--ticks 10000]
// Build first: pnpm --filter @warboard/wb-wasm build
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { Engine, initSync } from '../pkg/wb_wasm.js';

const { values } = parseArgs({
  options: {
    scenario: { type: 'string' },
    ticks: { type: 'string', default: '10000' },
  },
});
if (!values.scenario) {
  console.error('usage: run-scenario.mjs --scenario <yaml> [--ticks N]');
  process.exit(2);
}
const ticks = Number(values.ticks);
if (!Number.isInteger(ticks) || ticks < 0 || ticks > 0xffffffff) {
  console.error(`--ticks must be a whole number from 0 to 4294967295, got ${values.ticks}`);
  process.exit(2);
}

const pkgDir = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'pkg');
initSync({ module: readFileSync(resolve(pkgDir, 'wb_wasm_bg.wasm')) });

const engine = Engine.create(readFileSync(values.scenario, 'utf8'));
const started = performance.now();
engine.run_ticks(ticks);
const elapsed = performance.now() - started;

console.error(
  `${values.scenario}: ${engine.tick()} ticks, sim time ${(engine.sim_time_ms() / 1000).toFixed(1)} s, ` +
    `${((elapsed * 1000) / Math.max(ticks, 1)).toFixed(2)} µs per tick (wasm)`,
);
console.log(engine.state_hash());
engine.free();
