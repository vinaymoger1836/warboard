"""Territory bake (doc 05 §5).

Country territory = Natural Earth admin-0 land ∪ Marine Regions 12 nm sea, internal
and archipelagic waters. Land wins where the two overlap. Narrow gaps between the two
coastlines go to the country whose land is next to them, and countries listed under
`derived_territorial_sea` (absent from Marine Regions) get a 12 nm belt from their
Natural Earth coast, marked as an estimate in the manifest.

Outputs:
- `territory-<pack>.pmtiles`: country polygons for the map tint (layer `territory`, field `code`).
- `territory-<pack>.bin`: exact rings for H3 cells that a territory boundary crosses.
- work/territory_cells.npz: owner at the centre of every territory cell, and the border cells,
  for the terrain step.
"""

from __future__ import annotations

import json
import math
import shutil
import tempfile
from collections import defaultdict
from pathlib import Path

import h3.api.basic_int as h3
import numpy as np
import shapely
import yaml
from osgeo import gdal, ogr, osr
from shapely.geometry import box

from . import formats, sources
from .common import pool, GEODATA, Pack, jobs, log, run

ogr.UseExceptions()
osr.UseExceptions()
# Marine Regions coastlines are single features of tens of MB.
gdal.SetConfigOption("OGR_GEOJSON_MAX_OBJ_SIZE", "0")

TERRITORIAL_SEA_M = 22_224.0  # 12 nm
GAP_FILL_DEG = 0.03
CELL_MARGIN = 0.15  # border test and ring clip use the cell grown by 15 % around its centre
COORD_SCALE = 1e7

# Filled in the parent before forking the workers.
_TILE_PIECES: dict[tuple[int, int], list[tuple[int, bytes]]] = {}
_TILE_LINES: dict[tuple[int, int], bytes] = {}
_RES = 0
_SAMPLE = 0.0


def _settings() -> dict:
    return yaml.safe_load((GEODATA / "packs.yaml").read_text(encoding="utf-8")).get("territory", {})


def _read(path, clip: tuple[float, float, float, float] | None, fields: list[str]):
    ds = ogr.Open(str(path))
    layer = ds.GetLayer(0)
    if clip is not None:
        layer.SetSpatialFilterRect(*clip)
    for f in layer:
        g = f.GetGeometryRef()
        if g is None:
            continue
        geom = shapely.make_valid(shapely.from_wkb(bytes(g.ExportToIsoWkb())))
        if clip is not None:
            geom = shapely.clip_by_rect(geom, *clip)
        if not geom.is_empty:
            yield {k: f.GetField(k) for k in fields}, geom


def _ne_code(row: dict, main: dict[str, dict]) -> str:
    def own(r: dict) -> str:
        iso = (r["ISO_A3_EH"] or "").strip()
        return (iso if len(iso) == 3 and iso.isalpha() else r["ADM0_A3"]).lower()

    if row["TYPE"] in ("Disputed", "Indeterminate") or row["SOVEREIGNT"] not in main:
        return own(row)
    return own(main[row["SOVEREIGNT"]])


def load_land(clip) -> dict[str, shapely.Geometry]:
    fields = ["ADMIN", "SOVEREIGNT", "TYPE", "ISO_A3_EH", "ADM0_A3"]
    rows = list(_read(sources.natural_earth(), None, fields))
    main = {r["SOVEREIGNT"]: r for r, _ in rows if r["ADMIN"] == r["SOVEREIGNT"]}
    parts: dict[str, list] = defaultdict(list)
    for r, g in rows:
        if clip is not None:
            g = shapely.clip_by_rect(g, *clip)
            if g.is_empty:
                continue
        parts[_ne_code(r, main)].append(g)
    return {c: shapely.union_all(gs) for c, gs in sorted(parts.items())}


def load_seas(clip) -> dict[str, shapely.Geometry]:
    parts: dict[str, list] = defaultdict(list)
    for shp in sources.marine_regions():
        for r, g in _read(shp, clip, ["iso_sov1"]):
            code = (r["iso_sov1"] or "").strip().lower()
            if len(code) == 3:
                parts[code].append(g)
    return {c: shapely.union_all(gs) for c, gs in sorted(parts.items())}


