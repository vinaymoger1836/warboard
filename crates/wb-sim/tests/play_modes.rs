//! Play modes (doc 12, ADR-044), P9 acceptance: cross-side orders are
//! rejected in every mode, the truth canary holds for **both** sides in
//! Command both and Hot-seat, the umpire view is refused outside Command
//! both, the session never touches the state hash (G16), and a save keeps
//! the session. Runs on the generic fixtures (ADR-035).

use std::sync::Arc;
use wb_core::UnitId;
use wb_core::geo::LatLon;
use wb_sim::components::{Allegiance, BaseSite, Identity, Kinematics};
use wb_sim::orders::{Command, Order, RejectKind, TaskOrder};
use wb_sim::picture::SidePictures;
use wb_sim::session::{HandOver, HotSeat, Urgent};
use wb_sim::view::ViewEncoder;
use wb_sim::world::{UnitIdAlloc, UnitIndex};
use wb_sim::{Sim, details, satellites};

const TAG: &str = "CANARY-5d2b";

/// Two sides that find each other: both frigate/corvette radars see the
/// other ship, so both pictures fill up. Hidden units far away must never
/// be seen. Bases come from the fixtures' garrisoned start.
fn scenario(mode: &str, blue: &str, red: &str) -> String {
    format!(
        "
name: play modes
seed: 91
start_utc: 2026-10-27T20:00:00Z
mode: {mode}
sides:
  - {{ name: Blue, nations: [tst], player: true, controller: {blue} }}
  - {{ name: Red, nations: [opp], controller: {red} }}
units:
  - {{ id: 10, name: BLUE-{TAG}-SHIP, platform: test-frigate, nation: tst, lat_deg: 55.2, lon_deg: 17.045678, heading_deg: 90 }}
  - {{ id: 12, name: BLUE-{TAG}-FLIGHT, platform: test-fighter, nation: tst, count: 2, lat_deg: 55.376543, lon_deg: 16.345679, alt_m: 7000, heading_deg: 90 }}
  - {{ id: 500, name: RED-{TAG}-SHIP, platform: test-corvette, nation: opp, lat_deg: 55.234567, lon_deg: 18.123457, heading_deg: 270 }}
  - {{ id: 600, name: RED-{TAG}-HIDDEN, platform: test-corvette, nation: opp, lat_deg: 40.765432, lon_deg: 3.987654 }}
  - {{ id: 610, name: BLUE-{TAG}-HIDDEN, platform: test-corvette, nation: tst, lat_deg: 38.123457, lon_deg: 2.345679 }}
orders:
  - {{ tick: 0, side: 0, type: AssignTask, units: [10], task: {{ kind: move, waypoints: [[18.487654, 55.2]], speed: max }} }}
  - {{ tick: 0, side: 1, type: AssignTask, units: [500], task: {{ kind: move, waypoints: [[16.912345, 55.234567]] }} }}
"
    )
}

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn build(mode: &str, blue: &str, red: &str) -> Sim {
    let (db, hash) = content();
    Sim::from_yaml(&scenario(mode, blue, red), db, hash, None).expect("scenario builds")
}

