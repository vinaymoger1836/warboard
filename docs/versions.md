# Pinned versions

Every tool and dependency is pinned exactly. Upgrade only between phases (doc 08, R10) and update this file in the same change. Licences are checked against the CLAUDE.md rule 7 allowlist; Rust crates (including transitive ones) are enforced by `cargo deny check` (`deny.toml`).

Last updated: 10 Nov 2026 (P10). P10 adds four npm dependencies for the 3D view (owner's approval): `@cesium/engine`, `@cesium/core`, `@mapbox/vector-tile` and `pbf`. Before that, last updated 10 Nov 2026 (P9). No new dependency in P4, P5, P6a, P6b, P7 or P8. P9 adds one dev dependency, `@playwright/test` (owner's approval, browser tests only), and the workspace crate `wb-ai` gains its first dependencies (`wb-core`, `serde`).

## Toolchains

| Tool | Version | Pinned in | Licence |
|---|---|---|---|
| Rust | 1.99.1 (stable), components `rustfmt`, `clippy`, target `wasm32-unknown-unknown` | `rust-toolchain.toml` | MIT / Apache-2.0 |
| Rust edition | 2024, resolver 3 | `Cargo.toml` | — |
| Node.js | 24 (LTS line; local 24.15.0) | `.nvmrc`, `package.json` `engines` | MIT |
| pnpm | 11.18.0 | `package.json` `packageManager` | MIT |
| wasm-bindgen-cli | 0.2.129 (must equal the `wasm-bindgen` crate) | CI `WASM_BINDGEN_VERSION` | MIT / Apache-2.0 |
| cargo-deny | 0.20.2 | CI `CARGO_DENY_VERSION` | MIT / Apache-2.0 |
| Docker image `ghcr.io/osgeo/gdal` | `ubuntu-small-3.13.3@sha256:64250faf833c06d4b21afce4c27190039ba7ab58d70f0eebc87cf77d929c0b40` (GDAL 3.13.3, Python 3.14.4, numpy 2.3.5) | `geodata/Dockerfile` | MIT (GDAL); bundles GEOS (LGPL-2.1), build time only, never shipped |
| go-pmtiles CLI | 1.31.2 (SHA-256 checked) | `geodata/Dockerfile` | BSD-3-Clause |

## Rust crates (direct)

| Crate | Version | Used by | Licence | Notes |
|---|---|---|---|---|
| `libm` | 0.2.16 | wb-core | MIT / Apache-2.0 | Only source of transcendental maths |
| `bevy_ecs` | 0.19.1 | wb-sim | MIT / Apache-2.0 | `default-features = false, features = ["std"]`: no multi-threaded executor, no reflection. 0.20.0 exists; the docs lock 0.19.x |
| `blake3` | 1.8.7 | wb-sim, wb-content | CC0-1.0 / Apache-2.0 | `default-features = false, features = ["std"]` (portable code path on wasm32) |
| `serde` | 1.0.229 | wb-core, wb-sim, wb-content | MIT / Apache-2.0 | `derive` |
| `serde_yaml` | 0.9.34+deprecated | wb-sim, wb-content, wb-cli (P3: golden blocks) | MIT / Apache-2.0 | Archived upstream. Its parser `unsafe-libyaml` has advisory RUSTSEC-2024-0449 (unmaintained), ignored in `deny.toml`. Kept by decision in P0 and again in P1 (content files are trusted input) |
| `thiserror` | 2.0.21 | wb-sim, wb-content | MIT / Apache-2.0 | |
| `serde_path_to_error` | 0.1.20 | wb-content | MIT / Apache-2.0 | Field paths in content errors. Added in P1 |
| `postcard` | 1.1.3 | wb-content, wb-sim | MIT / Apache-2.0 | `default-features = false, features = ["use-std"]` (no heapless). Pack encoding (P1); snapshot, state hash and `.wbsave` body (P3) |
| `schemars` | 1.2.3 | wb-content (optional feature `schema`) | MIT | Default features (`derive`, `std`). JSON Schema of the content format. Added in P1 |
| `serde_json` | 1.0.154 | wb-cli, wb-wasm, wb-content (feature `schema`) | MIT / Apache-2.0 | Writes `content.schema.json` (P1); orders, details and events as JSON for the browser, `wb run --events` (P3) |
| `anyhow` | 1.0.104 | wb-cli | MIT / Apache-2.0 | CLI only |
| `clap` | 4.6.7 | wb-cli | MIT / Apache-2.0 | `derive` |
| `wasm-bindgen` | 0.2.129 | wb-wasm | MIT / Apache-2.0 | |
| `ts-rs` | 12.0.1 | wb-core, wb-sim, wb-content (optional feature `ts`) | MIT | Default features since P1: `serde-compat` reads the `#[serde]` attributes of the content types (P0 exports unchanged) |
| `h3o` | 0.11.0 | wb-geo | BSD-3-Clause | `default-features = false`: no `std`, so all trig goes through `libm` (deterministic). Nothing in the workspace may enable its `std` feature. Added in P2 |
| `ruzstd` | 0.9.1 | wb-geo (decoder), wb-sim (encoder and decoder for `.wbsave`), wb-geo dev (encoder in tests) | MIT | `default-features = false, features = ["std"]` (no `twox-hash`). Pure Rust, builds for wasm32. Added in P2; wb-sim since P3 |

