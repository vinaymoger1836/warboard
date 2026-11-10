//! War setup (doc 03 §2, doc 12 §4, ADR-046): the browser's choices for a new
//! war (coalitions, who controls each, start mode and time, default ROE)
//! turned into a [`Scenario`] the engine builds like any other.
//!
//! **Garrisoned start** (doc 03 §2): every force of every coalition nation's
//! inventory starts in the war. Aircraft and ships go to the inventory's
//! `basing` rows first; whatever the basing table leaves out (and any row a
//! base cannot take) is spread over the nation's bases of the right kind by
//! free capacity, most room first. Sites, systems and ground forces are set up
//! round the nation's capital, cities and bases. Satellites start in orbit.
//! Munitions the inventory's `munition_basing` leaves out are shared out over
//! the nation's bases by capacity. Everything runs in content order with no
//! randomness, so the same setup always gives the same war.
//!
//! **Deploy yourself**: human sides start with open, empty bases and an
//! inventory pool they station with `DeployForce` (see [`crate::deployment`]);
//! AI sides are garrisoned. Satellites start in orbit either way.
//!
//! Nothing here reads a war: it works on the content only.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use wb_content::compiled::{BaseIx, ContentDb, ForceIx, NationIx, PlatformIx};
use wb_content::source::PlaceKind;
use wb_content::source::world::BaseType;
use wb_core::geo::LatLon;

use crate::components::Roe;
use crate::deployment::{Place, default_loadout, place_of, site_position};
use crate::scenario::{BaseStockSpec, Scenario, ScenarioUnit, SideSpec, StartMode};
use crate::world::{Controller, Difficulty, PlayMode};

/// Aircraft per flight at most (doc 03 §4: 1–4 aircraft).
const FLIGHT_SIZE: u32 = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SideSetup {
    pub name: String,
    /// Content nation ids.
    pub nations: Vec<String>,
    pub controller: Controller,
    /// For an AI side; Normal when absent.
    #[serde(default)]
    pub difficulty: Difficulty,
    /// Side default ROE; Charlie when absent.
    #[serde(default = "default_roe")]
    pub roe: Roe,
}

fn default_roe() -> Roe {
    Roe::Charlie
}

/// What the war setup screens produce.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WarSetup {
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub seed: u64,
    /// War start, `YYYY-MM-DDTHH:MM:SSZ`.
    pub start_utc: String,
    #[serde(default)]
    pub start_mode: StartMode,
    /// Two humans: `command_both` or `hot_seat`; derived when absent.
    #[serde(default)]
    pub mode: Option<PlayMode>,
    /// Coalition A and B, in that order.
    pub sides: Vec<SideSetup>,
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("A war needs two coalitions")]
    Sides,
    #[error("{0}")]
    Invalid(String),
}

/// Forces the builder could not place, and why (shown on the setup screen).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Unplaced {
    pub nation: String,
    pub id: String,
    pub count: u32,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetupReport {
    /// Platforms started in the war (aircraft, ships, sites, satellites…).
    pub platforms: u32,
    /// Composite systems started.
    pub systems: u32,
    /// Sides that deploy themselves.
    pub deploy_sides: Vec<u8>,
    pub unplaced: Vec<Unplaced>,
}

pub struct Built {
    pub scenario: Scenario,
    pub report: SetupReport,
}

fn nation_ix(db: &ContentDb, id: &str) -> Option<NationIx> {
    db.nations
        .iter()
        .position(|n| db.str(n.id) == id)
        .map(|i| NationIx(i as u32))
}

