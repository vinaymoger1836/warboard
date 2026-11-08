"""Territory bake (doc 05 §5, doc 11 §2.2–2.3).

Nation territory is generated, not traced: a theatre pack (`territory: { world: … }`
in packs.yaml) grows its nations from the seeds in `content/world/<world>.yaml`
over its own H3 terrain cells, with 12 nm belts around the coasts (geodata/borders.py).
The world pack reuses the theatre's polygons (`territory: { from_pack: … }`), so the
two never disagree. Natural Earth admin-0 is read only for the overlap check below; it
is not part of any pack.

Runs after the terrain step, whose cells it needs, and finishes the terrain file with
the nation and border fields.

Outputs:
- `territory-<pack>.pmtiles`: layer `territory` (nation polygons, field `code`) and layer
  `borders` (outlines, field `kind`: `land` or `sea`) for the map.
- `territory-<pack>.bin`: exact rings for H3 cells that a territory boundary crosses.
- `terrain-<pack>.wbh3`: the terrain step's cells plus nation and border flag per cell.
- work/territory.json: codes and the overlap check result; work/nations.npz: owner of
  every cell in the theatre window (for `geodata places` and `geodata bases`).
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
from osgeo import ogr, osr
from shapely.geometry import box

from . import borders, formats, sources, terrain
from .common import WORK, Pack, jobs, log, pool, run

ogr.UseExceptions()
osr.UseExceptions()

CELL_MARGIN = 0.15  # border test and ring clip use the cell grown by 15 % around its centre
COORD_SCALE = 1e7
# Doc 11 §2.2: generated land borders may run within OVERLAP_M of a real admin-0
# border for at most OVERLAP_MAX of their length.
OVERLAP_M = 5_000.0
OVERLAP_MAX = 0.15
OVERLAP_STEP_M = 1_000.0

# Filled in the parent before forking the workers.
_TILE_PIECES: dict[tuple[int, int], list[tuple[int, bytes]]] = {}
_TILE_LINES: dict[tuple[int, int], bytes] = {}
_RES = 0
_SAMPLE = 0.0


def _read(path, clip: tuple[float, float, float, float] | None):
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
            yield geom


def _read_layer(path, layer: str, field: str) -> dict[str, shapely.Geometry]:
    ds = ogr.Open(str(path))
    out = {}
    for f in ds.GetLayerByName(layer):
        g = f.GetGeometryRef()
        if g is not None:
            out[f.GetField(field)] = shapely.from_wkb(bytes(g.ExportToIsoWkb()))
    return dict(sorted(out.items()))


def _project(center_lon: float, center_lat: float):
    """Transforms (lon, lat) geometry into metres around a centre (azimuthal equidistant)."""
    wgs = osr.SpatialReference()
    wgs.ImportFromEPSG(4326)
    wgs.SetAxisMappingStrategy(osr.OAMS_TRADITIONAL_GIS_ORDER)
    aeqd = osr.SpatialReference()
    aeqd.ImportFromProj4(f"+proj=aeqd +lat_0={center_lat:.6f} +lon_0={center_lon:.6f} +datum=WGS84 +units=m")
    fwd = osr.CoordinateTransformation(wgs, aeqd)

    def go(geom: shapely.Geometry) -> shapely.Geometry:
        if geom.is_empty:
            return geom
        g = ogr.CreateGeometryFromWkb(shapely.to_wkb(geom))
        g.Transform(fwd)
        return shapely.from_wkb(bytes(g.ExportToIsoWkb()))

    return go


def overlap_check(land_lines: shapely.Geometry, extent) -> dict:
    """Share of generated land-border length within OVERLAP_M of a Natural Earth admin-0
    land border (doc 11 §2.2). Natural Earth is read here and nowhere else."""
    w, s, e, n = extent
    clip = (w - 1, s - 1, e + 1, n + 1)
    polys = list(_read(sources.natural_earth(), clip))
    lines = shapely.union_all([shapely.boundary(g) for g in polys])
    coast = shapely.boundary(shapely.union_all(polys))
    real = shapely.difference(lines, shapely.buffer(coast, 0.01, quad_segs=2))
    proj = _project((w + e) / 2, (s + n) / 2)
    real_m = proj(real)
    ours_m = proj(land_lines)
    points = []
    for part in shapely.get_parts(ours_m):
        length = part.length
        if length <= OVERLAP_STEP_M / 2:
            continue
        points.append(shapely.line_interpolate_point(part, np.arange(OVERLAP_STEP_M / 2, length, OVERLAP_STEP_M)))
    pts = np.concatenate(points) if points else np.array([], dtype=object)
    parts = list(shapely.get_parts(real_m))
    near = 0
    if len(pts) and parts:
        tree = shapely.STRtree(parts)
        hits = tree.query(pts, predicate="dwithin", distance=OVERLAP_M)
        near = len(np.unique(hits[0]))
    share = near / len(pts) if len(pts) else 0.0
    return {
        "land_border_km": round(len(pts) * OVERLAP_STEP_M / 1000.0, 1),
        "within_km": OVERLAP_M / 1000.0,
        "share": round(share, 4),
        "max_share": OVERLAP_MAX,
        "pass": share <= OVERLAP_MAX,
    }


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


def _theatre(pack: Pack) -> tuple[dict[str, shapely.Geometry], dict[str, shapely.Geometry], dict]:
    """Grows the nations of the pack's world; writes work/nations.npz."""
    world = borders.load_world(pack.cfg["territory"]["world"])
    t = np.load(pack.work / "terrain_cells.npz")
    result = borders.generate(world, int(pack.cfg["terrain"]["h3_res"]), t["cells"], t["cls"])
    lines = borders.border_lines(result.territory, result.land_mask)
    log("territory: overlap check against Natural Earth admin-0 land borders")
    check = overlap_check(lines["land"], world.extent)
    log(
        f"territory: {check['share']:.1%} of {check['land_border_km']:,} km of land border lies within "
        f"{check['within_km']:g} km of a real border (limit {OVERLAP_MAX:.0%})"
    )
    np.savez(
        pack.work / "nations.npz",
        cells=result.grid.cells,
        owner=result.owner.astype(np.int16),
        landlike=result.landlike,
        sea=result.grid.sea,
        cls=result.grid.cls,
        codes=np.array(world.codes),
    )
    if not check["pass"]:
        raise RuntimeError(
            f"generated borders follow real borders for {check['share']:.1%} of their length "
            f"(limit {OVERLAP_MAX:.0%}): move a seed or change a weight"
        )
    return result.territory, lines, {"world": world.id, "overlap": check}