Workspace crates used across crates since P3: `wb-sim` depends on `wb-content` (rules from the pack) and `wb-geo` (order validation); `wb-wasm` on `wb-content`. No new third-party Rust crate in P3.

Transitive crates are recorded in `Cargo.lock`; `cargo deny check` passes (licences, bans, sources, advisories). Duplicate versions currently warned: `hashbrown`, `syn`.

## npm packages (direct, exact)

| Package | Version | Where | Licence |
|---|---|---|---|
| `react`, `react-dom` | 19.3.1 | apps/game | MIT |
| `vite` | 8.3.5 | apps/game | MIT |
| `@vitejs/plugin-react` | 6.1.2 | apps/game | MIT |
| `@types/react`, `@types/react-dom` | 19.3.0 | apps/game | MIT |
| `typescript` | 6.0.3 | root | Apache-2.0 |
| `eslint` | 10.13.0 | root | MIT |
| `@eslint/js` | 10.0.1 | root | MIT |
| `typescript-eslint` | 8.72.0 | root | MIT |
| `eslint-plugin-react-hooks` | 7.1.1 | root | MIT |
| `globals` | 17.13.0 | root | MIT |
| `@types/node` | 24.19.2 | root | MIT |
| `maplibre-gl` | 6.16.0 | apps/game | BSD-3-Clause |
| `pmtiles` | 4.5.0 | apps/game | BSD-3-Clause |
| `@deck.gl/core`, `@deck.gl/layers`, `@deck.gl/mapbox` | 9.4.0 | apps/game | MIT |
| `@luma.gl/core`, `@luma.gl/engine` | 9.4.2 | apps/game (deck.gl peers) | MIT |
| `@loaders.gl/core` | 4.5.4 | apps/game (deck.gl peer) | MIT |
| `@deck.gl/extensions` | 9.4.0 | apps/game (P3, approved: `PathStyleExtension` for dashed rings and patrol circles; no new transitive packages) | MIT |
| `@cesium/engine` | 26.4.0 | apps/game, lazy 3D chunk (P10, approved: the engine package of CesiumJS 1.146.0, 1 Oct 2026; the `cesium` meta-package is not used because it ships the widgets doc 10 §12 forbids). Static files self-hosted by `vite.config.ts` (`Workers/`, `ThirdParty/`, `Assets/Images`, `Assets/IAU2006_XYS`, `approximateTerrainHeights.json`; ≈ 6.1 MB). Transitive licences: Apache-2.0, MIT, BSD, ISC, plus `dompurify` 3.4.17 (MPL-2.0 OR Apache-2.0, taken under Apache-2.0) and `pako` 3.0.3 (MIT AND Zlib) | Apache-2.0 |
| `@cesium/core` | 0.1.0 | apps/game (P10, approved with Cesium: the maths and transforms CesiumJS 1.146 moved out of the engine). 0.1.1 is 8 days old, so 0.1.0 is pinned and forced for the engine too by `overrides` in `pnpm-workspace.yaml` (one copy) | Apache-2.0 |
| `@mapbox/vector-tile` | 3.0.0 | apps/game, 3D view (P10, approved: decodes the packs' vector tiles to draw the globe surface and borders). Brings `@mapbox/point-geometry` 1.1.0 (ISC) and `@types/geojson` (MIT) | BSD-3-Clause |
| `pbf` | 5.1.2 | apps/game, 3D view (P10, approved: protobuf reader for `@mapbox/vector-tile`). Brings `resolve-protobuf-schema` and `protocol-buffers-schema` (MIT) | BSD-3-Clause |
| `@capacitor/core`, `@capacitor/android` | 8.6.0 | apps/android (P12, named by the phase prompt and ADR-017: the Capacitor 8 wrapper of the web build). 8.6.0 is 28 Oct 2026; 8.6.1 and 8.6.2 are 12 Nov and too new for the two-week rule | MIT |
| `@capacitor/cli` | 8.6.0 | apps/android, build time only (P12). Its dependency tree adds 13 packages under **BlueOak-1.0.0** (`glob`, `minimatch`, `lru-cache`, `minipass`, `tar`, `rimraf`, `sax`, `path-scurry`, `chownr`, `yallist`, `graceful-fs`, `package-json-from-dist`, `@isaacs/nonrecursive-glob-match`) and 2 under **Unlicense** (`big-integer`, `stream-buffers`), none on the CLAUDE.md allowlist; they never reach the app or the APK. **Owner to confirm** (docs/phase-reports/P12.md); removing `apps/android` removes them | MIT |
| `@playwright/test` | 1.59.1 | apps/game, dev only (P9, approved: browser tests in `apps/game/e2e`, run on the installed Chrome with `channel: 'chrome'`, so no browser download). Brings `playwright` and `playwright-core` 1.59.1 (Apache-2.0) and the optional `fsevents` 2.3.2 (MIT, macOS only) | Apache-2.0 |

Notes:
- TypeScript 7.0 (native) is out, but `typescript-eslint` 8.72 supports only `>=4.8.4 <6.1.0`, so TypeScript stays on 6.0.3.
- `typescript-eslint` 8.72.1 was published the day before bootstrap; pnpm 11's minimum-release-age gate rejects it, so 8.72.0 is pinned instead of exempting it.
- Transitive packages are pinned by `pnpm-lock.yaml`. There is no automated JS licence check yet (see P0 report). A manual check in P2 found every package added with the map stack MIT, ISC, Apache-2.0 or BSD, and `tslib` 2.8.1 (0BSD, via loaders.gl); 0BSD was added to the allowlist after P2.

## Python packages (geodata image, build time only)

| Package | Version | Licence |
|---|---|---|
| `h3` | 4.5.0 | Apache-2.0 |
| `shapely` | 2.2.0 | BSD-3-Clause (wheel bundles GEOS, LGPL-2.1) |
| `zstandard` | 0.25.0 | BSD-3-Clause |
| `blake3` | 1.0.11 | CC0-1.0 / Apache-2.0 |
| `PyYAML` | 6.0.3 | MIT |

Pinned in `geodata/requirements.txt`; numpy and the GDAL bindings come with the base image.

## Fonts (OFL-1.1, accepted for fonts only in P2)

| Font | Version | Where |
|---|---|---|
| Noto Sans glyph PBFs (Regular, Italic, Medium) | `protomaps/basemaps-assets` @ `028c18f7` | `apps/game/public/fonts/glyphs/` |
| Noto Sans woff2 | `@fontsource/noto-sans` 5.3.0 files | `apps/game/public/fonts/web/` |
| JetBrains Mono woff2 | `@fontsource/jetbrains-mono` 5.3.0 files | `apps/game/public/fonts/web/` |

## Geodata sources

Pinned in `geodata/sources.lock.yaml`: Protomaps daily build 20261027 (basemap v4.15.2 schema), Copernicus DEM GLO-90 (AWS COG), ESA WorldCover 2021 v200, and Natural Earth admin-0 1:10m 5.1.1 (read only by the border overlap check, not in any pack). PF removed Marine Regions and OurAirports.

## GitHub Actions (pinned to commit SHA)

| Action | Tag | SHA |
|---|---|---|
| `actions/checkout` | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| `actions/setup-node` | v7.1.0 | `949feb2413d6458794dcd2491c4babbbce0c15c1` |
| `pnpm/action-setup` | v6.1.0 | `ea17c68df8912ef543352723c149a84f56e3d413` |
| `actions/upload-artifact` | v7.0.2 | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| `actions/download-artifact` | v8.0.2 | `9000827ccba6bdab643e8b6fd33ac0654aef8333` |
| `Swatinem/rust-cache` | v2.9.3 | `d1527c8f67ef9af357290475d4af4b0fab8f12e0` |
| `taiki-e/install-action` | v2.87.32 | `5488ac5a951645ce95062214902ac4b6bc1a1037` |
| `actions/upload-pages-artifact` | v5.0.0 | `fc324d3547104276b827a68afc52ff2a11cc49c9` (MIT; `pages.yml`) |
| `actions/deploy-pages` | v5.0.1 | `368f82528645a54fb793d4d04e342629a3f51346` (MIT; `pages.yml`) |

Runner image: `ubuntu-24.04`.

## Measured sizes

| Artifact | Size |
|---|---|
| `wb_wasm_bg.wasm` (release, no wasm-opt) | P1: 968 KB (279 KB gzip). P2 with `wb-geo`, `h3o`, `ruzstd`, BLAKE3 hasher: 1,211,216 bytes (354,344 gzip). P3 with the engine (bevy_ecs world, content, saves): 2,066,441 bytes (575,295 gzip). P4 with sensors, pictures and SGP4: 2,657,338 bytes. P5 with weapons, endgame, damage and ROE: 3,421,497 bytes (901 KB gzip). PF (places, `map_labels`): 3,625,278 bytes (935,972 gzip) |
| `build/content.wbpack` | P1 seed set, 67 entries: 22,401 bytes. P2 with 312 imported bases, 379 entries: 85,086 bytes. P3, schema 2 (realism `movement`, `fuel`, `bases`): 86,607 bytes. P4, schema 3, 395 entries (10 sensors and 6 satellites added): 94,258 bytes. P5, schema 4, 397 entries (weapons, loadouts, countermeasures, realism `weapons`, `endgame`, `damage`, `roe`): 97,300 bytes. PF, schema 6, 188 entries (fictional catalogue, 45 bases, places, no real content): 27,067 bytes |
| App shell (`apps/game/dist`) | P2: main chunk 1.97 MB (557 KB gzip), MapLibre worker 519 KB, fonts 2.3 MB (glyph PBFs 2.1 MB). P3: main chunk 2.16 MB, plus the content pack (87 KB) and the WASM engine (2.07 MB) as assets. P4: main chunk 2.17 MB, content pack 94 KB, engine 2.66 MB. P5: main chunk 2.18 MB (611 KB gzip), content pack 97 KB, engine 3.42 MB. PF: main chunk 2.20 MB. P10: main chunk 2.27 MB; lazy 3D chunk (`Map3D`, CesiumJS engine) 3.96 MB; self-hosted Cesium files 6.1 MB; class models 23 files, 392 KB (3,016 triangles) |
| Side view (`SideView` v1) | P3 benchmark, 2,000 platforms + 500 groups + 316 bases: 93,880 bytes per view. P5 (v3): `patrol-2000` 98,248 bytes, `sensors-2000` 55,288 bytes |
| `.wbsave` | P3 `scenarios/save-load.yaml` at tick 900 (≈ 150 entities with all bases, events): 17,109 bytes. P4 demo war (removed in PF) at tick 1800, with contacts on both sides: 26,395 bytes |

## Formats

| Format | Version | Defined in |
|---|---|---|
| Content pack schema | 7 (P11, ADR-048: platform `ground` stats, `deploy_as: formation`, realism `ground` section, `ground_supply` on base rules). Before: 6 (PF: schema 5 nations and design notes, ADR-041; schema 6 places, ADR-042; `gps` guidance renamed `satnav`). Before: 4 (P5: weapon flight, guidance, Pk and countermeasure fields; loadouts; realism `weapons`, `endgame` incl. `nominal_speed_fraction`, `countermeasures`, `damage`, `roe` incl. `territory_sigma`; P6b: radar `jam_resistance`, `ew_suite` modes with `jam_strength`, `coverage_km`, `sector_deg` and `effects`, weapon `seeker_resistance` and `nav_resistant`, realism `ew` constants; the legacy `eccm_db`, `seeker_eccm` and `j_db` are still read and migrated, number unchanged) | `crates/wb-content/src/pack.rs` |
| State hash | 8 (P12, ADR-049: the AI state of every AI side adds its operation list, serial, cooldown and planned objectives). Before: 7 (P11: ground formations, ground tasks, the ground state and terrain patches of test terrain). Before: 6 (P9: the sides carry controller, difficulty and the play mode; the snapshot adds the deployment pool and the AI state of every AI side; the order log marks AI orders). Before: 5 (P7: operations, the war state, fire missions on groups, the order log's submit tick). Before: 4 (P5: snapshot adds weapons in flight, magazines, health, ROE settings and requests, combat state; P6a: the effect struct changed, so P5 saves are refused, number unchanged; P6b: the unit record gains the jammer state (on or off, mode), so older saves are not expected to load, number unchanged) | `crates/wb-sim/src/snapshot.rs` |
| `SideView` | 4 (P9, ADR-044: header byte 7 is a flags byte, bit 0 `UMPIRE` with side 255; own-unit and weapon rows gain a `side u8` column, the owner, used by the umpire view). Before: 3 (P5: weapons section with shooter, target contact and tag; `RELATION`; weapon symbols 120–122; P6a: effects section filled and filtered by side, weapon contacts carry a weapon symbol, wire format unchanged; P6b: unit flag `JAMMING` and `JAMMED`, contact flag `JAMMED`, wire format unchanged) | `crates/wb-sim/src/view.rs`, doc 06 §3 |
| `.wbsave` | 6 (P12: snapshot format 8; version 5 saves are refused). Before: 5 (P11: snapshot format 7; version 4 saves are refused). Before: 4 (P9: snapshot format 6 and a session section beside it: active side, hot-seat settings, PIN hashes, `umpire_used`; version 3 saves are refused). Before: 3 (P7: snapshot format 5, replay checkpoints in the body; version 2 saves are refused). Before: 2 (after P6b: the snapshot changed in P5, P6a and P6b with the number left at 1, so old saves failed with a decode error; they are now refused as version 1) | `crates/wb-sim/src/save.rs`, doc 04 §7.1 |
| IndexedDB `warboard` | 2 (P3: `saves` store) | `apps/game/src/db.ts` |

### Region packs 2.0.0 (PF, built 8 Nov 2026: generated Theatre North borders, ADR-042)

| File | `world` | `europe` |
|---|---|---|
| `basemap-*.pmtiles` | 45,099,573 (z0–6, unchanged) | 601,820,577 (z7–9, unchanged) |
| `territory-*.pmtiles` | 43,700 (layers `territory`, `borders`) | 146,144 |
| `dem-*.wbdem` | 26,172,959 (unchanged) | 91,953,016 (unchanged) |
| `terrain-*.wbh3` | 1,837,857 (res 5, 549,333 cells) | 9,502,812 (res 7, 2,956,482 cells) |
| `territory-*.bin` | 80,729 (1,715 border cells) | 452,214 (14,987 border cells) |
| **Total** (plus `manifest.json`) | **73,234,818 (70 MiB)** | **703,874,763 (671 MiB)** |

Only the eight nations of Theatre North have territory now, so the territory files shrank from 39 MB to 0.7 MB in total. The border overlap check reports 3.5 % of 8,715 km of land border within 5 km of a real border (limit 15 %).

### Region packs 1.0.0 (P2, built 28 Oct 2026 from the sources in `geodata/sources.lock.yaml`; replaced in PF)

| File | `world` | `europe` |
|---|---|---|
| `basemap-*.pmtiles` | 45,099,573 (z0–6) | 601,820,577 (z7–9) |
| `territory-*.pmtiles` | 6,453,912 | 3,092,424 |
| `dem-*.wbdem` | 26,172,959 (2′, 26,475 tiles) | 91,953,016 (15″, 2,330 tiles) |
| `terrain-*.wbh3` | 2,178,341 (res 5, 672,016 cells) | 10,090,030 (res 7, 3,284,081 cells) |
| `territory-*.bin` | 32,828,315 (78,374 border cells) | 7,862,958 (84,697 border cells) |
| **Total** (plus `manifest.json`) | **112,733,100 (108 MiB)** | **714,819,005 (682 MiB)** |

Doc 05 §2 estimated ≈ 200 MB for `world` and 0.5–0.7 GB for `europe`: world came in smaller, europe slightly above the range, almost all of it basemap tiles. The world territory rings are larger than its terrain because 9 km res-5 border cells carry full 1:10m coastline detail.
