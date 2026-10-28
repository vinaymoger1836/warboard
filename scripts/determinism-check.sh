#!/usr/bin/env bash
# Runs each scenario natively (wb run) and in WebAssembly under Node, and fails
# if the state hashes differ (doc 02 §4, P0 acceptance). With region packs built, it
# also compares geo queries (territory, terrain, elevation, line of sight; P2).
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
# Geo queries (P2): only when the region packs have been built (they are not in git).
packs=${WB_PACKS:-build/packs}
if [ -f "$packs/world/manifest.json" ] && [ -f "$packs/europe/manifest.json" ]; then
  list="$packs/world,$packs/europe"
  native=$(./target/release/wb geo query --packs "$list" --probes geodata/probes.txt
           ./target/release/wb geo los --packs "$list" --from 49.30,20.10,1500 --to 49.06,20.30,1500)
  wasm=$(node crates/wb-wasm/node/geo-query.mjs --packs "$list" --probes geodata/probes.txt
         node crates/wb-wasm/node/geo-query.mjs --packs "$list" --los 49.30,20.10,1500 49.06,20.30,1500)
  if [ "$native" = "$wasm" ]; then
    echo "match    geo probes ($(echo "$native" | wc -l) lines) $(echo "$native" | sha256sum | cut -c1-16)"
  else
    echo "MISMATCH geo probes"
    diff <(echo "$native") <(echo "$wasm") || true
    status=1
  fi
else
  echo "skip     geo probes (no packs in $packs; build them with geodata/run.sh)"
fi
exit $status
