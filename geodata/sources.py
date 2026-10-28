"""Downloads of the vector and tabular sources into the cache (doc 05 §1)."""

from __future__ import annotations

import json
import re
import zipfile
from pathlib import Path

from .common import CACHE, download, load_sources, log


def natural_earth() -> Path:
    """Natural Earth admin-0 countries 1:10m, as a shapefile path."""
    url = load_sources()["natural_earth"]["url"]
    z = download(url, CACHE / "natural_earth" / "ne_10m_admin_0_countries.zip")
    folder = z.with_suffix("")
    shp = folder / "ne_10m_admin_0_countries.shp"
    if not shp.exists():
        with zipfile.ZipFile(z) as zf:
            zf.extractall(folder)
    return shp


MR_PAGE = 10


def marine_regions() -> list[Path]:
    """Marine Regions layers as GeoJSON pages of MR_PAGE features, sorted by `mrgid`.

    A whole layer in one response gets cut off by the server, so each layer is fetched
    in small pages; a page that does not parse is downloaded again.
    """
    src = load_sources()["marine_regions"]
    out: list[Path] = []
    for layer in src["layers"]:
        name = layer.split(":")[1]
        folder = CACHE / "marine_regions" / name
        base = f"{src['url']}?service=WFS&version=2.0.0&request=GetFeature&typeNames={layer}"
        hits_file = download(base + "&resultType=hits", folder / "hits.xml")
        total = int(re.search(r'numberMatched="(\d+)"', hits_file.read_text(encoding="utf-8")).group(1))
        for start in range(0, total, MR_PAGE):
            page = folder / f"page-{start:05d}.json"
            url = f"{base}&outputFormat=application/json&count={MR_PAGE}&startIndex={start}&sortBy=mrgid"
            for attempt in range(1, 9):
                download(url, page)
                try:
                    json.loads(page.read_text(encoding="utf-8"))
                    break
                except ValueError:
                    log(f"marine regions: {page} is truncated (attempt {attempt}), retrying")
                    page.unlink()
            else:
                raise RuntimeError(f"could not download {layer} page {start}")
            out.append(page)
    return out


def ourairports() -> tuple[Path, Path]:
    base = load_sources()["ourairports"]["url"]
    a = download(base + "airports.csv", CACHE / "ourairports" / "airports.csv")
    r = download(base + "runways.csv", CACHE / "ourairports" / "runways.csv")
    return a, r


def all_vector() -> None:
    log(str(natural_earth()))
    for p in marine_regions():
        log(str(p))
    log(str(ourairports()))
