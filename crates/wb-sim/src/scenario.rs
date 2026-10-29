//! Scenario files (`scenarios/*.yaml`).
//!
//! A scenario names its sides (coalitions of content countries), the start
//! mode, platform units (a group of `count` aircraft or ships at a base or in
//! the air), orders with their ticks, and optionally test-only blocks: bare
//! units and the determinism probe (P0), `force_fine_tick` and
//! `generated_units` (benchmarks), `golden` (bands read by `wb golden`),
//! `scripted_targets` and `test_terrain` (P4 goldens and tests).
//!
//! A unit is a group of `count` platforms, a composite `system` (its
//! components as one stationary group), or a satellite (a platform with an
//! orbit, no position).

use bevy_ecs::prelude::Resource;
use bevy_ecs::world::World;
use core::f64::consts::TAU;
use serde::{Deserialize, Serialize};
use wb_content::source::Emcon;
use std::collections::BTreeSet;
use wb_content::compiled::{CountryIx, LoadoutIx, PlatformIx};
use wb_core::geo::{LatLon, destination_point, normalize_bearing};
use wb_core::units::deg_to_rad;
use wb_core::{RngPurpose, UnitId, rng};

use crate::bases::{self, content_kind};
use crate::clock::{RngSeed, SimClock, TickLen};
use crate::components::{
    Allegiance, BaseRef, BaseSite, EmconState, Fuel, Group, GroupMember, Identity, Kinematics,
    Magazine, Mode, PlatformRef, Posture, SatelliteState, ScriptedTarget, Speed, Steer,
    TankerState, Task,
};
use crate::picture::SidePictures;
use crate::orders::{Order, OrderQueue};
use crate::probe::ProbeState;
use crate::rules::Rules;
use crate::snapshot::base_resources;
use crate::world::{
    FIRST_ENGINE_ID, ForceFineTick, SideInfo, Sides, UnitIdAlloc, entity, spawn_unit,
};

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("scenario YAML is invalid: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("start_utc '{0}' is not a UTC time like 2026-10-27T06:00:00Z")]
    StartTime(String),
    #[error("unit id {0} is used more than once")]
    DuplicateUnit(u32),
    #[error("unit {id} ({name}): {reason}")]
    InvalidUnit {
        id: u32,
        name: String,
        reason: String,
    },
    #[error("side {side}: {reason}")]
    InvalidSide { side: usize, reason: String },
    #[error("order {index} (tick {tick}): {reason}")]
    InvalidOrder {
        index: usize,
        tick: u64,
        reason: String,
    },
    #[error("generated_units: {0}")]
    Generated(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// Real bases of each side's countries are active at start (doc 03 §2).
    #[default]
    Garrisoned,
    /// No bases; place them with `PlaceBase` orders.
    Deploy,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub seed: u64,
    /// War start, `YYYY-MM-DDTHH:MM:SSZ`.
    pub start_utc: String,
    /// Enables the P0 determinism probe system (see `probe.rs`).
    #[serde(default)]
    pub determinism_probe: bool,
    /// Benchmark only: every tick is 100 ms (doc 02 §16 budget).
    #[serde(default)]
    pub force_fine_tick: bool,
    #[serde(default)]
    pub sides: Vec<SideSpec>,
    #[serde(default)]
    pub start_mode: StartMode,
    #[serde(default)]
    pub units: Vec<ScenarioUnit>,
    #[serde(default)]
    pub orders: Vec<Order>,
    /// Benchmark populations; one block or a list.
    #[serde(default, deserialize_with = "one_or_many")]
    pub generated_units: Vec<GeneratedUnits>,
    /// Test-only straight-and-level targets with a weapon's signature.
    #[serde(default)]
    pub scripted_targets: Vec<ScriptedTargetSpec>,
    /// Test-only terrain built in memory; replaces the region packs.
    #[serde(default)]
    pub test_terrain: Option<TestTerrain>,
    /// Golden-scenario variants and bands, read by `wb golden` (doc 08 §3).
    #[serde(default)]
    pub golden: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideSpec {
    pub name: String,
    /// Content country ids.
    pub countries: Vec<String>,
    #[serde(default)]
    pub player: bool,
}

/// A bare unit (P0: position only) or a group of platforms. A group with id
/// `n` and `count` members gives the members ids `n+1 … n+count`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioUnit {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default = "one")]
    pub count: u32,
    /// Content base id it is stationed at.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub loadout: Option<String>,
    #[serde(default)]
    pub lat_deg: Option<f64>,
    #[serde(default)]
    pub lon_deg: Option<f64>,
    #[serde(default)]
    pub alt_m: Option<f64>,
    #[serde(default)]
    pub heading_deg: f64,
    #[serde(default)]
    pub speed_mps: Option<f64>,
    /// Content system id (S-400 battalion): its components as one group at
    /// the unit's position, instead of `platform`.
    #[serde(default)]
    pub system: Option<String>,
    /// Emission control at start; the system's or doctrine's default otherwise.
    #[serde(default)]
    pub emcon: Option<Emcon>,
}

fn one() -> u32 {
    1
}

fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<GeneratedUnits>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(GeneratedUnits),
        Many(Vec<GeneratedUnits>),
    }
    Ok(match Option::<OneOrMany>::deserialize(d)? {
        None => Vec::new(),
        Some(OneOrMany::One(g)) => vec![g],
        Some(OneOrMany::Many(v)) => v,
    })
}

