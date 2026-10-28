"""Paths, configuration, logging and cached downloads shared by every step."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

ROOT = Path(os.environ.get("WARBOARD_ROOT", "/repo"))
GEODATA = ROOT / "geodata"
BUILD = ROOT / "build"
CACHE = BUILD / "geodata" / "cache"
WORK = BUILD / "geodata" / "work"
PACKS = BUILD / "packs"


def log(msg: str) -> None:
    print(f"[geodata] {msg}", file=sys.stderr, flush=True)


def pool(processes: int | None = None):
    """A process pool that forks, so workers inherit the step's module globals.
    (Python 3.14 defaults to `forkserver` on Linux, which does not.)"""
    import multiprocessing

    return multiprocessing.get_context("fork").Pool(processes or jobs())


def jobs() -> int:
    value = os.environ.get("GEODATA_JOBS") or ""
    return int(value) if value.isdigit() and int(value) > 0 else (os.cpu_count() or 1)


@dataclass(frozen=True)
class Pack:
    id: str
    cfg: dict[str, Any]

    @property
    def extent(self) -> tuple[float, float, float, float]:
        w, s, e, n = self.cfg["extent"]
        return float(w), float(s), float(e), float(n)

    @property
    def is_world(self) -> bool:
        return self.extent == (-180.0, -90.0, 180.0, 90.0)

    @property
    def out(self) -> Path:
        return PACKS / self.id

    @property
    def work(self) -> Path:
        return WORK / self.id


def load_packs() -> dict[str, Pack]:
    data = yaml.safe_load((GEODATA / "packs.yaml").read_text(encoding="utf-8"))
    return {k: Pack(k, v) for k, v in sorted(data["packs"].items())}


def load_sources() -> dict[str, dict[str, Any]]:
    data = yaml.safe_load((GEODATA / "sources.lock.yaml").read_text(encoding="utf-8"))
    # PyYAML turns unquoted dates into date objects; keep them as ISO strings.
    return {k: {kk: (str(vv) if kk == "accessed" else vv) for kk, vv in v.items()} for k, v in data.items()}


def download(url: str, dest: Path, attempts: int = 6) -> Path:
    """Downloads `url` to `dest` once; later calls reuse the cached file."""
    if dest.exists() and dest.stat().st_size > 0:
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    for attempt in range(1, attempts + 1):
        try:
            log(f"download {url}")
            req = urllib.request.Request(url, headers={"User-Agent": "warboard-geodata/1"})
            with urllib.request.urlopen(req, timeout=300) as resp, open(tmp, "wb") as f:
                shutil.copyfileobj(resp, f, length=1 << 20)
            tmp.replace(dest)
            return dest
        except Exception as err:  # network errors: retry with back-off
            log(f"  attempt {attempt} failed: {err}")
            if attempt == attempts:
                raise
            time.sleep(min(60, 2**attempt))
    return dest


def run(cmd: list[str], **kwargs: Any) -> None:
    log("$ " + " ".join(cmd))
    subprocess.run(cmd, check=True, **kwargs)


def write_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(data)
    tmp.replace(path)
