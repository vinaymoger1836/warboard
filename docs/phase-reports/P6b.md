# P6b — Electronic warfare as game stats: phase report

Date: 7 Nov 2026. Status: **done locally; all tests and golden scenarios pass**.

## What was built

| Area | Result |
|---|---|
| Schema & Realism (ADR-034) | Radar `jam_resistance` (0–10 integer), `ew_suite` modes with `jam_strength` (0–10 integer), `coverage_km`, `sector_deg`, and `effects` (`radar`, `datalink`, `navigation`). Weapons: `seeker_resistance` (0.0–1.0 float), `nav_resistant` (bool). Legacy fields `eccm_db`, `seeker_eccm` and `j_db` migrated cleanly with migration tests in `crates/wb-content/tests/fixtures.rs`. Realism constants added to `default.yaml` (`m_table`, `self_protect_r50_min_fraction: 0.35`, `off_axis_attenuation: 0.30`, `datalink_latency_factor: 3.0`, `datalink_loss_fraction: 0.30`, `nav_jam_cep_factor: 4.0`, `arm_memory_pk_factor: 0.45`, `seeker_jam: 0.25`, `emcon_blink_on_s: 10`, `emcon_blink_period_s: 60`). |
| Radar Jamming Effects | Implemented detection-range multiplier $m(a)$ as a function of electronic advantage $a = \text{jam\_strength} - \text{jam\_resistance}$. Self-protection jamming is limited to close-in limit 0.35. Escort and stand-off jamming check coverage, sector geometry, and off-axis attenuation (0.30). Multiple jammers compound multiplicatively. |
| Datalink & Navigation Disruption | Observer platforms covered by active enemy datalink jammers experience 3× latency and a 30% drop roll via RNG purpose `DatalinkLoss` (purpose 14). Non-nav-resistant weapons under navigation jamming have CEP multiplied by 4 and record $f_{ew} = 0.25$. |
| Seeker Jamming & Endgame | Guided weapons engaging targets covered by active radar jammers suffer Pk reduction $f_{ew} = 1.0 - \text{seeker\_jam} \times (1.0 - \text{seeker\_resistance})$. Endgames under jamming record $f_{ew}$ in `EventKind::WeaponTerminated` and attribute `Outcome::Jammed` when EW caused the miss. `SalvoReport` tracks jammed rounds. |
| Emitter-Homing & ARM Memory Mode | Emitter-homing weapons (`terminal: anti_radiation`) home on emitting radars. When the targeted radar shuts down (EMCON `off` or `blink`), the missile continues on inertial memory to the last known position, reducing Pk by `arm_memory_pk_factor` (0.45) and recording $f_{env} = 0.45$. |
| Orders & Tasks | Implemented `Task::Jam` (stand-off orbit circle) and `Task::Suppress` (SEAD strike against emitter contacts or points). Added `Command::SetJammer { units, on, mode }` and support for `SetEmcon` (`on`, `off`, `blink`). |
| Fixtures (ADR-035) | Generic fixtures added in `scenarios/fixtures/`: `test-jammer` suites (strengths 3, 5, 7, 9), `test-jammer-aircraft` platforms, `test-fighter` with `test-fighter-radar` (ground mapping + fire control), `test-arm` anti-radiation missile, and `test-fighter-sead` loadout. |
| UI & Visuals | Unit card displays active jammer status with On/Standby toggle and mode picker (Self-Protect, Escort, Stand-off). Map renders jammer coverage sector polygon overlay. Jammed radar contacts display `[JAMMED]` badge and affected-radar ring; jammed own units show `[JAMMED]`. All styling strictly uses tokens from `packages/symbols/src/tokens.ts`. |

## Acceptance results (local: Windows 11, Rust 1.99.1 release, Node 24)

| Criterion | Result |
|---|---|
| **G11** Stand-off jamming against SAM radar at advantage −2, 0, +2, +4 (500 seeds) | **Pass**: first detection range ratio matches $m(a)$ table: adv −2 = **0.90** (band 0.85..0.95), adv 0 = **0.74** (band 0.70..0.80), adv +2 = **0.58** (band 0.55..0.65), adv +4 = **0.43** (band 0.40..0.50). |
| **G12** ARM memory mode when radar shuts down (500 seeds) | **Pass**: $Pk(\text{radar\_on}) = \mathbf{0.72}$ (band 0.68..0.78), $Pk(\text{radar\_shutdown}) = \mathbf{0.34}$ (band 0.28..0.38), ratio = $\mathbf{0.47}$ (band 0.40..0.50, theoretical 0.45). |
| **G1–G10** Golden scenario suite | **Pass**: all 14 golden scenarios (G1, G2, G3, G4, G5, G6, G7, G8, G9, G10, G10-Priority, G10-SAM-Autonomous, G11, G12) pass. |
| **f_ew recording** | **Pass**: `crates/wb-sim/tests/ew.rs` verifies that every endgame under jamming records `f_ew < 1.0` (specifically 0.875 matching $1 - 0.25 \times 0.5$) in `WeaponTerminated`. |
| `cargo check --workspace` & `cargo test --workspace` | **Pass**. |
| `pnpm -r typecheck` | **Pass** across all packages. |

## Decisions and deviations

- **Integer EW stats:** Adopted 0–10 integer scale for radar `jam_resistance` and EW suite `jam_strength` as defined in doc 02 §9 and ADR-034.
- **RNG Purpose addition:** Appended `DatalinkLoss = 14` to `RngPurpose` to preserve determinism for datalink packet drop rolls without perturbing existing RNG streams.
- **Golden G12 Order Configuration:** Golden scenario G12 utilizes `TaskOrder::Suppress` with `point` targeting the SAM radar site, allowing the fighter to launch `test-arm` within missile parameters and exercise ARM memory mode upon radar shutdown.
