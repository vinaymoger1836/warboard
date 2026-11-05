# 11 — Fictional setting

**Decided 5 Nov 2026 (ADR-036).** WarBoard is set on **real Earth terrain** with **fictional nations, borders, bases, place names and weapon systems**. Coastlines, elevation, land cover, day and night, and orbital mechanics stay real; who lives where and what they fight with are invented.

## 1. What stays real and what is invented

| Real | Invented |
|---|---|
| Coastlines, seas, elevation, land cover (OSM water and land, Copernicus DEM, ESA WorldCover) | Nations, borders, territorial seas (12 nm drawn around the invented borders) |
| Physics of the models: horizon, terrain line of sight, RCS⁴ scaling, missile energy, day and night | Weapon systems, sensors, data links, satellites (fictional designations and stats) |
| Sun position and SGP4 orbits | Bases, ports, cities, all place names (seas, straits, regions) |
| The clock (real UTC) | Inventories, doctrines, alliances |

**Rule:** no real country, armed force, base, city name, weapon system, satellite or military unit appears in content, scenarios, tests, code, UI text or docs. Stats sit in the range of their **class** (doc 04 §4); they are never copied from a single real system.

## 2. Theatre North

The first theatre uses the terrain of the `europe` region pack (built in P2). In the game it is **Theatre North**: a large enclosed northern sea, its gulfs and islands, and the lands around it. All visible names are fictional.

### 2.1 Nations

Eight nations around the sea. Names are data (`content/nations/`) and can be changed at any time.

| Id | Nation | Character (drives doctrine and inventory mix) |
|---|---|---|
| `ALD` | Aldmark | Maritime trading state; strong navy, small air force |
| `VSM` | Vessmar | Large continental power; deep air defence network, many fighters |
| `TLV` | Talvenor | Northern, mountainous; fighters and coastal batteries, defensive doctrine |
| `HLV` | Halvast | Island republic; frigates, maritime patrol, drones |
| `KLD` | Kaldoria | Industrial state; long-range missiles, heavy fighters |
| `OST` | Ostrevia | Agrarian plains; ground forces, short-range air defence |
| `MRV` | Morvel | Small and wealthy; few but advanced systems, stealth fighters |
| `CND` | Cindria | Archipelago state; corvettes, coastal defence, drones |

There are **no fixed blocs**. War setup builds coalitions from any nations. Two suggested quick starts exist only for convenience (`content/scenarios/quickstart-*.yaml`). Neither should mirror a real alliance.

### 2.2 Borders

Borders are **generated, not traced**. `content/world/theatre-north.yaml` lists one seed point and a weight per nation. The geodata pipeline grows regions from the seeds over the H3 land cells. Mountains, wide rivers and sea cost more to cross, so borders settle along terrain features. Regions are then smoothed, small enclaves removed and fixed with a seed. Hand-edits go in `overrides` as cell lists. The result must not follow real borders: the pipeline reports the share of border length within 5 km of a Natural Earth admin-0 line, and the check fails above 15%. Natural Earth is used only for that check and is not shipped.

Islands go to the nearest region on the same side of the sea unless overridden. Land outside the eight nations is *unclaimed* (neutral, no forces).

### 2.3 Territorial seas

The 12 nm sea belt is computed in the pipeline: sea cells within 22.2 km of a nation's coast belong to that nation; where two belts meet, the boundary is the equidistance line. Marine Regions is no longer used.

### 2.4 Place names

- **Hidden:** all real labels from the basemap (countries, cities, seas, regions, roads). The style keeps only physical layers: land, water, land cover, coastline.
- **Added:** a fictional label layer from `content/world/places.yaml`: nation names, capitals and major cities (placed on large urban land-cover areas), seas, gulfs, straits and island groups. The pipeline proposes city locations from urban land cover; names come from a word list per nation, deterministic from a seed and editable.

### 2.5 Bases

Bases are authored per nation in `content/bases/<nation>.yaml` (no OurAirports, no real ports).

| Base | Proposed by the pipeline where | Then |
|---|---|---|
| Air base | Flat open land (slope and roughness limits) with room for the runway class, inside the nation, away from borders | Named, sized (runway class, shelters), saved |
| Naval base | Coast cell with sheltered water (bay or gulf), inside the nation | Named, berths by ship size, VLS reload |
| Other sites | Not preset; placed by players under doc 03 §3 | — |

