//! Deploy yourself (doc 03 §2, doc 12 §4, ADR-046): a side that chose this
//! start mode begins with its preset bases open and empty, and stations its
//! inventory at them with `DeployForce` orders, with the clock held, until it
//! sends `FinishDeployment`. Sides that do not deploy are garrisoned from
//! their inventories by the setup builder.
//!
//! Scope for P9: aircraft and ships go to a suitable base of their nation with
//! room; sites, systems and ground platforms are set up on the ground beside
//! a base. Free placement elsewhere comes with P11.
//!
//! The pool and the list of sides still deploying are war state (in the
//! snapshot, hashed). Deployment orders take effect when they are accepted,
//! because the clock is held and no tick would apply them.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::{ContentDb, ForceIx, LoadoutIx, NationIx, PlatformIx};
use wb_content::source::{Domain, PlatformCategory};
use wb_core::UnitId;
use wb_core::geo::{LatLon, destination_point};

use crate::bases::{self, BaseInfo};
use crate::orders::{BaseTarget, Resolved};
use crate::rules::Rules;
use crate::scenario::ScenarioUnit;
use crate::world::{Sides, UnitIdAlloc};

/// What is left to deploy, and who is still deploying.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Deployment {
    /// Sides still deploying, ascending.
    pub active: Vec<u8>,
    /// Inventory not yet stationed, by nation and force.
    pub pool: BTreeMap<(NationIx, ForceIx), u32>,
    /// Sites set up beside each base so far (spreads them out).
    pub placed_at: BTreeMap<UnitId, u32>,
    /// Groups made so far per nation and force (names).
    pub serial: BTreeMap<(NationIx, ForceIx), u32>,
}

/// Where a force can go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Flies from an air base.
    Airfield,
    /// Berths at a port.
    Port,
    /// Set up on the ground beside a base.
    Ground,
}

/// Which kind of base a force needs. Satellites never deploy (they start in orbit).
pub fn place_of(db: &ContentDb, force: ForceIx) -> Option<Place> {
    match force {
        ForceIx::System(_) => Some(Place::Ground),
        ForceIx::Platform(p) => {
            let p = &db.platforms[p.0 as usize];
            if p.orbit.is_some() {
                return None;
            }
            Some(match p.domain {
                Domain::Air => Place::Airfield,
                Domain::Surface | Domain::Subsurface => Place::Port,
                _ => Place::Ground,
            })
        }
    }
}

/// Loadout roles a platform category prefers for its default fit, best first.
/// Generic by category (rule 3): no platform is named.
fn preferred_roles(c: PlatformCategory) -> &'static [&'static str] {
    use PlatformCategory as C;
    match c {
        C::AircraftFighter => &["air_superiority", "interception", "air_defence"],
        C::AircraftAttack | C::AircraftBomber => &["strike", "close_support", "anti_ship"],
        C::AircraftMaritimePatrol => &["maritime_patrol", "anti_ship"],
        C::AircraftEw => &["ew"],
        C::HelicopterAttack => &["close_support", "anti_ship"],
        C::HelicopterUtility => &["anti_ship", "maritime_patrol"],
        C::ShipDestroyer
        | C::ShipFrigate
        | C::ShipCorvette
        | C::ShipPatrol
        | C::ShipAuxiliary
        | C::SubmarineAttack => &["air_defence", "anti_ship"],
        _ => &[],
    }
}

/// The loadout a unit gets when nobody chose one: the first of the platform's
/// loadouts that has the category's best role, else its first loadout.
pub fn default_loadout(db: &ContentDb, p: PlatformIx) -> Option<LoadoutIx> {
    let own: Vec<(usize, &wb_content::compiled::Loadout)> = db
        .loadouts
        .iter()
        .enumerate()
        .filter(|(_, l)| l.platform == p)
        .collect();
    let roles = preferred_roles(db.platforms[p.0 as usize].category);
    for role in roles {
        if let Some((i, _)) = own
            .iter()
            .find(|(_, l)| l.roles.iter().any(|r| db.str(*r) == *role))
        {
            return Some(LoadoutIx(*i as u32));
        }
    }
    own.first().map(|(i, _)| LoadoutIx(*i as u32))
}

