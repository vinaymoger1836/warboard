# 09 — Claude Code hand-off

How to turn this design into code with Claude Code, one phase at a time. Each phase below has a copy-paste prompt, its acceptance tests, and a short review checklist for you.

## 1. One-time setup

**Install** (all free): `rustup` (stable + `wasm32-unknown-unknown` target), `wasm-bindgen-cli`, Node 22 + `pnpm`, Python 3.12 + GDAL, the `pmtiles` CLI, `zstd`, git. Java is not needed.

**Create the repo**

```bash
mkdir warboard && cd warboard && git init
mkdir -p docs/ui-reference
cp /path/to/this/package/*.md docs/                    # README, 01–12
cp /path/to/screenshots/0*.png docs/ui-reference/      # the six renamed reference screenshots
cp docs/CLAUDE.md ./CLAUDE.md                          # Claude Code reads it from the repo root
git add . && git commit -m "Architecture package and UI reference"
```

The reference screenshots must keep these names, because the style guide and prompts refer to them: `01-strike-route.png`, `02-salvo-vs-ships.png`, `03-ships-targeted.png`, `04-interceptors.png`, `05-unit-card-rings.png`, `06-world-satellites.png`.

Create a GitHub repository (private is fine) so CI runs on every push.

## 2. Working agreement for every phase

1. **Start a fresh Claude Code session per phase** and paste that phase's prompt.
2. **Plan first.** Ask Claude Code to propose a plan (plan mode) and review it before any code is written. Push back if it touches things outside the phase.
3. **Small commits**, each with passing tests. One branch per phase; merge when the acceptance tests pass in CI.
4. **Phase report.** Claude Code writes `docs/phase-reports/P<n>.md`: what was built, deviations from the docs and why, measured numbers, known gaps.
5. **Design changes** go through a new ADR in `docs/07-adrs.md` (status *Proposed*) — never a silent change. You approve it.
6. **Versions** are pinned and recorded in `docs/versions.md`; upgrades happen only between phases.

### If Claude Code stops with a safety or policy message

1. Copy the exact message and note what Claude Code was doing (reading a doc, planning, writing a file).
2. Don't resend the same request. Check that the session isn't reading old work from an earlier blocked attempt (stash it), then start a fresh session with the phase prompt as written here.
3. If a plainly game-related task is still blocked, it may be a false positive: report it with `/feedback` (also `/bug`) in Claude Code, including the message, so Anthropic can review it.
4. Don't try to get around the check by disguising the task. If part of the design is what keeps getting flagged, bring it back to the design project and decide there whether to simplify that part.

## 3. Phase prompts

Paste the block as the first message of the session. Every prompt assumes `CLAUDE.md` is in the repo root.

### P0–P8 — Done

P0 Foundations, P1 Content schema, P2 Geodata and 2D map, P3 Engine core, P4 Sensors and fog of war, P5 Weapons, air combat and ROE, PF Fictional setting, P6a Naval combat and layered air defence, P6b Electronic warfare, P7 Operations and reports and P8 Admin app and Theatre North content are complete (see `docs/phase-reports/`). Their prompts are in git history.

### P8 — Admin app and Theatre North content

```text
Phase 8 — Admin app and Theatre North content.
WarBoard is set on real terrain with fictional nations and systems (doc 11).
Read first: CLAUDE.md, docs/04-data-model-and-admin.md, docs/11-fictional-setting.md, docs/07-adrs.md ADR-009, 010, 036.
Plan first, then build:
1. apps/admin: open content folder (File System Access API), browse/search, schema-driven forms (@rjsf/core) with custom widgets (RCS by aspect, stations, magazine mix, archetype band bars), live validation via wb-content WASM (including bands and denylist), nations and world editor (seeds, weights, border preview, places, bases), inventories table, compare view, diff of changed files.
2. Test bench: run N-seed Monte Carlo engagements via wb-wasm and chart outcome distributions.
3. Content: grow the catalogue to ~120 systems and ~90 weapons from the archetypes (doc 04 §4), with inventories and doctrines for the eight nations matching their character (doc 11 §2.1).
Content rules (strict):
- Every entry is fictional and names its archetype; stats stay inside the archetype bands or the note says why.
- Never copy one real system's figures; set values from the class bands and balance.
- Give nations different mixes, not better copies of the same thing.
- Shared systems are entered once and referenced by many inventories.
Acceptance:
- wb content check passes (bands, denylist); every entry has an archetype and a note.
- Test-bench sanity suite (scenarios/bench/*.yaml) within bands.
- A custom system created in the admin app via extends appears in the game for its assigned nation.
Finish with docs/phase-reports/P8.md with a table of systems per nation and archetype.
```

