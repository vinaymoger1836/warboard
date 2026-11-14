//! The engine side of the enemy AI (doc 03 §11, doc 12 §1, ADR-046).
//!
//! Every sim second, for each side whose controller is the AI: build a
//! [`wb_ai::AiPicture`] from that side's **own** units, bases, approval requests
//! and the contacts of its **own** picture (estimates, its own contact ids), let
//! `wb_ai::think` decide, and submit each action as an ordinary order of that
//! side ([`Origin::Ai`]), validated like a player's. Every decision is logged as
//! an `AiDecision` event to the AI's own side.
//!
//! This is the only place the AI touches the engine. `wb-ai` itself depends on
//! `wb-core` only, so it cannot reach the truth, the content, the test bench or
//! the umpire view (`tests/ai_guard.rs`). The adapter reads the same picture a
//! player's client gets; the one engine-internal field of a contact, `truth`,
//! is never read here.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::source::realism::ContactLevel;
use wb_content::source::{Domain, PlatformCategory, WeaponCategory};
use wb_core::UnitId;
use wb_core::geo::LatLon;
use wb_core::units::rad_to_deg;

use crate::clock::{RngSeed, SimClock};
use crate::components::{
    Allegiance, BaseSite, Fuel, Group, Kinematics, Magazine, Mode, PlatformRef, Posture,
    SatelliteState, Task,
};
use crate::events::{EventKind, emit};
use crate::operations::{CondSpec, OpState, OperationSpec, PhaseSpec, PhaseState, PhaseTaskSpec};
use crate::orders::{Command, Order, Origin, TaskOrder};
use crate::picture::{ContactIdentity, SidePictures, TypeRef};
use crate::rules::Rules;
use crate::world::{Difficulty, Sides, UnitIndex, entity};

/// The AI state of every AI side. Saved and hashed: it decides what the side
/// does next.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AiBrains(pub BTreeMap<u8, wb_ai::AiState>);

/// AI decisions run every sim second.
const PERIOD_MS: u64 = 1_000;
/// A contact this fast is taken for a weapon even before it is classified.
const WEAPON_SPEED_MPS: f64 = 450.0;

fn domain(d: Domain) -> wb_ai::Domain {
    match d {
        Domain::Air => wb_ai::Domain::Air,
        Domain::Surface => wb_ai::Domain::Surface,
        Domain::Subsurface => wb_ai::Domain::Subsurface,
        Domain::Ground => wb_ai::Domain::Ground,
        Domain::Space => wb_ai::Domain::Space,
    }
}

/// The stage: every second, each AI side thinks and acts.
pub fn run(world: &mut World) {
    if !world.resource::<SimClock>().due(PERIOD_MS) {
        return;
    }
    let sides = world.resource::<Sides>().clone();
    for side in 0..sides.len() as u8 {
        if !sides.is_ai(side) {
            continue;
        }
        let pic = picture(world, side);
        let mut brains = world.remove_resource::<AiBrains>().unwrap_or_default();
        let decisions = wb_ai::think(&pic, brains.0.entry(side).or_default());
        world.insert_resource(brains);
        for d in decisions {
            let refused = d.action.and_then(|a| act(world, side, a).err());
            emit(
                world,
                side,
                EventKind::AiDecision {
                    layer: d.layer.as_str().to_owned(),
                    summary: d.why,
                    refused,
                },
            );
        }
    }
}