def build(pack: Pack) -> None:
    global _TILE_PIECES, _TILE_LINES, _RES, _SAMPLE
    w, s, e, n = pack.extent
    if pack.is_world:
        bounds = (-180.0, -90.0, 180.0, 90.0)
    else:
        bounds = (max(-180, w - 1), max(-90, s - 1), min(180, e + 1), min(90, n + 1))

    pack.work.mkdir(parents=True, exist_ok=True)
    gpkg = pack.work / "territory.gpkg"
    cfg = pack.cfg.get("territory", {})
    if "world" in cfg:
        territory, lines, meta = _theatre(pack)
    elif "from_pack" in cfg:
        src = WORK / cfg["from_pack"]
        if not (src / "territory.gpkg").exists():
            raise FileNotFoundError(f"{src / 'territory.gpkg'} is missing; build the `{cfg['from_pack']}` pack first")
        territory = _read_layer(src / "territory.gpkg", "territory", "code")
        lines = _read_layer(src / "territory.gpkg", "borders", "kind")
        theirs = json.loads((src / "territory.json").read_text(encoding="utf-8"))
        meta = {"world": theirs["world"], "overlap": theirs["overlap"], "from_pack": cfg["from_pack"]}
    else:
        raise ValueError(f"pack `{pack.id}` has no `territory` source in packs.yaml")
    territory = {c: _polygonal(_clip(g, bounds)) for c, g in territory.items()}
    territory = {c: g for c, g in territory.items() if not g.is_empty}
    _write_gpkg(gpkg, territory, lines)
    codes = sorted(territory)
    index = {c: i + 1 for i, c in enumerate(codes)}
    log(f"territory: {len(codes)} nations: {' '.join(codes)}")
    _write_tiles(pack, gpkg)

    log("territory: splitting into 1° tiles")
    polys = [(index[c], g) for c, g in territory.items()]
    outlines = [(index[c], shapely.boundary(g)) for c, g in territory.items()]
    tile_pieces = _grid_fine(polys, bounds)
    tile_lines = _grid_fine(outlines, bounds)
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
    terrain.write(pack, codes, cells, owners, border_arr)
    (pack.work / "territory.json").write_text(json.dumps({"codes": codes} | meta, indent=1), encoding="utf-8")
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


def _write_gpkg(path, territory: dict[str, shapely.Geometry], lines: dict[str, shapely.Geometry]) -> None:
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
    layer = ds.CreateLayer("borders", srs, ogr.wkbMultiLineString, options=["SPATIAL_INDEX=NO"])
    layer.CreateField(ogr.FieldDefn("kind", ogr.OFTString))
    for kind, geom in lines.items():
        parts = [p for p in shapely.get_parts(geom) if p.geom_type == "LineString" and not p.is_empty]
        if not parts:
            continue
        f = ogr.Feature(layer.GetLayerDefn())
        f.SetField("kind", kind)
        f.SetGeometry(ogr.CreateGeometryFromWkb(shapely.to_wkb(shapely.MultiLineString(parts))))
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
        "borders",
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
