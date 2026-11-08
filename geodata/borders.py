"""Generated borders (doc 11 §2.2, §2.3): nations grown from seed points over the H3
cells of a theatre pack, and 12 nm sea belts drawn around the result.

1. Window: every cell of the pack's H3 resolution inside the world's `extent`.
   Cells the terrain step did not store (open water) count as water. Water that
   reaches the window edge, or is larger than LAKE_MAX_CELLS, is sea; the rest are
   lakes.
2. Growth: one priority queue for all nations. A nation's key at a cell is its path
   cost divided by its weight; the cheapest key claims the cell, and a claimed cell
   only grows its owner, so every region stays connected. Each step costs its length
   times the mean terrain factor of its two cells (COST, SEA_COST), scaled by noise from
   the world `seed`: borders settle on mountains, lakes and narrow seas, and wander on
   open plains. Islands go to whoever reaches them first across the water. A nation
   stops growing at `reach_km`; land nobody reaches is unclaimed.
3. Clean-up: a majority filter smooths ragged edges, then pieces smaller than
   MIN_PIECE_CELLS that touch another owner's land join that owner. Hand-edits from
   `border_overrides` come last.
4. Belts: sea cells within 22.224 km of a coast take the owner of the nearest coast
   cell (unclaimed coasts too), which gives the equidistance line where belts meet.
5. Polygons: the cells of each nation as one polygon, snapped and simplified as a
   coverage so neighbours keep one shared edge.

Everything is deterministic for the same terrain cells and world file: ties in the
queues break on cell index, never on hash order.
"""

from __future__ import annotations

import heapq
import math
from collections import Counter
from dataclasses import dataclass

import h3.api.basic_int as h3
import numpy as np
import shapely
import yaml
from shapely.geometry import box, shape

from . import formats
from .common import ROOT, log

EARTH_R_M = 6_371_008.8
TERRITORIAL_SEA_M = 22_224.0  # 12 nm
# Terrain factor per step (1 = open flat land). Pipeline tuning, listed in the PF report.
COST = {
    "open": 1.0,
    "urban": 1.0,
    "desert": 1.0,
    "forest": 1.25,
    "jungle": 1.25,
    "arctic": 1.5,
    "marsh": 2.0,
    "mountain": 3.5,
    "water": 4.0,  # lakes and wide rivers
}
SEA_COST = 5.0
# Seeded noise on every cost (world `seed`): wavelengths in km and weights. Without it,
# borders on open plains come out as straight lines and reach limits as arcs.
NOISE_OCTAVES = ((320.0, 0.5), (110.0, 0.3), (40.0, 0.2))
NOISE_STRENGTH = 0.8  # factor range about e^±0.8 = 0.45..2.2
LAKE_MAX_CELLS = 12_000
MIN_PIECE_CELLS = 150
SMOOTH_PASSES = 2
# Coverage simplification tolerance, degrees (about three res-7 cells): removes the
# hexagon zigzag of cell outlines; the 12 nm limit moves by at most about 2 km.
SIMPLIFY_DEG = 0.035
SNAP_DEG = 1e-7
UNCLAIMED = -1


@dataclass
class World:
    id: str
    seed: int
    extent: tuple[float, float, float, float]
    reach_m: float
    codes: list[str]
    seeds: list[tuple[float, float]]  # (lon, lat)
    weights: list[float]
    overrides: list[tuple[int, list[int]]]  # (nation index or UNCLAIMED, cells)


def load_world(world_id: str) -> World:
    path = ROOT / "content" / "world" / f"{world_id}.yaml"
    data = yaml.safe_load(path.read_text(encoding="utf-8"))
    nations = data["nations"]
    codes = [n["nation"] for n in nations]
    overrides = []
    for o in data.get("border_overrides", []):
        idx = codes.index(o["nation"]) if o.get("nation") else UNCLAIMED
        overrides.append((idx, [int(c, 16) for c in o["cells"]]))
    return World(
        id=data["id"],
        seed=int(data["seed"]),
        extent=tuple(float(v) for v in data["extent"]),
        reach_m=float(data["reach_km"]) * 1000.0,
        codes=codes,
        seeds=[(float(n["seed_at"][0]), float(n["seed_at"][1])) for n in nations],
        weights=[float(n["weight"]) for n in nations],
        overrides=overrides,
    )


