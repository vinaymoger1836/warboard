"""Downloads of the vector sources into the cache (doc 05 §1). Natural Earth is only
read by the border overlap check (doc 11 §2.2)."""

from __future__ import annotations

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


def all_vector() -> None:
    log(str(natural_earth()))
