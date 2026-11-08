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
**Consequences.** + Realistic behaviour from public data, fast, explainable. − Precision limited by class-level data; calibration needed (golden scenarios).

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
**Decision.** Platforms are composed from components; composite systems (a long-range SAM battalion) list component platforms; custom systems use `extends` + overrides. YAML in git is the source of truth; every entry names its class archetype and a balance note (since ADR-036; originally sources and confidence).
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
**Decision.** Only free/open tools and data: Rust, React, MapLibre, deck.gl, CesiumJS (self-hosted), Protomaps/OSM, Copernicus DEM, ESA WorldCover (Natural Earth, Marine Regions, OurAirports and CelesTrak dropped from the shipped build by ADR-036); Unity Personal and Cesium for Unity later. A credits screen carries every required attribution.
**Consequences.** + ₹0 running cost. − Attribution and licence duties (ODbL, CC-BY); revisit if the project ever becomes commercial (real names, trademarks, Unity tier).

## ADR-019 Pack files read by positional I/O and an LRU cache, natively and in the browser
**Status — Accepted, 28 Oct 2026 (P2).** Refines doc 05 §4 and ADR-011; supersedes nothing.
**Context.** Doc 05 §4 says native builds memory-map the DEM. Memory maps need `unsafe` (`memmap2::Mmap::map` is an `unsafe fn`), and the workspace sets `unsafe_code = "forbid"`. The browser reads the same files through OPFS `FileSystemSyncAccessHandle`, which is a positional read API, not a mapping.
**Decision.** `wb-geo` reads every pack file through one trait, `PackSource { len, read_exact_at }`. Natively it is a `std::fs::File` with positional reads (`FileExt::read_exact_at` on Unix, `seek_read` on Windows); in WebAssembly `wb-wasm` implements it over a JS object that wraps the sync access handle (`fs.readSync` under Node). Decoded DEM tiles and H3 blocks sit in small LRU caches (ordered maps only). Results are pure functions of the bytes, so the cache never changes an outcome.
**Alternatives.** (a) `memmap2` with a narrow `unsafe` exception in `wb-geo`: one more code path than the browser, and the forbid lint becomes a deny with an allow. (b) Read whole files into memory: the europe DEM alone is ~90 MB.
**Consequences.** + One code path for native, browser and Node; no `unsafe`; the determinism check compares them directly. − A system call per cache miss natively (tiles are 30–120 KB, so misses are rare in practice); `Geo` is single-threaded (`RefCell` caches), so the engine holds it as a non-`Send` resource.

## ADR-020 Icon atlas rasterised at app start
**Status — Proposed, 28 Oct 2026 (P3).** Refines doc 06 §6 ("packed into a texture atlas at build time").
**Context.** The icons are SVG strings in `packages/symbols`. Rasterising them at build time needs an SVG renderer in Node (resvg, sharp, a headless browser): a new dependency with native or WASM binaries.
**Decision.** The app draws every icon (symbol × side colour × estimate variant) into one canvas at start-up, in a fixed order, and hands deck.gl the canvas as a PNG data URL with its mapping (`buildAtlas`). Panels use the same SVGs directly. The 3D view (P10) and Unity (P13) take the same SVG sources.
**Alternatives.** (a) Build-time rasterisation with a new dependency. (b) Hand-drawn PNG sprites in git.
**Consequences.** + No dependency, one source of truth, icons in any colour token. − Some start-up work; the atlas depends on the browser's SVG renderer (tiny anti-aliasing differences between browsers, never simulation-relevant).

## ADR-021 Head for the tanker before bingo
**Status — Proposed, 28 Oct 2026 (P3).** Refines doc 02 §6 ("On bingo … unless a tanker … is reachable").
**Context.** Bingo is measured to the nearest suitable friendly base. A patrol near any airfield (the patrol of G7 is about 30 km from an air base) reaches bingo with only the fuel for those 30 km, when a tanker 150 km away is no longer reachable, so the rule as written never uses the tanker there.
**Decision.** Every second, a group on task looks for a tanker on its track that its member lowest on fuel can reach and get home from. It heads for the nearest such tanker when that member's fuel falls to `(fuel to the tanker + fuel from the tanker to its nearest base) × (1 + reserve)`, or at bingo if one is in reach. Otherwise bingo sends it home as before.
**Alternatives.** (a) Bingo measured to the home base instead of the nearest base. (b) A separate content "joker fuel" per platform.
**Consequences.** + Tankers extend patrols wherever they fly (G7: 3.9× time on station). + No new constant. − The landing reserve stays 10 % of the fuel to the nearest base, which is small near a base (open question in the P3 report).