The pipeline proposes 3–6 air bases and 1–3 naval bases per nation. The owner keeps or moves them in the admin app.

## 3. Fictional catalogue

### 3.1 Naming

- Every system has a **class designation** and a **name**: `VF-5 Vantor`, `Halvard-class frigate`, `AR-7`.
- Prefixes by role: `VF` stealth fighter, `HF` heavy fighter, `MF` multirole fighter, `EJ` electronic warfare jet, `UD` drone, `TK` tanker, `AW` airborne early warning, `SL` surface-to-air missile, `AR`/`AI` air-to-air missile (radar/IR), `AS` anti-ship missile, `EH` emitter-homing missile, `GB` guided bomb, `LR-SAM`/`MR-SAM`/`SR-SAM` air defence systems, `CB` coastal battery, `MBT` tank, `RA` rocket artillery.
- The content check refuses a short list of real designations and names (`content/denylist.txt`) to catch slips. The list is a few dozen common items only; it is a guard, not a catalogue.

### 3.2 Starter catalogue (replaces the P1 seed set)

| Id | Name | Class archetype |
|---|---|---|
| `vf-5` | VF-5 Vantor | Stealth multirole fighter |
| `hf-4` | HF-4 Corvan | Heavy air-superiority fighter |
| `mf-4` | MF-4 Sellis | Medium multirole fighter |
| `ej-2` | EJ-2 Wren | Electronic warfare jet |
| `ud-9` | UD-9 Longwatch | Medium-altitude long-endurance drone |
| `tk-3` | TK-3 Ferrin | Tanker |
| `aw-1` | AW-1 Lantern | Airborne early warning |
| `halvard` | Halvard-class frigate | Air-defence frigate with VLS |
| `kest` | Kest-class corvette | Missile corvette |
| `lr-sam-bastra` | LR-SAM Bastra | Long-range SAM battalion (surveillance radar, fire-control radar, command post, launchers) |
| `mr-sam-tolven` | MR-SAM Tolven | Medium-range SAM battery |
| `cb-2-strand` | CB-2 Strand | Coastal anti-ship battery |
| `sl-40` / `sl-20` / `sl-12` | SL-40, SL-20, SL-12 | Long, medium, short-range SAM missiles |
| `sn-30` | SN-30 | Naval area-defence SAM |
| `ar-7` | AR-7 | Medium-range radar air-to-air missile |
| `ar-9` | AR-9 | Long-range ramjet air-to-air missile |
| `ai-3` | AI-3 | Short-range IR air-to-air missile |
| `as-4-tarn` | AS-4 Tarn | Subsonic sea-skimming anti-ship missile |
| `as-8-spire` | AS-8 Spire | Supersonic anti-ship missile |
| `eh-2` | EH-2 | Emitter-homing missile |
| `gb-900` | GB-900 | 900 kg satellite-guided bomb |
| `orbit-sar-1` / `orbit-eo-2` | Orbit SAR-1, Orbit EO-2 | Imaging satellites (orbital elements authored in content) |
| `weave` / `tessel` | Weave, Tessel | Data links |

Sensors, jammers, countermeasures, loadouts and inventories follow the same scheme. P8 grows the catalogue to about 120 systems and 90 weapons from the archetype table (doc 04 §4).

## 4. Migration from the real-world content

Done in phase **PF** (doc 09) before P6a:

| Area | Change |
|---|---|
| Content | Remove real countries, systems, inventories, bases and data links; add nations, world seeds, places, the starter catalogue, archetypes and the denylist |
| Packs | Rebuild territory and the H3 nation field from generated borders; computed 12 nm belts; basemap style without real labels plus the fictional label layer; remove OurAirports, Marine Regions and Natural Earth from the shipped build |
| Satellites | Authored orbital elements replace CelesTrak TLEs (SGP4 unchanged) |
| Placement rules | `real_airfield` and `real_port` become terrain rules (doc 03 §3) |
| Golden scenarios | G1–G9 rebuilt on the fixtures of ADR-035 with the same bands |
| Code and tests | No real names left; a CI step runs the content denylist over `content/`, `scenarios/`, `crates/`, `apps/` and `docs/` |
| UI | Labels from nations and the catalogue; the reference screenshots are for the look only (doc 10) |