def geodesic_buffer(geom: shapely.Geometry, metres: float) -> shapely.Geometry:
    """Buffer in metres through an azimuthal equidistant projection centred on the shape."""
    c = geom.centroid
    wgs = osr.SpatialReference()
    wgs.ImportFromEPSG(4326)
    wgs.SetAxisMappingStrategy(osr.OAMS_TRADITIONAL_GIS_ORDER)
    aeqd = osr.SpatialReference()
    aeqd.ImportFromProj4(f"+proj=aeqd +lat_0={c.y:.6f} +lon_0={c.x:.6f} +datum=WGS84 +units=m")
    fwd = osr.CoordinateTransformation(wgs, aeqd)
    back = osr.CoordinateTransformation(aeqd, wgs)
    g = ogr.CreateGeometryFromWkb(shapely.to_wkb(geom))
    g.Transform(fwd)
    buffered = shapely.from_wkb(bytes(g.ExportToIsoWkb())).buffer(metres, quad_segs=8)
    g2 = ogr.CreateGeometryFromWkb(shapely.to_wkb(buffered))
    g2.Transform(back)
    return shapely.make_valid(shapely.from_wkb(bytes(g2.ExportToIsoWkb())))


def _minus(geom: shapely.Geometry, others: list[shapely.Geometry]) -> shapely.Geometry:
    """`geom` minus every geometry in `others` that touches its bounding box."""
    if geom.is_empty or not others:
        return geom
    tree = shapely.STRtree(others)
    hits = tree.query(geom, predicate="intersects")
    if len(hits) == 0:
        return geom
    return shapely.make_valid(shapely.difference(geom, shapely.union_all([others[i] for i in sorted(hits)])))


def combine(land, seas, derived_codes, clip, first_codes=()) -> tuple[dict[str, shapely.Geometry], list[str]]:
    land_parts = [p for g in land.values() for p in shapely.get_parts(g)]
    assigned: list[shapely.Geometry] = list(land_parts)
    sea_out: dict[str, shapely.Geometry] = {}
    estimated: list[str] = []
    # Belts that take precedence over another state's Marine Regions sea (e.g. Taiwan,
    # whose waters Marine Regions v4 codes as Chinese).
    for code in sorted(first_codes):
        if code not in land:
            continue
        belt = geodesic_buffer(land[code], TERRITORIAL_SEA_M)
        if clip is not None:
            belt = shapely.clip_by_rect(belt, *clip)
        belt = _minus(belt, land_parts)
        if not belt.is_empty:
            estimated.append(code)
            sea_out[code] = belt
            assigned.extend(shapely.get_parts(belt))
    for code, g in seas.items():
        g = _minus(g, assigned)
        sea_out[code] = shapely.union_all([sea_out[code], g]) if code in sea_out else g
        assigned.extend(shapely.get_parts(g))
    for code in sorted(derived_codes):
        if code not in land or code in seas:
            continue
        belt = geodesic_buffer(land[code], TERRITORIAL_SEA_M)
        if clip is not None:
            belt = shapely.clip_by_rect(belt, *clip)
        belt = _minus(belt, assigned)
        if not belt.is_empty:
            estimated.append(code)
            sea_out[code] = shapely.union_all([sea_out.get(code, shapely.Polygon()), belt])
            assigned.extend(shapely.get_parts(belt))
    fill: dict[str, shapely.Geometry] = {}
    for code, g in land.items():
        f = _minus(shapely.buffer(g, GAP_FILL_DEG, quad_segs=2), assigned)
        if clip is not None:
            f = shapely.clip_by_rect(f, *clip)
        if not f.is_empty:
            fill[code] = f
            assigned.extend(shapely.get_parts(f))
    out: dict[str, shapely.Geometry] = {}
    for code in sorted(set(land) | set(sea_out) | set(fill)):
        parts = [g for g in (land.get(code), sea_out.get(code), fill.get(code)) if g is not None and not g.is_empty]
        merged = shapely.make_valid(shapely.union_all(parts))
        polys = [p for p in shapely.get_parts(merged) if p.geom_type == "Polygon" and p.area > 0]
        if polys:
            out[code] = shapely.MultiPolygon(polys)
    return out, estimated


def _clip(g: shapely.Geometry, rect: tuple[float, float, float, float]) -> shapely.Geometry:
    """Fast rectangle clip; falls back to a full overlay when GEOS rejects a degenerate ring."""
    try:
        return shapely.clip_by_rect(g, *rect)
    except shapely.errors.GEOSException:
        return shapely.intersection(shapely.make_valid(g), box(*rect))


