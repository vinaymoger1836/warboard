#!/usr/bin/env bash
# Proves the determinism lints fire inside wb-sim (CLAUDE.md rule 2).
#
# Appends a forbidden call to crates/wb-sim/src/lib.rs, runs clippy, and
# requires it to fail with the expected lint. The file is always restored.
set -euo pipefail

cd "$(dirname "$0")/.."
target=crates/wb-sim/src/lib.rs
backup=$(mktemp)
cp "$target" "$backup"
restore() { cp "$backup" "$target"; rm -f "$backup"; }
trap restore EXIT

check() {
  local name=$1 lint=$2 code=$3
  cp "$backup" "$target"
  printf '\n%s\n' "$code" >> "$target"
  local out
  if out=$(cargo clippy -p wb-sim --quiet -- -D warnings 2>&1); then
    echo "FAIL: $name canary compiled cleanly; clippy did not reject it" >&2
    exit 1
  fi
  if ! grep -q "clippy::$lint" <<<"$out" && ! grep -q "$lint" <<<"$out"; then
    echo "FAIL: $name canary failed, but not with $lint:" >&2
    echo "$out" >&2
    exit 1
  fi
  echo "ok: $name rejected by clippy::$lint"
}

check "f64::sin" disallowed_methods \
  'pub fn __lint_canary_sin(x: f64) -> f64 { x.sin() }'
check "f64::powf" disallowed_methods \
  'pub fn __lint_canary_powf(x: f64) -> f64 { x.powf(0.25) }'
check "HashMap" disallowed_types \
  'pub fn __lint_canary_map() -> usize { let m: std::collections::HashMap<u32, u32> = Default::default(); m.len() }'
check "HashSet" disallowed_types \
  'pub fn __lint_canary_set() -> usize { let s: std::collections::HashSet<u32> = Default::default(); s.len() }'

echo "lint canaries: all rejected"