/// The AI picture of `side`: its own things and its own picture only.
pub fn picture(world: &mut World, side: u8) -> wb_ai::AiPicture {
    let rules = world.resource::<Rules>().clone();
    let db = &rules.db;
    let sides = world.resource::<Sides>().clone();
    let clock = world.resource::<SimClock>().clone();
    let seed = world.resource::<RngSeed>().0;
    let info = &sides.sides[usize::from(side)];
    let nation = sides.nation_ix[usize::from(side)].first().copied();
    let doctrine = nation
        .map(|n| &db.doctrines[db.nations[n.0 as usize].doctrine.0 as usize])
        .map(|d| wb_ai::Doctrine {
            aggression: d.aggression,
            risk_tolerance: d.risk_tolerance,
            emcon_discipline: d.emcon_discipline,
            missile_reserve_fraction: d.missile_reserve_fraction,
            cap_aircraft: d.cap_aircraft,
            cap_rotation_min: d.cap_rotation_s / 60.0,
            reaction_time_s: d.reaction_time_s,
            iads_layers: d
                .iads_layers
                .iter()
                .map(|s| db.str(*s).to_owned())
                .collect(),
            strike_package: d
                .strike_package
                .iter()
                .map(|s| db.str(*s).to_owned())
                .collect(),
        })
        .unwrap_or(wb_ai::Doctrine {
            aggression: 0.5,
            risk_tolerance: 0.5,
            emcon_discipline: 0.5,
            missile_reserve_fraction: 0.0,
            cap_aircraft: 2,
            cap_rotation_min: 60.0,
            reaction_time_s: 30.0,
            iads_layers: Vec::new(),
            strike_package: vec!["strike".to_owned()],
        });

    let mut groups = Vec::new();
    let mut bases = Vec::new();
    let index: Vec<(UnitId, Entity)> = world
        .resource::<UnitIndex>()
        .0
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    for (id, e) in index {
        if world.get::<Allegiance>(e).map(|a| a.side) != Some(side) {
            continue;
        }
        if let Some(b) = world.get::<BaseSite>(e) {
            if b.active {
                bases.push(wb_ai::OwnBase {
                    id,
                    at: b.at,
                    air: b.air,
                    port: b.port,
                });
            }
            continue;
        }
        if world.get::<SatelliteState>(e).is_some() {
            groups.push(wb_ai::OwnGroup {
                id,
                role: wb_ai::Role::Satellite,
                domain: wb_ai::Domain::Space,
                at: world
                    .get::<Kinematics>(e)
                    .map_or(LatLon::new(0.0, 0.0), |k| k.pos()),
                members: 1,
                activity: wb_ai::Activity::Tasked,
                fuel: 1.0,
                air_rounds: 0,
                surface_rounds: 0,
                ground_rounds: 0,
                arm_rounds: 0,
                speed_mps: 0.0,
            });
            continue;
        }
        let Some(g) = world.get::<Group>(e) else {
            continue;
        };
        let Some(lead) = g.lead().and_then(|l| entity(world, l)) else {
            continue;
        };
        let Some(pix) = world.get::<PlatformRef>(lead).map(|p| p.0) else {
            continue;
        };
        let platform = rules.platform(pix);
        let at = world
            .get::<Kinematics>(lead)
            .map_or(LatLon::new(0.0, 0.0), |k| k.pos());
        let (mut air, mut surface, mut ground, mut arm) = (0, 0, 0, 0);
        let mut fuel: f64 = 1.0;
        let mut ready = true;
        for m in &g.members {
            let Some(me) = entity(world, *m) else {
                continue;
            };
            if let Some(mag) = world.get::<Magazine>(me) {
                for s in &mag.stations {
                    let w = &db.weapons[s.weapon.0 as usize];
                    if w.category == WeaponCategory::Arm {
                        arm += s.count;
                    }
                    if w.targets.contains(&Domain::Air) {
                        air += s.count;
                    }
                    if w.targets.contains(&Domain::Surface) {
                        surface += s.count;
                    }
                    if w.targets.contains(&Domain::Ground) {
                        ground += s.count;
                    }
                }
            }
            if let Some(f) = world.get::<Fuel>(me) {
                fuel = fuel.min(f.kg / f.max_kg.max(1e-9));
            }
            if let Some(Posture::AtBase { ready: r, .. }) = world.get::<Posture>(me) {
                ready &= *r;
            }
        }
        use PlatformCategory as C;
        let role = match platform.category {
            _ if g.system.is_some() && air > 0 => wb_ai::Role::AirDefence,
            C::GroundRadar => wb_ai::Role::Radar,
            C::GroundLauncher | C::GroundC2 if air > 0 => wb_ai::Role::AirDefence,
            C::ShipDestroyer
            | C::ShipFrigate
            | C::ShipCorvette
            | C::ShipPatrol
            | C::ShipAuxiliary
            | C::SubmarineAttack => wb_ai::Role::Ship,
            C::AircraftAew => wb_ai::Role::Aew,
            C::AircraftTanker => wb_ai::Role::Tanker,
            C::AircraftEw => wb_ai::Role::Jammer,
            _ if platform.domain == Domain::Air && arm > 0 => wb_ai::Role::Sead,
            C::AircraftFighter if air > 0 => wb_ai::Role::Fighter,
            _ if platform.domain == Domain::Air && (surface > 0 || ground > 0) => {
                wb_ai::Role::Strike
            }
            _ if platform.domain == Domain::Air && air > 0 => wb_ai::Role::Fighter,
            _ if platform.domain == Domain::Air => wb_ai::Role::Recon,
            _ => wb_ai::Role::Other,
        };
        let activity = match &g.mode {
            Mode::AtBase { .. } => wb_ai::Activity::AtBase { ready },
            Mode::Return { .. } | Mode::ToTanker { .. } | Mode::Refuelling { .. } => {
                wb_ai::Activity::Returning
            }
            Mode::Task => match &g.task {
                Task::Idle => wb_ai::Activity::Idle,
                Task::Cap { .. } => wb_ai::Activity::Cap,
                Task::Intercept { .. } => wb_ai::Activity::Intercept,
                Task::Strike { .. } => wb_ai::Activity::Strike,
                Task::Suppress { .. } => wb_ai::Activity::Suppress,
                Task::Jam { .. } => wb_ai::Activity::Jam,
                _ => wb_ai::Activity::Tasked,
            },
        };
        groups.push(wb_ai::OwnGroup {
            id,
            role,
            domain: domain(platform.domain),
            at,
            members: g.members.len() as u32,
            activity,
            fuel,
            air_rounds: air,
            surface_rounds: surface,
            ground_rounds: ground,
            arm_rounds: arm,
            speed_mps: rules.cruise_mps(platform, rules.cruise_alt_m(platform)),
        });
    }

    // The side's own picture: estimates and its own contact ids only.
    let now = clock.sim_time_ms;
    let memory_ms = (rules.realism().roe.return_fire_memory_s * 1_000.0) as u64;
    let combat = world.resource::<crate::weapons::Combat>();
    let pics = world.resource::<SidePictures>();
    let mut contacts = Vec::new();
    if let Some(pic) = pics.sides.get(usize::from(side)) {
        for c in pic.contacts.values() {
            let Some(est) = c.est else { continue };
            let classed = c.level >= ContactLevel::Class;
            contacts.push(wb_ai::Seen {
                id: c.id,
                domain: domain(c.domain),
                at: est.at,
                hostile: c.identity == ContactIdentity::Hostile,
                weapon: (classed && matches!(c.type_ref, TypeRef::Weapon(_)))
                    || (c.domain == Domain::Air && est.speed_mps >= WEAPON_SPEED_MPS),
                threat_m: if c.level >= ContactLevel::Type {
                    c.type_ref.threat_m(&rules).unwrap_or(0.0)
                } else {
                    0.0
                },
                in_territory: crate::roe::own_territory(world, &rules, side, est.at),
                fired_at_us: combat
                    .hostile_acts
                    .get(&(side, c.id))
                    .is_some_and(|t| now.saturating_sub(*t) <= memory_ms),
                stale: c.lost,
                count_max: c.count.1,
                speed_mps: est.speed_mps,
                typed: c.level >= ContactLevel::Type,
            });
        }
    }
    let requests = world
        .resource::<crate::roe::RoeRequests>()
        .open
        .iter()
        .filter(|r| r.side == side && !r.approved)
        .map(|r| wb_ai::Request {
            id: r.id,
            group: r.group,
            contact: r.contact,
        })
        .collect();
    let operations = world
        .resource::<crate::operations::Operations>()
        .ops
        .iter()
        .filter(|o| o.side == side)
        .map(|o| wb_ai::OwnOperation {
            id: o.id,
            name: o.name.clone(),
            status: match o.state {
                OpState::Planned => wb_ai::OpStatus::Planned,
                OpState::Running => wb_ai::OpStatus::Running,
                OpState::Paused => wb_ai::OpStatus::Paused,
                OpState::Completed => wb_ai::OpStatus::Completed,
                OpState::Aborted => wb_ai::OpStatus::Aborted,
            },
            phases: o
                .phases
                .iter()
                .map(|p| match p.state {
                    PhaseState::Waiting => wb_ai::PhaseStatus::Waiting,
                    PhaseState::Running => wb_ai::PhaseStatus::Running,
                    PhaseState::Done => wb_ai::PhaseStatus::Done,
                    PhaseState::Aborted => wb_ai::PhaseStatus::Aborted,
                    PhaseState::Cancelled => wb_ai::PhaseStatus::Cancelled,
                })
                .collect(),
            started_ms: o.started.map(|s| s.ms),
        })
        .collect();
    wb_ai::AiPicture {
        side,
        tick: clock.tick,
        now_ms: now,
        seed,
        difficulty: match info.difficulty {
            Difficulty::Easy => wb_ai::Difficulty::Easy,
            Difficulty::Normal => wb_ai::Difficulty::Normal,
            Difficulty::Hard => wb_ai::Difficulty::Hard,
        },
        doctrine,
        groups,
        bases,
        contacts,
        requests,
        operations,
    }
}

