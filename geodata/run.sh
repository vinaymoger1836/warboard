#!/usr/bin/env bash
# Runs the geodata pipeline inside the pinned GDAL image (doc 05 §7).
#   bash geodata/run.sh image                  build the image
#   bash geodata/run.sh build <pack> [--step s] build a pack into build/packs/<pack>/
#   bash geodata/run.sh <any python -m geodata args>
set -euo pipefail

IMAGE=warboard-geodata:p2
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${1:-}" == "image" ]]; then
  docker build -t "$IMAGE" "$ROOT/geodata"
  exit 0
fi

# Git Bash on Windows rewrites /repo-style arguments; turn that off.
export MSYS_NO_PATHCONV=1
exec docker run --rm \
  -v "$ROOT:/repo" \
  -e GEODATA_JOBS="${GEODATA_JOBS:-}" \
  "$IMAGE" "$@"
