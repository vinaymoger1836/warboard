//! Tasks and standing behaviours (doc 03 §5): what each group does this tick.
//!
//! Groups are visited in `UnitId` order. Each sets its lead's [`Steer`]
//! target; movement integrates it and the followers keep formation.

use bevy_ecs::prelude::*;
use core::f64::consts::FRAC_PI_2;
use wb_content::compiled::Platform;
use wb_core::UnitId;
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing, normalize_bearing};
use wb_core::math::atan;

use crate::bases::{self, BaseInfo};
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, BaseSite, Fuel, Group, GroupMember, Identity, Kinematics, Magazine, Mode,
    PlatformRef, Posture, ReturnWhy, Speed, Steer, TankerState, Task,
};
use crate::events::{EventKind, emit};
use crate::fuel;
use crate::orders;
use crate::rules::Rules;
use crate::world::entity;

/// Standing behaviours (bingo, winchester) are checked this often.
const BEHAVIOUR_PERIOD_MS: u64 = 1_000;
/// Radius an aircraft holds at the end of a route.
const HOLD_RADIUS_M: f64 = 5_000.0;
/// Longest lead time when pursuing a tanker.
const PURSUIT_LEAD_MAX_S: f64 = 120.0;

/// Third stage of every tick.
pub fn run(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let due = clock.due(BEHAVIOUR_PERIOD_MS);
    let mut q = world.query::<(Entity, &Identity, &Group)>();
    let mut groups: Vec<(UnitId, Entity)> = q.iter(world).map(|(e, i, _)| (i.unit, e)).collect();
    groups.sort_by_key(|g| g.0);
    let bases = bases::list(world);
    let mut q = world.query::<(&Identity, &Group)>();
    let mut tankers: Vec<UnitId> = q
        .iter(world)
        .filter(|(_, g)| matches!(g.task, Task::TankerTrack { .. }))
        .filter_map(|(_, g)| g.lead())
        .collect();
    tankers.sort();
    let ctx = Ctx {
        rules: &rules,
        clock: &clock,
        bases: &bases,
        tankers: &tankers,
        due,
    };

    for (gid, ge) in groups {
        let side = world.get::<Allegiance>(ge).map_or(0, |a| a.side);
        let mut g = std::mem::take(&mut *world.get_mut::<Group>(ge).expect("group"));
        step(world, &ctx, side, gid, &mut g);
        *world.get_mut::<Group>(ge).expect("group") = g;
    }
}

/// What every group's step reads this tick.
struct Ctx<'a> {
    rules: &'a Rules,
    clock: &'a SimClock,
    bases: &'a [BaseInfo],
    /// Leads of groups flying a tanker track, sorted.
    tankers: &'a [UnitId],
    /// Standing behaviours run this tick.
    due: bool,
}