/// A test-only target (G1): flies straight and level with the signature of
/// a content weapon.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptedTargetSpec {
    pub id: u32,
    pub name: String,
    pub weapon: String,
    pub country: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
    pub heading_deg: f64,
    pub speed_mps: f64,
}

/// A straight ridge: `[lon, lat]` ends in degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RidgeSpec {
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub height_m: f64,
    pub half_width_m: f64,
}

/// Synthetic terrain for goldens and tests that must run without packs
/// (`wb_geo::synthetic`). Saved with the war, so a reload rebuilds it.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestTerrain {
    /// South-west corner of the first 1° tile, degrees.
    pub west: i16,
    pub south: i16,
    #[serde(default = "one_u16")]
    pub cols: u16,
    #[serde(default = "one_u16")]
    pub rows: u16,
    #[serde(default)]
    pub base_m: i16,
    #[serde(default)]
    pub ridges: Vec<RidgeSpec>,
}

fn one_u16() -> u16 {
    1
}

impl TestTerrain {
    pub fn geo(&self) -> Result<wb_geo::Geo, String> {
        let ridges: Vec<wb_geo::synthetic::Ridge> = self
            .ridges
            .iter()
            .map(|r| wb_geo::synthetic::Ridge {
                from: LatLon::from_deg(r.from[1], r.from[0]),
                to: LatLon::from_deg(r.to[1], r.to[0]),
                height_m: r.height_m,
                half_width_m: r.half_width_m,
            })
            .collect();
        let pack = wb_geo::synthetic::ridge_pack(
            "test-terrain",
            self.west,
            self.south,
            self.cols,
            self.rows,
            self.base_m,
            &ridges,
        )
        .map_err(|e| e.to_string())?;
        let mut geo = wb_geo::Geo::new();
        geo.add_pack(pack).map_err(|e| e.to_string())?;
        Ok(geo)
    }
}

/// Benchmark population: groups spread over an area, each on a patrol.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedUnits {
    /// Platforms in total (members, not groups).
    pub count: u32,
    pub group_size: u32,
    /// Platform ids, used in turn.
    pub platforms: Vec<String>,
    pub country: String,
    /// `[south, west, north, east]` in degrees.
    pub area: [f64; 4],
    /// Patrol circle radius range, km.
    pub patrol_radius_km: [f64; 2],
}

