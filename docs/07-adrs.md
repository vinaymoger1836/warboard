# 07 — Architecture decision records

All ADRs: **Status — Accepted, 27 Oct 2026.** Changing one needs a new ADR that supersedes it.

---

## ADR-001 Deterministic Rust engine shared by every client
**Context.** Browser now, Unity on Android later, PvP someday; the simulation is the hardest and most valuable code.
**Decision.** One engine crate (`wb-sim`) in Rust, compiled to WebAssembly for the browser, natively for the CLI and as a C-ABI library for Unity.
**Alternatives.** (a) C# core shared with Unity, run in the browser via .NET WebAssembly — heavier download and slower start. (b) TypeScript now, rewrite later — two engines to keep in sync. (c) Unity for everything with a WebGL build — large downloads, weak mobile-browser performance, and the 2D vector map experience is poorer.
**Consequences.** + One engine, fast, portable, testable headless. − Rust learning curve for review; FFI boundary to design.

## ADR-002 Engine in a Web Worker; UI gets binary side views
**Decision.** The engine runs in a dedicated worker. Each 100 ms of real time it transfers a structure-of-arrays `SideView`; the UI interpolates to 60 fps. Orders go the other way.
**Alternatives.** Engine on the main thread; JSON messages; SharedArrayBuffer.
**Consequences.** + Smooth UI at ×10; zero-copy; no cross-origin isolation headers. − A binary format to version and decode.

## ADR-003 Fog of war enforced by the engine API
**Decision.** The engine never exposes truth to a client. Views contain own units and contacts; contacts have their own IDs; orders target contact IDs.
**Alternatives.** Send everything and hide in the UI.
**Consequences.** + No leaks, fair AI, PvP-ready. − Every UI feature works from contacts; a debug "truth view" exists only in dev builds.

## ADR-004 Truth-based association with modelled imperfections
**Decision.** Sensor reports associate to contacts using ground truth internally, then degrade: groups appear as one contact with unknown count, decoys and jammers create their own contacts, errors and lag come from the sensor.
**Alternatives.** Full multi-target tracking (gating, MHT/JPDA) on noisy measurements.
**Consequences.** + Cheap, deterministic, believable. − No emergent ghost-track or mis-association effects beyond those modelled.

## ADR-005 Engineering-level models, not physics
**Decision.** Radar range scales with RCS^¼ around a published reference; horizon and DEM line of sight; jamming via burn-through and stand-off formulas; missiles with boost/sustain/coast energy and proportional navigation; endgame Pk from a base value and explained modifiers.
**Alternatives.** 6-DOF flight models, full radar equation with antenna patterns, or pure probability tables.
**Consequences.** + Realistic behaviour from public data, fast, explainable. − Precision limited by OSINT; calibration needed (golden scenarios).

## ADR-006 Determinism rules and counter-based RNG
**Decision.** Single-threaded schedule, `UnitId`-ordered iteration where order matters, no hash-map iteration, `libm` for transcendental maths, and random numbers drawn as `rng(seed, purpose, entity, tick, n)`.
**Alternatives.** A stateful RNG stream; trusting platform maths.
**Consequences.** + Identical results across browser, CLI and Unity; reproducible bugs; replays. − Discipline enforced by lints and CI hash checks.

## ADR-007 Adaptive fixed step with multi-rate systems
**Decision.** 100 ms tick while anything is engaging, 1 s otherwise; sensors, AI and logistics run at their own rates; scans staggered by unit.
**Alternatives.** Variable time step; one rate for everything.
**Consequences.** + ×10 at scale and cheap ×30/×60 transits, still deterministic. − Scheduler complexity; endgame needs closest-approach maths within a tick.

## ADR-008 Two renderers on one view
**Decision.** MapLibre GL JS v6 + deck.gl for the 2D strategic map; CesiumJS for the 3D tactical view, lazy-loaded. Both draw the same decoded `SideView` and share the icon atlas.
**Alternatives.** Cesium for both (softer 2D look, heavier); MapLibre globe/terrain only (weak for high-altitude objects, orbits and true 3D trajectories).
**Consequences.** + Crisp 2D like the reference screenshots and a real 3D globe. − Two integrations; parity tests needed.

## ADR-009 Component-based content in YAML with provenance
**Decision.** Platforms are composed from components; composite systems (S-400 battalion) list component platforms; custom systems use `extends` + overrides. YAML in git is the source of truth; every entry has sources, confidence and listed estimates.
**Alternatives.** A database (SQLite) as source of truth; hard-coded system classes.
**Consequences.** + New systems without code; diffable history; honest about uncertainty. − Schema grows; validation must be strong.

## ADR-010 Admin as a local web app without a server
**Decision.** `apps/admin` edits the repo's YAML through the File System Access API and validates with `wb-content` compiled to WASM; forms are generated from the JSON Schema.
**Alternatives.** Hosted admin with a backend and database.
**Consequences.** + Free, no infrastructure, same validation as the build. − Chromium-only (fine: one admin).