/// Builds the scenario for a setup.
pub fn build_scenario(db: &ContentDb, setup: &WarSetup) -> Result<Built, SetupError> {
    if setup.sides.len() != 2 {
        return Err(SetupError::Sides);
    }
    let bad = |m: String| SetupError::Invalid(m);
    let mut used = BTreeSet::new();
    for (i, s) in setup.sides.iter().enumerate() {
        if s.nations.is_empty() {
            return Err(bad(format!("Coalition {} has no nation", ['A', 'B'][i])));
        }
        for n in &s.nations {
            if nation_ix(db, n).is_none() {
                return Err(bad(format!("Unknown nation `{n}`")));
            }
            if !used.insert(n.clone()) {
                return Err(bad(format!("`{n}` is in both coalitions")));
            }
        }
    }
    let controllers: Vec<Controller> = setup.sides.iter().map(|s| s.controller).collect();
    let mode = match setup.mode {
        Some(m) => m,
        None => PlayMode::derive(&controllers),
    };
    mode.fits(&controllers).map_err(bad)?;

    let human: Vec<u8> = (0..2u8)
        .filter(|i| setup.sides[usize::from(*i)].controller == Controller::Human)
        .collect();
    let deploy_sides: Vec<u8> = if setup.start_mode == StartMode::Deploy {
        human.clone()
    } else {
        Vec::new()
    };

    let mut b = Builder {
        db,
        units: Vec::new(),
        next_id: 1_000,
        report: SetupReport {
            deploy_sides: deploy_sides.clone(),
            ..SetupReport::default()
        },
        air_load: BTreeMap::new(),
        ship_load: BTreeMap::new(),
        ground_n: BTreeMap::new(),
        serial: BTreeMap::new(),
    };
    let mut stock = Vec::new();
    for (si, s) in setup.sides.iter().enumerate() {
        let garrison = !deploy_sides.contains(&(si as u8));
        let mut nations: Vec<NationIx> =
            s.nations.iter().filter_map(|n| nation_ix(db, n)).collect();
        nations.sort();
        for n in nations {
            b.nation(n, garrison);
            stock.extend(b.base_stock(n));
        }
    }
    if b.next_id >= crate::world::FIRST_ENGINE_ID {
        return Err(bad("The chosen coalitions field too many units".to_owned()));
    }
    let scenario = Scenario {
        name: setup.name.clone(),
        seed: setup.seed,
        start_utc: setup.start_utc.clone(),
        determinism_probe: false,
        force_fine_tick: false,
        sides: setup
            .sides
            .iter()
            .enumerate()
            .map(|(i, s)| SideSpec {
                name: s.name.clone(),
                nations: s.nations.clone(),
                player: human.first() == Some(&(i as u8)),
                controller: s.controller,
                difficulty: s.difficulty,
                roe: Some(s.roe),
                self_defence: None,
            })
            .collect(),
        start_mode: StartMode::Garrisoned,
        mode: Some(mode),
        deploy_sides,
        units: b.units,
        orders: Vec::new(),
        generated_units: Vec::new(),
        scripted_targets: Vec::new(),
        test_terrain: None,
        base_stock: stock,
        test_territory: Vec::new(),
        golden: None,
        content: None,
    };
    scenario
        .validate()
        .map_err(|e| SetupError::Invalid(e.to_string()))?;
    Ok(Built {
        scenario,
        report: b.report,
    })
}

struct Builder<'a> {
    db: &'a ContentDb,
    units: Vec<ScenarioUnit>,
    next_id: u32,
    report: SetupReport,
    /// Aircraft placed per base, and ships.
    air_load: BTreeMap<BaseIx, u32>,
    ship_load: BTreeMap<BaseIx, u32>,
    /// Ground forces placed so far per nation (anchor rotation).
    ground_n: BTreeMap<NationIx, u32>,
    /// Groups made per nation and force (names).
    serial: BTreeMap<(NationIx, ForceIx), u32>,
}

