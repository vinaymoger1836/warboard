# 02 — Simulation engine

The engine (`wb-sim`) is the game. Clients only draw what it reports and send orders. Everything below runs identically in the browser (WebAssembly), in the CLI and later inside Unity.

## 1. Principles

1. **One source of truth.** Only the engine holds exact state.
2. **Deterministic.** Same content pack + same setup + same orders at the same ticks ⇒ same war, on every platform.
3. **Data-driven.** Every number that describes a system lives in content. Every tuning constant lives in a *realism profile* in content, not in code.
4. **Explainable.** Every outcome records the factors behind it, so the after-action report can say *why* a missile missed.
5. **Fair.** The player and the AI both act only on their side's picture.

## 2. World representation

ECS (`bevy_ecs`, single-threaded executor). Entities are platforms, groups, bases, base components, weapons in flight, satellites, decoys and contacts are **not** entities (they live in each side's picture).

| Component | Holds |
|---|---|
| `Identity` | `UnitId` (stable u32), name, country, coalition, content type id |
| `Kinematics` | lat, lon (rad, f64), alt (m), heading, pitch, speed (m/s), domain (air/surface/sub/ground/space) |
| `Fuel` | kg remaining, burn model ref, bingo kg (derived) |
| `Magazine` | stations/cells → weapon type, count; countermeasures (chaff, flares, decoys) |
| `Sensors` | list of sensor instances (content ref, on/off, scan phase, fire channels in use) |
| `Emitter` | EMCON state, active emitters (for ESM) |
| `Jammer` | EW suite instance, mode (off / self-screen / escort / stand-off), target bands |
| `Signature` | RCS by aspect, IR, acoustic, visual size; loadout multiplier |
| `Durability` | damage points or kill model, damaged subsystems |
| `Task` | current task state machine + queue |
| `GroupMember` | group id, formation slot |
| `RoeSetting` | Alpha / Bravo / Charlie, self-defence flag, inherited or overridden |
| `BaseRef` | home base, stationed base, sortie state |
| `Network` | data link memberships |
| `WeaponFlight` | (weapons) shooter, target ref, guidance state, energy, seeker state |
| `FormationStrength` | (ground) equipment counts, personnel, posture, morale, supply |

Resources: `SimClock`, `RealismProfile`, `ContentDb` (read-only), `TerrainDb`, `SidePictures`, `EventLog`, `OperationsBook`, `SpatialIndex`, `RngSeed`.

## 3. Time and scheduler

**Fixed step with adaptive size.** The tick is 100 ms of sim time whenever any weapon is in flight or any hostile contact is inside an engagement envelope of any unit; otherwise 1 s. The rule depends only on state, so it is deterministic. This lets ×30 and ×60 transits run cheaply.

**Multi-rate systems** (in sim time):

| System | Rate | Notes |
|---|---|---|
| Weapons in flight (guidance, energy, endgame) | every tick | Endgame uses closest-point-of-approach within the tick, so nothing tunnels |
| Aircraft / ship / ground movement, fuel | every tick | Great-circle steps |
| Radar & IRST scans | per sensor scan period (1–12 s) | Staggered by `hash(UnitId) mod period` to spread load |
| ESM | 2 s | |
| Satellites (SGP4, footprint) | 5 s | Reports delivered after downlink delay |
| Fusion / contact ageing | 1 s | |
| TEWA, ROE, auto-engagement | 1 s, and immediately on new threat | |
| Logistics (turnaround, refuel, repairs) | 10 s | |
| Ground combat resolution | 60 s | Phase 11 |
| AI tactical / operational / strategic | 1 s / 30 s / 5 min | |
| State hash checkpoint | every 600 ticks | Desync and determinism tests |

**Real clock mapping.** A war starts at the real current UTC time (or a chosen time). `advance(real_dt_ms)` adds `real_dt × speed` to an accumulator and runs whole ticks. Speeds: pause, ×1, ×3, ×5, ×10, ×30, ×60. The engine **never skips ticks**: if the device cannot keep up, it reports the achieved speed (e.g. "×10 requested, ×7.6 actual").

**Auto-slow.** On a new hostile contact, a launch against own forces, or an ROE approval request, the engine emits `AutoSlow`; the client drops to ×1 (or pauses, per setting). Speed is not part of the simulation, so it never affects outcomes.

**Day and night** come from a solar position calculation at each target's location. Weather is a later phase; v1 is clear sky.

## 4. Determinism rules (non-negotiable)

| Rule | Why |
|---|---|
| Single-threaded ECS executor | Parallel system order is not reproducible |
| Every system that draws random numbers or resolves conflicts iterates in `UnitId` order | Archetype order must never matter |
| No `HashMap`/`HashSet` iteration in sim code (`BTreeMap`, `IndexMap`, sorted `Vec` only) | Hash iteration order varies |
| Floats: `f64`, transcendental functions only via `libm` (clippy `disallowed-methods` bans `f64::sin` etc.) | Platform libm results differ |
| **Counter-based RNG**: `u = rng(seed, purpose, entity, tick, n)` (SplitMix64/Philox-style hash) | A random value never depends on how many other draws happened |
| Orders carry the tick they apply at; they are logged | Replays and future PvP lockstep |
| Terrain reads are pure functions of position (cache state never changes a result) | Lazy tile loading cannot change outcomes |
| CI runs the same scenario natively and in WASM (Node) and compares state hashes | Catches drift early |

## 5. Movement

- **Earth model:** sphere, R = 6,371,008.8 m for navigation and distances; 4/3-Earth radius for radio horizon. Local tangent-plane (ENU) maths for intercept geometry.
- **Aircraft:** speed/altitude envelope (min, cruise, max, max with afterburner), climb rate, turn rate (sustained G). Flight profile per task leg (cruise altitude, low-level ingress). Formation flight keeps slots.
- **Ships:** cruise and max speed (knots), turn radius; must stay on water cells; port approach via base berth.
- **Submarines:** depth bands (periscope, shallow, deep), speed affects noise.
- **Ground (Phase 11):** speed by H3 terrain class and posture; A* pathfinding on the theatre H3 grid (resolution 7). Rivers and roads are later refinements.
- **Satellites:** SGP4 from TLE (CelesTrak) or orbital elements authored in content.

## 6. Fuel and logistics

- **Burn model** per platform: kg/h at cruise, military and afterburner, scaled by altitude band.
- **Bingo** = fuel to reach the nearest suitable friendly base (runway class, capacity) at cruise + reserve (content, default 10%). On bingo the unit returns automatically unless a tanker on the same side is reachable on its track; then it refuels (rate from content) and resumes.
- **Winchester** (weapons gone) → return to base, per task setting.
- **Turnaround** at base: refuel + rearm time from content (e.g. fighter 45–90 min); munitions drawn from base stock.
- **Fixed inventory:** no production; losses are permanent; base stocks come from country inventory at war start.

## 7. Sensors

Each sensor instance has a content type (detection parameters) and a mounting (height, field of regard). For every candidate pair the engine checks, in order: **domain and altitude limits → max range → radio/visual horizon → terrain line of sight → detection probability**.

### 7.1 Radar

```
σ_eff   = σ(aspect) × m_stores                 # RCS interpolated front/side/rear/top; external stores raise it
R50     = R_ref × (σ_eff / σ_ref)^(1/4)         # R_ref = 50% detection range vs σ_ref (usually 1 m²)
R50'    = R50 × f_clutter × f_jam              # §9 for jamming
horizon = 4.12 × (√h_radar_m + √h_target_m) km
Pd      = 1 / (1 + (d / R50')^k)               # per scan, k from realism profile (default 8)
track   = 2 detections in last 3 scans; dropped after 3 misses
```

- **Look-down / clutter:** targets below 300 m over land or 50 m over sea get `f_clutter` from the radar's clutter rejection (0–1).
- **Doppler notch:** pulse-Doppler radars lose targets whose radial speed is < 25 m/s in look-down (beaming): Pd × 0.2.
- **Terrain line of sight:** DEM sampled along the great circle at `max(DEM cell, d/64)`; blocked if terrain + Earth bulge rises above the ray.
- Typical outcome the model must reproduce: a frigate radar at 30 m height first sees a sea-skimmer at 5 m at **~32 km**, whatever its nominal range.

### 7.2 Passive sensors

| Sensor | Model |
|---|---|
| ESM / ELINT | Detects *emitting* radars: `R = R_ref × √(ERP / ERP_ref)` (one-way path), capped by horizon/LOS. Bearing only (σ 1–3°). A position needs two bearings (two platforms, or one platform over time) → large ellipse. Emitter library maps radar type → system type, which is how "SITE S-400 (our estimate)" appears |
| IRST | `R = R_ref × √(I / I_ref)`, aspect (rear hot, afterburner ×3), works day and night |
| EO/IR camera (drones, aircraft pods) | Detection by size; classification by ground sample distance (< 1 m → type and count, < 3 m → class); EO needs daylight, IR works at night |
| Visual / recon teams | Range by terrain (open 4 km, forest 0.5 km, urban 0.3 km) × target size × night factor (×0.3 without night vision) |
| Sonar (simplified) | Passive range from target noise at its speed; active range from target strength; a thermal layer comes later |

### 7.3 Satellites

- Orbit by SGP4; footprint = swath along the ground track.
- **EO imaging:** sun elevation at target > 10°, clear sky. **SAR:** day or night. **SIGINT:** emitters in footprint.
- Output is an **intel report** delivered after a downlink delay (content, e.g. 15 min). Contacts from it are time-stamped and age like any other.
- The player can **task** a satellite on an area; the engine computes the next pass and schedules the report.

## 8. Fog of war: contacts, fusion, sharing

Each side has a **picture** (COP) of contacts. The UI and the AI read only pictures.

```text
Contact {
  id: ContactId              // per side; never equal to an entity id
  truth: EntityId            // engine-internal; never leaves the engine
  est_pos, est_vel, alt_band
  ellipse { a_m, b_m, rot }  // grows with age: σ(t) = σ0 + v_max(class) × age
  level: Unknown | Domain | Class | Type | Precise
  domain, class, type_id?, count: Range?
  identity: Unknown | Hostile | Neutral | Friendly
  quality: FireControl | Track | Report
  sources[], first_seen, last_update
}
```

- **Association** uses ground truth internally (cheap and deterministic) with modelled imperfections: a group shows as **one contact with an unknown count** until a sensor resolves it; decoys and jammers create their own contacts; errors and lag come from the sensor.
- **Classification** rises with better sensors: search radar → domain, speed, altitude; fire-control radar or several looks → class; ESM emitter match or imagery → type; imagery or recon → precise count.
- **Ageing:** contacts not refreshed become *Report* quality, then drop to a *last known* marker (air 2 min, surface 30 min, mobile ground 2 h, fixed sites kept).
- **Data links:** each side has networks (e.g. "Link 16", "RU-TKS-2", none). A detection reaches units on the same network after its latency (default 2 s) and the rest of the side via C2 reports (default 60 s, lower quality).
- **Engage on remote:** a shooter may fire on a networked track only if both the network and the weapon support it (content flag). Otherwise it needs its own fire-control track.

## 9. Electronic warfare

| Effect | Model |
|---|---|
| **Self-screening jamming** | Burn-through range `R_bt = R50² / 10^(J_eff_dB/20)` (km). Detection range becomes `min(R50, R_bt)` while the jammer is on and in band |
| **Stand-off / escort jamming** | `R_eff = R50 × (1 + 10^(J_eff_dB/10) × G_sl / R_j²)^(-1/4)`; `R_j` = jammer–radar distance (km); `G_sl` sidelobe gain (−30 dB default, 0 dB in main beam). Several jammers add |
| **Effective jamming power** | `J_eff_dB = J_dB − ECCM_dB(radar) − band_mismatch_dB` |
| **Comms jamming** | Raises network latency and drops links in the jammed area; affected units fall back to their own sensors |
| **GPS jamming** | GPS-guided weapons in the area: CEP × factor unless the weapon has anti-jam |
| **Seeker jamming** | Reduces endgame Pk (§11) by `jam_effect × (1 − seeker_ECCM)` |
| **Decoys** | Chaff vs radar seekers, flares vs IR, towed and air-launched decoys create false contacts that draw interceptors |
| **EMCON** | Off = invisible to ESM but blind on own radar. The core trade-off for SAM sites and ships |
| **Anti-radiation missiles** | Home on emitters; if the radar shuts down they continue in memory mode with reduced Pk |

The constants (`k`, sidelobe default, latency defaults) live in the realism profile; golden scenarios pin their behaviour.

## 10. Weapons in flight

Every missile, bomb, shell salvo, torpedo and decoy in flight is an entity.

- **Energy profile:** boost (time, acceleration) → sustain (speed at profile altitude) → coast `V(t) = V_burnout × e^(−Δt/τ(alt))` until `V < V_min` (out of energy). Air-breathing cruise missiles keep speed until fuel (range) runs out. Manoeuvre costs energy in proportion to lateral acceleration.
- **Calibration rule:** fired at its reference altitude and speed against a non-manoeuvring head-on target, a weapon must reach its published range ± 10%. Ranges against crossing or beaming targets fall out of the model.
- **Altitude profiles:** sea-skimming (5–15 m), high–low, ballistic with apogee, quasi-ballistic, glide (bombs: range from release altitude and speed).
- **Guidance:** proportional navigation (N 3–4) limited by max G. Before seeker activation the weapon flies on shooter or data-link updates (if the shooter still tracks the target), otherwise to the predicted point. Semi-active weapons need the shooter's illuminator until impact (it occupies a fire channel). IR weapons need lock. Anti-radiation weapons need an emitter. GPS/INS weapons fly to coordinates with CEP.
- **Retargeting:** if the target dies first, an active seeker may acquire another valid target in its field of view (e.g. the next ship); otherwise it self-destructs.

## 11. Endgame

At closest approach within the tick:

```
Pk = clamp( Pk_base[weapon, target_class]
          × f_energy     # √((V − V_min)/(V_nom − V_min)), 0..1
          × f_aspect     # head-on/tail/beam table per seeker
          × f_maneuver   # aware target: min(1, G_missile / (3 × G_target)), floor 0.2
          × f_cm         # Π (1 − cm_effect × (1 − seeker_resistance)) for chaff/flares/decoys used
          × f_ew         # seeker jamming
          × f_env        # clutter, terrain, night/weather for EO/IR
          , 0, 0.99)
hit  ⇔  rng(seed, ENDGAME, weapon_id, tick) < Pk
```

Outcomes: `Hit`, `Miss`, `Decoyed`, `Jammed`, `OutOfEnergy`, `Intercepted`, `LostTrack`, `SelfDestruct`. The event stores every factor, so a report can say "missed — out of energy (f_energy 0.11), target beaming".

Bombs and other unguided or GPS weapons use a CEP roll (Rayleigh) against the target's lethal radius instead of `Pk_base`.

## 12. Defence: threat evaluation and weapon assignment (TEWA)

Runs every second and immediately on a new threat, for every unit or IADS cell allowed by ROE.

1. **Threat buckets, in order:** weapons aimed at *me* → weapons aimed at defended assets (high-value list) → weapons aimed at other friendlies in my coverage → hostile shooters (only if ROE allows).
2. Within a bucket, sort by time to impact, then threat lethality.
3. For each threat, candidate interceptors must reach the predicted intercept point inside their envelope (min/max range and altitude, crossing penalty), have a free fire channel, and have magazine above the reserve (the reserve is ignored for self-defence).
4. Choose the best Pk per cost. **Salvo doctrine:** shoot-look-shoot when time to impact allows two shots plus assessment; otherwise shoot-shoot (two interceptors) for supersonic, ballistic and hypersonic threats, one for subsonic.
5. **Layers:** long-range SAM → medium → short → CIWS (last 2–4 km) → soft kill (chaff, decoys, jamming) for radar-homing threats inside ~30 s.
6. **Fire channels:** each fire-control radar or illuminator handles N simultaneous engagements (content), which is what saturation attacks exploit.
7. **IADS:** SAM sites, radars and C2 nodes share tracks over the IADS network. Destroying the C2 node drops sites into autonomous mode (own radar only, slower reaction).

## 13. Damage

| Target | Model |
|---|---|
| Aircraft | Hit → destroyed with `p_kill_on_hit` (fighters ~0.8), else damaged → mission abort, return to base |
| Ships | Damage points from content (scaled by displacement). Warhead damage = kg × type multiplier. Hit location roll can knock out radar, VLS, propulsion, CIWS. ≤ 25% → out of action; ≤ 0 → sinks after a delay |
| Base components | Runway, shelters, fuel, munitions store, radar, C2, each with hardness; damaged components have repair times (runway 4–12 h) |
| Ground formations | Equipment and personnel counts; strikes remove vehicles by weapon lethality and posture (a moving column is more exposed than a dug-in unit) |
| Satellites | Not targetable in v1 |

## 14. Ground combat (Phase 11)

- Formations, not individual soldiers: e.g. "Tank battalion — 31 × T-90M, 120 personnel".
- **Combat value** = Σ equipment value × readiness × modifiers: terrain (urban, forest, mountain favour the defender), posture (attack / hasty / prepared defence), supply, morale, fatigue, night capability, artillery and air support, EW on C2.
- **Resolution** every 60 s for formations in contact (adjacent H3 cells or within direct-fire range): Lanchester-style attrition with bounded random variation → losses, retreat, breakthrough.
- **Special forces and recon:** small, hard to detect, high classification quality, sabotage of base components, laser designation for strikes.
- **Airborne and marines:** insertion by airlift or landing ship (later phase).

## 15. Events

Every meaningful change is appended to the event log: detections and classification upgrades, intel reports, launches (with salvo id), every weapon termination with outcome and factors, intercepts, damage, losses, base damage, task and phase changes, ROE requests and approvals, border crossings, EMCON and jammer changes, refuelling, bingo returns. Each event carries tick, sim time, and a **visibility mask** (which sides know it, and from when). Reports and replays are built only from this log (see doc 03 §12 and doc 04 §7).

## 16. Performance budget

Target: 2,000 units, 300 weapons in flight, ×10 → 100 ticks per real second → **≤ 6 ms per tick average** on a mid-range desktop, leaving headroom.

| Work per 100 ms tick | Budget |
|---|---|
| Movement + fuel | 0.5 ms |
| Weapons in flight + endgame | 0.6 ms |
| Sensors (staggered scans, spatial index, LOS cache) | 2.5 ms |
| Fusion + data links | 0.5 ms |
| TEWA + ROE | 0.5 ms |
| AI (amortised) | 0.6 ms |
| Logistics, operations, events | 0.3 ms |
| Side view build (10 Hz real time, amortised) | 0.3 ms |

Techniques: spatial index on H3 resolution 3 buckets rebuilt per scan wave; candidate pruning by max range and horizon before LOS; LOS memoised by (sensor cell, target cell, altitude bands) — a deterministic approximation; staggered scans; adaptive 1 s ticks when nothing is engaging. CI publishes a benchmark scenario's per-system timings on every merge.

## 17. Calibration

- All tuning constants live in `content/realism/default.yaml` (Pd curve steepness, track rules, contact timeouts, jamming defaults, CEP distribution, ground combat coefficients).
- **Golden scenarios** (doc 08 §3) pin expected outcome bands, e.g. "12 × JSM vs 2 × FREMM: 60–95% of missiles intercepted or decoyed". A change that moves a golden result out of band fails CI and needs an explicit decision.
- The admin app's **test bench** runs Monte Carlo engagements (e.g. 1,000 × AIM-120D vs Su-35S at 60 km, beaming) so OSINT values can be sanity-checked before they ship.