/// Bearing and distance of the `n`-th site beside a base: a spiral that keeps
/// sites a few km apart.
pub fn site_offset(n: u32) -> (f64, f64) {
    let bearing = (f64::from(n % 8) * 67.5 + 20.0).to_radians();
    (bearing, 4_000.0 + 1_500.0 * f64::from(n / 8))
}

pub fn site_position(base: LatLon, n: u32) -> LatLon {
    let (b, d) = site_offset(n);
    destination_point(base, b, d)
}

impl Deployment {
    /// The pool of every side in `deploy_sides`: each nation's inventory
    /// except satellites.
    pub fn new(rules: &Rules, sides: &Sides, deploy_sides: &[u8]) -> Self {
        let mut d = Deployment::default();
        for &s in deploy_sides {
            let Some(nations) = sides.nation_ix.get(usize::from(s)) else {
                continue;
            };
            if !d.active.contains(&s) {
                d.active.push(s);
            }
            for n in nations {
                let Some(inv) = rules.db.inventories.iter().find(|i| i.nation == *n) else {
                    continue;
                };
                for (f, count) in &inv.platforms {
                    if place_of(&rules.db, *f).is_some() && *count > 0 {
                        *d.pool.entry((*n, *f)).or_insert(0) += count;
                    }
                }
            }
        }
        d.active.sort_unstable();
        d
    }

    /// Someone is still deploying: the clock must stay held.
    pub fn is_active(&self) -> bool {
        !self.active.is_empty()
    }
}

// ---------------------------------------------------------------------------
// What the client shows
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LoadoutChoiceView {
    pub id: String,
    pub name: String,
}