## ADR-022 Terrain in the tick
**Status — Proposed, 29 Oct 2026 (P4).** Refines ADR-019 and the P3 note that geo is read only to validate orders.
**Context.** Radar, ESM, IRST, EO/IR and visual detection need terrain line of sight every scan (doc 02 §7). CI has no region packs, and G3 must still run there.
**Decision.** The engine holds the installed packs as a non-`Send` world resource (`sensors::GeoRes`); line of sight and the land/sea test for clutter read them every tick. Without packs, line of sight is smooth Earth (the horizon only) and the surface under a target counts as sea. A scenario may give `test_terrain` (flat ground with straight ridges, built in memory by `wb_geo::synthetic`); it replaces the packs, is saved with the war and rebuilt on load. The packs are therefore a simulation input: two runs agree only with the same packs, which saves already record.
**Alternatives.** (a) Line of sight only when packs are installed, G3 ignored in CI. (b) Ship a small binary test pack in git.
**Consequences.** + Terrain masking in every war with packs; goldens run anywhere. − Results depend on which packs are installed; P9 war setup should require the theatre's packs before a war starts.

## ADR-023 Line-of-sight memo as mask altitude, and the radar gate
**Status — Proposed, 29 Oct 2026 (P4).** Refines doc 02 §16 ("LOS memoised by sensor cell, target cell, altitude bands").
**Decision.** The memo key is (sensor H3 res-8 cell, sensor height-above-ground band, target H3 res-8 cell). Bands are geometric: below 2 m, then each 1.1 × the last (realism `spatial`); the band's lower edge stands for the sensor. The value is the lowest target altitude that clears terrain plus 4/3-Earth bulge between the two cell centres, from one DEM pass: `h_min = max_t [h_s + (terrain(t) − h_s) / t]`. A target is in sight when its altitude ≥ `h_min`. Values are pure functions of the key, so the memo is never saved and clearing it (at `los_memo_capacity`) never changes a result. Before any of this, a radar skips a target beyond 3 × the R50 of the target's largest aspect (Pd < 1.5·10⁻⁴ per scan).
**Alternatives.** Exact line of sight per pair (G3 needs ~300 DEM samples per pair per scan); per-pair memo of the true positions (not a function of the key, so a save/load would change results).
**Consequences.** + One DEM pass answers every target altitude in a cell; deterministic. − Positions are approximated to ~460 m cells; G3's band allows for it (first detection at +4 m clearance, 8 s after the exact line clears).

## ADR-024 SGP4 in the engine
**Status — Proposed, 29 Oct 2026 (P4).**
**Context.** Satellite positions must be identical natively and in WebAssembly. Rust SGP4 crates use std floating-point functions, whose last bits differ between platforms.
**Decision.** `wb-core::orbit`: two-line element parsing and the near-Earth SGP4 of Vallado et al. (2006), WGS-72, on `wb_core::math` (libm), with unit tests against the published test vectors 00005 and 06251. Deep-space orbits (period ≥ 225 min) are refused: imaging and SIGINT satellites fly low. TEME → Earth-fixed through GMST (IAU-82), polar motion ignored; the sub-satellite point goes onto the engine's sphere.
**Alternatives.** The `sgp4` crate (MIT): less code, a new dependency, a determinism risk.
**Consequences.** + No dependency; native == WASM. − About 400 lines to maintain; no geostationary or Molniya orbits until SDP4 is added.

## ADR-025 Satellite collection: tasked imaging, routine SIGINT
**Status — Proposed, 29 Oct 2026 (P4).** Refines doc 02 §7.3.
**Decision.** Optical and SAR satellites image only tasked areas, on the first geometric pass found when the order is accepted (the acknowledgement names the pass and report times). Every pass gives a report, including an explained failure ("No image: sun −35° at the target"). SIGINT satellites report emitters in their footprint routinely. `swath_km` in content is the strip a satellite can image along its track (its field of regard), not the instantaneous swath at nadir.
**Alternatives.** Every pass images everything under the swath (near-global coverage for free, a flooded intel log).
**Consequences.** + Tasking is the player's decision, as in reality; G5 is explainable. − No routine imagery; P9's AI must task its satellites.

