# CLAUDE.md — WarBoard

Strategy wargame on real Earth terrain with fictional nations and weapon systems: browser now (offline web app), Android later (Capacitor, then Unity). Deterministic Rust engine shared by all clients. Architecture lives in `docs/`; read the files a phase prompt lists before planning.

## What this project is

WarBoard is a strategy game for entertainment (one player vs the AI, or two players on one device), built as a personal hobby project. It is set on real Earth terrain with **fictional nations, borders, bases, place names and weapon systems** (doc 11, ADR-036). Combat uses simplified game rules; stats are game balance values in the range of their class, not data about any real system. The game does not connect to real equipment or live data feeds and is not a planning tool for real operations.

## Non-negotiable rules

1. **Truth never leaves the engine.** Clients and `wb-ai` get side pictures only: own units + contacts with their own `ContactId`. Never put an `EntityId`, exact enemy position or enemy name into a view, details response or event payload for the other side. This holds for **every side in every play mode** (doc 12). The one exception is `umpire_view()`: only when the mode is Command both and the player turned it on, read-only, recorded as `umpire_used`, never reachable from `wb-ai` (ADR-044).
2. **Determinism.** In `wb-sim`, `wb-ai`, `wb-geo`, `wb-core`:
   - single-threaded ECS schedule;
   - iterate in `UnitId` order wherever order affects results;
   - no `HashMap`/`HashSet` iteration (use `BTreeMap`, `IndexMap`, sorted `Vec`);
   - transcendental maths only through `libm` (std `f64::sin` etc. are banned by clippy);
   - random numbers only via `wb_core::rng(seed, purpose, entity, tick, n)`;
   - no wall-clock time, no I/O whose result depends on timing.