/// Every entity (unit, group, base) of a side, with its name and position.
fn of_side(sim: &Sim, side: u8) -> Vec<(u32, String, Option<LatLon>)> {
    let w = sim.world();
    w.resource::<UnitIndex>()
        .0
        .iter()
        .filter(|(_, e)| w.get::<Allegiance>(**e).map(|a| a.side) == Some(side))
        .map(|(&id, &e)| {
            let pos = w
                .get::<Kinematics>(e)
                .map(|k| k.pos())
                .or_else(|| w.get::<BaseSite>(e).map(|b| b.at));
            (
                id.0,
                w.get::<Identity>(e)
                    .map_or(String::new(), |i| i.name.clone()),
                pos,
            )
        })
        .collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Pins the truth canary for one viewer: nothing of the other side's names,
/// ids or exact coordinates in the view, details, contact details, ground
/// tracks, approvals or events. Returns how many contacts the viewer holds.
fn check_viewer(sim: &Sim, enc: &mut ViewEncoder, viewer: u8, seen_events: &mut usize) -> usize {
    let other = 1 - viewer;
    let hidden = of_side(sim, other);
    let names: Vec<&str> = hidden
        .iter()
        .map(|(_, n, _)| n.as_str())
        .filter(|n| !n.is_empty())
        .collect();
    let mut exact: Vec<(Vec<Vec<u8>>, Vec<String>)> = Vec::new();
    for (_, _, pos) in &hidden {
        if let Some(p) = pos {
            let (lat, lon) = (p.lat_deg(), p.lon_deg());
            let bytes = vec![lat.to_le_bytes().to_vec(), lon.to_le_bytes().to_vec()];
            let strings = (6..=8)
                .flat_map(|d| [format!("{lat:.d$}"), format!("{lon:.d$}")])
                .collect();
            exact.push((bytes, strings));
        }
    }

    let view = enc.encode(sim.world(), viewer).to_vec();
    for n in &names {
        assert!(
            !contains(&view, n.as_bytes()),
            "side {viewer}: `{n}` in the view"
        );
    }
    for (bytes, _) in &exact {
        for b in bytes {
            assert!(
                !contains(&view, b),
                "side {viewer}: an exact coordinate in the view"
            );
        }
    }

    let mut texts: Vec<String> = Vec::new();
    let next = sim.world().resource::<UnitIdAlloc>().next;
    let mut ids: Vec<u32> = (0..2_000).collect();
    ids.extend(1_000_000..next + 5);
    let other_ids: Vec<u32> = hidden.iter().map(|(i, _, _)| *i).collect();
    for id in ids {
        match details::details(sim.world(), viewer, id) {
            Ok(d) => {
                assert!(!other_ids.contains(&id), "side {viewer}: details of {id}");
                texts.push(format!("{d:?}"));
            }
            Err(e) => assert_eq!(e, format!("Unknown unit {id}"), "rejection reveals nothing"),
        }
        if let Ok(t) = satellites::ground_track(sim.world(), viewer, id, 10.0, 10.0, 60.0) {
            texts.push(format!("{t:?}"));
        }
    }
    for id in 0..64 {
        match details::contact_details(sim.world(), viewer, id) {
            Ok(c) => texts.push(serde_json::to_string(&c).expect("json")),
            Err(e) => assert_eq!(e, format!("Unknown contact {id}")),
        }
    }
    texts.push(serde_json::to_string(&wb_sim::roe::approvals(sim.world(), viewer)).expect("json"));
    let log = sim.world().resource::<wb_sim::events::EventLog>();
    for e in log.visible(viewer, *seen_events) {
        texts.push(serde_json::to_string(e).expect("json"));
    }
    *seen_events = log.0.len();
    for t in &texts {
        for n in &names {
            assert!(!t.contains(n), "side {viewer}: `{n}` leaked: {t}");
        }
        for (_, strings) in &exact {
            for s in strings {
                assert!(
                    !t.contains(s.as_str()),
                    "side {viewer}: coordinate {s} leaked: {t}"
                );
            }
        }
    }

    let pics = sim.world().resource::<SidePictures>();
    for c in pics.sides[usize::from(viewer)].contacts.values() {
        if let Some(e) = c.est {
            for (_, _, pos) in &hidden {
                assert!(
                    pos.is_none_or(|p| e.at != p),
                    "side {viewer}: a contact estimate equals the truth (ADR-026)"
                );
            }
        }
    }
    let hidden_ids = [600u32, 610];
    for h in hidden_ids {
        assert!(
            !pics.sides[usize::from(viewer)]
                .by_truth
                .contains_key(&UnitId(h)),
            "the far-away unit {h} was detected"
        );
    }
    pics.sides[usize::from(viewer)].contacts.len()
}

fn canary_both_sides(mode: &str) {
    let mut sim = build(mode, "human", "human");
    let mut enc = ViewEncoder::new();
    let mut seen = [0usize; 2];
    let mut contacts = [0usize; 2];
    for _ in 0..60 {
        sim.run_ticks(75);
        for viewer in 0..2u8 {
            contacts[usize::from(viewer)] = contacts[usize::from(viewer)].max(check_viewer(
                &sim,
                &mut enc,
                viewer,
                &mut seen[usize::from(viewer)],
            ));
        }
    }
    assert!(contacts[0] > 0, "{mode}: Blue never detected Red");
    assert!(contacts[1] > 0, "{mode}: Red never detected Blue");
}

#[test]
fn truth_canary_holds_for_both_sides_in_command_both() {
    canary_both_sides("command_both");
}

#[test]
fn truth_canary_holds_for_both_sides_in_hot_seat() {
    canary_both_sides("hot_seat");
}

// ---------------------------------------------------------------------------

fn modes() -> [(&'static str, &'static str, &'static str); 4] {
    [
        ("vs_ai", "human", "ai"),
        ("command_both", "human", "human"),
        ("hot_seat", "human", "human"),
        ("watch", "ai", "ai"),
    ]
}

fn first_of(sim: &Sim, side: u8, base: bool) -> u32 {
    let w = sim.world();
    w.resource::<UnitIndex>()
        .0
        .iter()
        .find(|(_, e)| {
            w.get::<Allegiance>(**e).map(|a| a.side) == Some(side)
                && w.get::<BaseSite>(**e).is_some() == base
                && (base || w.get::<wb_sim::components::Group>(**e).is_some())
        })
        .map(|(id, _)| id.0)
        .expect("an entity of that side")
}

fn order(sim: &Sim, side: u8, command: Command) -> Order {
    Order {
        tick: sim.clock().tick,
        side,
        command,
    }
}

#[test]
fn an_order_naming_the_other_sides_things_is_rejected_in_every_mode() {
    let mut contact_cases = 0;
    for (mode, blue, red) in modes() {
        let mut sim = build(mode, blue, red);
        sim.run_ticks(900);
        let controllers = [blue, red];
        for a in 0..2u8 {
            let b = 1 - a;
            let their_unit = first_of(&sim, b, false);
            let their_base = first_of(&sim, b, true);
            let my_unit = first_of(&sim, a, false);
            let cases = [
                order(
                    &sim,
                    a,
                    Command::AssignTask {
                        units: vec![their_unit],
                        task: TaskOrder::Rtb { base: None },
                    },
                ),
                order(
                    &sim,
                    a,
                    Command::AssignTask {
                        units: vec![my_unit],
                        task: TaskOrder::Land {
                            base: wb_sim::orders::BaseTarget::Id(their_base),
                        },
                    },
                ),
                order(
                    &sim,
                    a,
                    Command::SetRoe {
                        units: Some(vec![their_unit]),
                        roe: None,
                        self_defence: Some(false),
                    },
                ),
            ];
            for o in cases {
                let err = sim
                    .submit_typed(&o)
                    .expect_err("a cross-side order is rejected");
                if controllers[usize::from(a)] == "ai" {
                    assert_eq!(err.kind, RejectKind::AiControlled, "{mode}: {err:?}");
                    assert!(err.reason.contains("controlled by the AI"), "{mode}");
                } else {
                    assert!(err.kind.is_other_side(), "{mode} side {a}: {err:?}");
                    // The text is the one an unknown id gets (rule 1).
                    assert!(err.reason.starts_with("Unknown "), "{mode}: {}", err.reason);
                }
            }
            // A contact id that exists only in the other side's picture.
            let pics = sim.world().resource::<SidePictures>();
            let foreign = pics.sides[usize::from(b)]
                .contacts
                .keys()
                .find(|id| pics.sides[usize::from(a)].get(**id).is_none())
                .map(|c| c.0);
            if let (Some(c), "human") = (foreign, controllers[usize::from(a)]) {
                let err = sim
                    .submit_typed(&order(
                        &sim,
                        a,
                        Command::AssignTask {
                            units: vec![my_unit],
                            task: TaskOrder::Intercept { contact: c },
                        },
                    ))
                    .expect_err("another side's contact is not ours");
                assert_eq!(err.kind, RejectKind::OtherSideContact(c), "{mode}");
                assert_eq!(err.reason, format!("Unknown contact C{c}"));
                contact_cases += 1;
            }
        }
    }
    assert!(
        contact_cases > 0,
        "no mode had a contact only the other side holds"
    );
}

#[test]
fn a_client_cannot_command_an_ai_side_but_scenario_orders_can() {
    let mut sim = build("vs_ai", "human", "ai");
    let red_unit = first_of(&sim, 1, false);
    let err = sim
        .submit_typed(&order(
            &sim,
            1,
            Command::AssignTask {
                units: vec![red_unit],
                task: TaskOrder::Rtb { base: None },
            },
        ))
        .expect_err("the AI side takes no client orders");
    assert_eq!(err.kind, RejectKind::AiControlled);
    // The scenario above carries scripted orders for side 1 and still built.
}

// ---------------------------------------------------------------------------

#[test]
fn the_umpire_view_is_refused_outside_command_both() {
    for (mode, blue, red) in modes() {
        let mut sim = build(mode, blue, red);
        let mut enc = ViewEncoder::new();
        let on = sim.set_umpire(true);
        if mode == "command_both" {
            on.expect("allowed in Command both");
        } else {
            assert!(on.is_err(), "{mode}: umpire must be refused");
            assert!(sim.umpire_view(&mut enc).is_err(), "{mode}");
            assert!(!sim.session().umpire_used, "{mode}");
        }
    }
}

#[test]
fn the_umpire_view_needs_to_be_turned_on_and_marks_the_war() {
    let mut sim = build("command_both", "human", "human");
    let mut enc = ViewEncoder::new();
    assert!(sim.umpire_view(&mut enc).is_err(), "off by default");
    assert!(!sim.session().umpire_used);
    sim.set_umpire(true).expect("turn on");
    assert!(!sim.session().umpire_used, "turning it on is not using it");
    sim.run_ticks(300);
    let view = sim.umpire_view(&mut enc).expect("umpire view").to_vec();
    assert!(sim.session().umpire_used);
    // Header: umpire flag, side 255, and both sides' units by name.
    assert_eq!(view[7] & 1, 1, "umpire flag");
    assert_eq!(view[6], 255);
    assert_eq!(
        u16::from_le_bytes([view[4], view[5]]),
        wb_sim::view::VERSION
    );
    let contacts = u32::from_le_bytes(view[28..32].try_into().unwrap());
    assert_eq!(contacts, 0, "no contacts in the umpire view");
    assert!(contains(&view, format!("BLUE-{TAG}-SHIP").as_bytes()));
    assert!(contains(&view, format!("RED-{TAG}-SHIP").as_bytes()));
    // A normal view after it is a plain side view again, strings resent.
    let blue = enc.encode(sim.world(), 0).to_vec();
    assert_eq!(blue[7] & 1, 0);
    assert!(!contains(&blue, format!("RED-{TAG}-SHIP").as_bytes()));
    // The mark survives a save and a load, and stays when turned off.
    sim.set_umpire(false).expect("turn off");
    assert!(sim.session().umpire_used, "never cleared");
    let (db, hash) = content();
    let bytes = sim.save("t", vec![], 0);
    let loaded = Sim::load(&bytes, db, hash, None).expect("loads");
    assert!(loaded.session().umpire_used);
}

// ---------------------------------------------------------------------------

#[test]
fn the_session_is_saved_but_never_hashed() {
    let mut sim = build("hot_seat", "human", "human");
    sim.run_ticks(200);
    let before = sim.state_hash();
    sim.set_active_side(1).expect("switch");
    sim.set_hot_seat(HotSeat {
        hand_over: HandOver::Turns { minutes: 15 },
        urgent: Urgent::Hold,
    })
    .expect("settings");
    sim.set_pin(1, Some("4821"), [7; 16]).expect("pin");
    assert_eq!(sim.state_hash(), before, "the session is not war state");
    assert!(sim.session().check_pin(1, "4821"));
    assert!(!sim.session().check_pin(1, "0000"));
    assert!(sim.session().check_pin(0, "anything"), "no PIN on side 0");
    assert!(
        sim.set_pin(0, Some("12"), [0; 16]).is_err(),
        "4 digits only"
    );

    let (db, hash) = content();
    let bytes = sim.save("t", vec![], 0);
    let loaded = Sim::load(&bytes, db, hash, None).expect("loads");
    assert_eq!(loaded.state_hash(), before);
    assert_eq!(loaded.session(), sim.session(), "session round-trips");
    assert!(loaded.session().check_pin(1, "4821"));
    assert_eq!(
        loaded.session().active_side,
        1,
        "a hot-seat save keeps the active side"
    );
    // The PIN is stored as a hash, never as the digits.
    assert!(!contains(&bytes, b"4821"));
    // Hot-seat settings are refused in other modes.
    let mut solo = build("command_both", "human", "human");
    assert!(solo.set_hot_seat(HotSeat::default()).is_err());
}

#[test]
fn switching_sides_follows_the_mode() {
    let mut vs = build("vs_ai", "human", "ai");
    assert_eq!(vs.session().active_side, 0);
    assert!(vs.set_active_side(1).is_err(), "no toggle against the AI");
    let mut watch = build("watch", "ai", "ai");
    watch
        .set_active_side(1)
        .expect("Watch toggles between pictures");
    let mut both = build("command_both", "human", "human");
    both.set_active_side(1).expect("instant");
    assert!(both.set_active_side(2).is_err());
}

// ---------------------------------------------------------------------------

/// G16: the same scripted two-sided order log gives the same state hash
/// whichever side's view (or the umpire view) was requested, and however
/// often the side was switched, the PIN set, the settings changed.
#[test]
fn g16_views_and_side_switching_never_change_the_war() {
    fn run(switching: bool) -> (Vec<(u64, [u8; 32])>, [u8; 32]) {
        let mut sim = build("command_both", "human", "human");
        let mut enc = ViewEncoder::new();
        if switching {
            sim.set_umpire(true).expect("umpire on");
        }
        let blue_unit = first_of(&sim, 0, false);
        let mut hashes = Vec::new();
        for step in 0..120u32 {
            // The same scripted orders in both runs, on the same ticks.
            if step == 10 {
                sim.submit(&order(
                    &sim,
                    0,
                    Command::AssignTask {
                        units: vec![blue_unit],
                        task: TaskOrder::Move {
                            waypoints: vec![[18.0, 55.3]],
                            alt_m: None,
                            speed: None,
                        },
                    },
                ))
                .expect("blue order");
            }
            if step == 40 {
                sim.submit(&order(
                    &sim,
                    1,
                    Command::SetRoe {
                        units: None,
                        roe: Some(wb_sim::orders::RoeChoice::Alpha),
                        self_defence: None,
                    },
                ))
                .expect("red order");
            }
            sim.run_ticks(10);
            if switching {
                // Look at every picture, flip sides, set a PIN: none of it is war.
                let _ = enc.encode(sim.world(), (step % 2) as u8).len();
                let _ = sim.umpire_view(&mut enc).expect("umpire").len();
                let _ = enc.encode(sim.world(), ((step + 1) % 2) as u8).len();
                sim.set_active_side((step % 2) as u8).expect("switch");
                if step % 7 == 0 {
                    sim.set_pin((step % 2) as u8, Some("1234"), [step as u8; 16])
                        .expect("pin");
                }
                let _ = details::details(sim.world(), (step % 2) as u8, blue_unit);
            }
            hashes.push((sim.clock().tick, sim.state_hash()));
        }
        (hashes, sim.state_hash())
    }
    let (plain, plain_final) = run(false);
    let (switched, switched_final) = run(true);
    assert_eq!(plain, switched, "every checkpoint agrees");
    assert_eq!(plain_final, switched_final);
}