## ADR-026 Contact estimates never equal the truth
**Status — Proposed, 29 Oct 2026 (P4).** Refines ADR-003.
**Decision.** Every position a sensor reports is the truth plus normal errors (`rng(seed, ContactError | EsmBearing | Satellite, …)`), with a floor of `contact_sigma_floor_m` (25 m), so no estimate equals the truth. `ContactId`s are a per-side sequence in their own namespace: views carry them in the contacts section and string kind 2, the API answers them through `contact_details`, and events carry only contact data and own unit ids. Labels come from type and class names, never from unit names. A truth-canary test (`crates/wb-sim/tests/truth_canary.rs`) scans every view, details response and visible event for the hidden units' names, unit ids and exact coordinates.
**Consequences.** + No path from a client to the truth; "precise" contacts are still estimates. − Precise contacts jitter by metres between reports.

## ADR-027 Weapon and damage events go to one side
**Status — Accepted, 3 Nov 2026 (P5; written after the code).** Refines ADR-026.
**Decision.** `WeaponLaunched` and `WeaponTerminated` go to the shooter's side only, naming the target by that side's own contact or a point. Damage events go to the damaged side only. The other side learns of a weapon only through its sensors (a weapon contact) and of the damage it dealt only through its picture.
**Alternatives.** Broadcast combat events to both sides (leaks shooter identity and battle damage).
**Consequences.** + No truth reaches the other side through the event log; the truth canary covers it. − Battle damage assessment is the picture's job, not the log's.

## ADR-028 Flyout tables for launch decisions
**Status — Accepted, 3 Nov 2026 (P5; written after the code).** Refines doc 02 §10.
**Decision.** Each weapon gets a table, per second of flight, of the ground distance flown and `f_energy`, built once by flying the weapon through the real flight model (rockets, loft included) from its calibration launch conditions in a scratch world. Launch range against a target closing at `v` is `max_i(d_i + v·t_i)`; launch doctrine (max range, no-escape zone, close) reads it. Tables are a pure function of the content, built on first use and never saved.
**Alternatives.** A closed-form range formula (drifts from the flight model); flying a trial missile per decision (too slow).
**Consequences.** + Launch decisions agree with how the weapon actually flies; deterministic and save-free. − Tables assume calibration altitude and speed; shots from other conditions are approximate.

## ADR-029 Intercept and Strike orders are pre-approved
**Status — Accepted, 3 Nov 2026 (P5; written after the code).** Refines doc 03 §7.
**Decision.** A player's Intercept (chase a contact and engage it) or Strike (close on a contact or point and fire, optionally a given weapon and count) is itself the approval: the engagement of that target skips the ROE gate and is logged with reason `ordered`. Every other engagement goes through ROE.
**Alternatives.** Run ordered targets through ROE too (an order under Charlie would just raise a request).
**Consequences.** + An explicit order does what it says under any ROE. − A player can fire across a border under Bravo by ordering it; the event log says so.

## ADR-030 Test territory without packs
**Status — Accepted, 3 Nov 2026 (P5; written after the code).** Like `test_terrain` (P2).
**Decision.** Territory comes from the packs' `territory_owner` when packs are installed; otherwise from the scenario's `test_territory` polygons (country, `[lon, lat]` corners), saved with the war; otherwise nowhere is anyone's territory.
**Alternatives.** Require packs for ROE tests (goldens would need the multi-GB packs in CI).
**Consequences.** + G8 and ROE tests run in CI without packs. − Test polygons must stay out of real scenarios.

## ADR-031 Self-defence for ships and ground units only
**Status — Accepted, 3 Nov 2026 (P5; written after the code).** Refines doc 03 §7.
**Decision.** Self-defence (on by default, per unit toggle) lets ships and ground units engage, without approval, weapon contacts their side's picture shows heading for them: predicted closest approach inside `roe.threat_cpa_m` within `roe.threat_horizon_s`, earliest impact first, one interceptor per threat, again after `kill_assessment_s` if it survived. Aircraft defend by beaming and countermeasures in the endgame, not by shooting at missiles.
**Alternatives.** Let aircraft fire at incoming missiles too (no fielded air-to-air weapon does this routinely).
**Consequences.** + G9: a frigate under Charlie intercepts without approval and never fires first. − Self-defence uses the picture, so an undetected missile is not engaged.

