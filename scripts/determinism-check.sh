#!/usr/bin/env bash
# Runs each scenario natively (wb run) and in WebAssembly under Node, and fails
# if the state hashes differ (doc 02 §4, P0 acceptance). P3 adds 1 sim hour runs
# of the flying scenarios, a save/reload mid-flight in both, and a save written
# natively and continued in WASM. With region packs built, it also compares geo
# queries (territory, terrain, elevation, line of sight; P2). P4 adds the
# sensor goldens G1–G5 and a save/reload of the demo war with contacts in the
# pictures.
# Needs crates/wb-wasm/pkg built first: pnpm --filter @warboard/wb-wasm build:wasm
set -euo pipefail

cd "$(dirname "$0")/.."
ticks=${TICKS:-10000}
scenarios=("$@")
if [ ${#scenarios[@]} -eq 0 ]; then
  scenarios=(scenarios/empty.yaml scenarios/determinism-probe.yaml
             scenarios/golden/g7-f35a-patrol.yaml scenarios/save-load.yaml scenarios/demo-baltic.yaml
             scenarios/golden/g1-frigate-vs-sea-skimmer.yaml scenarios/golden/g2-rcs-ratio.yaml
             scenarios/golden/g3-ridge-masking.yaml scenarios/golden/g4-esm-vs-s400.yaml
             scenarios/golden/g5-satellite-night.yaml)
fi

cargo build -q --release -p wb-cli
wb=./target/release/wb
pack=build/content.wbpack
$wb content build --out "$pack" > /dev/null
status=0
compare() { # label native wasm
  if [ "$2" = "$3" ]; then
    echo "match    $1 $2"
  else
    echo "MISMATCH $1"
    echo "  native $2"
    echo "  wasm   $3"
    status=1
  fi
}
for s in "${scenarios[@]}"; do
  native=$($wb run --content "$pack" --scenario "$s" --ticks "$ticks" --print-hash)
  wasm=$(node crates/wb-wasm/node/run-scenario.mjs --content "$pack" --scenario "$s" --ticks "$ticks")
  compare "$s ($ticks ticks)" "$native" "$wasm"
done

# P3: save mid-flight, reload, run to 1 sim hour; equal to an uninterrupted run,
# natively, in WASM, and from a native save continued in WASM.
s=scenarios/save-load.yaml
whole=$($wb run --content "$pack" --scenario "$s" --ticks 3600 --print-hash)
native=$($wb run --content "$pack" --scenario "$s" --ticks 3600 --save-at 900 --print-hash)
compare "save/reload native ($s, save at 900)" "$whole" "$native"
wasm=$(node crates/wb-wasm/node/run-scenario.mjs --content "$pack" --scenario "$s" --ticks 3600 --save-at 900)
compare "save/reload wasm   ($s, save at 900)" "$whole" "$wasm"
save=$(mktemp)
$wb run --content "$pack" --scenario "$s" --ticks 900 --save-out "$save" > /dev/null
cross=$(node crates/wb-wasm/node/run-scenario.mjs --content "$pack" --load "$save" --ticks 3600)
rm -f "$save"
compare "native save → wasm ($s, save at 900)" "$whole" "$cross"

# P4: the demo war with sensors, contacts and satellites, saved after contacts
# exist and continued (pictures, tracks and pending reports are in the save).
s=scenarios/demo-baltic.yaml
whole=$($wb run --content "$pack" --scenario "$s" --ticks 3600 --print-hash)
native=$($wb run --content "$pack" --scenario "$s" --ticks 3600 --save-at 1800 --print-hash)
compare "save/reload native ($s, save at 1800)" "$whole" "$native"
wasm=$(node crates/wb-wasm/node/run-scenario.mjs --content "$pack" --scenario "$s" --ticks 3600 --save-at 1800)
compare "save/reload wasm   ($s, save at 1800)" "$whole" "$wasm"

# Geo queries (P2): only when the region packs have been built (they are not in git).
packs=${WB_PACKS:-build/packs}
if [ -f "$packs/world/manifest.json" ] && [ -f "$packs/europe/manifest.json" ]; then
  list="$packs/world,$packs/europe"
  native=$($wb geo query --packs "$list" --probes geodata/probes.txt
           $wb geo los --packs "$list" --from 49.30,20.10,1500 --to 49.06,20.30,1500)
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
