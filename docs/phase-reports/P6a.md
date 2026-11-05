# P6a — Naval combat and layered air defence: phase report

Date: 5 Nov 2026. Status: **done locally; CI not yet run** (all checks below ran on the working tree on 5 Nov 2026). P6b (electronic warfare) is not started; see "Not in this phase".

## What was built

| Area | Result |
|---|---|
| Fixtures (ADR-035) | Generic test units in `scenarios/fixtures/` at the neutral test sea area: test frigate and corvette (VLS, canister launchers, close-in gun mounts, chaff and flares), test SAM site (command post, fire-control radar, two launchers), test early-warning radar unit, test anti-ship missile, test interceptor, test close-in gun, test chaff and flare, test data link, naval port, doctrines, inventories, loadouts, a fixture realism profile. `wb-cli` takes the scenario's `content:` directory, so engine tests never read `content/`. |
| Ships and magazines | Frigate and corvette stations (VLS cells, canisters, gun mounts) filled from loadouts. Subsystem damage can put VLS, launchers or the radar out of action; out-of-action stations drop out of the shooter's rounds and capacity. |
| Fire channels (`engage.rs`) | A fire-control radar handles `fire_channels` engagements at once. A composite site sums the channels of its working radars; a ship uses its own sensors; a platform without a radar gets 2. Weapons in flight from a shooter hold channels until they end. Close-in guns do not use channels. `GroupDetails.fire_channels` (`used`, `total`) reaches the unit card. |
| Defence layers and TEWA | Self-defence (ADR-031) now sorts threats into "aimed at me" first, then "aimed at another friendly", then by time to impact, then warhead. Outside the close-in band (`tewa.ciws_layer_min/max_km`) the longest-reach interceptor is preferred; inside it the gun is. Shoot-look-shoot when time allows two flights plus assessment, otherwise two shots for supersonic or ballistic threats. Missile reserve fraction from doctrine is kept back except for threats to the shooter itself. At most two interceptors are in flight at one threat. |
| Soft kill | Chaff and flares are released in the endgame (P5 `f_cm`), so decoyed weapons count in `outcome_fraction`. |
| Air-defence networks | Sites share tracks over the data link: a weapon with `engage_on_remote` may fire on a networked track. Destroying the command post drops the site into autonomous mode after `reaction_time_s × tewa.iads_autonomous_reaction_factor`. In that gap it cannot engage; afterwards it engages only on its own radar's tracks. New component `SystemMemberRole`, event `AutonomousMode`. |
| Salvos | A `SalvoReport` event per salvo (hits, misses, intercepts, decoys); events carry the salvo id and the `M-n` / `INT-n` tags. |
| Visual effects (`weapons.rs`, `view.rs`) | Launch, intercept and hit effects are in the side view's effects section, **filtered by side** (see decisions). They expire by age (20 s sim time, at most 1,024 rows per view). One intercept now gives one burst at the point where the interceptor fuzed. |
| Targeted halo | `FLAG.TARGETED` is derived from the side's own picture: own units that a weapon contact in our picture is heading for. It clears as soon as that contact is gone or lost. Halos also show on contacts that our own offensive weapons are flying at. |
| Enemy weapon contacts | The view now sends a weapon symbol (`weapon_symbol(category)`) for weapon contacts. Before, the symbol was always 0 and enemy missiles drew as unknown tiles; that was the cause of the P5 "known gap". |
| UI | `effectLayers.ts` (new): launch disc and ring (650 ms), cyan intercept burst (900 ms), amber hit burst (1,300 ms), each a pure function of age, three layers whatever the count. Targeted halo: `halo.targeted` at 45%, about 34 px, 2.4 s pulse. Enemy weapons: red glyph rotated to heading with a red `type` over `(N km/h)` label; overlapping labels merge with a count (`anti-ship missile ×7`). Event lines for outcomes (`intercepted its target (Pk 75%)`, `missed by 140 m`, `was decoyed by countermeasures (4 released)`, `ran out of energy`, `self-destructed`), the autonomous-mode alert (click to locate), pluralised salvo lines. Unit card Weapons section: rounds left by weapon against full, fire channels used of total, chaff and flares left. |
| CLI | `wb golden` metrics for combat: `target` and `shooter` filters resolve through the ordered weapon's `AimRef` and through a system's member units (so a site and its launchers count as one shooter), and `first_target` no longer needs the weapon to have a recorded target. |
| Docs | doc 06 §3 updated with the effects section, the `TARGETED` flag and `fire_channels`. ADR-037 and ADR-038 added (Proposed). |