## ADR-011 Offline region packs in OPFS
**Decision.** A small world pack plus theatre packs (first: `europe`), downloaded once, hash-verified, stored in OPFS. The worker reads DEM tiles synchronously with `FileSystemSyncAccessHandle`.
**Alternatives.** Streaming tiles from the internet; one huge world pack.
**Consequences.** + Fully offline with a manageable install. − Download before the first war in a theatre; storage eviction handling.

## ADR-012 H3 grid for terrain and ground movement; DEM for line of sight
**Decision.** Terrain class, slope, coast and country per H3 cell (resolution 5 world, 7 theatre); DEM tiles for line of sight and 3D terrain.
**Alternatives.** Raster grids only; vector land-cover polygons.
**Consequences.** + Fast lookups, uniform neighbours for pathfinding, small packs. − Ground detail is cell-level.

## ADR-013 Operations as phase graphs shared by player and AI
**Decision.** An operation is a DAG of phases with triggers (start, time, after/with phase, event, manual), end and abort conditions. The AI builds the same objects.
**Alternatives.** Simple waypoint missions; separate AI plans.
**Consequences.** + Expressive planning, parallel phases, one implementation to test. − The planner UI is the largest UI piece.

## ADR-014 ROE Alpha/Bravo/Charlie with inheritance and approvals
**Decision.** Side → group → unit inheritance. Bravo uses coalition land + 12 nm waters + airspace and always allows return fire. Charlie allows self-defence by default and queues every other engagement for approval. Operation strike tasks pre-approve their designated targets.
**Consequences.** + Matches the brief, safe defaults, clear UI. − Approval queue can flood at scale (auto-pause and batching mitigate).

## ADR-015 AI inside the engine, same fog, utility + doctrine
**Decision.** Three AI layers (strategic, operational, tactical) reading only the AI side's picture; utility scoring weighted by per-country doctrine; templates for IADS, CAP and strike packages; difficulty by reaction time and coordination.
**Alternatives.** Scripted triggers; search-based planners; an LLM (needs network, not deterministic).
**Consequences.** + Fair, deterministic, offline. − Weaker creativity than a human; tuned through soak tests.

## ADR-016 Event log as the source of reports, replays and saves
**Decision.** Every meaningful change is an event with a visibility mask. Saves are snapshot + order log + events; replays re-simulate from checkpoints.
**Consequences.** + Detailed reports for free, explainable outcomes, bug repro. − Log size (compressed, pruned checkpoints).

## ADR-017 Android: Capacitor first, Unity later
**Decision.** Ship the web build in a Capacitor 8 wrapper first; build the Unity client later on `wb-ffi` with Cesium for Unity and Blender glTF models.
**Alternatives.** Go straight to Unity.
**Consequences.** + Android early for almost no work; Unity effort only when the game is proven. − Two Android generations; WebView performance limits.

## ADR-018 Free and open stack, attributions in-game
**Decision.** Only free/open tools and data: Rust, React, MapLibre, deck.gl, CesiumJS (self-hosted), Protomaps/OSM, Natural Earth, Copernicus DEM, ESA WorldCover, Marine Regions, OurAirports, CelesTrak; Unity Personal and Cesium for Unity later. A credits screen carries every required attribution.
**Consequences.** + ₹0 running cost. − Attribution and licence duties (ODbL, CC-BY); revisit if the project ever becomes commercial (real names, trademarks, Unity tier).

## ADR-019 Pack files read by positional I/O and an LRU cache, natively and in the browser
**Status — Proposed, 28 Oct 2026 (P2).** Refines doc 05 §4 and ADR-011; supersedes nothing.
**Context.** Doc 05 §4 says native builds memory-map the DEM. Memory maps need `unsafe` (`memmap2::Mmap::map` is an `unsafe fn`), and the workspace sets `unsafe_code = "forbid"`. The browser reads the same files through OPFS `FileSystemSyncAccessHandle`, which is a positional read API, not a mapping.
**Decision.** `wb-geo` reads every pack file through one trait, `PackSource { len, read_exact_at }`. Natively it is a `std::fs::File` with positional reads (`FileExt::read_exact_at` on Unix, `seek_read` on Windows); in WebAssembly `wb-wasm` implements it over a JS object that wraps the sync access handle (`fs.readSync` under Node). Decoded DEM tiles and H3 blocks sit in small LRU caches (ordered maps only). Results are pure functions of the bytes, so the cache never changes an outcome.
**Alternatives.** (a) `memmap2` with a narrow `unsafe` exception in `wb-geo`: one more code path than the browser, and the forbid lint becomes a deny with an allow. (b) Read whole files into memory: the europe DEM alone is ~90 MB.
**Consequences.** + One code path for native, browser and Node; no `unsafe`; the determinism check compares them directly. − A system call per cache miss natively (tiles are 30–120 KB, so misses are rare in practice); `Geo` is single-threaded (`RefCell` caches), so the engine holds it as a non-`Send` resource.
