"""DEM: Copernicus GLO-90 resampled to the pack resolution in 1° tiles (doc 05 §2.1, §4).

Each output tile is an (n+1)×(n+1) grid of points at whole multiples of the resolution,
edges included, so a sample never needs the neighbouring tile.

Two passes:
1. fetch: read each needed source COG once, at its 4× overview (12″ in latitude), through
   GDAL's /vsis3/; full-resolution data is never downloaded. Cached as compressed int16.
2. warp: average the cached overviews onto the pack grid, offline, one 1° tile per job.
"""

from __future__ import annotations

import math
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np
from osgeo import gdal

from . import formats
from .common import pool, CACHE, Pack, download, jobs, load_sources, log

gdal.UseExceptions()

OVERVIEW_LEVEL = 1  # second overview: 4× coarser than the 3″ source
FETCH_THREADS = 24
SRC_CACHE = CACHE / "copernicus_dem" / f"ovr{OVERVIEW_LEVEL}"

_RES = 0
_TILES: set[str] = set()
_WORK: Path | None = None


def _name(lat: int, lon: int) -> str:
    ns = f"N{lat:02d}" if lat >= 0 else f"S{-lat:02d}"
    ew = f"E{lon:03d}" if lon >= 0 else f"W{-lon:03d}"
    return f"Copernicus_DSM_COG_30_{ns}_00_{ew}_00_DEM"


def tile_list() -> set[str]:
    url = load_sources()["copernicus_dem"]["url"] + "tileList.txt"
    path = download(url, CACHE / "copernicus_dem" / "tileList.txt")
    return {line.strip() for line in path.read_text().splitlines() if line.strip()}


def _fetch(name: str) -> None:
    out = SRC_CACHE / f"{name}.npz"
    if out.exists():
        return
    path = f"/vsis3/copernicus-dem-90m/{name}/{name}.tif"
    for attempt in range(1, 7):
        try:
            ds = gdal.OpenEx(path, open_options=[f"OVERVIEW_LEVEL={OVERVIEW_LEVEL}"])
            arr = ds.GetRasterBand(1).ReadAsArray()
            gt = np.array(ds.GetGeoTransform())
            break
        except RuntimeError as err:
            if attempt == 6:
                raise
            log(f"dem: fetch {name} attempt {attempt}: {err}")
            # A dropped transfer leaves a short block in GDAL's HTTP cache; drop it and back off.
            gdal.VSICurlPartialClearCache(path)
            time.sleep(5 * attempt)
    tmp = out.with_suffix(".tmp.npz")
    np.savez_compressed(tmp, z=np.clip(np.round(arr), -32000, 32000).astype(np.int16), gt=gt)
    tmp.replace(out)


def _mem(name: str):
    data = np.load(SRC_CACHE / f"{name}.npz")
    z, gt = data["z"], data["gt"]
    ds = gdal.GetDriverByName("MEM").Create("", z.shape[1], z.shape[0], 1, gdal.GDT_Int16)
    ds.SetGeoTransform(tuple(gt))
    ds.SetProjection("EPSG:4326")
    ds.GetRasterBand(1).WriteArray(z)
    return ds


def _tile_job(key: tuple[int, int]) -> tuple[tuple[int, int], np.ndarray | None]:
    lon, lat = key
    cached = _WORK / f"{lat:+03d}{lon:+04d}.npy"
    if cached.exists():
        return key, np.load(cached)
    if _name(lat, lon) not in _TILES:
        return key, None
    srcs = []
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            la, lo = lat + dy, lon + dx
            if -90 <= la < 90 and -180 <= lo < 180 and _name(la, lo) in _TILES:
                srcs.append(_mem(_name(la, lo)))
    r = _RES / 3600.0
    n = 3600 // _RES
    ds = gdal.Warp(
        "",
        srcs,
        format="MEM",
        outputBounds=(lon - r / 2, lat - r / 2, lon + 1 + r / 2, lat + 1 + r / 2),
        width=n + 1,
        height=n + 1,
        dstSRS="EPSG:4326",
        resampleAlg="average",
        outputType=gdal.GDT_Float32,
        dstNodata=-32768,
        multithread=False,
    )
    arr = ds.GetRasterBand(1).ReadAsArray()
    # Points outside every source tile (open sea next to the coast) are sea level.
    arr = np.where(arr <= -32767, 0.0, arr)
    grid = np.flipud(np.clip(np.round(arr), -32000, 32000)).astype(np.int16)
    np.save(cached, grid)
    return key, grid


def build(pack: Pack) -> None:
    global _RES, _TILES, _WORK
    _RES = int(pack.cfg["dem"]["res_arcsec"])
    assert 3600 % _RES == 0
    _TILES = tile_list()
    _WORK = pack.work / "dem"
    _WORK.mkdir(parents=True, exist_ok=True)
    SRC_CACHE.mkdir(parents=True, exist_ok=True)
    w, s, e, n = pack.extent
    lon0, lat0 = math.floor(w), math.floor(s)
    cols, rows = math.ceil(e) - lon0, math.ceil(n) - lat0
    keys = [(lon, lat) for lat in range(lat0, lat0 + rows) for lon in range(lon0, lon0 + cols)]

    needed_set: set[str] = set()
    for lon, lat in keys:
        for la in range(lat - 1, lat + 2):
            for lo in range(lon - 1, lon + 2):
                if -90 <= la < 90 and -180 <= lo < 180 and _name(la, lo) in _TILES:
                    needed_set.add(_name(la, lo))
    needed = sorted(needed_set)
    todo = [nm for nm in needed if not (SRC_CACHE / f"{nm}.npz").exists()]
    log(f"dem: {len(needed)} source tiles, {len(todo)} to fetch on {FETCH_THREADS} threads")
    with ThreadPoolExecutor(FETCH_THREADS) as ex:
        for i, _ in enumerate(ex.map(_fetch, todo)):
            if i % 1000 == 0:
                log(f"  fetched {i}/{len(todo)}")

    log(f"dem: warping {len(keys)} tiles to {_RES}″ on {jobs()} processes")
    tiles: dict[tuple[int, int], np.ndarray] = {}
    with pool() as workers:
        for i, (key, grid) in enumerate(workers.imap(_tile_job, keys, chunksize=16)):
            if grid is not None:
                tiles[(key[0] - lon0, key[1] - lat0)] = grid
            if i % 5000 == 0:
                log(f"  {i}/{len(keys)}")
    out = pack.out / f"dem-{pack.id}.wbdem"
    formats.write_dem(out, _RES, lon0, lat0, cols, rows, tiles)
    log(f"dem: wrote {out.name} ({out.stat().st_size:,} bytes, {len(tiles)} tiles)")