def _polygonal(g: shapely.Geometry) -> shapely.Geometry:
    polys = [p for p in shapely.get_parts(g) if p.geom_type == "Polygon" and p.area > 0]
    return shapely.MultiPolygon(polys) if polys else shapely.MultiPolygon()


def _grid(geoms: list[tuple[int, shapely.Geometry]], step: float, bounds) -> dict[tuple[int, int], list[tuple[int, shapely.Geometry]]]:
    """Clips (index, geometry) pairs into a grid of `step`-degree tiles keyed by (ix, iy) from -180/-90."""
    tree = shapely.STRtree([g for _, g in geoms])
    w, s, e, n = bounds
    out: dict[tuple[int, int], list] = {}
    for ix in range(math.floor((w + 180) / step), math.ceil((e + 180) / step)):
        for iy in range(math.floor((s + 90) / step), math.ceil((n + 90) / step)):
            rect = (ix * step - 180, iy * step - 90, (ix + 1) * step - 180, (iy + 1) * step - 90)
            hits = tree.query(box(*rect))
            pieces = []
            for i in sorted(hits):
                idx, g = geoms[i]
                c = _clip(g, rect)
                if not c.is_empty:
                    pieces.append((idx, c))
            if pieces:
                out[(ix, iy)] = pieces
    return out


def _grid_fine(geoms: list[tuple[int, shapely.Geometry]], bounds) -> dict[tuple[int, int], list[tuple[int, shapely.Geometry]]]:
    """Two-level split (10° then 1°) so big shapes are not clipped thousands of times."""
    coarse = _grid(geoms, 10.0, bounds)
    fine: dict[tuple[int, int], list] = {}
    for (cx, cy), pieces in sorted(coarse.items()):
        rect = (cx * 10 - 180, cy * 10 - 90, cx * 10 - 170, cy * 10 - 80)
        for key, sub in _grid(pieces, 1.0, rect).items():
            fine.setdefault(key, []).extend(sub)
    return fine


def _cell_poly(cell: int) -> tuple[np.ndarray, bool]:
    """Cell boundary as (lon, lat) grown by CELL_MARGIN; shifted east past 180° when it straddles it."""
    pts = np.array([(lng, lat) for lat, lng in h3.cell_to_boundary(cell)])
    shifted = pts[:, 0].max() - pts[:, 0].min() > 180
    if shifted:
        pts[:, 0] = np.where(pts[:, 0] < 0, pts[:, 0] + 360, pts[:, 0])
    c = pts.mean(axis=0)
    return c + (pts - c) * (1 + CELL_MARGIN), bool(shifted)


def _tile_job(key: tuple[int, int]) -> tuple[list[int], list[int], list[int]]:
    """Interior cells (centre → owner) and border cells of one 1° tile."""
    cells: list[int] = []
    owners: list[int] = []
    for idx, wkb in _TILE_PIECES.get(key, []):
        g = shapely.from_wkb(wkb)
        for p in shapely.get_parts(g):
            if p.geom_type != "Polygon":
                continue
            for c in h3.geo_to_cells(p, _RES):
                cells.append(c)
                owners.append(idx)
    border: list[int] = []
    lines_wkb = _TILE_LINES.get(key)
    if lines_wkb:
        lines = shapely.from_wkb(lines_wkb)
        pts = shapely.get_coordinates(shapely.segmentize(lines, _SAMPLE))
        hit = {h3.latlng_to_cell(float(lat), float(lng), _RES) for lng, lat in pts}
        cand = sorted({n for c in hit for n in h3.grid_disk(c, 1)})
        shapely.prepare(lines)
        for c in cand:
            poly, shifted = _cell_poly(c)
            if shifted:
                continue  # handled with the antimeridian pass in the parent
            if shapely.intersects(lines, shapely.Polygon(poly)):
                border.append(c)
    return cells, owners, border


def _rings(geom: shapely.Geometry, dx: float = 0.0) -> list[np.ndarray]:
    out = []
    for p in shapely.get_parts(geom):
        if p.geom_type != "Polygon" or p.is_empty:
            continue
        for ring in [p.exterior, *p.interiors]:
            xy = np.asarray(ring.coords)[:-1]
            if len(xy) < 3:
                continue
            xy = xy.copy()
            xy[:, 0] += dx
            out.append(np.round(xy * COORD_SCALE).astype(np.int32))
    return out