fn step(world: &mut World, ctx: &Ctx, side: u8, gid: UnitId, g: &mut Group) {
    let (rules, bases, due) = (ctx.rules, ctx.bases, ctx.due);
    let Some(lead) = g.lead() else { return };
    let Some(le) = entity(world, lead) else {
        return;
    };
    let Some(lp) = world.get::<PlatformRef>(le).map(|p| rules.platform(p.0)) else {
        return;
    };
    let air = rules.is_air(lp);

    if let Mode::AtBase { base } = g.mode {
        let go = g.launch || (g.task.is_sortie() && g.behaviour.resume_after_turnaround);
        if !go || !orders::all_ready(world, &g.members) {
            return;
        }
        take_off(world, side, base, &g.members);
        g.launch = false;
        g.wp = 0;
        g.mode = match g.task {
            Task::Rtb { base: b } | Task::Land { base: b } => Mode::Return {
                base: b,
                why: ReturnWhy::Order,
            },
            Task::Transfer { base: b } => Mode::Return {
                base: b,
                why: ReturnWhy::Transfer,
            },
            _ => Mode::Task,
        };
    }

    // Standing behaviours.
    if due && matches!(g.mode, Mode::Task | Mode::ToTanker { .. }) {
        for m in &g.members {
            fuel::update_bingo(world, rules, ctx.clock, bases, *m);
        }
    }
    if due && g.mode == Mode::Task {
        let bingo = g.behaviour.bingo_rtb && at_bingo(world, &g.members);
        // A tanker in reach takes the group when the fuel to reach it and
        // still get home (reserve on both legs) runs out, or at bingo
        // (doc 02 §6).
        let tanker = if g.behaviour.bingo_rtb && air && lp.tanker.is_none() {
            find_tanker(world, rules, bases, side, &g.members, ctx.tankers)
        } else {
            None
        };
        match tanker {
            Some(t) if bingo || t.fuel_kg <= t.joker_kg => {
                leave_station(world, side, gid, g);
                g.mode = Mode::ToTanker { tanker: t.tanker };
                emit(
                    world,
                    side,
                    EventKind::ToTanker {
                        group: gid,
                        tanker: t.tanker,
                    },
                );
            }
            _ if bingo => {
                leave_station(world, side, gid, g);
                go_home(world, rules, bases, side, gid, g, ReturnWhy::Bingo);
            }
            _ if g.behaviour.winchester_rtb && winchester(world, &g.members) => {
                leave_station(world, side, gid, g);
                go_home(world, rules, bases, side, gid, g, ReturnWhy::Winchester);
            }
            _ => {}
        }
    }
    match g.mode {
        Mode::ToTanker { tanker } => {
            if !tanker_ok(world, rules, side, tanker, &g.members) {
                go_home(world, rules, bases, side, gid, g, ReturnWhy::Bingo);
            } else if let (Some(a), Some(b)) = (pos(world, lead), pos(world, tanker))
                && distance_m(a, b) <= rules.realism().logistics.tanker_join_m
            {
                g.mode = Mode::Refuelling { tanker, served: 0 };
            }
        }
        Mode::Refuelling { tanker, served }
            if !tanker_ok(world, rules, side, tanker, &g.members)
                || usize::from(served) >= g.members.len() =>
        {
            g.mode = Mode::Task;
        }
        _ => {}
    }

    let Some(k) = world.get::<Kinematics>(le).copied() else {
        return;
    };
    let steer = match g.mode {
        Mode::AtBase { .. } => return,
        Mode::Task => task_steer(world, rules, side, gid, g, lp, &k),
        Mode::ToTanker { tanker } | Mode::Refuelling { tanker, .. } => {
            pursue(world, rules, lp, &k, tanker)
        }
        Mode::Return { base, .. } => match bases.iter().find(|b| b.id == base) {
            Some(b) => approach(rules, lp, &k, b),
            None => hold(rules, lp, &k, k.pos(), None, Speed::Cruise),
        },
    };
    if let Some(mut s) = world.get_mut::<Steer>(le) {
        *s = steer;
    }
}

fn pos(world: &World, unit: UnitId) -> Option<LatLon> {
    entity(world, unit)
        .and_then(|e| world.get::<Kinematics>(e))
        .map(|k| k.pos())
}

fn take_off(world: &mut World, side: u8, base: UnitId, members: &[UnitId]) {
    let site = entity(world, base).and_then(|e| world.get::<BaseSite>(e).cloned());
    for m in members {
        let Some(e) = entity(world, *m) else { continue };
        *world.get_mut::<Posture>(e).expect("posture") = Posture::Moving;
        if let (Some(site), Some(mut k)) = (&site, world.get_mut::<Kinematics>(e)) {
            k.lat = site.at.lat;
            k.lon = site.at.lon;
            k.alt_m = site.elevation_m;
            k.speed_mps = 0.0;
        }
        if let Some(mut f) = world.get_mut::<Fuel>(e) {
            f.bingo_base = None;
        }
        emit(world, side, EventKind::TookOff { unit: *m, base });
    }
    if let Some(be) = entity(world, base)
        && let Some(mut b) = world.get_mut::<BaseSite>(be)
    {
        b.stationed.retain(|s| !members.contains(s));
    }
}

fn at_bingo(world: &World, members: &[UnitId]) -> bool {
    members.iter().any(|m| {
        entity(world, *m)
            .and_then(|e| world.get::<Fuel>(e))
            .is_some_and(|f| f.bingo_kg > 0.0 && f.kg <= f.bingo_kg)
    })
}

/// Every armed member has used all its weapons.
fn winchester(world: &World, members: &[UnitId]) -> bool {
    let mags: Vec<&Magazine> = members
        .iter()
        .filter_map(|m| entity(world, *m).and_then(|e| world.get::<Magazine>(e)))
        .filter(|m| m.initial > 0)
        .collect();
    !mags.is_empty() && mags.iter().all(|m| m.remaining() == 0)
}

