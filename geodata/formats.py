"""Writers for the engine's pack files. Layouts are specified in doc 05 §2.2 and read by
crates/wb-geo; change both together and bump the version field.

All integers are little-endian. zstd frames use a fixed level and no checksum or
content-size quirks that depend on the machine, so the same input gives the same bytes.
"""

from __future__ import annotations

import struct
from pathlib import Path

import numpy as np
import zstandard

ZSTD_LEVEL = 19
HEADER_LEN = 32
BLOCK_ENTRY = struct.Struct("<QQII")  # first_cell, offset, len, count
DEM_ENTRY = struct.Struct("<QIhh")  # offset, len, min_m, max_m

# Terrain classes (doc 05 §3). The engine reads the same numbers.
CLASS = {
    "water": 0,
    "open": 1,
    "forest": 2,
    "jungle": 3,
    "urban": 4,
    "desert": 5,
    "mountain": 6,
    "marsh": 7,
    "arctic": 8,
}
FLAG_COAST = 1
FLAG_BORDER = 2
FLAG_URBAN = 4


def _zstd(data: bytes) -> bytes:
    return zstandard.ZstdCompressor(level=ZSTD_LEVEL, write_checksum=False, write_content_size=True).compress(data)


def country_table(codes: list[str]) -> bytes:
    """Index 0 is "no country"; codes are lower-case, at most 4 ASCII bytes."""
    out = bytearray(4)
    for c in codes:
        b = c.encode("ascii")
        assert 0 < len(b) <= 4, c
        out += b.ljust(4, b"\0")
    return bytes(out)


def write_dem(path: Path, res_arcsec: int, lon0: int, lat0: int, cols: int, rows: int, tiles: dict[tuple[int, int], np.ndarray]) -> None:
    """`tiles[(col, row)]` is an (n+1)×(n+1) int16 grid, row 0 at the south edge."""
    samples = 3600 // res_arcsec + 1
    header = struct.pack("<4sHHHHhhHH", b"WBDM", 1, res_arcsec, samples, 0, lon0, lat0, cols, rows).ljust(HEADER_LEN, b"\0")
    index = bytearray()
    payload = bytearray()
    base = HEADER_LEN + rows * cols * DEM_ENTRY.size
    for row in range(rows):
        for col in range(cols):
            grid = tiles.get((col, row))
            if grid is None:
                index += DEM_ENTRY.pack(0, 0, 0, 0)
                continue
            assert grid.shape == (samples, samples) and grid.dtype == np.int16
            blob = _zstd(grid.astype("<i2").tobytes())
            index += DEM_ENTRY.pack(base + len(payload), len(blob), int(grid.min()), int(grid.max()))
            payload += blob
    _write(path, header + bytes(index) + bytes(payload))


def _blocks(cells: np.ndarray, block_size: int, encode) -> tuple[bytes, bytes, int]:
    index = bytearray()
    payload = bytearray()
    n_blocks = (len(cells) + block_size - 1) // block_size
    for b in range(n_blocks):
        lo, hi = b * block_size, min(len(cells), (b + 1) * block_size)
        blob = _zstd(encode(lo, hi))
        index += BLOCK_ENTRY.pack(int(cells[lo]), len(payload), len(blob), hi - lo)
        payload += blob
    return bytes(index), bytes(payload), n_blocks


def _blocked_file(magic: bytes, res: int, cells: np.ndarray, block_size: int, codes: list[str], encode) -> bytes:
    table = country_table(codes)
    index, payload, n_blocks = _blocks(cells, block_size, encode)
    data_start = HEADER_LEN + len(table) + len(index)
    # Offsets in the index are relative to the payload; make them absolute.
    fixed = bytearray()
    for i in range(n_blocks):
        first, off, ln, cnt = BLOCK_ENTRY.unpack_from(index, i * BLOCK_ENTRY.size)
        fixed += BLOCK_ENTRY.pack(first, data_start + off, ln, cnt)
    header = struct.pack("<4sHBBQIIH", magic, 1, res, 0, len(cells), n_blocks, block_size, len(codes) + 1).ljust(HEADER_LEN, b"\0")
    return header + table + bytes(fixed) + payload


def write_h3(path: Path, res: int, codes: list[str], cells: np.ndarray, cls: np.ndarray, elev: np.ndarray, slope: np.ndarray, flags: np.ndarray, country: np.ndarray) -> None:
    order = np.argsort(cells, kind="stable")
    cells, cls, elev, slope, flags, country = (a[order] for a in (cells, cls, elev, slope, flags, country))
    assert np.all(np.diff(cells.astype(np.uint64)) > 0), "duplicate cells"

    def enc(lo: int, hi: int) -> bytes:
        return b"".join(
            [
                cells[lo:hi].astype("<u8").tobytes(),
                cls[lo:hi].astype("u1").tobytes(),
                elev[lo:hi].astype("<i2").tobytes(),
                slope[lo:hi].astype("u1").tobytes(),
                flags[lo:hi].astype("u1").tobytes(),
                country[lo:hi].astype("<u2").tobytes(),
            ]
        )

    _write(path, _blocked_file(b"WBH3", res, cells, 4096, codes, enc))


def encode_rings_record(pieces: list[tuple[int, list[np.ndarray]]], shifted: bool) -> bytes:
    """One border cell: `pieces` = [(country index, [ring (k×2 int32 lon/lat ×1e7)])]."""
    out = bytearray(struct.pack("<HH", len(pieces), 1 if shifted else 0))
    for country, rings in pieces:
        out += struct.pack("<HH", country, len(rings))
        for ring in rings:
            out += struct.pack("<I", len(ring))
            out += ring.astype("<i4").tobytes()
    return bytes(out)


def write_territory(path: Path, res: int, codes: list[str], cells: np.ndarray, records: list[bytes]) -> None:
    order = np.argsort(cells, kind="stable")
    cells = cells[order]
    records = [records[i] for i in order]

    def enc(lo: int, hi: int) -> bytes:
        offs = bytearray()
        body = bytearray()
        for r in records[lo:hi]:
            offs += struct.pack("<I", len(body))
            body += r
        return cells[lo:hi].astype("<u8").tobytes() + bytes(offs) + bytes(body)

    _write(path, _blocked_file(b"WBTR", res, cells, 256, codes, enc))


def _write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(data)
    tmp.replace(path)