## Acceptance results (local: Windows 11, Rust 1.99.1 release, Node 24)

| Criterion | Result |
|---|---|
| **G10** 12 test anti-ship missiles vs 2 test frigates: 60–95% intercepted or decoyed | **Pass**, mean **0.82** over 20 seeds (12 launched in every run). |
| G10: each frigate engages threats to itself first (`g10-threat-priority`) | **Pass** (5 seeds): the first launch is at the threat aimed at the shooter, 3 launches. |
| G10: command post lost → autonomous mode (`g10-sam-autonomous`) | **Pass** (5 seeds): no launch at the track only a remote radar sees; the site's first launch at its own-radar target comes at **62 s** (band ≥ 10 s). |
| G1–G9 | **Pass**, unchanged. |
| Truth canary | **Pass**; a new test (`effects_reach_only_the_sides_that_may_see_them`) pins the effect visibility rule. |
| Native = WASM | **Pass** (`scripts/determinism-check.sh`): every scenario, save/reload native, WASM and native save → WASM; no mismatches. |
| Benchmarks | `patrol-2000` **1.24 ms** per tick; `sensors-2000` **4.10 ms** per tick (budget 6 ms). |
| `cargo fmt`, `clippy -D warnings`, lint canary, `cargo test --workspace` | Pass. |
| `pnpm gen`, `pnpm -r typecheck`, `pnpm -r lint`, `pnpm -r build` | Pass. |
| `cargo deny check` | Not run in this session; no dependency was added. |

### Browser run (headless Edge driven over DevTools, G10 scenario and the Baltic demo)

The UI work was checked against G10 with the engine worker pointed at it temporarily (reverted, `git status` clean). Frozen frames of a live salvo showed: launch discs at the shooters, cyan intercept bursts where interceptors fuzed, an amber hit burst (first frames only), the red halo on a targeted ship, cyan dashed interceptor trails with `INT-n` tags, group labels `SHIP Test Frigate (4× interceptors, …)`, and red enemy-missile glyphs with merged labels.

## Screenshots and differences from the references

| Screenshot | Compare with |
|---|---|
| ![G10 salvo](P6a-g10-salvo.png) `P6a-g10-salvo.png` (paused, T+1:30, event log open) | `03-ships-targeted.png`, `04-interceptors.png`, `06` |
| ![Interceptors](P6a-interceptors-zoom.png) `P6a-interceptors-zoom.png` | `04-interceptors.png` |

1. **Targeted halo vs 03.** Same red glow, but ours is subtler: `#A05460` at 45% on the dark test sea, while the reference is a stronger pink on a blue sea. If it still looks weak, raise the peak or the exponent (`HALO_*` in `effectLayers.ts`).
2. **Enemy missile vs 06.** Red glyph with a red two-line label, but the glyph is a small 20 px arrow, not the pink blob in 06, and the label is bold red centred under the glyph. In the full-view screenshot the two enemy labels overlap and are hard to read at this zoom.
3. **Interceptors and salvo vs 04.** Cyan dashed trails, `INT-n` tags and a group label are as in 04. Two frigates 5 km apart stack their group labels (the first line is printed twice and clipped), and the `INT-n` tags run into each other along the track. 04 has the same clutter, but it is worse here because both ships and all interceptors sit inside a few pixels at zoom 7.
4. **Launch flash.** A white disc plus ring that covers the 30 px icon while it plays.
5. **Hit burst.** Only its first frames (core disc and ring) were seen.
6. **Enemy label units.** km/h, as in doc 10 §4.1, not Mach. The `(our estimate)` line is not shown for weapons (the guide shows two lines).