def _record(cell: int, pieces_at) -> bytes:
    poly, shifted = _cell_poly(cell)
    x0, y0 = poly.min(axis=0)
    x1, y1 = poly.max(axis=0)
    by_country: dict[int, list[np.ndarray]] = defaultdict(list)
    for ix in range(math.floor(x0) + 180, math.floor(x1) + 181):
        for iy in range(math.floor(y0) + 90, math.floor(y1) + 91):
            dx = 360.0 if ix >= 360 else 0.0
            for idx, g in pieces_at((ix % 360, iy)):
                c = _clip(g, (x0 - dx, y0, x1 - dx, y1))
                if not c.is_empty:
                    by_country[idx].extend(_rings(c, dx))
    pieces = [(idx, rings) for idx, rings in sorted(by_country.items()) if rings]
    return formats.encode_rings_record(pieces, shifted)


def build(pack: Pack) -> None:
    global _TILE_PIECES, _TILE_LINES, _RES, _SAMPLE
    settings = _settings()
    w, s, e, n = pack.extent
    clip = None if pack.is_world else (max(-180, w - 1), max(-90, s - 1), min(180, e + 1), min(90, n + 1))
    bounds = clip or (-180.0, -90.0, 180.0, 90.0)

    pack.work.mkdir(parents=True, exist_ok=True)
    gpkg = pack.work / "territory.gpkg"
    combined = pack.work / "territory_combined.json"
    if gpkg.exists() and combined.exists():
        # Merging land and seas takes minutes; reuse it (delete the work folder to redo it).
        log(f"territory: reusing {gpkg.name}")
        meta = json.loads(combined.read_text(encoding="utf-8"))
        estimated, missing = meta["estimated"], meta["missing"]
        territory = {r["code"]: g for r, g in _read(gpkg, None, ["code"])}
    else:
        log("territory: loading Natural Earth land")
        land = load_land(clip)
        log("territory: loading Marine Regions seas")
        seas = load_seas(clip)
        missing = sorted(set(land) - set(seas))
        log(f"territory: {len(missing)} land codes have no Marine Regions sea: {' '.join(missing)}")
        territory, estimated = combine(
            land,
            seas,
            settings.get("derived_territorial_sea", []),
            clip,
            settings.get("derived_territorial_sea_first", []),
        )
        _write_gpkg(gpkg, territory)
        combined.write_text(json.dumps({"estimated": estimated, "missing": missing}), encoding="utf-8")
    # Buffers (gap fill, derived belts) can spill past ±180° or the poles; keep the globe.
    territory = {c: _polygonal(_clip(g, bounds)) for c, g in territory.items()}
    territory = {c: g for c, g in territory.items() if not g.is_empty}
    codes = sorted(territory)
    index = {c: i + 1 for i, c in enumerate(codes)}
    log(f"territory: {len(codes)} countries; derived 12 nm belts (estimate): {estimated or 'none'}")
    if not (pack.out / f"territory-{pack.id}.pmtiles").exists():
        _write_gpkg(gpkg, territory)
        _write_tiles(pack, gpkg)

    log("territory: splitting into 1° tiles")
    polys = [(index[c], g) for c, g in territory.items()]
    lines = [(index[c], shapely.boundary(g)) for c, g in territory.items()]
    tile_pieces = _grid_fine(polys, bounds)
    tile_lines = _grid_fine(lines, bounds)
    _TILE_PIECES = {k: [(i, shapely.to_wkb(g)) for i, g in v] for k, v in tile_pieces.items()}
    _TILE_LINES = {k: shapely.to_wkb(shapely.union_all([g for _, g in v])) for k, v in tile_lines.items()}
    _RES = int(pack.cfg["terrain"]["h3_res"])
    _SAMPLE = float(pack.cfg["terrain"]["boundary_sample_deg"])

    keys = sorted(set(_TILE_PIECES) | set(_TILE_LINES))
    log(f"territory: baking {len(keys)} tiles at H3 res {_RES} on {jobs()} processes")
    all_cells: list[np.ndarray] = []
    all_owners: list[np.ndarray] = []
    border: set[int] = set()
    with pool() as workers:
        for i, (cells, owners, b) in enumerate(workers.imap(_tile_job, keys, chunksize=4)):
            all_cells.append(np.asarray(cells, dtype=np.uint64))
            all_owners.append(np.asarray(owners, dtype=np.uint16))
            border.update(b)
            if i % 500 == 0:
                log(f"  {i}/{len(keys)} tiles")
    border.update(_antimeridian_border(tile_lines))

    cells = np.concatenate(all_cells) if all_cells else np.zeros(0, np.uint64)
    owners = np.concatenate(all_owners) if all_owners else np.zeros(0, np.uint16)
    order = np.lexsort((owners, cells))
    cells, owners = cells[order], owners[order]
    keep = np.ones(len(cells), bool)
    keep[1:] = cells[1:] != cells[:-1]
    cells, owners = cells[keep], owners[keep]
    border_arr = np.array(sorted(border), dtype=np.uint64)
    log(f"territory: {len(cells):,} territory cells, {len(border_arr):,} border cells")

    def pieces_at(key):
        return tile_pieces.get(key, [])

    log("territory: writing border-cell rings")
    records = [_record(int(c), pieces_at) for c in border_arr]
    formats.write_territory(pack.out / f"territory-{pack.id}.bin", _RES, codes, border_arr, records)
    np.savez(pack.work / "territory_cells.npz", cells=cells, owners=owners, border=border_arr)
    (pack.work / "territory.json").write_text(
        json.dumps({"codes": codes, "estimated_seas": estimated, "missing_seas": missing}, indent=1), encoding="utf-8"
    )
    log("territory: done")