3. **Data, not code.** No system-specific logic (`if type == "lr-sam-bastra"`). Behaviour comes from content components and the realism profile in `content/realism/`.
4. **Every outcome is explainable.** Weapon terminations record all endgame factors; engagements emit events.
5. **Orders are commands** with a tick and a `side`, validated by the engine, logged, and rejected with a human-readable reason (including an order that names another side's unit, base or contact).
6. **Fictional setting.** No real country, armed force, base, city, weapon system, satellite or military unit in content, scenarios, tests, code, UI text or docs. Content entries name their class archetype and carry a short balance note (doc 04 §4). `content/denylist.txt` is checked in CI.
7. **Free and open only.** New dependencies must be on the licence allowlist (MIT, Apache-2.0, BSD-2/3, ISC, 0BSD, Zlib, Unicode, CC0). Ask before adding any dependency.
8. **Design changes need an ADR** (`docs/07-adrs.md`, status Proposed). Do not silently diverge from the docs.
9. **Nothing sensitive or generated goes into git.** Secrets, passwords, API keys, tokens, `.env*` files, private keys and certificates must be listed in `.gitignore`, as must dependency and build output (`node_modules/`, `target/`, `dist/`, caches, logs, OS/editor clutter). When a change introduces a new file of this kind, update `.gitignore` in the same change. Never hard-code a secret in source or content.
10. **No git commits or pushes.** Do not run `git commit`, `git push` or anything else that writes history or updates a remote (including amend, rebase, tag, merge). The user commits and pushes manually. Leave changes in the working tree and say what changed.
11. **Stay a game.** No integrations with real military or sensor systems, no live data feeds, and no exports to real-world mission-planning formats. Electronic warfare stays at the game-stat level of doc 02 §9 (ADR-034).

## UI reference (follow it)

- The look is defined by `docs/ui-reference/*.png` and `docs/10-ui-style-guide.md`. Open the relevant screenshots and the guide before building any map layer, icon, label, panel or button.
- Match them: dark basemap; own coalition purple, enemy blue (land **and** 12 nm waters); hexagon ships; squadron tiles with `SQN` and `3 × MF-4 Sellis` labels; `SITE` / `BN` prefixes; dashed outline and `(our estimate)` for estimated contacts; amber missiles with `M-n` tags and salvo labels; cyan interceptors with `INT-n`; rings blue (sensor), orange (engagement), purple dashed (combat radius); the unit card as in `05-unit-card-rings.png`.
- Colours come only from the tokens in the guide (`packages/symbols/tokens.ts`). No emoji, no default MapLibre or Cesium widgets.
- Where screenshots disagree, the guide decides (e.g. offensive missiles are amber, not cyan as in `02`).
- The screenshots are for the look only. Their labels and map names come from an earlier real-world mock-up and are not used; the game uses nation and catalogue names from content.
- Before finishing a UI task: run the app, screenshot the same kind of view as the closest reference, and list the differences in the phase report.

## Repo map

| Path | What |
|---|---|
| `crates/wb-core` | Geo maths, units, IDs, counter RNG, `libm` wrappers |
| `crates/wb-content` | Content schema, validation, pack compiler, schema/TS export |
| `crates/wb-geo` | Territory, H3 terrain, DEM, line of sight |
| `crates/wb-sim` | Engine (ECS, scheduler, systems, events, save/load) |
| `crates/wb-ai` | AI (picture-only API) |
| `crates/wb-wasm` | Browser bindings |
| `crates/wb-cli` | `wb run`, `wb content check/build`, golden tests, Monte Carlo, benchmarks |
| `apps/game` | React + MapLibre v6 + deck.gl + CesiumJS (lazy `map3d/`); `scene/` is the drawable picture both views share (ADR-047) |
| `apps/admin` | Content editor (File System Access API) |
| `apps/android` | Capacitor 8 wrapper of the production build; `android/` is the Gradle project (ADR-050) |
| `packages/protocol` | Generated TS types, SideView decoder |
| `packages/symbols` | Icons, milsymbol adapter, atlas builder |
| `content/` | YAML source of truth: nations, world seeds and places, bases, catalogue, archetypes, realism (doc 11) |
| `geodata/` | Pack build pipeline (Python in the pinned GDAL Docker image) |
| `scenarios/` | Golden, bench and tutorial scenarios |
| `docs/` | Architecture, ADRs, UI style guide, phase reports, versions |
| `docs/ui-reference/` | Reference screenshots for the look (source of truth with doc 10) |

## Commands

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
bash scripts/lint-canary.sh                  # f64::sin / HashMap in wb-sim must fail clippy
cargo deny check                             # licence allowlist, advisories
cargo run -p wb-cli -- run --scenario scenarios/empty.yaml --ticks 10000 --print-hash
cargo run -p wb-cli -- run --scenario scenarios/golden/<name>.yaml --print-hash
cargo run -p wb-cli -- run --scenario scenarios/save-load.yaml --ticks 3600 --save-at 900 --print-hash   # save, reload, continue
cargo run -p wb-cli -- run --scenario <yaml> --ticks N --events         # event log as JSON lines (--content, --packs, --load, --save-out)
cargo run --release -p wb-cli -- golden scenarios/golden/*.yaml         # golden bands (doc 08 §3)
cargo run --release -p wb-cli -- bench --scenario scenarios/bench/patrol-2000.yaml   # G15: ≤ 6 ms per 100 ms tick
cargo run --release -p wb-cli -- bench --scenario scenarios/bench/sensors-2000.yaml  # P4: two sides, sensors on, ≤ 6 ms
cargo run --release -p wb-cli -- golden scenarios/bench/sanity-*.yaml   # P8: catalogue sanity suite (ADR-045)
cargo test -p wb-sim --test custom_variant   # P8: an `extends` variant reaches its nation's inventory
pnpm --filter admin dev                      # P8: content editor (needs pnpm gen first)
cargo test -p wb-sim --test truth_canary         # P4: no hidden enemy name, id or exact position reaches a client
cargo test -p wb-sim --test play_modes           # P9: cross-side orders refused, truth canary for both sides, umpire rules, session not hashed, G16
cargo test -p wb-sim --test setup                # P9: war setup builder, garrisoned start from inventories, deploy yourself
cargo test -p wb-sim --test ai_incursion         # P9: AI answers an incursion within its reaction time ±20%, AI only for AI sides, replay exact
cargo test -p wb-ai                              # P9: AI-truth guard (wb-ai depends on wb-core and serde only)
cargo test --release -p wb-sim --test ai_operations   # P12: the AI plans decoy, jam, suppression, strike; suppression first; save and replay exact
cargo test --release -p wb-sim --test ai_soak -- --ignored --nocapture   # P9: 24 h Watch-mode soak (WB_SOAK_HOURS=n for less)
pnpm --filter android sync                       # P12: copy apps/game/dist into the Android project (pnpm --filter game build first); apps/android/README.md for the APK
WB_E2E_PERF2D=1 pnpm --filter game e2e --project perf2d   # P12: 500 units at x5 in 2D on a production build under CPU throttling (WB_E2E_CPU_SLOWDOWN=4, WB_E2E_PROFILE=1)
pnpm --filter game e2e                           # P9: Playwright browser tests on the installed Chrome (handover, PIN, hot-seat save, side switching, umpire); needs pnpm gen
                                                 # P10: also e2e/map3d.spec.ts (2D↔3D keeps state, parity of selectable objects, no Cesium widgets or ion, inspect); installs the world pack from build/packs
WB_E2E_PERF=1 pnpm --filter game e2e --project perf3d      # P10: ≥ 30 fps with 500 visible units (scenarios/bench/view-500.yaml), headed on the GPU
WB_E2E_OFFLINE=1 pnpm --filter game e2e --project offline  # P10: production build + service worker; the 3D view opens and draws offline
node packages/symbols/models/build-models.ts     # P10: low-poly class models → apps/game/public/models/ (also run by pnpm gen)
cargo test --release -p wb-sim --test ground     # P11: routes over terrain, refused ground orders, FOB supply, raid, designation, save mid-battle, own-side-only ground events
cargo run --release -p wb-cli -- golden scenarios/golden/strike-chain.yaml scenarios/golden/g18-ground-terrain.yaml   # P11: G17 strike chain, G18 terrain loss ratios
# P11 ground UI scene: cargo run -p wb-cli -- content build --dir scenarios/fixtures --out build/fixtures.wbpack, then
#   WB_SCENARIO=scenarios/ground-demo.yaml WB_CONTENT_PACK=build/fixtures.wbpack pnpm --filter game dev
cargo test --release -p wb-sim --test operations # P7: Rising Sun end to end, report totals = event counts, replay hashes, save mid-operation
cargo run --release -p wb-cli -- report --scenario scenarios/golden/rising-sun.yaml --ticks 14000 --side 0 --truth --out build/report   # JSON, CSV tables, printable HTML
cargo run --release -p wb-cli -- replay --scenario scenarios/golden/rising-sun.yaml --ticks 14000 --seek 3000,7777   # every checkpoint and seek re-simulated to its hash
cargo run -p wb-cli -- content check && cargo run -p wb-cli -- content build   # pack → build/content.wbpack, hash on stdout
cargo run -p wb-cli -- content schema        # JSON Schema → packages/protocol/src/generated/ (also run by pnpm gen)
pnpm install && pnpm gen                     # ts-rs types + content schema + wb-wasm build + build/content.wbpack (needed before typecheck/build/dev)
node crates/wb-wasm/node/run-scenario.mjs --content build/content.wbpack --scenario scenarios/empty.yaml --ticks 10000
bash scripts/determinism-check.sh            # native == Node WASM for each scenario, plus save/reload and native save → WASM load
pnpm -r typecheck && pnpm -r lint && pnpm -r build
pnpm --filter game dev                       # serves build/packs/ at /packs/ for the pack manager
# hosted build (ADR-040; .github/workflows/pages.yml does this): fixtures pack, then
#   WB_SCENARIO=scenarios/hosted-test.yaml WB_CONTENT_PACK=build/fixtures.wbpack VITE_PACKS_URL=<r2 url> pnpm --filter game build
rclone copy build/packs R2:warboard-packs --s3-no-check-bucket --s3-upload-cutoff=100M --s3-chunk-size=100M --progress   # upload packs (ADR-040)
bash geodata/run.sh image                    # build the pinned GDAL image (Docker)
bash geodata/run.sh build <world|europe> [--step basemap|dem|terrain|territory|manifest]   # europe first: it grows the borders, world reuses them
bash geodata/run.sh places                   # PF: propose nation labels, capitals, cities → content/world/places.yaml
bash geodata/run.sh bases                    # PF: propose air and naval base sites per nation → content/bases/<nation>/
cargo run -p wb-cli -- geo info|query|los    # query build/packs (e.g. geo query --at 56.0,18.0)
cargo test -p wb-geo --test packs -- --ignored   # P2 acceptance tests on the real packs
cargo test -p wb-sim --test placement -- --include-ignored   # P3 placement and ship-route checks (ignored ones need the packs)
```

(Keep this list current as commands are added.)

## Conventions

- SI units inside the engine (m, s, kg, rad); content fields carry unit suffixes (`range_km`, `speed_mach`).
- Lat/lon in radians internally, degrees at the edges (content, UI, orders).
- Errors: `thiserror` in libraries, `anyhow` only in `wb-cli`.
- Golden scenarios use the generic fixtures in `scenarios/fixtures/` at the neutral test sea area (ADR-035). Engine sessions don't need to read `content/` beyond the schema.
- Testing favours end-to-end tests: golden scenarios in `scenarios/golden/` (bands in the file) run through `wb-cli`, and browser flows in the apps. Do not write unit tests for every code change; add one only where an end-to-end test cannot pin the behaviour down (e.g. determinism-critical maths in `wb-core`).
- React renders panels only; map layers update from the decoded view buffer.
- UI text is plain English, sentence case; map unit labels and command buttons follow doc 10 (prefixes, uppercase buttons).

## Working style

- Plan first; confirm scope with the phase prompt; stay inside the phase.
- Work in small, reviewable steps; the existing end-to-end tests must still pass. The user commits (see rule 10).
- End each phase with `docs/phase-reports/P<n>.md` and update `docs/versions.md`.
- When a doc is ambiguous, propose an answer in the plan and ask.

## Phase status

| Phase | Status |
|---|---|
| P0 Foundations | done, CI green (`docs/phase-reports/P0.md`) |
| P1 Content schema & seed data | done, CI green (`docs/phase-reports/P1.md`) |
| P2 Geodata & 2D map | done, CI green (`docs/phase-reports/P2.md`) |
| P3 Engine core | done, CI green (`docs/phase-reports/P3.md`) |
| P4 Sensors & fog of war | done, CI green (`docs/phase-reports/P4.md`) |
| P5 Weapons, air combat & ROE | done, CI green (`docs/phase-reports/P5.md`) |
| PF Fictional setting (doc 11) | done locally, CI not yet run (`docs/phase-reports/PF.md`) |
| P6a Naval combat & layered air defence | done locally, CI not yet run (`docs/phase-reports/P6a.md`) |
| P6b Electronic warfare (game stats) | done locally, CI not yet run (`docs/phase-reports/P6b.md`) |
| P7 Operations & reports | done locally, CI not yet run (`docs/phase-reports/P7.md`) |
| P8 Admin app & Theatre North content | done locally, CI not yet run (`docs/phase-reports/P8.md`) |
| P9 Enemy AI v1, war setup & play modes (doc 12) | done locally, CI not yet run (`docs/phase-reports/P9.md`) |
| P10 3D tactical view (+ inspect view, ADR-047) | done locally, CI not yet run (`docs/phase-reports/P10.md`) |
| P11 Ground forces (ADR-048) | done locally, CI not yet run (`docs/phase-reports/P11.md`) |
| P12 AI v2, Android wrapper, polish | done locally; APK not built or run on a device here, CI not yet run (`docs/phase-reports/P12.md`) |
| P13 Unity client | later |