fn leave_station(world: &mut World, side: u8, gid: UnitId, g: &mut Group) {
    if g.on_station {
        g.on_station = false;
        emit(
            world,
            side,
            EventKind::StationChanged {
                group: gid,
                on: false,
            },
        );
    }
}

/// Sends a group home. On bingo every member records the reserve it should
/// land with (the G7 check).
fn go_home(
    world: &mut World,
    rules: &Rules,
    bases: &[BaseInfo],
    side: u8,
    gid: UnitId,
    g: &mut Group,
    why: ReturnWhy,
) {
    // The base the member lowest on fuel would choose.
    let lowest = g
        .members
        .iter()
        .filter_map(|m| {
            let f = entity(world, *m).and_then(|e| world.get::<Fuel>(e))?;
            Some((f.kg - f.bingo_kg, *m, f.bingo_base))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let home = g
        .lead()
        .and_then(|l| entity(world, l))
        .and_then(|e| world.get::<BaseRef>(e))
        .and_then(|r| r.home);
    let base = lowest.and_then(|l| l.2).or(home).or_else(|| {
        let le = entity(world, g.lead()?)?;
        let p = rules.platform(world.get::<PlatformRef>(le)?.0);
        let at = world.get::<Kinematics>(le)?.pos();
        bases::nearest_suitable(rules, bases, side, p, at, home).map(|b| b.id)
    });
    let Some(base) = base.and_then(|b| bases.iter().find(|x| x.id == b)) else {
        // Nowhere to go: carry on until the fuel runs out.
        g.mode = Mode::Task;
        return;
    };
    g.mode = Mode::Return { base: base.id, why };
    if why == ReturnWhy::Bingo {
        for m in g.members.clone() {
            let reserve = fuel::reserve_for(world, rules, m, base);
            let Some(e) = entity(world, m) else { continue };
            let Some(mut f) = world.get_mut::<Fuel>(e) else {
                continue;
            };
            f.reserve_at_bingo_kg = Some(reserve);
            let fuel_kg = f.kg;
            emit(
                world,
                side,
                EventKind::BingoRtb {
                    unit: m,
                    fuel_kg,
                    reserve_kg: reserve,
                    base: base.id,
                },
            );
        }
    } else {
        emit(
            world,
            side,
            EventKind::Return {
                group: gid,
                base: base.id,
                why,
            },
        );
    }
}

/// The tanker's group, if it is flying its track and has fuel to give.
fn tanker_ok(world: &World, rules: &Rules, side: u8, tanker: UnitId, members: &[UnitId]) -> bool {
    let Some(te) = entity(world, tanker) else {
        return false;
    };
    let on_track = world
        .get::<GroupMember>(te)
        .and_then(|m| entity(world, m.group))
        .and_then(|ge| world.get::<Group>(ge))
        .is_some_and(|g| matches!(g.task, Task::TankerTrack { .. }) && g.mode == Mode::Task);
    on_track
        && world.get::<Posture>(te) == Some(&Posture::Moving)
        && world.get::<Allegiance>(te).map(|a| a.side) == Some(side)
        && world
            .get::<TankerState>(te)
            .is_some_and(|t| t.offload_left_kg > 0.0)
        && fuel::aar_compatible(world, rules, tanker, members)
}

/// A tanker the group can use, seen from its member lowest on fuel.
struct TankerChoice {
    tanker: UnitId,
    fuel_kg: f64,
    /// Fuel at which the member must head for this tanker: reach it and get
    /// home from it, with the reserve on both legs.
    joker_kg: f64,
}

/// Nearest tanker on its track that the member lowest on fuel can reach and
/// still get home from (doc 02 §6).
fn find_tanker(
    world: &World,
    rules: &Rules,
    bases: &[BaseInfo],
    side: u8,
    members: &[UnitId],
    tankers: &[UnitId],
) -> Option<TankerChoice> {
    let reserve = rules.realism().logistics.fuel_reserve_fraction;
    let (m, f) = members
        .iter()
        .filter_map(|m| Some((*m, *entity(world, *m).and_then(|e| world.get::<Fuel>(e))?)))
        .min_by(|a, b| a.1.kg.total_cmp(&b.1.kg).then(a.0.cmp(&b.0)))?;
    let me = entity(world, m)?;
    let p = rules.platform(world.get::<PlatformRef>(me)?.0);
    let k = *world.get::<Kinematics>(me)?;
    let home = world.get::<BaseRef>(me).and_then(|r| r.home);
    tankers
        .iter()
        .filter(|t| tanker_ok(world, rules, side, **t, members))
        .filter_map(|&t| {
            let at = pos(world, t)?;
            let to = distance_m(k.pos(), at);
            let back = bases::nearest_suitable(rules, bases, side, p, at, home)?;
            let f_to = rules.fuel_for_distance(p, to, k.alt_m);
            let f_back = rules.fuel_for_distance(p, distance_m(at, back.at), k.alt_m);
            // Reachable: get there, then home with the reserve.
            (f_to + f_back * (1.0 + reserve) <= f.kg).then_some((
                to,
                TankerChoice {
                    tanker: t,
                    fuel_kg: f.kg,
                    joker_kg: (f_to + f_back) * (1.0 + reserve),
                },
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.tanker.cmp(&b.1.tanker)))
        .map(|(_, c)| c)
}

fn speed_for(rules: &Rules, p: &Platform, s: Speed, alt: f64) -> f64 {
    match s {
        Speed::Cruise => rules.cruise_mps(p, alt),
        Speed::Max => rules.max_mps(p, alt),
    }
}

/// Heading that flies a clockwise circle: tangent plus a correction toward
/// the circle.
fn circle_heading(at: LatLon, centre: LatLon, radius_m: f64) -> f64 {
    let r = distance_m(centre, at);
    if r < 1.0 {
        return 0.0;
    }
    let out = initial_bearing(centre, at);
    let err = (r - radius_m) / (0.25 * radius_m).max(1.0);
    normalize_bearing(out + FRAC_PI_2 + atan(err))
}

/// Holds near a point: aircraft circle it, ships stop.
fn hold(
    rules: &Rules,
    p: &Platform,
    k: &Kinematics,
    at: LatLon,
    alt: Option<f64>,
    s: Speed,
) -> Steer {
    if rules.is_air(p) {
        let alt = alt.unwrap_or(k.alt_m.max(rules.cruise_alt_m(p).min(1_000.0)));
        Steer {
            heading: circle_heading(k.pos(), at, HOLD_RADIUS_M),
            speed_mps: speed_for(rules, p, s, alt),
            alt_m: alt,
        }
    } else {
        Steer {
            heading: k.heading,
            speed_mps: 0.0,
            alt_m: 0.0,
        }
    }
}

fn to_point(rules: &Rules, p: &Platform, k: &Kinematics, at: LatLon, alt: f64, s: Speed) -> Steer {
    Steer {
        heading: initial_bearing(k.pos(), at),
        speed_mps: speed_for(rules, p, s, alt),
        alt_m: alt,
    }
}

fn set_station(world: &mut World, side: u8, gid: UnitId, g: &mut Group, on: bool) {
    if g.on_station != on {
        g.on_station = on;
        emit(world, side, EventKind::StationChanged { group: gid, on });
    }
}

fn task_steer(
    world: &mut World,
    rules: &Rules,
    side: u8,
    gid: UnitId,
    g: &mut Group,
    p: &Platform,
    k: &Kinematics,
) -> Steer {
    let r_wp = rules.realism().movement.waypoint_radius_m;
    let cruise_alt = rules.cruise_alt_m(p);
    match g.task.clone() {
        Task::Move {
            waypoints,
            alt_m,
            speed,
        } => {
            let alt = alt_m.unwrap_or(cruise_alt);
            let mut wp = g.wp as usize;
            while wp < waypoints.len() && distance_m(k.pos(), waypoints[wp]) <= r_wp {
                wp += 1;
            }
            g.wp = wp as u32;
            match waypoints.get(wp) {
                Some(t) => to_point(rules, p, k, *t, alt, speed),
                None => {
                    let last = *waypoints.last().expect("validated: at least one waypoint");
                    hold(rules, p, k, last, Some(alt), speed)
                }
            }
        }
        Task::PatrolRoute {
            waypoints,
            looped,
            alt_m,
            speed,
        } => route_steer(
            world, rules, side, gid, g, p, k, &waypoints, looped, alt_m, speed,
        ),
        Task::TankerTrack { a, b, alt_m } => route_steer(
            world,
            rules,
            side,
            gid,
            g,
            p,
            k,
            &[a, b],
            true,
            alt_m,
            Speed::Cruise,
        ),
        Task::PatrolCircle {
            centre,
            radius_m,
            alt_m,
            speed,
        } => {
            let alt = alt_m.unwrap_or(cruise_alt);
            let on = distance_m(centre, k.pos()) <= radius_m * 1.25;
            set_station(world, side, gid, g, on);
            if rules.is_air(p) || distance_m(centre, k.pos()) > radius_m * 0.5 {
                Steer {
                    heading: circle_heading(k.pos(), centre, radius_m),
                    speed_mps: speed_for(rules, p, speed, alt),
                    alt_m: if rules.is_air(p) { alt } else { 0.0 },
                }
            } else {
                Steer {
                    heading: circle_heading(k.pos(), centre, radius_m),
                    speed_mps: speed_for(rules, p, speed, 0.0),
                    alt_m: 0.0,
                }
            }
        }
        Task::Idle | Task::Rtb { .. } | Task::Land { .. } | Task::Transfer { .. } => {
            hold(rules, p, k, k.pos(), None, Speed::Cruise)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn route_steer(
    world: &mut World,
    rules: &Rules,
    side: u8,
    gid: UnitId,
    g: &mut Group,
    p: &Platform,
    k: &Kinematics,
    waypoints: &[LatLon],
    looped: bool,
    alt_m: Option<f64>,
    speed: Speed,
) -> Steer {
    let alt = alt_m.unwrap_or(rules.cruise_alt_m(p));
    let r_wp = rules.realism().movement.waypoint_radius_m;
    let n = waypoints.len();
    let mut wp = (g.wp as usize).min(n);
    if wp < n && distance_m(k.pos(), waypoints[wp]) <= r_wp {
        wp += 1;
        if wp == n && looped {
            wp = 0;
        }
        set_station(world, side, gid, g, true);
    }
    g.wp = wp as u32;
    match waypoints.get(wp) {
        Some(t) => to_point(rules, p, k, *t, alt, speed),
        None => hold(rules, p, k, waypoints[n - 1], Some(alt), speed),
    }
}

/// Lead pursuit of a tanker: aim where it will be, close fast while far,
/// then match its speed.
fn pursue(world: &World, rules: &Rules, p: &Platform, k: &Kinematics, tanker: UnitId) -> Steer {
    let Some(tk) = entity(world, tanker).and_then(|e| world.get::<Kinematics>(e)) else {
        return hold(rules, p, k, k.pos(), None, Speed::Cruise);
    };
    let d = distance_m(k.pos(), tk.pos());
    let max = rules.max_mps(p, k.alt_m);
    let fast = (rules.cruise_mps(p, k.alt_m) * 1.1)
        .max(tk.speed_mps + 30.0)
        .min(max);
    let lead_s = (d / fast.max(1.0)).min(PURSUIT_LEAD_MAX_S);
    let aim = destination_point(tk.pos(), tk.heading, tk.speed_mps * lead_s);
    let speed = if d > 20_000.0 {
        fast
    } else {
        (tk.speed_mps + d / 60.0).min(max)
    };
    Steer {
        heading: initial_bearing(k.pos(), aim),
        speed_mps: speed,
        alt_m: tk.alt_m,
    }
}

/// Return to base: cruise, descend at the top of descent, circle down if
/// still high over the field (doc 02 §6 landing).
fn approach(rules: &Rules, p: &Platform, k: &Kinematics, b: &BaseInfo) -> Steer {
    let d = distance_m(k.pos(), b.at);
    let to_base = initial_bearing(k.pos(), b.at);
    if !rules.is_air(p) {
        return Steer {
            heading: to_base,
            speed_mps: rules.cruise_mps(p, 0.0),
            alt_m: 0.0,
        };
    }
    let log = &rules.realism().logistics;
    let pattern = b.elevation_m + log.pattern_alt_m;
    let v = rules.cruise_mps(p, k.alt_m);
    let d_desc = (k.alt_m - pattern).max(0.0) / rules.descent_rate_mps() * v;
    let alt = if d > d_desc + log.landing_radius_m {
        if k.alt_m < pattern + 500.0 {
            rules.cruise_alt_m(p)
        } else {
            k.alt_m
        }
    } else {
        pattern
    };
    let heading = if d <= log.landing_radius_m && k.alt_m > pattern + 100.0 {
        circle_heading(k.pos(), b.at, log.landing_radius_m * 0.6)
    } else {
        to_base
    };
    Steer {
        heading,
        speed_mps: v,
        alt_m: alt,
    }
}
