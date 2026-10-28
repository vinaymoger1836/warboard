"""Basemap: cut the region and zoom range out of the Protomaps daily build (doc 05 §7)."""

from __future__ import annotations

import subprocess
import time

from .common import Pack, load_sources, log, run


def build(pack: Pack) -> None:
    src = load_sources()["basemap"]["url"]
    out = pack.out / f"basemap-{pack.id}.pmtiles"
    if out.exists():
        log(f"basemap: {out.name} exists, skipping (delete it to rebuild)")
        return
    out.parent.mkdir(parents=True, exist_ok=True)
    zooms = pack.cfg["basemap"]
    tmp = out.with_suffix(".pmtiles.part")
    cmd = [
        "pmtiles",
        "extract",
        src,
        str(tmp),
        f"--minzoom={zooms['minzoom']}",
        f"--maxzoom={zooms['maxzoom']}",
        "--download-threads=4",
    ]
    if not pack.is_world:
        w, s, e, n = pack.extent
        cmd.append(f"--bbox={w},{s},{e},{n}")
    # `pmtiles extract` cannot resume, and a long transfer sometimes drops; retry it whole.
    for attempt in range(1, 6):
        tmp.unlink(missing_ok=True)
        try:
            run(cmd)
            break
        except subprocess.CalledProcessError:
            log(f"basemap: extract attempt {attempt} failed")
            if attempt == 5:
                raise
            time.sleep(30 * attempt)
    tmp.replace(out)
    log(f"basemap: wrote {out.name} ({out.stat().st_size:,} bytes)")
