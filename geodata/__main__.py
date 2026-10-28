"""Entry point: `python -m geodata build <pack> [--step …]`, `python -m geodata bases`."""

from __future__ import annotations

import argparse
import sys

from .common import load_packs, log

STEPS = ["basemap", "territory", "dem", "terrain", "manifest"]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="geodata")
    sub = parser.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="build a region pack into build/packs/<pack>/")
    b.add_argument("pack")
    b.add_argument("--step", choices=STEPS, action="append", help="run only these steps (repeatable)")
    sub.add_parser("bases", help="import OurAirports + curated ports into content/bases/")
    sub.add_parser("catalog", help="write build/packs/index.json from the built manifests")
    args = parser.parse_args(argv)

    if args.cmd == "bases":
        from . import bases

        bases.build()
        return 0
    if args.cmd == "catalog":
        from . import manifest

        manifest.write_catalog()
        return 0

    packs = load_packs()
    if args.pack not in packs:
        parser.error(f"unknown pack `{args.pack}` (known: {', '.join(packs)})")
    pack = packs[args.pack]
    for step in args.step or STEPS:
        log(f"== {pack.id}: {step}")
        module = __import__(f"geodata.{step}", fromlist=["build"])
        module.build(pack)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
