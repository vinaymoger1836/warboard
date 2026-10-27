// Builds wb-wasm for the browser and Node: cargo (wasm32) + wasm-bindgen --target web.
// Output: crates/wb-wasm/pkg/ (gitignored). Works on Windows and Linux.
import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const crateDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const root = resolve(crateDir, '..', '..');
const profile = process.argv.includes('--dev') ? 'debug' : 'release';

const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: 'inherit' });

run('cargo', [
  'build',
  '-p',
  'wb-wasm',
  '--target',
  'wasm32-unknown-unknown',
  ...(profile === 'release' ? ['--release'] : []),
]);
run('wasm-bindgen', [
  resolve(root, 'target', 'wasm32-unknown-unknown', profile, 'wb_wasm.wasm'),
  '--target',
  'web',
  '--out-dir',
  resolve(crateDir, 'pkg'),
]);