**You check:** browse each nation's inventory and check it fits its character. Edit one value in the admin app and see the change in the game.

### P9 — Enemy AI v1, war setup and play modes

P9 now also builds the play modes of doc 12. If the session gets long, split it at the line between steps 1–2 and 3–6 (P9a modes and setup, P9b AI); the acceptance list says which checks belong to which.

```text
Phase 9 — Enemy AI v1, the war setup flow and play modes.
WarBoard is a strategy game on real terrain with fictional nations and systems (doc 11).
Read first: CLAUDE.md, docs/12-play-modes.md, docs/03-gameplay-operations-roe.md §1–2, §7, §9, §11, §13, docs/04-data-model-and-admin.md §7, docs/06-clients-and-platforms.md §1–3, §7, docs/10-ui-style-guide.md §8, docs/07-adrs.md ADR-003, ADR-015, ADR-022, ADR-026, ADR-044.
Plan first, then build:
1. Controllers (doc 12 §1): controllers and mode in setup; every order carries side and is rejected when it names another side's unit, base or contact; set_view_side, and details/contact_details per side; a session section in the save (active side, hot-seat settings, PIN hashes, umpire_used) left out of the state hash; umpire_view only in Command both when enabled, unreachable from wb-ai; save version 4, SideView version 4.
2. War setup screens: coalition builder from any nations, Players step (Human or AI per coalition, difficulty per AI side, "same person or two players" for two humans, hot-seat settings), start mode (garrisoned / deploy yourself with paused clock, for each human side in turn), start time, default ROE per side, pack check (ADR-022). Same panel family as docs/10-ui-style-guide.md §7–§9.
3. Client (doc 12 §2–§3): side toggle with Tab, per-side store slices, per-side event streams, handover screen, hand-over-on-request and turn timer, urgent-event interrupt setting, optional PIN, umpire view with its banner, auto-slow for the active side only.
4. Garrisoned start from inventories' basing data, fallback distribution by base capacity.
5. wb-ai v1 for AI-controlled sides only: strategic posture and allocation, operational CAP stations, IADS layout, scrambles, incursion response, retaliation using strike package templates; tactical layer shared with player units. Reads only its side's picture.
6. Doctrine profiles drive weights; difficulty changes reaction delay and coordination.
Acceptance (steps 1–3):
- Every check in docs/12-play-modes.md §5, including the truth canary for both sides in Command both and Hot-seat, and G16.
- Browser test: the handover screen renders no map, panels, alerts or toasts, and the clock does not advance while it shows.
Acceptance (steps 4–6):
- 24 h sim-time Watch-mode soak (both sides AI) with no panics, bounded memory, and logged decisions.
- AI-truth guard: wb-ai compiles against a picture-only API (no access to truth types or umpire_view), enforced by crate visibility.
- An incursion into AI territory triggers a response within the doctrine reaction time ±20%.
Finish with docs/phase-reports/P9.md.
```

**You check:** start a quick-start war in Theatre North vs AI, garrisoned at Normal, and play an hour. Then start a Command both war, switch sides with Tab, and check each side sees the other only as contacts; turn the umpire view on once and see the mark in the report. Try a hot-seat handover with a PIN.

### P10 — 3D tactical view

```text
Phase 10 — 3D tactical view with CesiumJS.
Read first: CLAUDE.md, docs/06-clients-and-platforms.md §5–6, docs/05-map-and-geodata.md §4, docs/07-adrs.md ADR-008.
Plan first, then build:
1. Lazy-loaded map3d chunk; self-hosted Cesium assets; no ion token, no geocoder, no default imagery.
2. Terrain via CustomHeightmapTerrainProvider from .wbdem tiles; low-res public-domain globe texture; borders as polylines.
3. Same SideView decoder and icon atlas: billboards, trails and trajectories at true altitude, ellipses, rings, satellites with orbits, placeholder glTF models when zoomed in (≤ 200).
4. Camera modes: free, follow unit, watch engagement. 2D↔3D switch keeps selection, time and centre.
5. Follow docs/10-ui-style-guide.md §11 and §12: same tokens, atlas and label grammar as 2D; the button reads OPEN 2D MAP; no Cesium widgets.
Acceptance:
- ≥ 30 fps with 500 visible units on the benchmark scene; works offline.
- Parity test: the same view renders the same set of selectable objects in 2D and 3D.
Finish with docs/phase-reports/P10.md.
```

