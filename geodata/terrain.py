"""H3 terrain bake (doc 05 §3, ADR-012): class, mean elevation, slope, flags and country
per cell, written as `terrain-<pack>.wbh3`.

- Land cover: ESA WorldCover pixel counts per cell, read at a coarse COG overview.
- Relief: mean elevation, mean slope and elevation spread per cell from the pack's DEM.
- Country and border flag: from the territory step.

Thresholds come from geodata/packs.yaml. Results per source tile are cached in
build/geodata/work/<pack>/terrain/, so an interrupted run resumes.
"""

from __future__ import annotations

import json
import time
from pathlib import Path

import h3.api.basic_int as h3
import numpy as np
from osgeo import gdal

from . import formats
from .common import pool, CACHE, Pack, jobs, log

gdal.UseExceptions()

WC_PREFIX = "/vsis3/esa-worldcover/v200/2021/map/"
# WorldCover value → column in the per-cell count table.
WC_VALUES = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 100]
WC_COL = {v: i for i, v in enumerate(WC_VALUES)}
NCOL = len(WC_VALUES)
EARTH_M_PER_DEG = 111_320.0

_PACK: Pack | None = None


def wc_tiles() -> list[tuple[str, int, int]]:
    """(file name, south lat, west lon) of every WorldCover tile; the listing is cached."""
    listing = CACHE / "worldcover" / "tiles.txt"
    if not listing.exists():
        listing.parent.mkdir(parents=True, exist_ok=True)
        names = sorted(n for n in gdal.ReadDir(WC_PREFIX) or [] if n.endswith("_Map.tif"))
        if len(names) < 2000:
            raise RuntimeError(f"WorldCover listing looks incomplete ({len(names)} files)")
        listing.write_text("\n".join(names) + "\n", encoding="utf-8")
    out = []
    for name in listing.read_text(encoding="utf-8").split():
        code = name.split("_")[5]  # e.g. N54E018
        lat = int(code[1:3]) * (1 if code[0] == "N" else -1)
        lon = int(code[4:7]) * (1 if code[3] == "E" else -1)
        out.append((name, lat, lon))
    return out


def _cells_for(lats: np.ndarray, lons: np.ndarray, res: int) -> np.ndarray:
    """H3 cell for every (lat, lon) of a grid given by row latitudes and column longitudes."""
    out = np.empty((len(lats), len(lons)), dtype=np.uint64)
    lon_list = lons.tolist()
    for i, lat in enumerate(lats.tolist()):
        out[i] = [h3.latlng_to_cell(lat, lon, res) for lon in lon_list]
    return out


def _in_extent(lats: np.ndarray, lons: np.ndarray, extent) -> tuple[np.ndarray, np.ndarray]:
    w, s, e, n = extent
    return (lats >= s - 1) & (lats <= n + 1), (lons >= w - 1) & (lons <= e + 1)


def _wc_job(tile: tuple[str, int, int]) -> Path | None:
    name, _, _ = tile
    out = _PACK.work / "terrain" / "wc" / f"{name}.npz"
    if out.exists():
        return out
    level = int(_PACK.cfg["terrain"]["worldcover_overview"])
    res = int(_PACK.cfg["terrain"]["h3_res"])
    for attempt in range(1, 7):
        try:
            ds = gdal.OpenEx(WC_PREFIX + name, open_options=[f"OVERVIEW_LEVEL={level}"])
            arr = ds.GetRasterBand(1).ReadAsArray()
            gt = ds.GetGeoTransform()
            break
        except RuntimeError as err:
            if attempt == 6:
                raise
            log(f"terrain: {name} attempt {attempt}: {err}")
            gdal.VSICurlPartialClearCache(WC_PREFIX + name)
            time.sleep(5 * attempt)
    lats = gt[3] + (np.arange(arr.shape[0]) + 0.5) * gt[5]
    lons = gt[0] + (np.arange(arr.shape[1]) + 0.5) * gt[1]
    rows, cols = _in_extent(lats, lons, _PACK.extent)
    arr, lats, lons = arr[rows][:, cols], lats[rows], lons[cols]
    cells = _cells_for(lats, lons, res).ravel()
    lut = np.zeros(256, dtype=np.uint64)
    for v, c in WC_COL.items():
        lut[v] = c
    keys = cells * np.uint64(16) + lut[arr.ravel()]
    uniq, counts = np.unique(keys, return_counts=True)
    out.parent.mkdir(parents=True, exist_ok=True)
    tmp = out.with_suffix(".tmp.npz")
    np.savez_compressed(tmp, keys=uniq, counts=counts.astype(np.uint32))
    tmp.replace(out)
    return out