## ADR-032 Endgame nominal speed `V_nom`
**Status — Accepted, 3 Nov 2026 (P5).** Defines `V_nom` in doc 02 §11.
**Decision.** For rockets and ballistic weapons `V_nom = V_min + nominal_speed_fraction × (V_max − V_min)` (realism `endgame.nominal_speed_fraction`, 0.25, an estimate). At `V_nom` or faster a weapon has full `f_energy` and its full `max_g` in `f_maneuver`; below it, `f_energy = √((V − V_min)/(V_nom − V_min))` and endgame g scales with `(V / V_nom)^g_speed_exponent`. The flight model's turn limit still scales against `V_max`. Air-breathing weapons keep `V_nom = V_max`.
**Alternatives.** `V_nom = V_max` (the first P5 build): with a coasting rocket's speed falling roughly linearly with distance, a half-range medium-range air-to-air missile reached the target at Mach 1.8 with `f_energy` 0.49 and 8 g, Pk 0.14, against G6's 0.6–0.85.
**Consequences.** + G6 holds: at half range head-on Pk 0.70; at max range against a beaming target every first shot runs out of energy. Energy only costs Pk in the last part of a flight. − Endgame g and flight turn limit differ for slow missiles.

## ADR-033 Bravo needs the error ellipse inside our territory
**Status — Accepted, 3 Nov 2026 (P5).** Refines doc 03 §7 (Bravo).
**Decision.** Under Bravo a contact counts as inside our territory only when its estimated position and eight points on its current error ellipse scaled by `roe.territory_sigma` (3, an estimate) are all inside. The ellipse is the one the player sees, so the decision uses only the side picture. Return fire is unchanged.
**Alternatives.** The estimated centre alone (the first P5 build): in one G8 run in 20 the estimate crossed a second before the aircraft did, and the fighters fired at a target 82 m outside the border.
**Consequences.** + "Never engaged outside" holds against the truth; the shooter waits until the picture is sure. − Engagements come later after a crossing (up to 22 s in G8, as an air contact's ellipse grows quickly between scans); a contact hugging the border inside it may never be engaged.

## ADR-034 Electronic warfare as game stats
**Status — Accepted, 5 Nov 2026 (before P6).** Supersedes the jamming formulas in ADR-005 and doc 02 §9; the rest of ADR-005 stands.
**Context.** The first design modelled jamming with signal-level formulas (decibels, sidelobe gain, ECCM in dB). Public sources give almost no real values for jammer power or radar jamming resistance, so every input would have been a guess (risk R1).
**Decision.** Jammers carry `jam_strength` 0–10 per mode with coverage and sector; radars carry `jam_resistance` 0–10; seekers `seeker_resistance` 0–1. A realism-profile table maps the advantage to a detection-range multiplier; fixed game rules cover burn-through, sector, seekers, data links, navigation and emitter-homing weapons (doc 02 §9). P6b migrates `eccm_db` and `seeker_eccm` in the schema and seed content.
**Alternatives.** Keep the formulas with guessed decibel values; drop electronic warfare.
**Consequences.** + Same gameplay effects; easier data entry and balancing; a plainer, game-level hand-off. − Less physically grounded; effect sizes are set by balance testing.

## ADR-035 Engine golden scenarios use generic test fixtures
**Status — Accepted, 5 Nov 2026 (before P6a).**
**Context.** Engine phases only need behaviour (detection, interception, jamming, damage), not real systems. Scenarios that stage strikes on named real units at real places add nothing to an engine test, and the P6 sessions in Claude Code were stopped by safety checks twice.
**Decision.** From P6a on, golden scenarios use generic fixtures in `scenarios/fixtures/` (test frigate, test corvette, test SAM site with radar, launchers and command post, test fighter, test jammer, test anti-ship missile, test interceptor, test emitter-homing missile) with stats in the range of their class, at a neutral open-sea test area with `test_territory`. Engine-phase sessions do not read `content/` beyond the schema. Real systems and the real map stay in content and packs; the P8 test bench checks them against the same bands. Existing G1–G9 stay as they are.
**Consequences.** + Engine tests are smaller, faster and independent of content edits; real-system data can change without breaking CI. − Fixtures must be kept in plausible ranges; real-system checks move to P8.

## ADR-036 Fictional nations and systems on real terrain
**Status — Accepted, 5 Nov 2026 (before P6a).** Supersedes the real-world scope in the README (countries, first theatre, OSINT content), the real-data parts of ADR-018 and the provenance rule of ADR-009. Names in earlier ADRs were replaced with class terms.
**Context.** Three Claude Code sessions for Phase 6 were stopped by safety checks, including a clean start with generic test units. The likely reason is the subject as a whole: a realistic simulation of a war between real, current powers with their real forces and bases. Rewording would only work around the check; changing the setting changes what is being built.
**Decision.** Keep real Earth terrain, land cover, physics and the real clock. Make nations, borders, territorial seas, bases, place names, weapon systems, satellites and inventories fictional (doc 11). Stats stay in the range of their class (archetype table, doc 04 §4) but are never copied from one real system. A denylist check keeps real names out of content, scenarios, code and docs. The first theatre is Theatre North on the `europe` terrain pack with eight nations.
**Alternatives.** A fully fictional world (needs a terrain generator); a historical 1980s setting (still real forces and places); generic weapons on today's map (still today's powers and bases).
**Consequences.** + The P0–P5 engine carries over unchanged in design; no OSINT research in P8; inventories never go stale; the setting is free to balance. − A migration phase (PF): content, packs, place names, base proposals and golden scenarios; the game no longer shows real countries or systems.


