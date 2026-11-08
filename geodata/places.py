"""Place proposal (doc 11 §2.4): nation labels, capitals and cities for the map's
fictional label layer, written into content/world/places.yaml.

- Nation label: the point deepest inside the nation's largest piece of land.
- Capital and cities: the largest urban land-cover areas of the nation (terrain class
  `urban`), largest first, at least CITY_SPACING_KM apart. The capital is the largest
  within CAPITAL_RADIUS_KM of the nation's seed (its heartland), else the largest.
- Names: geodata/names.yaml, deterministic from the world seed.

Entries written here carry `generated: true`; a re-run replaces those and keeps every
other entry (the authored seas, gulfs, straits and islands, and any edited name whose
flag was removed). Needs a built theatre pack (work/<pack>/nations.npz).
"""

from __future__ import annotations

import math

import numpy as np
import shapely
from shapely.ops import polylabel
import yaml

from . import formats
from .borders import Namer, _cells_shape, components, load_theatre
from .common import ROOT, log

CITY_SPACING_KM = 90.0
CAPITAL_RADIUS_KM = 300.0
LAND_CELLS_PER_CITY = 25_000
MAX_CITIES = 5
HEADER = """# Map labels of Theatre North (doc 11 §2.4), drawn by the map's label layer.
# Seas, gulfs, straits and islands are authored here. `geodata places` proposes the
# nation labels, capitals and cities (marked `generated: true`) and replaces only those
# on a re-run; remove the flag from an entry to keep an edit. `at` is [lon, lat].
"""


def _dist_km(lat1, lon1, lat2, lon2) -> float:
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dl = math.radians(lon2 - lon1)
    a = math.sin((p2 - p1) / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * 6371.0088 * math.asin(math.sqrt(min(a, 1.0)))


def _fmt(p: dict) -> str:
    parts = [f"name: {p['name']}", f"kind: {p['kind']}"]
    if p.get("nation"):
        parts.append(f"nation: {p['nation']}")
    parts.append(f"at: [{p['at'][0]:.3f}, {p['at'][1]:.3f}]")
    if p.get("generated"):
        parts.append("generated: true")
    return "  - { " + ", ".join(parts) + " }"


def proposals(th, namer: Namer, names: dict[str, str]) -> list[dict]:
    out: list[dict] = []
    urban_all = th.landlike & (th.cls == formats.CLASS["urban"])
    for i, code in enumerate(th.codes):
        land = th.landlike & (th.owner == i)
        if not land.any():
            continue
        comp = components(th.nbr, land)
        ids, sizes = np.unique(comp[comp >= 0], return_counts=True)
        main = ids[np.lexsort((ids, -sizes))[0]]
        poly = _cells_shape(th.cells[comp == main])
        poly = max(shapely.get_parts(poly), key=lambda g: g.area)
        label = polylabel(poly, tolerance=0.02)
        out.append({"name": names[code], "kind": "nation", "nation": code, "at": [label.x, label.y], "generated": True})

        urban = urban_all & (th.owner == i)
        if not urban.any():
            urban = land & ((th.flags & formats.FLAG_URBAN) > 0)
        ucomp = components(th.nbr, urban)
        uids, usizes = np.unique(ucomp[ucomp >= 0], return_counts=True)
        want = 1 + min(MAX_CITIES, int(land.sum()) // LAND_CELLS_PER_CITY + 1)
        seed_lon, seed_lat = th.world.seeds[i]
        areas = []
        for u in uids[np.lexsort((uids, -usizes))].tolist():
            members = np.flatnonzero(ucomp == u)
            la, lo = float(th.lat[members].mean()), float(th.lon[members].mean())
            best = members[np.argmin((th.lat[members] - la) ** 2 + (th.lon[members] - lo) ** 2)]
            areas.append((float(th.lat[best]), float(th.lon[best])))
        # The capital is the largest urban area in the nation's heartland, near its seed.
        near = [a for a in areas if _dist_km(a[0], a[1], seed_lat, seed_lon) <= CAPITAL_RADIUS_KM]
        if near:
            areas.remove(near[0])
            areas.insert(0, near[0])
        chosen: list[tuple[float, float]] = []
        for la, lo in areas:
            if any(_dist_km(la, lo, a, b) < CITY_SPACING_KM for a, b in chosen):
                continue
            kind = "capital" if not chosen else "city"
            out.append({"name": namer.name(code, "city", len(chosen)), "kind": kind, "nation": code, "at": [lo, la], "generated": True})
            chosen.append((la, lo))
            if len(chosen) >= want:
                break
        log(f"places: {code}: {len(chosen)} cities from {len(uids)} urban areas")
    return out


def build(pack_id: str = "europe") -> None:
    th = load_theatre(pack_id)
    nations = {}
    for code in th.codes:
        data = yaml.safe_load((ROOT / "content" / "nations" / f"{code}.yaml").read_text(encoding="utf-8"))
        nations[code] = data["name"]
    path = ROOT / "content" / "world" / "places.yaml"
    if path.exists():
        doc = yaml.safe_load(path.read_text(encoding="utf-8"))
    else:
        doc = {"id": f"{th.world.id}-places", "world": th.world.id, "places": []}
    kept = [p for p in doc.get("places", []) if not p.get("generated")]
    namer = Namer(th.world.seed)
    namer.used |= {p["name"] for p in kept}
    generated = proposals(th, namer, nations)
    lines = [HEADER.rstrip("\n"), f"id: {doc['id']}", "kind: places", f"world: {doc['world']}", "places:"]
    lines += [_fmt(p) for p in kept]
    lines += [_fmt(p) for p in generated]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    log(f"places: wrote {path} ({len(kept)} kept, {len(generated)} generated)")