@dataclass
class Grid:
    res: int
    cells: np.ndarray  # sorted uint64
    lat: np.ndarray  # radians
    lon: np.ndarray
    nbr: np.ndarray  # (n, 6) int64 indexes, -1 outside the window
    cls: np.ndarray  # terrain class (formats.CLASS), water where the terrain has no cell
    land: np.ndarray  # bool: not water
    lake: np.ndarray  # bool: water that is not sea
    sea: np.ndarray  # bool

    def index(self, cells) -> np.ndarray:
        cells = np.asarray(cells, dtype=np.uint64)
        pos = np.minimum(np.searchsorted(self.cells, cells), len(self.cells) - 1)
        return np.where(self.cells[pos] == cells, pos, -1)


def haversine(lat1, lon1, lat2, lon2):
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = np.sin(dlat / 2) ** 2 + np.cos(lat1) * np.cos(lat2) * np.sin(dlon / 2) ** 2
    return 2 * EARTH_R_M * np.arcsin(np.sqrt(np.minimum(a, 1.0)))


def neighbours(cells: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Centre latitude and longitude (degrees) and the six neighbour indexes of sorted
    `cells` (-1 where a neighbour is not in the list or a pentagon has only five)."""
    lat = np.empty(len(cells))
    lon = np.empty(len(cells))
    ring = np.full((len(cells), 6), 0, dtype=np.uint64)
    short = np.zeros((len(cells), 6), dtype=bool)
    for i, c in enumerate(cells.tolist()):
        la, lo = h3.cell_to_latlng(c)
        lat[i], lon[i] = la, lo
        r = sorted(h3.grid_ring(c, 1))
        ring[i, : len(r)] = r
        short[i, len(r) :] = True
    grid_index = np.minimum(np.searchsorted(cells, ring.ravel()), len(cells) - 1)
    nbr = np.where(cells[grid_index] == ring.ravel(), grid_index, -1).reshape(ring.shape)
    nbr[short] = -1
    return lat, lon, nbr


def build_grid(res: int, extent, t_cells: np.ndarray, t_cls: np.ndarray) -> Grid:
    w, s, e, n = extent
    cells = np.array(sorted(h3.geo_to_cells(box(w, s, e, n), res)), dtype=np.uint64)
    log(f"borders: {len(cells):,} cells at res {res} in the window")
    lat, lon, nbr = neighbours(cells)

    pos = np.minimum(np.searchsorted(t_cells, cells), max(len(t_cells) - 1, 0))
    hit = t_cells[pos] == cells if len(t_cells) else np.zeros(len(cells), bool)
    cls = np.where(hit, t_cls[pos], formats.CLASS["water"]).astype(np.uint8)
    land = cls != formats.CLASS["water"]

    # Water components: sea if they reach the window edge or are very large.
    water = ~land
    comp = np.full(len(cells), -1, dtype=np.int64)
    is_sea = []
    for start in np.flatnonzero(water).tolist():
        if comp[start] >= 0:
            continue
        cid = len(is_sea)
        comp[start] = cid
        stack = [start]
        size = 0
        edge = False
        while stack:
            c = stack.pop()
            size += 1
            for nb in nbr[c].tolist():
                if nb < 0:
                    edge = True
                elif water[nb] and comp[nb] < 0:
                    comp[nb] = cid
                    stack.append(nb)
        is_sea.append(edge or size > LAKE_MAX_CELLS)
    is_sea_arr = np.array(is_sea + [False], dtype=bool)
    sea = water & is_sea_arr[comp]
    lake = water & ~sea
    log(f"borders: {int(land.sum()):,} land, {int(lake.sum()):,} lake, {int(sea.sum()):,} sea cells")
    return Grid(res, cells, np.radians(lat), np.radians(lon), nbr, cls, land, lake, sea)


def _hash01(ix: np.ndarray, iy: np.ndarray, octave: int, seed: int) -> np.ndarray:
    """Deterministic value in [0, 1) per lattice point (splitmix64 of the coordinates)."""
    with np.errstate(over="ignore"):
        x = (ix.astype(np.int64).astype(np.uint64) * np.uint64(0x9E3779B97F4A7C15)) ^ (
            iy.astype(np.int64).astype(np.uint64) * np.uint64(0xC2B2AE3D27D4EB4F)
        )
        x ^= np.uint64((seed * 1_000_003 + octave * 7919) & 0xFFFFFFFFFFFFFFFF)
        x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
        x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
        x ^= x >> np.uint64(31)
    return (x >> np.uint64(11)).astype(np.float64) / float(1 << 53)


def noise(grid: Grid, seed: int) -> np.ndarray:
    """Fractal value noise in [-1, 1] per cell from the world seed, on a km grid."""
    lat0 = float(np.mean(grid.lat))
    x = grid.lon * EARTH_R_M * math.cos(lat0) / 1000.0
    y = grid.lat * EARTH_R_M / 1000.0
    total = np.zeros(len(grid.cells))
    norm = 0.0
    for octave, (wavelength_km, amp) in enumerate(NOISE_OCTAVES):
        gx, gy = x / wavelength_km, y / wavelength_km
        ix, iy = np.floor(gx), np.floor(gy)
        fx, fy = gx - ix, gy - iy
        sx, sy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
        v00 = _hash01(ix, iy, octave, seed)
        v10 = _hash01(ix + 1, iy, octave, seed)
        v01 = _hash01(ix, iy + 1, octave, seed)
        v11 = _hash01(ix + 1, iy + 1, octave, seed)
        v = (v00 * (1 - sx) + v10 * sx) * (1 - sy) + (v01 * (1 - sx) + v11 * sx) * sy
        total += amp * (2 * v - 1)
        norm += amp
    return total / norm


def _factor(grid: Grid, seed: int) -> np.ndarray:
    names = {v: k for k, v in formats.CLASS.items()}
    lut = np.array([COST[names[v]] for v in range(len(names))])
    base = np.where(grid.sea, SEA_COST, lut[grid.cls])
    return base * np.exp(NOISE_STRENGTH * noise(grid, seed))


def grow(grid: Grid, world: World) -> np.ndarray:
    """Owner index of every cell (sea cells included as transit), UNCLAIMED beyond reach."""
    factor = _factor(grid, world.seed)
    owner = np.full(len(grid.cells), UNCLAIMED, dtype=np.int64)
    best = np.full(len(grid.cells), np.inf)
    heap: list[tuple[float, int, int, float]] = []
    for i, (lon, lat) in enumerate(world.seeds):
        c = h3.latlng_to_cell(lat, lon, grid.res)
        idx = int(grid.index([c])[0])
        if idx < 0 or not grid.land[idx]:
            raise ValueError(f"seed of `{world.codes[i]}` at {lon}, {lat} is not on land inside the window")
        best[idx] = 0.0
        heapq.heappush(heap, (0.0, idx, i, 0.0))
    nbr = grid.nbr
    lat, lon = grid.lat, grid.lon
    claimed = 0
    while heap:
        key, c, i, raw = heapq.heappop(heap)
        if owner[c] != UNCLAIMED:
            continue
        owner[c] = i
        claimed += 1
        w = world.weights[i]
        nb = nbr[c]
        nb = nb[nb >= 0]
        nb = nb[owner[nb] == UNCLAIMED]
        if len(nb) == 0:
            continue
        d = haversine(lat[c], lon[c], lat[nb], lon[nb]) * (factor[c] + factor[nb]) * 0.5
        for n_, step in zip(nb.tolist(), d.tolist()):
            r = raw + step
            k = r / w
            if k <= world.reach_m and k < best[n_]:
                best[n_] = k
                heapq.heappush(heap, (k, n_, i, r))
    log(f"borders: grew {claimed:,} cells")
    return owner


def _smooth(grid: Grid, owner: np.ndarray, landlike: np.ndarray) -> np.ndarray:
    """Majority filter on land: a cell joins the owner of 4 or more of its land neighbours."""
    for _ in range(SMOOTH_PASSES):
        nb_owner = np.where(grid.nbr >= 0, owner[np.maximum(grid.nbr, 0)], -2)
        nb_owner = np.where((grid.nbr >= 0) & landlike[np.maximum(grid.nbr, 0)], nb_owner, -2)
        counts = np.stack([(nb_owner == nb_owner[:, k : k + 1]).sum(axis=1) for k in range(6)], axis=1)
        counts = np.where(nb_owner == -2, 0, counts)
        k = np.argmax(counts, axis=1)
        top = nb_owner[np.arange(len(owner)), k]
        top_n = counts[np.arange(len(owner)), k]
        switch = landlike & (top_n >= 4) & (top != owner) & (top != -2)
        owner = np.where(switch, top, owner)
    return owner


def _pieces(grid: Grid, owner: np.ndarray, landlike: np.ndarray) -> np.ndarray:
    """Pieces below MIN_PIECE_CELLS that touch another owner's land join the most common one."""
    for _ in range(4):
        comp = np.full(len(owner), -1, dtype=np.int64)
        changed = 0
        for start in np.flatnonzero(landlike).tolist():
            if comp[start] >= 0:
                continue
            comp[start] = start
            members = [start]
            stack = [start]
            others: Counter = Counter()
            while stack:
                c = stack.pop()
                for nb in grid.nbr[c].tolist():
                    if nb < 0 or not landlike[nb]:
                        continue
                    if owner[nb] == owner[start]:
                        if comp[nb] < 0:
                            comp[nb] = start
                            members.append(nb)
                            stack.append(nb)
                    else:
                        others[int(owner[nb])] += 1
            if len(members) < MIN_PIECE_CELLS and others:
                target = min(others.items(), key=lambda kv: (-kv[1], kv[0]))[0]
                owner[np.array(members)] = target
                changed += 1
        if changed == 0:
            break
        log(f"borders: merged {changed} small pieces")
    return owner


def belts(grid: Grid, land_owner: np.ndarray, landlike: np.ndarray) -> np.ndarray:
    """Owner of sea cells within 12 nm of a coast: the owner of the nearest coast cell."""
    half = 0.5 * float(np.median(haversine(grid.lat[:-1], grid.lon[:-1], grid.lat[grid.nbr[:-1, 0]], grid.lon[grid.nbr[:-1, 0]])))
    sea = grid.sea
    out = np.full(len(grid.cells), UNCLAIMED, dtype=np.int64)
    site = np.full(len(grid.cells), -1, dtype=np.int64)
    best = np.full(len(grid.cells), np.inf)
    heap: list[tuple[float, int, int]] = []
    coast = np.flatnonzero(landlike & np.any((grid.nbr >= 0) & sea[np.maximum(grid.nbr, 0)], axis=1))
    lat, lon = grid.lat, grid.lon
    for s in coast.tolist():
        nb = grid.nbr[s]
        nb = nb[(nb >= 0)]
        nb = nb[sea[nb]]
        d = haversine(lat[s], lon[s], lat[nb], lon[nb]) - half
        for n_, dd in zip(nb.tolist(), d.tolist()):
            if dd < best[n_] or (dd == best[n_] and s < site[n_]):
                best[n_] = dd
                site[n_] = s
                heapq.heappush(heap, (dd, n_, s))
    done = np.zeros(len(grid.cells), bool)
    while heap:
        d, c, s = heapq.heappop(heap)
        if done[c] or site[c] != s:
            continue
        done[c] = True
        if d > TERRITORIAL_SEA_M:
            continue
        out[c] = land_owner[s]
        nb = grid.nbr[c]
        nb = nb[nb >= 0]
        nb = nb[sea[nb] & ~done[nb]]
        if len(nb) == 0:
            continue
        dd = haversine(lat[s], lon[s], lat[nb], lon[nb]) - half
        for n_, x in zip(nb.tolist(), dd.tolist()):
            if x <= TERRITORIAL_SEA_M + half and (x < best[n_] or (x == best[n_] and s < site[n_])):
                best[n_] = x
                site[n_] = s
                heapq.heappush(heap, (x, n_, s))
    return out


@dataclass
class Result:
    world: World
    grid: Grid
    owner: np.ndarray  # final owner per cell: land, lakes and belts; UNCLAIMED elsewhere
    landlike: np.ndarray
    territory: dict[str, shapely.Geometry]
    land_mask: shapely.Geometry


def _cells_shape(cells: np.ndarray) -> shapely.Geometry:
    if len(cells) == 0:
        return shapely.MultiPolygon()
    geo = h3.cells_to_h3shape([int(c) for c in cells.tolist()])
    return shapely.make_valid(shape(geo.__geo_interface__))


def generate(world: World, res: int, t_cells: np.ndarray, t_cls: np.ndarray) -> Result:
    grid = build_grid(res, world.extent, t_cells, t_cls)
    owner = grow(grid, world)
    landlike = grid.land | grid.lake
    land_owner = np.where(landlike, owner, UNCLAIMED)
    land_owner = _smooth(grid, land_owner, landlike)
    land_owner = _pieces(grid, land_owner, landlike)
    for idx, cells in world.overrides:
        pos = grid.index(cells)
        land_owner[pos[pos >= 0]] = idx
    # Lakes in unclaimed land stay unclaimed; lakes inside a nation are its waters.
    sea_owner = belts(grid, land_owner, landlike)
    final = np.where(landlike, land_owner, sea_owner)

    log("borders: polygons")
    raw = []
    for i, code in enumerate(world.codes):
        g = _cells_shape(grid.cells[final == i])
        raw.append(shapely.set_precision(g, SNAP_DEG))
        log(f"  {code}: {int((final == i).sum()):,} cells ({int(((final == i) & landlike).sum()):,} land)")
    simplified = shapely.coverage_simplify(np.array(raw, dtype=object), SIMPLIFY_DEG, simplify_boundary=True)
    territory = {}
    for code, g in zip(world.codes, simplified.tolist()):
        g = shapely.make_valid(g)
        polys = [p for p in shapely.get_parts(g) if p.geom_type == "Polygon" and p.area > 0]
        territory[code] = shapely.MultiPolygon(polys)
    land_mask = shapely.set_precision(_cells_shape(grid.cells[landlike]), SNAP_DEG)
    return Result(world, grid, final, landlike, territory, land_mask)


def border_lines(territory: dict[str, shapely.Geometry], land_mask: shapely.Geometry) -> dict[str, shapely.Geometry]:
    """Every nation outline, noded once: `land` where it crosses land, `sea` elsewhere."""
    lines = shapely.union_all([shapely.boundary(g) for g in territory.values() if not g.is_empty])
    near_land = shapely.buffer(land_mask, -0.004, quad_segs=2)
    shapely.prepare(near_land)
    land = shapely.line_merge(shapely.intersection(lines, near_land))
    sea = shapely.line_merge(shapely.difference(lines, near_land))
    return {"land": land, "sea": sea}


@dataclass
class Theatre:
    """A built theatre for `geodata places` and `geodata bases`: the cells of the window
    with their owner and the terrain step's fields."""

    world: World
    codes: list[str]
    cells: np.ndarray
    lat: np.ndarray  # degrees
    lon: np.ndarray
    nbr: np.ndarray
    owner: np.ndarray  # nation index, UNCLAIMED
    landlike: np.ndarray
    sea: np.ndarray
    cls: np.ndarray
    elev: np.ndarray  # m, 0 where the terrain has no cell
    slope: np.ndarray  # degrees
    flags: np.ndarray


def load_theatre(pack_id: str = "europe") -> Theatre:
    from .common import WORK

    d = np.load(WORK / pack_id / "nations.npz")
    t = np.load(WORK / pack_id / "terrain_cells.npz")
    cells = d["cells"]
    lat, lon, nbr = neighbours(cells)
    pos = np.minimum(np.searchsorted(t["cells"], cells), len(t["cells"]) - 1)
    hit = t["cells"][pos] == cells
    world_id = __import__("json").loads((WORK / pack_id / "territory.json").read_text(encoding="utf-8"))["world"]
    return Theatre(
        world=load_world(world_id),
        codes=[str(c) for c in d["codes"]],
        cells=cells,
        lat=lat,
        lon=lon,
        nbr=nbr,
        owner=d["owner"].astype(np.int64),
        landlike=d["landlike"],
        sea=d["sea"],
        cls=d["cls"],
        elev=np.where(hit, t["elev"][pos], 0).astype(np.float64),
        slope=np.where(hit, t["slope"][pos], 0).astype(np.float64) / 2.0,
        flags=np.where(hit, t["flags"][pos], 0).astype(np.uint8),
    )


def hops_from(nbr: np.ndarray, sources: np.ndarray, passable: np.ndarray) -> np.ndarray:
    """Steps from the nearest source cell through passable cells (large where unreachable)."""
    from collections import deque

    far = np.iinfo(np.int32).max
    out = np.full(len(nbr), far, dtype=np.int64)
    q = deque()
    for s in np.flatnonzero(sources).tolist():
        out[s] = 0
        q.append(s)
    while q:
        c = q.popleft()
        d = out[c] + 1
        for nb in nbr[c].tolist():
            if nb >= 0 and passable[nb] and out[nb] > d:
                out[nb] = d
                q.append(nb)
    return out


def components(nbr: np.ndarray, member: np.ndarray) -> np.ndarray:
    """Connected component id per member cell (the lowest index in it); -1 elsewhere."""
    comp = np.full(len(nbr), -1, dtype=np.int64)
    for start in np.flatnonzero(member).tolist():
        if comp[start] >= 0:
            continue
        comp[start] = start
        stack = [start]
        while stack:
            c = stack.pop()
            for nb in nbr[c].tolist():
                if nb >= 0 and member[nb] and comp[nb] < 0:
                    comp[nb] = start
                    stack.append(nb)
    return comp


class Namer:
    """Deterministic fictional names per nation from geodata/names.yaml."""

    def __init__(self, seed: int):
        from .common import GEODATA

        self.seed = seed
        self.parts = yaml.safe_load((GEODATA / "names.yaml").read_text(encoding="utf-8"))["nations"]
        self.used: set[str] = set()

    def _pick(self, items: list[str], *key) -> str:
        h = 1469598103934665603
        for k in (self.seed, *key):
            for b in str(k).encode("utf-8"):
                h = ((h ^ b) * 1099511628211) & 0xFFFFFFFFFFFFFFFF
            h = ((h ^ 0xFF) * 1099511628211) & 0xFFFFFFFFFFFFFFFF
        return items[h % len(items)]

    def name(self, nation: str, *key) -> str:
        p = self.parts[nation]
        for attempt in range(64):
            k = (nation, *key, attempt)
            stem = self._pick(p["onsets"], *k, "o") + self._pick(p["vowels"], *k, "v") + self._pick(p["codas"], *k, "c")
            if self._pick([0, 1, 2, 3], *k, "two") == 0:
                stem += self._pick(p["vowels"], *k, "v2") + self._pick(p["codas"], *k, "c2")
            ending = self._pick(p["endings"], *k, "e")
            word = stem + ending if not stem.endswith(ending[0]) else stem + ending[1:]
            word = word[0].upper() + word[1:]
            if word not in self.used and 4 <= len(word) <= 10 and self._sounds_right(word):
                self.used.add(word)
                return word
        raise RuntimeError(f"no free name for {nation} {key}")

    @staticmethod
    def _sounds_right(word: str) -> bool:
        """No three consonants or three equal letters in a row, no doubled vowel at the start."""
        import re

        w = word.lower()
        vowels = "aeiouyäöøåé"
        if w[0] in vowels and w[1] in vowels:
            return False
        if re.search(r"(.)", w):
            return False
        run = 0
        for ch in w:
            run = 0 if ch in vowels else run + 1
            if run >= 3 and not w.startswith(("sch", "str", "skr", "spr")):
                return False
        return True