/// One line of the deploy panel: a force still in the pool.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PoolLine {
    /// Nation id, `ald`.
    pub nation: String,
    /// Content id of the platform or system.
    pub id: String,
    pub name: String,
    pub class_name: String,
    /// `airfield`, `port` or `ground`.
    pub place: String,
    pub left: u32,
    pub loadouts: Vec<LoadoutChoiceView>,
    /// The loadout used when none is chosen.
    pub default_loadout: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DeployBase {
    /// Unit id of the base.
    pub id: u32,
    pub name: String,
    pub nation: String,
    pub kind: String,
    pub air: bool,
    pub port: bool,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub aircraft_room: u32,
    pub berth_room: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DeploymentView {
    /// This side still has forces to station and has not finished.
    pub deploying: bool,
    /// Some side is deploying, so the clock is held.
    pub clock_held: bool,
    pub pool: Vec<PoolLine>,
    /// The side's own preset bases.
    pub bases: Vec<DeployBase>,
}

/// The side's own pool and bases. Own side only (rule 1).
pub fn view(world: &mut World, side: u8) -> DeploymentView {
    let rules = world.resource::<Rules>().clone();
    let dep = world.resource::<Deployment>().clone();
    let sides = world.resource::<Sides>().clone();
    let db = &rules.db;
    let nations: &[NationIx] = sides
        .nation_ix
        .get(usize::from(side))
        .map_or(&[][..], Vec::as_slice);
    let mut pool = Vec::new();
    if dep.active.contains(&side) {
        for ((n, f), left) in &dep.pool {
            if *left == 0 || !nations.contains(n) {
                continue;
            }
            let (id, name, class, loadouts, default) = match f {
                ForceIx::Platform(p) => {
                    let pl = &db.platforms[p.0 as usize];
                    let ls: Vec<LoadoutChoiceView> = db
                        .loadouts
                        .iter()
                        .filter(|l| l.platform == *p)
                        .map(|l| LoadoutChoiceView {
                            id: db.str(l.id).to_owned(),
                            name: db.str(l.name).to_owned(),
                        })
                        .collect();
                    let d = default_loadout(db, *p)
                        .map(|l| db.str(db.loadouts[l.0 as usize].id).to_owned());
                    (pl.id, pl.name, &pl.design, ls, d)
                }
                ForceIx::System(x) => {
                    let sy = &db.systems[x.0 as usize];
                    (sy.id, sy.name, &sy.design, Vec::new(), None)
                }
            };
            pool.push(PoolLine {
                nation: db.str(db.nations[n.0 as usize].id).to_owned(),
                id: db.str(id).to_owned(),
                name: db.str(name).to_owned(),
                class_name: class
                    .class_name
                    .map(|c| db.str(c).to_owned())
                    .unwrap_or_default(),
                place: match place_of(db, *f) {
                    Some(Place::Airfield) => "airfield",
                    Some(Place::Port) => "port",
                    _ => "ground",
                }
                .to_owned(),
                left: *left,
                loadouts,
                default_loadout: default,
            });
        }
    }
    let bases = bases::list(world)
        .into_iter()
        .filter(|b| b.side == side && b.content.is_some())
        .map(|b| {
            let nation = b.content.map_or(String::new(), |c| {
                db.str(db.nations[db.bases[c.0 as usize].nation.0 as usize].id)
                    .to_owned()
            });
            DeployBase {
                id: b.id.0,
                name: b.name.clone(),
                nation,
                kind: bases::kind_name(b.kind).to_owned(),
                air: b.air,
                port: b.port,
                lat_deg: b.at.lat_deg(),
                lon_deg: b.at.lon_deg(),
                aircraft_room: b.aircraft_capacity.saturating_sub(b.stationed as u32),
                berth_room: b.ship_berths.saturating_sub(b.stationed as u32),
            }
        })
        .collect();
    DeploymentView {
        deploying: dep.active.contains(&side),
        clock_held: dep.is_active(),
        pool,
        bases,
    }
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

fn find_force(db: &ContentDb, id: &str) -> Option<ForceIx> {
    db.platforms
        .iter()
        .position(|p| db.str(p.id) == id)
        .map(|i| ForceIx::Platform(PlatformIx(i as u32)))
        .or_else(|| {
            db.systems
                .iter()
                .position(|s| db.str(s.id) == id)
                .map(|i| ForceIx::System(wb_content::compiled::SystemIx(i as u32)))
        })
}

/// Checks a `DeployForce` order.
pub fn validate(
    world: &mut World,
    side: u8,
    force: &str,
    count: u32,
    base: &BaseTarget,
    loadout: Option<&str>,
) -> Result<Resolved, String> {
    let dep = world.resource::<Deployment>().clone();
    if !dep.is_active() && dep.pool.is_empty() {
        return Err("This war has no deployment phase".to_owned());
    }
    if !dep.active.contains(&side) {
        return Err("Your side has finished deploying".to_owned());
    }
    if !(1..=64).contains(&count) {
        return Err("Deploy 1–64 at a time".to_owned());
    }
    let rules = world.resource::<Rules>().clone();
    let db = &rules.db;
    let f = find_force(db, force).ok_or_else(|| format!("Unknown force `{force}`"))?;
    let place = place_of(db, f).ok_or_else(|| "Satellites start in orbit".to_owned())?;
    let infos = bases::list(world);
    let b: &BaseInfo = match base {
        BaseTarget::Id(id) => infos.iter().find(|b| b.id == UnitId(*id)),
        BaseTarget::Content(c) => infos.iter().find(|b| {
            b.content
                .is_some_and(|ix| db.str(db.bases[ix.0 as usize].id) == c)
        }),
    }
    .filter(|b| b.side == side)
    .ok_or_else(|| "Unknown base".to_owned())?;
    let Some(bix) = b.content else {
        return Err("Forces are stationed at preset bases".to_owned());
    };
    let nation = db.bases[bix.0 as usize].nation;
    let left = dep.pool.get(&(nation, f)).copied().unwrap_or(0);
    if left == 0 {
        return Err(format!(
            "{} has none of that left to deploy",
            db.str(db.nations[nation.0 as usize].name)
        ));
    }
    if count > left {
        return Err(format!("Only {left} left to deploy"));
    }
    let mut lix = None;
    if let ForceIx::Platform(p) = f {
        match place {
            Place::Airfield | Place::Port => {
                bases::suitable(&rules, b, &db.platforms[p.0 as usize])?;
                let room = bases::room(&rules, b, &db.platforms[p.0 as usize]);
                if count > room {
                    return Err(format!("{} has room for {room} more", b.name));
                }
            }
            Place::Ground => {}
        }
        if let Some(l) = loadout {
            lix = Some(
                db.loadouts
                    .iter()
                    .position(|x| db.str(x.id) == l && x.platform == p)
                    .map(|i| LoadoutIx(i as u32))
                    .ok_or_else(|| format!("No loadout `{l}` for that platform"))?,
            );
        } else {
            lix = default_loadout(db, p);
        }
    } else if loadout.is_some() {
        return Err("A system comes with its own weapons".to_owned());
    }
    Ok(Resolved::Deploy {
        force: f,
        nation,
        count,
        base: b.id,
        loadout: lix,
    })
}

/// Stations the force now.
pub fn apply(
    world: &mut World,
    side: u8,
    force: ForceIx,
    nation: NationIx,
    count: u32,
    base: UnitId,
    loadout: Option<LoadoutIx>,
) {
    let rules = world.resource::<Rules>().clone();
    let db = &rules.db;
    let infos = bases::list(world);
    let Some(b) = infos.iter().find(|b| b.id == base).cloned() else {
        return;
    };
    let nation_id = db.str(db.nations[nation.0 as usize].id).to_owned();
    let (serial, n_site) = {
        let mut dep = world.resource_mut::<Deployment>();
        if let Some(l) = dep.pool.get_mut(&(nation, force)) {
            *l = l.saturating_sub(count);
        }
        let s = dep.serial.entry((nation, force)).or_insert(0);
        *s += 1;
        let serial = *s;
        let placed = dep.placed_at.entry(base).or_insert(0);
        let n = *placed;
        if place_of(db, force) == Some(Place::Ground) {
            *placed += 1;
        }
        (serial, n)
    };
    let first = {
        let mut alloc = world.resource_mut::<UnitIdAlloc>();
        let first = alloc.next;
        let total = match force {
            ForceIx::Platform(_) => count,
            ForceIx::System(x) => db.systems[x.0 as usize]
                .components
                .iter()
                .map(|c| c.count)
                .sum(),
        };
        alloc.next += 1 + total;
        first
    };
    let mut u = ScenarioUnit {
        id: first,
        name: String::new(),
        platform: None,
        nation: Some(nation_id.clone()),
        count,
        base: None,
        loadout: None,
        lat_deg: None,
        lon_deg: None,
        alt_m: None,
        heading_deg: 0.0,
        speed_mps: None,
        system: None,
        emcon: None,
        jammer: None,
        jammer_mode: None,
    };
    match force {
        ForceIx::Platform(p) => {
            let pl = &db.platforms[p.0 as usize];
            u.name = format!(
                "{} {} {serial}",
                nation_id.to_uppercase(),
                db.str(pl.short_name)
            );
            u.platform = Some(db.str(pl.id).to_owned());
            u.loadout = loadout.map(|l| db.str(db.loadouts[l.0 as usize].id).to_owned());
            match place_of(db, force) {
                Some(Place::Airfield | Place::Port) => {
                    u.base = b
                        .content
                        .map(|ix| db.str(db.bases[ix.0 as usize].id).to_owned());
                }
                _ => {
                    let at = site_position(b.at, n_site);
                    u.lat_deg = Some(at.lat_deg());
                    u.lon_deg = Some(at.lon_deg());
                }
            }
        }
        ForceIx::System(x) => {
            let sy = &db.systems[x.0 as usize];
            u.name = format!(
                "{} {} {serial}",
                nation_id.to_uppercase(),
                db.str(sy.short_name)
            );
            u.system = Some(db.str(sy.id).to_owned());
            u.count = 1;
            let at = site_position(b.at, n_site);
            u.lat_deg = Some(at.lat_deg());
            u.lon_deg = Some(at.lon_deg());
        }
    }
    // The order was validated, so the spawn cannot be refused for content
    // reasons; the side is the base's (`spawn_unit_spec` reads it from the nation).
    debug_assert!(b.side == side);
    let spawned = crate::scenario::spawn_unit_spec(world, &rules, &u, true);
    debug_assert!(spawned.is_ok(), "validated deployment failed: {spawned:?}");
}

/// The side is done; when every side is, the clock may run.
pub fn finish(world: &mut World, side: u8) {
    let mut dep = world.resource_mut::<Deployment>();
    dep.active.retain(|s| *s != side);
    if dep.active.is_empty() {
        // Whatever was left unstationed is simply not in the war.
        dep.pool.clear();
    }
}
