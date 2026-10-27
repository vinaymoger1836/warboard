# WarBoard — Architecture

A real-world strategy wargame for the browser, built to grow into an Android game. You pick a country or a coalition, deploy that country's real weapon systems on a real world map, plan multi-phase operations, and fight an AI opponent in real time. Fog of war, terrain, sensors, electronic warfare and weapon energy all decide who wins.

**Status: Design locked — 27 Oct 2026.** This folder is the hand-off to Claude Code. Numbers marked *estimate* are planning figures to replace with measurements during the phases.

## Locked scope

| Dimension | Decision |
|---|---|
| Platform now | Browser, fully offline after install (installable web app) |
| Platform later | Android: Capacitor wrapper first, then a Unity client (Blender models) reusing the same engine |
| Modes | Single player vs AI. PvP later (the engine is deterministic from day one) |
| Sides | Any countries grouped into two coalitions; player controls one, AI the other |
| Map | Whole real world: borders, coastlines, 12 nm territorial seas, terrain and land cover |
| Views | 2D strategic map **and** 3D tactical view, both in the browser |
| Realism | Engineering-level models (radar range vs RCS, horizon, terrain line of sight, jamming and burn-through, missile energy, probability of kill), not full flight physics |
| Scale target | Theatre war: ~2,000 units and ~300 weapons in flight, smooth at ×10 |
| Logistics | Fixed inventory. Fuel and munitions run out; no production |
| Starting map | Real bases preloaded; player picks "start garrisoned" or "deploy everything yourself" |
| Admin | Only the owner edits the database; content ships with game updates, format ready for mods |
| First theatre | **Russia & Europe.** Tier 1: Russia, Belarus, Ukraine, US (forces in Europe), UK, France, Germany, Poland, Italy, Sweden, Finland, Norway. Tier 2: Estonia, Latvia, Lithuania, Netherlands, Spain, Romania, Turkey, Greece |
| Content size (Tier 1) | ~200 system types, ~150 weapon types (*estimate*) |
| Budget | Personal project, free and open tools only |
| Out of scope (v1) | Nuclear, chemical, biological weapons; cyber; anti-satellite weapons; economy and production |

## Architecture in one paragraph

All game state lives in a **deterministic simulation engine written in Rust**. In the browser it runs as WebAssembly inside a Web Worker; on Android-Unity it will run as a native library. The engine owns the truth. The UI never sees it: every frame the engine hands the UI a binary **side view** with your own units plus the *contacts* your side has detected, each with an uncertainty area. The UI draws that view twice, with **MapLibre + deck.gl** for the 2D map and **CesiumJS** for the 3D view, and sends back **orders** (tasks, operations, ROE, approvals). Systems, weapons, sensors and country inventories are **data, not code**: YAML files in the repo, edited with a local admin web app, compiled into a content pack. Map data (borders, territorial seas, terrain, land cover, real bases) is built ahead of time into **region packs** stored on the device. Every detection, launch, intercept and hit is an **event**; after-action reports and replays are built from those events.

## Documents

| # | File | What's inside |
|---|---|---|
| 01 | [Overview](01-overview.md) | Requirements, NFRs, system context, components, tech stack, repo layout |
| 02 | [Simulation engine](02-simulation-engine.md) | Tick scheduler, determinism, movement, sensors, fog of war, EW, weapons, endgame, damage, logistics, performance |
| 03 | [Gameplay, operations & ROE](03-gameplay-operations-roe.md) | War setup, bases, deployment, tasks, loadouts, operations, ROE, time control, AI, after-action reports |
| 04 | [Data model & admin](04-data-model-and-admin.md) | Content schema with examples, provenance, inventories, admin app, content pipeline, saves, events |
| 05 | [Map & geodata](05-map-and-geodata.md) | Data sources and licences, geodata pipeline, region packs, H3 terrain, elevation, territory, map styling |
| 06 | [Clients & platforms](06-clients-and-platforms.md) | Browser app structure, worker bridge, view format, 2D/3D rendering, symbology, UI layout, storage, Android path |
| 07 | [Architecture decisions](07-adrs.md) | 18 ADRs with alternatives and consequences |
| 08 | [Roadmap, metrics & risks](08-roadmap-and-risks.md) | Phases 0–12 (+ Unity in Phase 13) with exit criteria, success metrics, golden scenarios, risk register |
| 09 | [Claude Code hand-off](09-claude-code-handoff.md) | How to run the build with Claude Code, per-phase prompts, acceptance tests, review checklists |
| 10 | [UI style guide](10-ui-style-guide.md) | Tokens, map style, unit symbols and labels, weapon visuals, unit card, chrome — derived from the reference screenshots in `ui-reference/` |
| — | [CLAUDE.md](CLAUDE.md) | Drop-in project memory file for the repo root |

## Headline targets

| Metric | Target |
|---|---|
| Engine step, 2,000 units, desktop | ≤ 6 ms average per 100 ms tick (smooth ×10) |
| 2D map | 60 fps with 2,000 icons and 300 missiles in flight |
| 3D view | ≥ 30 fps with 500 visible units on a mid-range laptop |
| Determinism | Same state hash, native vs WebAssembly, after 1 hour of sim time |
| Fog of war | No code path from UI to enemy truth (enforced by API and tests) |
| Offline | Full play with the network disabled after pack install |
| Install size | World pack ≈ 200 MB, Europe pack ≈ 0.5–0.7 GB (*estimate*, measured in Phase 2) |
| Save / load | < 2 s for a 2,000-unit war |