def _antimeridian_border(tile_lines) -> set[int]:
    """Border cells that straddle 180°: test them against lines on both sides, shifted east."""
    out: set[int] = set()
    for iy in range(180):
        west = tile_lines.get((0, iy), [])
        east = tile_lines.get((359, iy), [])
        if not west and not east:
            continue
        geoms = [g for _, g in east] + [shapely.transform(g, lambda xy: xy + [360.0, 0.0]) for _, g in west]
        lines = shapely.union_all(geoms)
        lat = iy - 90 + 0.5
        cand = set()
        for lat_s in np.linspace(lat - 0.5, lat + 0.5, 12):
            for lng in (179.999, -179.999):
                c = h3.latlng_to_cell(float(lat_s), lng, _RES)
                cand.update(h3.grid_disk(c, 1))
        for c in sorted(cand):
            poly, shifted = _cell_poly(c)
            if shifted and shapely.intersects(lines, shapely.Polygon(poly)):
                out.add(c)
    return out


def _write_gpkg(path, territory: dict[str, shapely.Geometry]) -> None:
    path.unlink(missing_ok=True)
    drv = ogr.GetDriverByName("GPKG")
    ds = drv.CreateDataSource(str(path))
    srs = osr.SpatialReference()
    srs.ImportFromEPSG(4326)
    layer = ds.CreateLayer("territory", srs, ogr.wkbMultiPolygon, options=["SPATIAL_INDEX=NO"])
    layer.CreateField(ogr.FieldDefn("code", ogr.OFTString))
    for code, geom in territory.items():
        f = ogr.Feature(layer.GetLayerDefn())
        f.SetField("code", code)
        f.SetGeometry(ogr.CreateGeometryFromWkb(shapely.to_wkb(geom)))
        layer.CreateFeature(f)
    ds = None


def _write_tiles(pack: Pack, gpkg) -> None:
    out = pack.out / f"territory-{pack.id}.pmtiles"
    out.unlink(missing_ok=True)
    # GDAL keeps a temporary MBTiles next to the output; build inside the container,
    # where Docker's Windows bind mounts cannot leave a stale one behind.
    local = Path(tempfile.mkdtemp()) / out.name
    z = pack.cfg["territory_tiles"]
    cmd = [
        "ogr2ogr",
        "-f",
        "PMTiles",
        str(local),
        str(gpkg),
        "territory",
        "-dsco",
        f"MINZOOM={z['minzoom']}",
        "-dsco",
        f"MAXZOOM={z['maxzoom']}",
        "-dsco",
        "NAME=territory",
        "-dsco",
        "MAX_SIZE=1000000",
        "-t_srs",
        "EPSG:3857",
        "--config",
        "GDAL_NUM_THREADS",
        "1",
    ]
    # Web Mercator stops at ±85.0511°; Antarctica would otherwise fail to project.
    w, s, e, n = pack.extent
    cmd += ["-clipsrc", str(w), str(max(s, -85.05)), str(e), str(min(n, 85.05))]
    run(cmd)
    shutil.copyfile(local, out)
    shutil.rmtree(local.parent, ignore_errors=True)