impl Builder<'_> {
    fn nation_id(&self, n: NationIx) -> String {
        self.db.str(self.db.nations[n.0 as usize].id).to_owned()
    }

    fn unit(&self, id: u32, name: String, n: NationIx) -> ScenarioUnit {
        ScenarioUnit {
            id,
            name,
            platform: None,
            nation: Some(self.nation_id(n)),
            count: 1,
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
            posture: None,
        }
    }

    fn name_of(&mut self, n: NationIx, f: ForceIx) -> String {
        let db = self.db;
        let short = match f {
            ForceIx::Platform(p) => db.str(db.platforms[p.0 as usize].short_name),
            ForceIx::System(x) => db.str(db.systems[x.0 as usize].short_name),
        };
        let s = self.serial.entry((n, f)).or_insert(0);
        *s += 1;
        // Coalition partners may field the same type: the nation id keeps names apart.
        let nation = db.str(db.nations[n.0 as usize].id).to_uppercase();
        format!("{nation} {short} {s}")
    }

    fn fail(&mut self, n: NationIx, f: ForceIx, count: u32, reason: &str) {
        let db = self.db;
        let id = match f {
            ForceIx::Platform(p) => db.platforms[p.0 as usize].id,
            ForceIx::System(x) => db.systems[x.0 as usize].id,
        };
        self.report.unplaced.push(Unplaced {
            nation: self.nation_id(n),
            id: db.str(id).to_owned(),
            count,
            reason: reason.to_owned(),
        });
    }

    /// Bases of a nation, in content order.
    fn bases_of(&self, n: NationIx) -> Vec<BaseIx> {
        self.db
            .bases
            .iter()
            .enumerate()
            .filter(|(_, b)| b.nation == n)
            .map(|(i, _)| BaseIx(i as u32))
            .collect()
    }

    /// Whether the content base can take the platform (doc 03 §3).
    fn can_host(&self, base: BaseIx, p: PlatformIx) -> bool {
        let db = self.db;
        let b = &db.bases[base.0 as usize];
        let pl = &db.platforms[p.0 as usize];
        match place_of(db, ForceIx::Platform(p)) {
            Some(Place::Airfield) => {
                matches!(b.base_type, BaseType::AirBase | BaseType::AirBaseAndPort)
                    && match (pl.mobility.min_runway_m, b.runway_length_m) {
                        (Some(need), Some(have)) => have >= need,
                        _ => true,
                    }
            }
            Some(Place::Port) => matches!(b.base_type, BaseType::Port | BaseType::AirBaseAndPort),
            _ => false,
        }
    }

    fn capacity(&self, base: BaseIx, place: Place) -> u32 {
        let b = &self.db.bases[base.0 as usize];
        let used = match place {
            Place::Airfield => self.air_load.get(&base),
            _ => self.ship_load.get(&base),
        }
        .copied()
        .unwrap_or(0);
        match place {
            Place::Airfield => b.aircraft_capacity.saturating_sub(used),
            _ => b.ship_berths.saturating_sub(used),
        }
    }

    /// Anchors for ground forces: the capital, the cities, then the bases.
    fn anchors(&self, n: NationIx) -> Vec<LatLon> {
        let db = self.db;
        let mut out = Vec::new();
        for kind in [PlaceKind::Capital, PlaceKind::City] {
            for p in &db.places {
                if p.kind == kind && p.nation == Some(n) {
                    out.push(LatLon::new(p.lat_rad, p.lon_rad));
                }
            }
        }
        for b in self.bases_of(n) {
            let base = &db.bases[b.0 as usize];
            out.push(LatLon::new(base.lat_rad, base.lon_rad));
        }
        out
    }

    /// Starts one nation's inventory. `garrison` false (deploy yourself):
    /// only the satellites, which start in orbit.
    fn nation(&mut self, n: NationIx, garrison: bool) {
        let db = self.db;
        let Some(inv) = db.inventories.iter().find(|i| i.nation == n) else {
            return;
        };
        let anchors = self.anchors(n);
        for &(f, count) in &inv.platforms {
            let place = place_of(db, f);
            if place.is_none() {
                // A satellite: one unit each, no base or position.
                if let ForceIx::Platform(p) = f {
                    for _ in 0..count {
                        let id = self.next_id;
                        // The scenario reserves a group's id and one member.
                        self.next_id += 2;
                        let mut u = self.unit(id, String::new(), n);
                        u.name = self.name_of(n, f);
                        u.platform = Some(db.str(db.platforms[p.0 as usize].id).to_owned());
                        self.units.push(u);
                        self.report.platforms += 1;
                    }
                }
                continue;
            }
            if !garrison {
                continue;
            }
            match (f, place) {
                (ForceIx::Platform(p), Some(pl @ (Place::Airfield | Place::Port))) => {
                    self.base_platform(n, f, p, pl, count, inv);
                }
                _ => self.ground(n, f, count, &anchors),
            }
        }
    }

    /// Aircraft and ships: basing rows first, then the most room.
    fn base_platform(
        &mut self,
        n: NationIx,
        f: ForceIx,
        p: PlatformIx,
        place: Place,
        count: u32,
        inv: &wb_content::compiled::Inventory,
    ) {
        let mut left = count;
        let mut plan: Vec<(BaseIx, u32)> = Vec::new();
        for row in inv.basing.iter().filter(|r| r.force == f) {
            let host = db_base_nation(self.db, row.base) == n && self.can_host(row.base, p);
            let take = row.count.min(left);
            if host && take > 0 {
                plan.push((row.base, take));
                left -= take;
                let load = if place == Place::Airfield {
                    &mut self.air_load
                } else {
                    &mut self.ship_load
                };
                *load.entry(row.base).or_insert(0) += take;
            }
        }
        // The rest, one flight (or ship) at a time to the base with most room.
        let chunk = if place == Place::Airfield {
            FLIGHT_SIZE
        } else {
            1
        };
        let hosts: Vec<BaseIx> = self
            .bases_of(n)
            .into_iter()
            .filter(|b| self.can_host(*b, p))
            .collect();
        let mut stranded = 0;
        while left > 0 {
            let take = chunk.min(left);
            let best = hosts
                .iter()
                .map(|b| (self.capacity(*b, place), *b))
                .filter(|(room, _)| *room > 0)
                .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
            let Some((room, base)) = best else {
                stranded += left;
                break;
            };
            let take = take.min(room);
            plan.push((base, take));
            let load = if place == Place::Airfield {
                &mut self.air_load
            } else {
                &mut self.ship_load
            };
            *load.entry(base).or_insert(0) += take;
            left -= take;
        }
        if stranded > 0 {
            let why = if hosts.is_empty() {
                "no base of the nation can take it"
            } else {
                "every suitable base is full"
            };
            self.fail(n, f, stranded, why);
        }
        // One group per (base, flight): merge a base's plan rows, then cut
        // them into flights (ships go one by one).
        let mut per_base: BTreeMap<BaseIx, u32> = BTreeMap::new();
        for (b, c) in plan {
            *per_base.entry(b).or_insert(0) += c;
        }
        for (base, total) in per_base {
            let groups = total.div_ceil(chunk);
            let mut rest = total;
            for g in 0..groups {
                let size = rest.div_ceil(groups - g);
                rest -= size;
                let id = self.next_id;
                self.next_id += 1 + size;
                let name = self.name_of(n, f);
                let mut u = self.unit(id, name, n);
                u.platform = Some(self.db.str(self.db.platforms[p.0 as usize].id).to_owned());
                u.count = size;
                u.base = Some(self.db.str(self.db.bases[base.0 as usize].id).to_owned());
                u.loadout = default_loadout(self.db, p)
                    .map(|l| self.db.str(self.db.loadouts[l.0 as usize].id).to_owned());
                self.units.push(u);
                self.report.platforms += size;
            }
        }
    }

    /// Sites, systems and ground forces round the nation's anchors.
    fn ground(&mut self, n: NationIx, f: ForceIx, count: u32, anchors: &[LatLon]) {
        if anchors.is_empty() {
            self.fail(
                n,
                f,
                count,
                "the nation has no capital, city or base to set up at",
            );
            return;
        }
        for _ in 0..count {
            let i = {
                let c = self.ground_n.entry(n).or_insert(0);
                let i = *c;
                *c += 1;
                i
            };
            let anchor = anchors[i as usize % anchors.len()];
            let at = site_position(anchor, i / anchors.len() as u32);
            let id = self.next_id;
            let name = self.name_of(n, f);
            let mut u = self.unit(id, name, n);
            u.lat_deg = Some(at.lat_deg());
            u.lon_deg = Some(at.lon_deg());
            match f {
                ForceIx::System(x) => {
                    let sy = &self.db.systems[x.0 as usize];
                    let total: u32 = sy.components.iter().map(|c| c.count).sum();
                    u.system = Some(self.db.str(sy.id).to_owned());
                    self.next_id += 1 + total;
                    self.report.systems += 1;
                }
                ForceIx::Platform(p) => {
                    u.platform = Some(self.db.str(self.db.platforms[p.0 as usize].id).to_owned());
                    self.next_id += 2;
                    self.report.platforms += 1;
                }
            }
            self.units.push(u);
        }
    }

    /// Each base's munitions at war start: its `munition_basing` plus a share
    /// of whatever the inventory's totals have beyond it.
    fn base_stock(&self, n: NationIx) -> Vec<BaseStockSpec> {
        let db = self.db;
        let Some(inv) = db.inventories.iter().find(|i| i.nation == n) else {
            return Vec::new();
        };
        let bases = self.bases_of(n);
        if bases.is_empty() {
            return Vec::new();
        }
        let mut stock: BTreeMap<BaseIx, BTreeMap<String, u32>> = BTreeMap::new();
        let mut basing_total: BTreeMap<u32, u32> = BTreeMap::new();
        for (b, rows) in &inv.munition_basing {
            let e = stock.entry(*b).or_default();
            for (w, c) in rows {
                *e.entry(db.str(db.weapons[w.0 as usize].id).to_owned())
                    .or_insert(0) += c;
                *basing_total.entry(w.0).or_insert(0) += c;
            }
        }
        let weights: Vec<(BaseIx, u32)> = bases
            .iter()
            .map(|b| {
                let base = &db.bases[b.0 as usize];
                (*b, (base.aircraft_capacity + base.ship_berths).max(1))
            })
            .collect();
        let wsum: u32 = weights.iter().map(|(_, w)| w).sum();
        let biggest = weights
            .iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(b, _)| *b)
            .expect("bases not empty");
        let mut touched = BTreeSet::new();
        for (w, total) in &inv.munitions {
            let extra = total.saturating_sub(basing_total.get(&w.0).copied().unwrap_or(0));
            if extra == 0 {
                continue;
            }
            let wid = db.str(db.weapons[w.0 as usize].id).to_owned();
            let mut given = 0;
            for (b, wt) in &weights {
                let share = (u64::from(extra) * u64::from(*wt) / u64::from(wsum)) as u32;
                if share > 0 {
                    *stock.entry(*b).or_default().entry(wid.clone()).or_insert(0) += share;
                    touched.insert(*b);
                    given += share;
                }
            }
            if given < extra {
                *stock.entry(biggest).or_default().entry(wid).or_insert(0) += extra - given;
                touched.insert(biggest);
            }
        }
        touched
            .into_iter()
            .map(|b| BaseStockSpec {
                base: db.str(db.bases[b.0 as usize].id).to_owned(),
                munitions: stock.remove(&b).unwrap_or_default(),
            })
            .collect()
    }
}