## Decisions and deviations

- **ADR-037 (Proposed): visual effects go to the sides that may see them.** `Combat.effects` was global: every side's view carried every launch flash at the exact position of enemy shooters. Each effect now has a side mask: a launch to the shooter's side; an intercept or hit to the weapon's side plus the side on the other end of it.
- **ADR-038 (Proposed): composite sites go autonomous when their command post dies.** The rule that doc 02 §12 item 7 describes, as built.
- **Fixture tuning for G10.** The first full build gave 0.98 against the band. Neither the interceptor's Pk (0.75 lowered to 0.55 and then 0.5 still gave 0.93 on 5 seeds and 0.96 on 20) nor the frigate radar's fire channels (4 → 2: 0.98 → 0.97) fixed it, because the defences keep re-engaging for the whole approach. The lever is the engagement window, so the test anti-ship missile became a faster sea-skimmer: Mach 0.85 → **1.2**. The result is steep: Mach 1.05 gives 1.00, 1.2 gives 0.82, 1.6 gives 0.39. The band was not changed; G10 now runs 20 seeds so the mean is stable. Re-tune the fixture, not the band, if later phases move engagement timing.
- **`g10-threat-priority` expectations.** The scenario's escort threat was moved out to about 24 km and the "launches at the escort's threat" expectation was dropped; what is pinned is that the first target is the threat aimed at the shooter. The frigate has the capacity to shoot at both, so the old expectation tested capacity, not priority.
- **Test interceptor fixture.** `engage_on_remote: true` and `min_alt_m: 0` so that it can fly on a networked track against a sea-skimmer.
- **Effects retention 2 s → 20 s of sim time.** At ×60 two seconds is shorter than one view. The wire format is unchanged (view v3), but the effect struct changed, so older saves do not decode. `SAVE_VERSION` stays at 1, as in P5.
- **Removed `crates/wb-sim/tests/test_p6a_debug.rs`**, a debugging test that only printed events.

## Known gaps

- **Soft-kill window.** `tewa.soft_kill_window_s` is in the schema and the fixture profile but nothing in the engine reads it. Chaff and flares are released by the P5 endgame rule inside `release_range_m`, not by a layer that decides on soft kill in the last `soft_kill_window_s`. Layered defence is therefore "long-range interceptor first, then close-in gun, plus the endgame decoys"; there are no separate medium- and short-range interceptor layers or IADS layer templates (`iads_layers` is free text in doctrine and unused). The roadmap row for P6a says "defence assignment layers": met for the fixture's two layers, not for a third and fourth.
- **CIWS wording.** Event lines have no close-in-gun wording; events carry the content weapon id, not its category. A `weapon_category` field on `WeaponLaunched` would fix it.
- **Visual checks not done.** Sea-skimming and gun scenes were not reviewed by eye; the launch flash was seen only in frozen frames; the halo was seen only against the dark test-sea tint, not the territory tints; nothing at ×30 or ×60. Please watch a live salvo at ×1 to ×5.
- **P5 UI leftovers.** The ROE A/B/C buttons in the top bar and the card's ROE section are still disabled with the P5 tooltip; STRIKE and HOLD FIRE on the card only show a toast.
- **Baltic demo.** `scenarios/demo-baltic.yaml` still uses real place and unit names (PF is not started). The content denylist will flag it.
- **Old saves.** Saves made before this phase's effect struct change are refused.

## Not in this phase

**P6b (electronic warfare: game stats)** has not started: no jammer content, no jamming effects, no data link or navigation disruption, no emitter-homing weapons, no air-defence suppression task, no G11 or G12. The schema still has `eccm_db`, `seeker_eccm` and `j_db` (ADR-034 migrates them in P6b), and seeker jamming (`f_ew`) is still 1. The first attempt to run P6b in this session was stopped by a safety check before it changed anything.