def _dem_job(path: Path) -> tuple[np.ndarray, np.ndarray] | None:
    """Per-cell sums of (count, elevation, elevation², slope°) from one cached DEM tile."""
    out = _PACK.work / "terrain" / "dem" / (path.stem + ".npz")
    if out.exists():
        d = np.load(out)
        return d["cells"], d["sums"]
    res_arcsec = int(_PACK.cfg["dem"]["res_arcsec"])
    res = int(_PACK.cfg["terrain"]["h3_res"])
    lat0, lon0 = int(path.stem[:3]), int(path.stem[3:])
    z = np.load(path).astype(np.float64)
    if not np.any(z):
        return None
    step = res_arcsec / 3600.0
    n = z.shape[0]
    lats = lat0 + np.arange(n) * step
    lons = lon0 + np.arange(n) * step
    # The last row and column repeat the neighbour's first; count each sample once.
    z, lats, lons = z[:-1, :-1], lats[:-1], lons[:-1]
    dy = step * EARTH_M_PER_DEG
    dx = dy * np.cos(np.radians(lats))[:, None]
    gy, gx = np.gradient(z, axis=(0, 1))
    slope = np.degrees(np.arctan(np.hypot(gy / dy, gx / dx)))
    cells = _cells_for(lats, lons, res).ravel()
    order = np.argsort(cells, kind="stable")
    cs = cells[order]
    starts = np.flatnonzero(np.r_[True, cs[1:] != cs[:-1]])
    zz = z.ravel()[order]
    ss = slope.ravel()[order]
    sums = np.stack(
        [
            np.diff(np.r_[starts, len(cs)]).astype(np.float64),
            np.add.reduceat(zz, starts),
            np.add.reduceat(zz * zz, starts),
            np.add.reduceat(ss, starts),
        ],
        axis=1,
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    np.savez(out, cells=cs[starts], sums=sums)
    return cs[starts], sums


def _merge_sums(parts: list[tuple[np.ndarray, np.ndarray]]) -> tuple[np.ndarray, np.ndarray]:
    cells = np.concatenate([p[0] for p in parts])
    sums = np.concatenate([p[1] for p in parts])
    order = np.argsort(cells, kind="stable")
    cells, sums = cells[order], sums[order]
    starts = np.flatnonzero(np.r_[True, cells[1:] != cells[:-1]])
    return cells[starts], np.add.reduceat(sums, starts, axis=0)


def classify(counts: np.ndarray, lat: np.ndarray, slope: np.ndarray, rough: np.ndarray, t: dict) -> tuple[np.ndarray, np.ndarray]:
    """Terrain class and flags from WorldCover counts (columns per WC_VALUES) and relief."""
    c = counts.astype(np.float64)
    total = c[:, 1:].sum(axis=1)
    safe = np.where(total > 0, total, 1)
    col = WC_COL
    water = c[:, col[80]] / safe
    built = c[:, col[50]] / safe
    groups = np.stack(
        [
            c[:, col[20]] + c[:, col[30]] + c[:, col[40]],  # open
            c[:, col[10]],  # forest
            c[:, col[50]],  # urban
            c[:, col[60]],  # desert
            c[:, col[90]] + c[:, col[95]],  # marsh
            c[:, col[70]] + c[:, col[100]],  # arctic
        ],
        axis=1,
    )
    group_class = np.array([formats.CLASS[k] for k in ("open", "forest", "urban", "desert", "marsh", "arctic")], dtype=np.uint8)
    cls = group_class[np.argmax(groups, axis=1)]  # ties go to the first group listed
    cls = np.where((cls == formats.CLASS["forest"]) & (np.abs(lat) <= 23.5), formats.CLASS["jungle"], cls)
    cls = np.where(built >= t["urban_fraction"], formats.CLASS["urban"], cls)
    steep = (slope >= t["mountain_slope_deg"]) | (rough >= t["mountain_rough_m"])
    cls = np.where(steep & (cls != formats.CLASS["urban"]), formats.CLASS["mountain"], cls)
    cls = np.where((total == 0) | (water >= t["water_fraction"]), formats.CLASS["water"], cls).astype(np.uint8)
    flags = np.zeros(len(cls), dtype=np.uint8)
    flags |= np.where((total > 0) & (water > 0.05) & (water < 0.95), formats.FLAG_COAST, 0).astype(np.uint8)
    flags |= np.where(built >= 0.10, formats.FLAG_URBAN, 0).astype(np.uint8)
    return cls, flags


def build(pack: Pack) -> None:
    global _PACK
    _PACK = pack
    t = pack.cfg["terrain"]
    res = int(t["h3_res"])
    w, s, e, n = pack.extent

    terr = np.load(pack.work / "territory_cells.npz")
    codes = json.loads((pack.work / "territory.json").read_text(encoding="utf-8"))["codes"]

    tiles = [x for x in wc_tiles() if x[1] + 3 > s - 1 and x[1] < n + 1 and x[2] + 3 > w - 1 and x[2] < e + 1]
    log(f"terrain: {len(tiles)} WorldCover tiles at overview {t['worldcover_overview']} on {jobs()} processes")
    with pool() as workers:
        wc_paths = []
        for i, p in enumerate(workers.imap(_wc_job, tiles, chunksize=1)):
            wc_paths.append(p)
            if i % 100 == 0:
                log(f"  {i}/{len(tiles)}")
    keys = np.concatenate([np.load(p)["keys"] for p in wc_paths])
    kc = np.concatenate([np.load(p)["counts"] for p in wc_paths])
    order = np.argsort(keys, kind="stable")
    keys, kc = keys[order], kc[order]
    starts = np.flatnonzero(np.r_[True, keys[1:] != keys[:-1]])
    keys, kc = keys[starts], np.add.reduceat(kc, starts)
    wc_cells_all = keys // np.uint64(16)
    wc_col = (keys % np.uint64(16)).astype(np.int64)
    wc_cells, inv = np.unique(wc_cells_all, return_inverse=True)
    counts = np.zeros((len(wc_cells), NCOL), dtype=np.uint32)
    np.add.at(counts, (inv, wc_col), kc)
    log(f"terrain: {len(wc_cells):,} cells with WorldCover data")

    dem_tiles = sorted((pack.work / "dem").glob("*.npy"))
    log(f"terrain: relief from {len(dem_tiles)} DEM tiles")
    with pool() as workers:
        parts = [p for p in workers.imap(_dem_job, dem_tiles, chunksize=4) if p is not None]
    dem_cells, sums = _merge_sums(parts) if parts else (np.zeros(0, np.uint64), np.zeros((0, 4)))

    # Cells to store: every territory or border cell, and every cell with land pixels.
    land_px = counts[:, 1:].sum(axis=1) - counts[:, WC_COL[80]]
    cells = np.unique(np.concatenate([terr["cells"], terr["border"], wc_cells[land_px > 0]]))
    log(f"terrain: {len(cells):,} cells to store")

    def pick(src_cells: np.ndarray, src: np.ndarray, fill) -> np.ndarray:
        """Rows of `src` for `cells` (src_cells sorted); `fill` where a cell is absent."""
        out = np.full((len(cells),) + src.shape[1:], fill, dtype=src.dtype)
        if len(src_cells) == 0:
            return out
        pos = np.minimum(np.searchsorted(src_cells, cells), len(src_cells) - 1)
        hit = src_cells[pos] == cells
        out[hit] = src[pos[hit]]
        return out

    cnt = pick(wc_cells, counts, 0)
    rel = pick(dem_cells, sums, 0.0)
    num = np.where(rel[:, 0] > 0, rel[:, 0], 1)
    mean = rel[:, 1] / num
    rough = np.sqrt(np.maximum(rel[:, 2] / num - mean * mean, 0))
    slope = rel[:, 3] / num
    lat = np.array([h3.cell_to_latlng(int(c))[0] for c in cells])

    cls, flags = classify(cnt, lat, slope, rough, t)
    border = np.isin(cells, terr["border"])
    flags |= np.where(border, formats.FLAG_BORDER, 0).astype(np.uint8)
    country = pick(terr["cells"], terr["owners"], 0).astype(np.uint16)
    elev = np.clip(np.round(mean), -32000, 32000).astype(np.int16)
    slope_u8 = np.clip(np.round(slope * 2), 0, 255).astype(np.uint8)

    out = pack.out / f"terrain-{pack.id}.wbh3"
    formats.write_h3(out, res, codes, cells, cls, elev, slope_u8, flags, country)
    hist = {k: int((cls == v).sum()) for k, v in formats.CLASS.items()}
    log(f"terrain: wrote {out.name} ({out.stat().st_size:,} bytes); classes {hist}")
    (pack.work / "terrain_classes.json").write_text(json.dumps(hist, indent=1), encoding="utf-8")
