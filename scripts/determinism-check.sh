#!/usr/bin/env bash
# Runs each scenario natively (wb run) and in WebAssembly under Node, and fails
# if the state hashes differ (doc 02 §4, P0 acceptance).
# Needs crates/wb-wasm/pkg built first: pnpm --filter @warboard/wb-wasm build:wasm
set -euo pipefail

cd "$(dirname "$0")/.."
ticks=${TICKS:-10000}
scenarios=("$@")
if [ ${#scenarios[@]} -eq 0 ]; then
  scenarios=(scenarios/empty.yaml scenarios/determinism-probe.yaml)
fi

cargo build -q --release -p wb-cli
status=0
for s in "${scenarios[@]}"; do
  native=$(./target/release/wb run --scenario "$s" --ticks "$ticks" --print-hash)
  wasm=$(node crates/wb-wasm/node/run-scenario.mjs --scenario "$s" --ticks "$ticks")
  if [ "$native" = "$wasm" ]; then
    echo "match    $s ($ticks ticks) $native"
  else
    echo "MISMATCH $s ($ticks ticks)"
    echo "  native $native"
    echo "  wasm   $wasm"
    status=1
  fi
done
exit $status
