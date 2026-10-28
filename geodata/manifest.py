"""Pack manifest with BLAKE3 hashes and attributions, and the pack catalogue (doc 05 §2.1).

Nothing here reads the clock: versions and dates come from packs.yaml and sources.lock.yaml,
so rebuilding from the same inputs gives the same manifest.
"""

from __future__ import annotations

import json
from pathlib import Path

import blake3

from .common import PACKS, Pack, load_packs, load_sources, log, write_atomic

FORMAT = 1
ROLES = {
    "basemap": "basemap-{id}.pmtiles",
    "territory_tiles": "territory-{id}.pmtiles",
    "dem": "dem-{id}.wbdem",
    "terrain": "terrain-{id}.wbh3",
    "territory": "territory-{id}.bin",
}
# Which sources each file is made from, for the credits screen.
FILE_SOURCES = {
    "basemap": ["basemap"],
    "territory_tiles": ["natural_earth", "marine_regions"],
    "dem": ["copernicus_dem"],
    "terrain": ["worldcover", "copernicus_dem", "natural_earth", "marine_regions"],
    "territory": ["natural_earth", "marine_regions"],
}


def file_hash(path: Path) -> str:
    h = blake3.blake3(max_threads=blake3.blake3.AUTO)
    with open(path, "rb") as f:
        while chunk := f.read(16 << 20):
            h.update(chunk)
    return h.hexdigest()


def build(pack: Pack) -> None:
    sources = load_sources()
    files = []
    used: set[str] = set()
    for role, pattern in ROLES.items():
        path = pack.out / pattern.format(id=pack.id)
        if not path.exists():
            raise FileNotFoundError(f"{path} is missing; run the other steps first")
        files.append({"name": path.name, "role": role, "size": path.stat().st_size, "blake3": file_hash(path)})
        used.update(FILE_SOURCES[role])
    terr = json.loads((pack.work / "territory.json").read_text(encoding="utf-8"))
    classes = pack.work / "terrain_classes.json"
    order = [k for k in sources if k in used]
    manifest = {
        "format": FORMAT,
        "id": pack.id,
        "name": pack.cfg["name"],
        "version": pack.cfg["version"],
        "extent": list(pack.extent),
        "basemap_zoom": [pack.cfg["basemap"]["minzoom"], pack.cfg["basemap"]["maxzoom"]],
        "dem_res_arcsec": pack.cfg["dem"]["res_arcsec"],
        "h3_res": pack.cfg["terrain"]["h3_res"],
        "files": files,
        "size": sum(f["size"] for f in files),
        "sources": [
            {k: sources[s][k] for k in ("name", "version", "url", "licence", "accessed")} | {"id": s} for s in order
        ],
        "attribution": [sources[s]["attribution"] for s in order],
        "notes": {
            "estimated_territorial_seas": terr["estimated_seas"],
            "terrain_classes": json.loads(classes.read_text(encoding="utf-8")) if classes.exists() else {},
        },
    }
    out = pack.out / "manifest.json"
    write_atomic(out, (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))
    log(f"manifest: {out} ({manifest['size']:,} bytes in {len(files)} files)")
    write_catalog()


def write_catalog() -> None:
    """build/packs/index.json: the packs a client can install, with manifest hashes."""
    entries = []
    for pid, pack in load_packs().items():
        m = pack.out / "manifest.json"
        if not m.exists():
            continue
        data = json.loads(m.read_text(encoding="utf-8"))
        entries.append(
            {
                "id": pid,
                "name": data["name"],
                "version": data["version"],
                "size": data["size"],
                "manifest": f"{pid}/manifest.json",
                "manifest_blake3": file_hash(m),
            }
        )
    write_atomic(PACKS / "index.json", (json.dumps({"format": FORMAT, "packs": entries}, indent=2) + "\n").encode("utf-8"))
    log(f"catalog: {len(entries)} packs")