## ADR-037 Visual effects go only to the sides that may see them
**Status — Proposed, 5 Nov 2026 (P6a).** Refines doc 06 §3 (effects section) and rule 1 of CLAUDE.md.
**Context.** The first effects build kept one global list in `Combat`, so every side's view carried every launch flash at the exact position of the enemy shooter, which is truth the picture does not give.
**Decision.** Each effect carries a side mask. A launch flash goes to the shooter's side only. An intercept or a hit goes to the weapon's side and to the side of the unit or weapon at the other end. `ViewEncoder` filters by the viewer's side. Effects expire by age (`view::effect::RETENTION_MS`, 20 s of sim time) and at most 1,024 rows go into a view.
**Alternatives.** Send all effects and let the client filter by what it sees (puts truth in the buffer); send none for the enemy's launches (the player would not see a hit on their own ship from a weapon they have no contact for).
**Consequences.** + No truth in the effects section; a hit on our ship is always shown to us. − The enemy's launches are not flashed for us even when we hold a contact for the weapon; we see the weapon's own glyph instead. Saves from before the change do not decode (`SAVE_VERSION` unchanged, as in P5).

## ADR-038 Composite sites go autonomous when their command post is lost
**Status — Proposed, 5 Nov 2026 (P6a).** Implements doc 02 §12 item 7.
**Context.** Doc 02 §12 says that destroying an IADS node drops sites into autonomous mode but gives no timing or limits.
**Decision.** When the last member with the `c2` role of a composite site is destroyed, the site gets `autonomous_after_ms = now + reaction_time_s × tewa.iads_autonomous_reaction_factor` (the country's doctrine and the realism profile). Until then the site cannot engage. Afterwards it engages only on tracks of its own radars and ignores remote and networked tracks. An `AutonomousMode` event goes to its side.
**Alternatives.** Autonomous at once (no gap); keep using the network after losing C2.
**Consequences.** + The command post becomes a real target and G10 can pin the behaviour (`g10-sam-autonomous`: first launch at 62 s, none at the remote-only track). − A site whose own radar is out of action cannot defend until repaired; there is no re-link to another command post yet.

## ADR-039 A contact's JAMMED flag comes from the picture
**Status — Proposed, 7 Nov 2026 (P6b).** Refines doc 06 §3 (contact flags) and rule 1 of CLAUDE.md.
**Context.** The first P6b build set `cflag::JAMMED` by looking up the hidden unit behind each contact. That read truth, and it also never fired: a contact's `truth` is the group's id, and the group entity carries no platform, so the radar test was always false and no jammed radar was ever marked.
**Decision.** The flag is set when the side's picture holds the contact at `Class` or better, the contact's identified type carries a radar (`TypeRef::has_radar`, any component of a system), the contact has an estimated position, and that estimate lies inside the coverage of an active jammer of the same side. No hidden unit is read, and without an estimate there is no flag. `Class` already shows the platform category (`radar`, `frigate`), so it may also show whether that kind of platform carries a radar.
**Alternatives.** Require `Type` (nothing is marked until a contact is fully identified, so a radar seen only as `radar` never shows it); mark every contact inside coverage (marks vehicles and ships without radars); keep reading the hidden unit (truth in the view).
**Consequences.** + Rule 1 holds and `a_hostile_radar_inside_our_jammers_coverage_is_flagged_jammed` pins both flags. − At `Class` the flag tells the player one bit about the content type (whether that kind of platform has a radar) beyond its category name, and the content check does not require that a category implies a radar (a `fighter` platform without a radar would simply never show the flag).

## ADR-040 Host the app on GitHub Pages and the packs on Cloudflare R2
**Status — Proposed, 7 Nov 2026 (between P6b and PF).** Refines doc 06 §9 ("packs as release assets").
**Context.** The app is about 9 MB, but the region packs are 790 MB and `basemap-europe.pmtiles` alone is 602 MB. Cloudflare Pages allows 25 MiB per file, GitHub Pages 1 GB per site, Vercel Hobby 100 GB of traffic a month, and GitHub release assets carry no CORS headers, so the pack manager cannot fetch them. Until PF the default war (the P4 demo war) used real nations and forces, which must not go public (rule 6, ADR-036).
**Decision.** A manual GitHub Actions workflow (`pages.yml`) builds the game and publishes it to GitHub Pages. Packs live in a public R2 bucket (`r2.dev` URL; free tier 10 GB, no egress fees), uploaded with rclone; the bucket's CORS rule allows `GET`/`HEAD` from the Pages origin and `http://localhost:5173` only. Build-time settings choose the pack URL (`VITE_PACKS_URL`) and the war (`WB_SCENARIO`, `WB_CONTENT_PACK`, through Vite aliases so only the chosen scenario is bundled). The hosted build opens `scenarios/hosted-test.yaml` on the fixtures, and the workflow fails if the bundle holds a name from `content/denylist.txt` (changed in PF; it first looked for the real-world demo war by name).
**Alternatives.** Vercel or Render (built for servers; Vercel's traffic cap is about 125 full pack downloads a month and its builds lack Rust); Cloudflare Pages for both (25 MiB files); GitHub Releases for packs (no CORS); a Worker that proxies R2 under the app's origin (more moving parts).
**Consequences.** + Free, static, no server, and the app still works offline after the first visit. − `r2.dev` URLs are rate-limited and meant for development; wider use needs a custom domain. Packs must be uploaded again after PF regenerates them. The fixtures have no airbase, so the test war's aircraft have nowhere to return to when fuel runs low.

## ADR-041 PF content schema: design notes, nations, worlds, archetypes
**Status — Proposed, 8 Nov 2026 (PF).** Implements doc 04 §2–§4 and doc 11 §4 (content); refines the doc 04 examples where noted.
**Context.** PF replaces provenance (sources, confidence, estimates, `as_of`) with a design note and archetype bands, and countries with fictional nations. The doc 04 examples leave a few details open or clash with the loader.
**Decision.**
- Every entry carries `design: { archetype, note }` with a non-empty note. `archetype` is required on catalogue kinds (platform, system, sensor, weapon, EW suite, countermeasure, data link) and optional on nations, bases, inventories, doctrines and loadouts. Archetypes, worlds and the realism profile need no design block.
- Values outside an archetype's bands are **warnings** (`ContentSet::warnings`, printed by `wb content check`), not errors. A band whose field an entry lacks is skipped.
- Nation ids are **three lower-case letters** (`ald`), like every other id and like the territory codes in the packs; the UI shows them upper-case (`ALD`). Doc 04 writes `ALD`.
- The world's hand-edits are `border_overrides`, because `overrides` is already the meta key for `extends` (doc 04 §3.6).
- One inventory per nation, no `as_of`; bases lose `icao`.
- Base placement (doc 03 §3 says `preset`, doc 11 §4 says terrain rules): air and naval bases use `preset` (they must match a base in `content/bases/`); road strips, heliports and other player-placed sites use `terrain` rules.
- Until the starter catalogue lands, existing entries point at a placeholder archetype `legacy`, removed at the end of PF.
- Pack schema 5 (nations and design notes in the pack).
**Alternatives.** Upper-case nation ids (a second id rule and a case mapping to the pack codes); bands as errors (blocks deliberate outliers that the note explains).
**Consequences.** + The schema matches the fictional setting and the content check guides balance. − Schema-4 packs are refused; `GroupDetails` and `SatelliteDetails` show the archetype's class name instead of a data confidence.