fn db_base_nation(db: &ContentDb, b: BaseIx) -> NationIx {
    db.bases[b.0 as usize].nation
}

// ---------------------------------------------------------------------------
// What the setup screens show
// ---------------------------------------------------------------------------

/// A nation as the coalition builder lists it. Content only.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NationOption {
    /// `ald`.
    pub id: String,
    pub name: String,
    /// `#RRGGBB`.
    pub colour: String,
    /// The generated flag (doc 04 §3.0): layout and two or three `#RRGGBB`.
    pub flag_pattern: wb_content::source::world::FlagPattern,
    pub flag_colours: Vec<String>,
    pub doctrine: String,
    pub note: String,
    /// Aircraft, ships, sites… in the inventory.
    pub platforms: u32,
    pub systems: u32,
    pub bases: u32,
    /// `[west, south, east, north]` degrees round the nation's bases and
    /// places; what the pack check (ADR-022) needs installed.
    pub extent: [f64; 4],
}

pub fn nation_options(db: &ContentDb) -> Vec<NationOption> {
    let deg = 180.0 / std::f64::consts::PI;
    db.nations
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let nix = NationIx(i as u32);
            let inv = db.inventories.iter().find(|x| x.nation == nix);
            let (mut platforms, mut systems) = (0, 0);
            if let Some(inv) = inv {
                for (f, c) in &inv.platforms {
                    match f {
                        ForceIx::Platform(_) => platforms += c,
                        ForceIx::System(_) => systems += c,
                    }
                }
            }
            let mut pts: Vec<(f64, f64)> = db
                .bases
                .iter()
                .filter(|b| b.nation == nix)
                .map(|b| (b.lon_rad * deg, b.lat_rad * deg))
                .collect();
            let bases = pts.len() as u32;
            pts.extend(
                db.places
                    .iter()
                    .filter(|p| p.nation == Some(nix))
                    .map(|p| (p.lon_rad * deg, p.lat_rad * deg)),
            );
            let extent =
                pts.iter()
                    .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |e, (lon, lat)| {
                        [
                            e[0].min(*lon),
                            e[1].min(*lat),
                            e[2].max(*lon),
                            e[3].max(*lat),
                        ]
                    });
            NationOption {
                id: db.str(n.id).to_owned(),
                name: db.str(n.name).to_owned(),
                colour: db.str(n.colour).to_owned(),
                flag_pattern: n.flag_pattern,
                flag_colours: n
                    .flag_colours
                    .iter()
                    .map(|c| db.str(*c).to_owned())
                    .collect(),
                doctrine: db.str(db.doctrines[n.doctrine.0 as usize].name).to_owned(),
                note: db.str(n.design.note).to_owned(),
                platforms,
                systems,
                bases,
                extent: if pts.is_empty() { [0.0; 4] } else { extent },
            }
        })
        .collect()
}