**You check:** follow a missile salvo in 3D; switch back to 2D mid-flight.

### P11 — Ground forces

```text
Phase 11 — Ground forces.
Read first: CLAUDE.md, docs/02-simulation-engine.md §5, §13–14, docs/03-gameplay-operations-roe.md §3, §5, §8.2, docs/05-map-and-geodata.md §3.
Plan first, then build:
1. Formations with equipment and personnel, posture, morale, supply.
2. Movement on the theatre H3 grid with A* and terrain multipliers; FOBs.
3. Combat resolution every 60 s with terrain and posture modifiers; artillery and rocket artillery; strikes on formations by weapon lethality and posture.
4. SOF and recon teams (detection difficulty, recon quality, sabotage, designation); ground tasks from doc 03 §5.
5. Scenario scenarios/golden/strike-chain.yaml implementing doc 03 §8.2.
Acceptance:
- Strike-chain scenario passes with the expected sequence of events.
- Terrain tests: same attack on open vs urban vs mountain cells gives ordered loss ratios.
Finish with docs/phase-reports/P11.md.
```

### P12 — AI v2, Android wrapper, polish

```text
Phase 12 — AI that plans operations, Capacitor Android build, polish.
Read first: CLAUDE.md, docs/03-gameplay-operations-roe.md §8, §11, docs/06-clients-and-platforms.md §10–11, docs/07-adrs.md ADR-013, 015, 017.
Plan first, then build:
1. wb-ai v2: builds Operation objects (strike packages, air-defence suppression then strike, decoys, timing) against objectives from doctrine.
2. Capacitor 8 Android project wrapping the production build; touch adaptations (bottom-sheet unit card keeping the doc 10 §7 layout); pack manager on device.
3. Performance pass with the benchmark scenes; credits screen from pack manifests.
Acceptance:
- In a soak test the AI launches at least one multi-phase operation with air-defence suppression before strike.
- APK runs a 500-unit war at ×5 on a mid-range phone at ≥ 30 fps in 2D.
Finish with docs/phase-reports/P12.md.
```

### P13 — Unity client (later)

```text
Phase 13 — Unity client.
Read first: CLAUDE.md, docs/06-clients-and-platforms.md §12, docs/07-adrs.md ADR-001, 017.
Build wb-ffi (C ABI mirroring wb-wasm), C# bindings with csbindgen, a Unity 6 project with Cesium for Unity (terrain converted from .wbdem to quantized-mesh with an open tiler), the SideView decoder in C#, glTF models via glTFast, and UI Toolkit panels.
Acceptance: a save made in the browser loads in Unity and, after 1 sim hour, matches the browser's state hash.
```

## 4. Definition of done (every phase)

- Acceptance tests pass in CI; no clippy warnings; types regenerated.
- No new dependency outside the licence allowlist; `docs/versions.md` updated.
- `CLAUDE.md` rules respected (fog API, determinism, data-not-code, UI reference).
- UI phases: screenshots compared with `docs/ui-reference/`, differences listed in the report.
- Phase report written; docs updated where behaviour changed (with an ADR if a decision changed).

## 5. Your review checklist

| Check | How |
|---|---|
| It runs | Play the phase's feature yourself, offline |
| It looks right | Put the report's screenshots next to the matching reference; anything off goes back as a fix |
| Tests are real | Open one acceptance test and read what it asserts |
| Fog holds | Search the diff for truth types reaching `wb-wasm` views or `wb-ai`; `umpire_view` gated by mode (P9) |
| Determinism holds | CI determinism job green; no `HashMap` iteration or std trig in `wb-sim` |
| Data not code | No system names (e.g. `"lr-sam-bastra"`) hard-coded in engine logic |
| Fiction holds | Denylist check green; no real names in new content, scenarios, UI text or docs |
| Docs current | Phase report and any ADR updates present |