fn lon_lat(p: LatLon) -> [f64; 2] {
    [rad_to_deg(p.lon), rad_to_deg(p.lat)]
}

/// Submits one action as an order of `side`; the refusal reason if any.
fn act(world: &mut World, side: u8, a: wb_ai::Action) -> Result<(), String> {
    use wb_ai::Action as A;
    let km = |m: f64| m / 1_000.0;
    let command = match a {
        A::Cap {
            group,
            centre,
            radius_m,
            commit_m,
        } => Command::AssignTask {
            units: vec![group.0],
            task: TaskOrder::Cap {
                centre: lon_lat(centre),
                radius_km: km(radius_m),
                commit_km: km(commit_m),
                alt_m: None,
            },
        },
        A::Intercept { group, contact } => Command::AssignTask {
            units: vec![group.0],
            task: TaskOrder::Intercept { contact: contact.0 },
        },
        A::Strike { group, contact } => Command::AssignTask {
            units: vec![group.0],
            task: TaskOrder::Strike {
                contact: Some(contact.0),
                point: None,
                weapon: None,
                count: None,
            },
        },
        A::Suppress { group, contact } => Command::AssignTask {
            units: vec![group.0],
            task: TaskOrder::Suppress {
                contact: Some(contact.0),
                point: None,
                weapon: None,
                count: None,
            },
        },
        A::Jam {
            group,
            centre,
            radius_m,
        } => Command::AssignTask {
            units: vec![group.0],
            task: TaskOrder::Jam {
                centre: lon_lat(centre),
                radius_km: km(radius_m),
                alt_m: None,
                speed: None,
            },
        },
        A::Rtb { group } => Command::AssignTask {
            units: vec![group.0],
            task: TaskOrder::Rtb { base: None },
        },
        A::Emcon { groups, emcon } => Command::SetEmcon {
            units: groups.iter().map(|g| g.0).collect(),
            emcon: match emcon {
                wb_ai::Emcon::On => wb_content::source::Emcon::On,
                wb_ai::Emcon::Off => wb_content::source::Emcon::Off,
                wb_ai::Emcon::Blink => wb_content::source::Emcon::Blink,
            },
        },
        A::Decide { request, approve } => {
            if approve {
                Command::ApproveEngagement { request }
            } else {
                Command::DenyEngagement { request }
            }
        }
        A::Image {
            satellite,
            centre,
            radius_m,
        } => Command::TaskSatellite {
            satellite: satellite.0,
            centre: lon_lat(centre),
            radius_km: km(radius_m),
        },
        A::Operation(plan) => Command::CreateOperation {
            operation: operation_spec(&plan),
            replace: None,
            start: Some(true),
        },
        A::AbortOperation { operation } => Command::AbortOperation { operation },
    };
    let order = Order {
        tick: world.resource::<SimClock>().tick,
        side,
        command,
    };
    let geo = world
        .get_non_send::<crate::sensors::GeoRes>()
        .and_then(|g| g.0.clone());
    crate::orders::submit_from(world, geo.as_deref(), &order, Origin::Ai)
        .map(|_| ())
        .map_err(|r| r.reason)
}