impl Scenario {
    /// Parses and checks what can be checked without content.
    pub fn from_yaml(yaml: &str) -> Result<Self, ScenarioError> {
        let scenario: Scenario = serde_yaml::from_str(yaml)?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn validate(&self) -> Result<(), ScenarioError> {
        parse_utc_ms(&self.start_utc)?;
        let mut seen = BTreeSet::new();
        for u in &self.units {
            let bad = |reason: &str| ScenarioError::InvalidUnit {
                id: u.id,
                name: u.name.clone(),
                reason: reason.to_owned(),
            };
            if u.platform.is_some() && u.system.is_some() {
                return Err(bad("give a platform or a system, not both"));
            }
            let ids = if u.system.is_some() {
                // Members are counted from content at build time; reserve the id.
                u.id..=u.id
            } else if u.platform.is_some() {
                if u.count == 0 || u.count > 64 {
                    return Err(bad("count must be 1–64"));
                }
                u.id..=u.id + u.count
            } else {
                u.id..=u.id
            };
            for id in ids {
                if id >= FIRST_ENGINE_ID {
                    return Err(bad("ids must be below 1000000"));
                }
                if !seen.insert(id) {
                    return Err(ScenarioError::DuplicateUnit(id));
                }
            }
            if let Some(lat) = u.lat_deg
                && !(-90.0..=90.0).contains(&lat)
            {
                return Err(bad("lat_deg must be between -90 and 90"));
            }
            if let Some(lon) = u.lon_deg
                && !(-180.0..=180.0).contains(&lon)
            {
                return Err(bad("lon_deg must be between -180 and 180"));
            }
            if u.alt_m.is_some_and(|a| !a.is_finite()) || !u.heading_deg.is_finite() {
                return Err(bad("alt_m and heading_deg must be finite numbers"));
            }
            if u.speed_mps.is_some_and(|s| !s.is_finite() || s < 0.0) {
                return Err(bad("speed_mps must be zero or positive"));
            }
            let unit = u.platform.is_some() || u.system.is_some();
            match (unit, &u.base, u.lat_deg.zip(u.lon_deg)) {
                (false, _, None) => return Err(bad("a bare unit needs lat_deg and lon_deg")),
                (true, Some(_), Some(_)) => {
                    return Err(bad("give a base or a position, not both"));
                }
                _ => {}
            }
            if u.system.is_some() && u.lat_deg.zip(u.lon_deg).is_none() {
                return Err(bad("a system needs lat_deg and lon_deg"));
            }
        }
        Ok(())
    }

    /// Builds the world: resources, bases, units, then the scenario's orders
    /// (validated like any other order; `geo` checks ship routes).
    pub(crate) fn build(
        &self,
        rules: Rules,
        geo: Option<&wb_geo::Geo>,
    ) -> Result<World, ScenarioError> {
        let start_utc_ms = parse_utc_ms(&self.start_utc)?;
        let mut world = World::new();
        base_resources(&mut world, rules.clone());
        world.insert_resource(SimClock {
            tick: 0,
            sim_time_ms: 0,
            tick_len: TickLen::Coarse,
            start_utc_ms,
        });
        world.insert_resource(RngSeed(self.seed));
        world.insert_resource(ForceFineTick(self.force_fine_tick));
        world.insert_resource(UnitIdAlloc {
            next: FIRST_ENGINE_ID,
        });
        world.insert_resource(OrderQueue::default());
        world.insert_resource(self.sides(&rules)?);
        if self.determinism_probe {
            world.insert_resource(ProbeState::new());
        }

        if self.start_mode == StartMode::Garrisoned {
            spawn_real_bases(&mut world, &rules);
        }
        let mut units: Vec<&ScenarioUnit> = self.units.iter().collect();
        units.sort_by_key(|u| u.id);
        for u in units {
            self.spawn(&mut world, &rules, u)?;
        }
        for (i, gen_spec) in self.generated_units.iter().enumerate() {
            generate(&mut world, &rules, self.seed.wrapping_add(i as u64), gen_spec)?;
        }
        for t in &self.scripted_targets {
            spawn_scripted(&mut world, &rules, t)?;
        }
        if let Some(t) = &self.test_terrain {
            t.geo().map_err(ScenarioError::Generated)?;
            world.insert_resource(t.clone());
        }
        let n_sides = world.resource::<Sides>().len();
        world.insert_resource(SidePictures::new(n_sides));
        for (index, o) in self.orders.iter().enumerate() {
            crate::orders::submit(&mut world, geo, o).map_err(|reason| {
                ScenarioError::InvalidOrder {
                    index,
                    tick: o.tick,
                    reason,
                }
            })?;
        }
        Ok(world)
    }

    fn sides(&self, rules: &Rules) -> Result<Sides, ScenarioError> {
        let mut sides = Vec::new();
        let mut country_ix = Vec::new();
        let mut used = BTreeSet::new();
        for (i, s) in self.sides.iter().enumerate() {
            let mut ixs = Vec::new();
            for c in &s.countries {
                let ix = country(rules, c).ok_or_else(|| ScenarioError::InvalidSide {
                    side: i,
                    reason: format!("unknown country `{c}`"),
                })?;
                if !used.insert(ix) {
                    return Err(ScenarioError::InvalidSide {
                        side: i,
                        reason: format!("country `{c}` is on more than one side"),
                    });
                }
                ixs.push(ix);
            }
            ixs.sort();
            country_ix.push(ixs);
            sides.push(SideInfo {
                name: s.name.clone(),
                countries: s.countries.clone(),
                player: s.player,
            });
        }
        if sides.len() > 8 {
            return Err(ScenarioError::InvalidSide {
                side: 8,
                reason: "at most 8 sides".into(),
            });
        }
        Ok(Sides { sides, country_ix })
    }

    fn spawn(
        &self,
        world: &mut World,
        rules: &Rules,
        u: &ScenarioUnit,
    ) -> Result<(), ScenarioError> {
        let bad = |reason: String| ScenarioError::InvalidUnit {
            id: u.id,
            name: u.name.clone(),
            reason,
        };
        if let Some(sid) = &u.system {
            return spawn_system(world, rules, u, sid);
        }
        let Some(pid) = &u.platform else {
            // P0 bare unit.
            let p = LatLon::from_deg(u.lat_deg.unwrap_or(0.0), u.lon_deg.unwrap_or(0.0));
            spawn_unit(
                world,
                UnitId(u.id),
                (
                    Identity {
                        unit: UnitId(u.id),
                        name: u.name.clone(),
                    },
                    Kinematics {
                        lat: p.lat,
                        lon: p.lon,
                        alt_m: u.alt_m.unwrap_or(0.0),
                        heading: normalize_bearing(deg_to_rad(u.heading_deg)),
                        speed_mps: u.speed_mps.unwrap_or(0.0),
                        climb_mps: 0.0,
                    },
                ),
            );
            return Ok(());
        };
        let pix = rules
            .platform_by_id(pid)
            .ok_or_else(|| bad(format!("unknown platform `{pid}`")))?;
        let cid = u
            .country
            .as_deref()
            .ok_or_else(|| bad("needs a country".into()))?;
        let cix = country(rules, cid).ok_or_else(|| bad(format!("unknown country `{cid}`")))?;
        let side = world
            .resource::<Sides>()
            .side_of(cix)
            .ok_or_else(|| bad(format!("country `{cid}` is on no side")))?;
        let loadout = match &u.loadout {
            Some(l) => Some(
                rules
                    .db
                    .loadouts
                    .iter()
                    .position(|x| rules.db.str(x.id) == l && x.platform == pix)
                    .map(|i| LoadoutIx(i as u32))
                    .ok_or_else(|| bad(format!("no loadout `{l}` for `{pid}`")))?,
            ),
            None => None,
        };
        let emcon = u.emcon;
        if rules.platform(pix).orbit.is_some() {
            if u.base.is_some() || u.lat_deg.is_some() {
                return Err(bad("a satellite takes no base or position; its orbit places it".into()));
            }
            spawn_unit(
                world,
                UnitId(u.id),
                (
                    Identity {
                        unit: UnitId(u.id),
                        name: u.name.clone(),
                    },
                    Allegiance { side, country: cix },
                    PlatformRef(pix),
                    Kinematics::default(),
                    SatelliteState::default(),
                ),
            );
            return Ok(());
        }
        let at = match &u.base {
            None if u.lat_deg.is_none() => {
                return Err(bad("needs a base or lat_deg and lon_deg".into()));
            }
            Some(b) => {
                let info = bases::list(world)
                    .into_iter()
                    .find(|x| {
                        x.side == side
                            && x.content.is_some_and(|ix| {
                                rules.db.str(rules.db.bases[ix.0 as usize].id) == b
                            })
                    })
                    .ok_or_else(|| bad(format!("`{b}` is not an active base of this side")))?;
                bases::suitable(rules, &info, rules.platform(pix)).map_err(bad)?;
                Start::Base(info.id)
            }
            None => Start::Air {
                at: LatLon::from_deg(u.lat_deg.unwrap_or(0.0), u.lon_deg.unwrap_or(0.0)),
                alt_m: u.alt_m,
                heading: deg_to_rad(u.heading_deg),
                speed_mps: u.speed_mps,
            },
        };
        let members: Vec<UnitId> = (1..=u.count).map(|i| UnitId(u.id + i)).collect();
        spawn_group(
            world,
            rules,
            UnitId(u.id),
            &u.name,
            &members,
            pix,
            Allegiance { side, country: cix },
            loadout,
            at,
            Task::Idle,
        );
        let default = default_emcon(rules, cix, None);
        for m in &members {
            if let Some(e) = entity(world, *m) {
                world
                    .entity_mut(e)
                    .insert(EmconState(emcon.unwrap_or(default)));
            }
        }
        Ok(())
    }
}

/// Starting emission control: the system's default, else the country
/// doctrine's (doc 03 §11).
fn default_emcon(rules: &Rules, country: CountryIx, system: Option<wb_content::compiled::SystemIx>) -> Emcon {
    if let Some(s) = system {
        return rules.db.systems[s.0 as usize].emcon_default;
    }
    let c = &rules.db.countries[country.0 as usize];
    rules.db.doctrines[c.doctrine.0 as usize].emcon_default
}

/// A composite system: every component platform (count each) as one
/// stationary group at the unit's position.
fn spawn_system(world: &mut World, rules: &Rules, u: &ScenarioUnit, sid: &str) -> Result<(), ScenarioError> {
    let bad = |reason: String| ScenarioError::InvalidUnit {
        id: u.id,
        name: u.name.clone(),
        reason,
    };
    let six = rules
        .db
        .systems
        .iter()
        .position(|s| rules.db.str(s.id) == sid)
        .map(|i| wb_content::compiled::SystemIx(i as u32))
        .ok_or_else(|| bad(format!("unknown system `{sid}`")))?;
    let cid = u.country.as_deref().ok_or_else(|| bad("needs a country".into()))?;
    let cix = country(rules, cid).ok_or_else(|| bad(format!("unknown country `{cid}`")))?;
    let side = world
        .resource::<Sides>()
        .side_of(cix)
        .ok_or_else(|| bad(format!("country `{cid}` is on no side")))?;
    let sys = &rules.db.systems[six.0 as usize];
    let comps: Vec<(PlatformIx, u32)> = sys.components.iter().map(|c| (c.platform, c.count)).collect();
    let total: u32 = comps.iter().map(|c| c.1).sum();
    if total == 0 || total > 64 {
        return Err(bad("a system needs 1–64 components".into()));
    }
    for id in u.id + 1..=u.id + total {
        if id >= FIRST_ENGINE_ID || entity(world, UnitId(id)).is_some() {
            return Err(ScenarioError::DuplicateUnit(id));
        }
    }
    let at = LatLon::from_deg(u.lat_deg.unwrap_or(0.0), u.lon_deg.unwrap_or(0.0));
    let heading = normalize_bearing(deg_to_rad(u.heading_deg));
    let emcon = EmconState(u.emcon.unwrap_or(default_emcon(rules, cix, Some(six))));
    let mut members = Vec::new();
    let mut next = u.id + 1;
    for (pix, count) in comps {
        let p = rules.platform(pix);
        for i in 0..count {
            let id = UnitId(next);
            next += 1;
            let slot = members.len() as u8;
            let name = format!("{} {} {}", u.name, rules.db.str(p.short_name), i + 1);
            spawn_unit(
                world,
                id,
                (
                    Identity { unit: id, name },
                    Allegiance { side, country: cix },
                    PlatformRef(pix),
                    Kinematics {
                        lat: at.lat,
                        lon: at.lon,
                        alt_m: 0.0,
                        heading,
                        speed_mps: 0.0,
                        climb_mps: 0.0,
                    },
                    Steer::default(),
                    GroupMember { group: UnitId(u.id), slot },
                    BaseRef { home: None },
                    Posture::Moving,
                    Magazine {
                        loadout: None,
                        rounds: Vec::new(),
                        initial: 0,
                    },
                    emcon,
                ),
            );
            members.push(id);
        }
    }
    spawn_unit(
        world,
        UnitId(u.id),
        (
            Identity {
                unit: UnitId(u.id),
                name: u.name.clone(),
            },
            Allegiance { side, country: cix },
            Group {
                members,
                system: Some(six),
                ..Group::default()
            },
        ),
    );
    Ok(())
}

fn spawn_scripted(world: &mut World, rules: &Rules, t: &ScriptedTargetSpec) -> Result<(), ScenarioError> {
    let bad = |reason: String| ScenarioError::InvalidUnit {
        id: t.id,
        name: t.name.clone(),
        reason,
    };
    if t.id >= FIRST_ENGINE_ID || entity(world, UnitId(t.id)).is_some() {
        return Err(ScenarioError::DuplicateUnit(t.id));
    }
    let wix = rules
        .db
        .weapons
        .iter()
        .position(|w| rules.db.str(w.id) == t.weapon)
        .map(|i| wb_content::compiled::WeaponIx(i as u32))
        .ok_or_else(|| bad(format!("unknown weapon `{}`", t.weapon)))?;
    if rules.db.weapons[wix.0 as usize].signature.is_none() {
        return Err(bad(format!("weapon `{}` has no signature", t.weapon)));
    }
    let cix = country(rules, &t.country).ok_or_else(|| bad(format!("unknown country `{}`", t.country)))?;
    let side = world
        .resource::<Sides>()
        .side_of(cix)
        .ok_or_else(|| bad(format!("country `{}` is on no side", t.country)))?;
    let p = LatLon::from_deg(t.lat_deg, t.lon_deg);
    spawn_unit(
        world,
        UnitId(t.id),
        (
            Identity {
                unit: UnitId(t.id),
                name: t.name.clone(),
            },
            Allegiance { side, country: cix },
            Kinematics {
                lat: p.lat,
                lon: p.lon,
                alt_m: t.alt_m,
                heading: normalize_bearing(deg_to_rad(t.heading_deg)),
                speed_mps: t.speed_mps,
                climb_mps: 0.0,
            },
            ScriptedTarget { weapon: wix },
        ),
    );
    Ok(())
}

/// Where a new group starts.
#[derive(Clone, Copy)]
enum Start {
    Base(UnitId),
    Air {
        at: LatLon,
        alt_m: Option<f64>,
        heading: f64,
        speed_mps: Option<f64>,
    },
}

fn country(rules: &Rules, id: &str) -> Option<CountryIx> {
    rules
        .db
        .countries
        .iter()
        .position(|c| rules.db.str(c.id) == id)
        .map(|i| CountryIx(i as u32))
}

/// Garrisoned start: every content base of a side's country, in content order.
fn spawn_real_bases(world: &mut World, rules: &Rules) {
    let sides = world.resource::<Sides>().clone();
    for (i, b) in rules.db.bases.iter().enumerate() {
        let Some(side) = sides.side_of(b.country) else {
            continue;
        };
        let (kind, air, port) = content_kind(b);
        let id = world.resource_mut::<UnitIdAlloc>().take();
        spawn_unit(
            world,
            id,
            (
                Identity {
                    unit: id,
                    name: rules.db.str(b.name).to_owned(),
                },
                Allegiance {
                    side,
                    country: b.country,
                },
                BaseSite {
                    content: Some(wb_content::compiled::BaseIx(i as u32)),
                    kind,
                    air,
                    port,
                    at: LatLon::new(b.lat_rad, b.lon_rad),
                    elevation_m: b.elevation_m,
                    runway_m: b.runway_length_m,
                    aircraft_capacity: b.aircraft_capacity,
                    ship_berths: b.ship_berths,
                    active_at_ms: 0,
                    active: true,
                    stationed: Vec::new(),
                },
            ),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_group(
    world: &mut World,
    rules: &Rules,
    gid: UnitId,
    name: &str,
    members: &[UnitId],
    pix: PlatformIx,
    allegiance: Allegiance,
    loadout: Option<LoadoutIx>,
    start: Start,
    task: Task,
) {
    let p = rules.platform(pix);
    let bases = bases::list(world);
    let (base_site, kin, mode) = match start {
        Start::Base(b) => {
            let s = bases.iter().find(|x| x.id == b).expect("checked base");
            (
                Some(b),
                Kinematics {
                    lat: s.at.lat,
                    lon: s.at.lon,
                    alt_m: s.elevation_m,
                    ..Kinematics::default()
                },
                Mode::AtBase { base: b },
            )
        }
        Start::Air {
            at,
            alt_m,
            heading,
            speed_mps,
        } => {
            let alt = alt_m.unwrap_or(rules.cruise_alt_m(p));
            (
                None,
                Kinematics {
                    lat: at.lat,
                    lon: at.lon,
                    alt_m: alt,
                    heading: normalize_bearing(heading),
                    speed_mps: speed_mps.unwrap_or(rules.cruise_mps(p, alt)),
                    climb_mps: 0.0,
                },
                Mode::Task,
            )
        }
    };
    let home = base_site.or_else(|| {
        bases::nearest_suitable(rules, &bases, allegiance.side, p, kin.pos(), None).map(|b| b.id)
    });
    let rounds = loadout
        .map(|l| bases::loadout_rounds(rules, l))
        .unwrap_or_default();
    let initial = rounds.iter().map(|(_, n)| n).sum();
    for (slot, m) in members.iter().enumerate() {
        let mname = if members.len() == 1 {
            name.to_owned()
        } else {
            format!("{name} {}", slot + 1)
        };
        let e = spawn_unit(
            world,
            *m,
            (
                Identity {
                    unit: *m,
                    name: mname,
                },
                allegiance,
                PlatformRef(pix),
                kin,
                Steer::default(),
                GroupMember {
                    group: gid,
                    slot: slot as u8,
                },
                BaseRef { home },
                match base_site {
                    Some(base) => Posture::AtBase {
                        base,
                        ready_at_ms: 0,
                        ready: true,
                    },
                    None => Posture::Moving,
                },
                Magazine {
                    loadout,
                    rounds: rounds.clone(),
                    initial,
                },
            ),
        );
        if let Some(cap) = rules.fuel_capacity(p) {
            world.entity_mut(e).insert(Fuel {
                kg: cap,
                max_kg: cap,
                bingo_kg: 0.0,
                bingo_base: None,
                reserve_at_bingo_kg: None,
            });
        }
        if let Some(t) = &p.tanker {
            world.entity_mut(e).insert(TankerState {
                offload_left_kg: t.offload_kg,
            });
        }
    }
    if let Some(b) = base_site
        && let Some(be) = entity(world, b)
        && let Some(mut site) = world.get_mut::<BaseSite>(be)
    {
        site.stationed.extend(members.iter().copied());
        site.stationed.sort();
    }
    spawn_unit(
        world,
        gid,
        (
            Identity {
                unit: gid,
                name: name.to_owned(),
            },
            allegiance,
            Group {
                members: members.to_vec(),
                task,
                mode,
                ..Group::default()
            },
        ),
    );
}

/// Benchmark population, drawn from the scenario seed.
fn generate(
    world: &mut World,
    rules: &Rules,
    seed: u64,
    g: &GeneratedUnits,
) -> Result<(), ScenarioError> {
    let err = |m: String| ScenarioError::Generated(m);
    if g.group_size == 0 || g.group_size > 16 || g.platforms.is_empty() {
        return Err(err("group_size must be 1–16 and platforms not empty".into()));
    }
    let pixs = g
        .platforms
        .iter()
        .map(|p| {
            rules
                .platform_by_id(p)
                .ok_or_else(|| err(format!("unknown platform `{p}`")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cix = country(rules, &g.country)
        .ok_or_else(|| err(format!("unknown country `{}`", g.country)))?;
    let side = world
        .resource::<Sides>()
        .side_of(cix)
        .ok_or_else(|| err(format!("country `{}` is on no side", g.country)))?;
    let [south, west, north, east] = g.area;
    let groups = g.count.div_ceil(g.group_size);
    let mut left = g.count;
    for i in 0..groups {
        let r = |n: u32| rng(seed, RngPurpose::ScenarioGen, u64::from(i), 0, n);
        let pix = pixs[i as usize % pixs.len()];
        let p = rules.platform(pix);
        let at = LatLon::from_deg(south + (north - south) * r(0), west + (east - west) * r(1));
        let size = left.min(g.group_size);
        left -= size;
        let gid = world.resource_mut::<UnitIdAlloc>().take();
        let members: Vec<UnitId> = (0..size)
            .map(|_| world.resource_mut::<UnitIdAlloc>().take())
            .collect();
        let radius_m = 1_000.0
            * (g.patrol_radius_km[0] + (g.patrol_radius_km[1] - g.patrol_radius_km[0]) * r(2));
        let task = if i % 2 == 0 {
            Task::PatrolCircle {
                centre: at,
                radius_m,
                alt_m: None,
                speed: Speed::Cruise,
            }
        } else {
            let wps = (0..3)
                .map(|n| destination_point(at, r(3 + n) * TAU, radius_m * (0.5 + r(6 + n))))
                .collect();
            Task::PatrolRoute {
                waypoints: wps,
                looped: true,
                alt_m: None,
                speed: Speed::Cruise,
            }
        };
        let heading = r(9) * TAU;
        spawn_group(
            world,
            rules,
            gid,
            &format!("{} {}", rules.db.str(p.name), i + 1),
            &members,
            pix,
            Allegiance { side, country: cix },
            None,
            Start::Air {
                at,
                alt_m: None,
                heading,
                speed_mps: None,
            },
            task,
        );
    }
    Ok(())
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` to Unix epoch milliseconds.
pub fn parse_utc_ms(s: &str) -> Result<i64, ScenarioError> {
    let err = || ScenarioError::StartTime(s.to_owned());
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return Err(err());
    }
    let num = |range: std::ops::Range<usize>| -> Result<i64, ScenarioError> {
        s[range].parse::<u32>().map(i64::from).map_err(|_| err())
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || se > 59
    {
        return Err(err());
    }
    let days = days_from_civil(y, mo, d);
    Ok(((days * 24 + h) * 60 + mi) * 60_000 + se * 1_000)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc() {
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(
            parse_utc_ms("2000-03-01T00:00:00Z").unwrap(),
            951_868_800_000
        );
        assert_eq!(
            parse_utc_ms("2026-10-27T06:30:15Z").unwrap(),
            1_793_082_615_000
        );
        assert!(parse_utc_ms("2026-02-29T00:00:00Z").is_err());
        assert!(parse_utc_ms("2026-10-27 06:30:15").is_err());
    }

    #[test]
    fn rejects_duplicate_units() {
        let yaml = "name: t\nseed: 1\nstart_utc: 2026-10-27T00:00:00Z\nunits:\n  - {id: 1, name: a, lat_deg: 0, lon_deg: 0}\n  - {id: 1, name: b, lat_deg: 0, lon_deg: 0}\n";
        let err = Scenario::from_yaml(yaml).unwrap_err();
        assert_eq!(err.to_string(), "unit id 1 is used more than once");
    }
}