/// An AI plan as the operation a player's planner would send (ADR-013).
fn operation_spec(plan: &wb_ai::OpPlan) -> OperationSpec {
    OperationSpec {
        name: plan.name.clone(),
        phases: plan
            .phases
            .iter()
            .map(|p| PhaseSpec {
                id: p.id.clone(),
                name: p.name.clone(),
                tasks: p
                    .tasks
                    .iter()
                    .map(|t| PhaseTaskSpec {
                        units: t.groups.iter().map(|g| g.0).collect(),
                        task: task_order(&t.action),
                        fire: None,
                        near_km: None,
                    })
                    .collect(),
                trigger: cond(&p.trigger),
                end: p.end.as_ref().map(cond),
                abort: p.abort.as_ref().map(cond),
            })
            .collect(),
    }
}

fn task_order(a: &wb_ai::PlanAction) -> TaskOrder {
    use wb_ai::PlanAction as P;
    match a {
        P::Move { waypoints } => TaskOrder::Move {
            waypoints: waypoints.iter().map(|w| lon_lat(*w)).collect(),
            alt_m: None,
            speed: None,
        },
        P::Jam { centre, radius_m } => TaskOrder::Jam {
            centre: lon_lat(*centre),
            radius_km: radius_m / 1_000.0,
            alt_m: None,
            speed: None,
        },
        P::Cap {
            centre,
            radius_m,
            commit_m,
        } => TaskOrder::Cap {
            centre: lon_lat(*centre),
            radius_km: radius_m / 1_000.0,
            commit_km: commit_m / 1_000.0,
            alt_m: None,
        },
        P::Strike { contact } => TaskOrder::Strike {
            contact: Some(contact.0),
            point: None,
            weapon: None,
            count: None,
        },
        P::Suppress { contact } => TaskOrder::Suppress {
            contact: Some(contact.0),
            point: None,
            weapon: None,
            count: None,
        },
    }
}

fn cond(c: &wb_ai::PlanCond) -> CondSpec {
    use wb_ai::PlanCond as C;
    match c {
        C::All(v) => CondSpec::All {
            of: v.iter().map(cond).collect(),
        },
        C::Any(v) => CondSpec::Any {
            of: v.iter().map(cond).collect(),
        },
        C::Start { delay_min } => CondSpec::Start {
            delay_min: Some(*delay_min),
        },
        C::PhaseStarted { phase, delay_min } => CondSpec::PhaseStarted {
            phase: phase.clone(),
            delay_min: Some(*delay_min),
        },
        C::PhaseEnded { phase, delay_min } => CondSpec::PhaseEnded {
            phase: phase.clone(),
            delay_min: Some(*delay_min),
        },
        C::WeaponsDone => CondSpec::WeaponsDone,
        C::Elapsed { min } => CondSpec::Elapsed { min: *min },
        C::LossesPct { pct } => CondSpec::LossesPct { pct: *pct },
    }
}
