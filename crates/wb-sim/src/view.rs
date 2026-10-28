//! `SideView` v1 encoder (doc 06 §3, ADR-002): what one side may see, as a
//! little-endian structure of arrays the UI decodes without copying.
//!
//! Only the side's own platforms and bases are listed; contacts, weapons and
//! effects arrive with P4/P5 (their sections are present, with count 0).
//! Nothing about other sides' units is written (CLAUDE.md rule 1).
//!
//! ```text
//! header (48 bytes)
//!   0  magic "WBSV"        4  version u16       6  side u8      7  reserved u8
//!   8  tick u64           16  sim_time_ms i64
//!  24  units u32          28  contacts u32     32  weapons u32  36  effects u32
//!  40  strings u32        44  string bytes u32
//! sections, each column padded to 8 bytes:
//!   units:    lon f64, lat f64, id u32, flags u32, group u32, alt_m f32,
//!             heading_deg f32, speed_mps f32, symbol u16, fuel_pct u8
//!   contacts: lon f64, lat f64, contact_id u32, alt_m f32, heading_deg f32,
//!             speed_mps f32, ellipse_a_m f32, ellipse_b_m f32, ellipse_rot f32,
//!             age_s f32, symbol u16, count_min u16, count_max u16,
//!             identity u8, level u8
//!   weapons:  lon f64, lat f64, id u32, salvo u32, alt_m f32, heading_deg f32,
//!             speed_mps f32, symbol u16, relation u8
//!   effects:  lon f64, lat f64, alt_m f32, age_ms u32, kind u8
//!   strings:  (id u32, kind u8, len u16, utf-8 bytes)*  — new or changed only
//! ```

use bevy_ecs::prelude::*;
use std::collections::BTreeMap;
use wb_content::source::PlatformCategory;
use wb_content::source::realism::BaseKind;
use wb_core::units::rad_to_deg;

use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseSite, Fuel, Group, GroupMember, Identity, Kinematics, Mode, PlatformRef,
    Posture, ReturnWhy, Task,
};
use crate::rules::Rules;
use crate::world::{UnitIndex, entity};

pub const MAGIC: [u8; 4] = *b"WBSV";
pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 48;

/// `flags` bits.
pub mod flag {
    pub const SELECTABLE: u32 = 1 << 0;
    pub const DAMAGED: u32 = 1 << 1;
    pub const EMITTING: u32 = 1 << 2;
    pub const JAMMING: u32 = 1 << 3;
    /// At a base (landed or berthed); not drawn on the map.
    pub const AT_BASE: u32 = 1 << 4;
    pub const GROUP_LEAD: u32 = 1 << 5;
    pub const BASE: u32 = 1 << 6;
    pub const BINGO: u32 = 1 << 7;
    /// Bits 8–11 hold a [`super::Status`].
    pub const STATUS_SHIFT: u32 = 8;
    pub const STATUS_MASK: u32 = 0xF << STATUS_SHIFT;
}

/// Unit status shown on the card and in labels (doc 10 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    None = 0,
    Idle = 1,
    Transit = 2,
    Patrol = 3,
    OnStation = 4,
    Rtb = 5,
    Refuel = 6,
    Landed = 7,
    Turnaround = 8,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::None => "",
            Status::Idle => "IDLE",
            Status::Transit => "TRANSIT",
            Status::Patrol => "PATROL",
            Status::OnStation => "ON STATION",
            Status::Rtb => "RTB",
            Status::Refuel => "REFUEL",
            Status::Landed => "LANDED",
            Status::Turnaround => "TURNAROUND",
        }
    }
}

/// Status names by value (index = `Status as u8`), for the generated TS.
pub const STATUS_NAMES: &[&str] = &[
    "",
    "IDLE",
    "TRANSIT",
    "PATROL",
    "ON STATION",
    "RTB",
    "REFUEL",
    "LANDED",
    "TURNAROUND",
];

/// `flags` bits by name, for the generated TS.
pub const FLAG_NAMES: &[(&str, u32)] = &[
    ("SELECTABLE", flag::SELECTABLE),
    ("DAMAGED", flag::DAMAGED),
    ("EMITTING", flag::EMITTING),
    ("JAMMING", flag::JAMMING),
    ("AT_BASE", flag::AT_BASE),
    ("GROUP_LEAD", flag::GROUP_LEAD),
    ("BASE", flag::BASE),
    ("BINGO", flag::BINGO),
    ("STATUS_SHIFT", flag::STATUS_SHIFT),
    ("STATUS_MASK", flag::STATUS_MASK),
];

/// Icon codes, shared with `packages/symbols` (atlas) through the generated
/// `symbols.ts`. Append only.
pub const SYMBOLS: &[(u16, &str)] = &[
    (0, "unknown"),
    (1, "fighter"),
    (2, "bomber"),
    (3, "attack"),
    (4, "tanker"),
    (5, "aew"),
    (6, "ew_aircraft"),
    (7, "transport"),
    (8, "maritime_patrol"),
    (9, "drone"),
    (10, "helicopter_attack"),
    (11, "helicopter_utility"),
    (20, "destroyer"),
    (21, "frigate"),
    (22, "corvette"),
    (23, "patrol_ship"),
    (24, "auxiliary"),
    (25, "submarine"),
    (40, "radar"),
    (41, "launcher"),
    (42, "c2"),
    (43, "armour"),
    (44, "artillery"),
    (45, "infantry"),
    (60, "satellite_eo"),
    (61, "satellite_sar"),
    (62, "satellite_sigint"),
    (100, "air_base"),
    (101, "naval_base"),
    (102, "air_naval_base"),
    (103, "road_strip"),
    (104, "heliport"),
    (105, "anchorage"),
    (106, "army_base"),
    (107, "sam_site"),
    (108, "missile_site"),
    (109, "hq"),
];

pub fn platform_symbol(c: PlatformCategory) -> u16 {
    use PlatformCategory::*;
    match c {
        AircraftFighter => 1,
        AircraftBomber => 2,
        AircraftAttack => 3,
        AircraftTanker => 4,
        AircraftAew => 5,
        AircraftEw => 6,
        AircraftTransport => 7,
        AircraftMaritimePatrol => 8,
        AircraftUav => 9,
        HelicopterAttack => 10,
        HelicopterUtility => 11,
        ShipDestroyer => 20,
        ShipFrigate => 21,
        ShipCorvette => 22,
        ShipPatrol => 23,
        ShipAuxiliary => 24,
        SubmarineAttack => 25,
        GroundRadar => 40,
        GroundLauncher => 41,
        GroundC2 => 42,
        GroundArmour => 43,
        GroundArtillery => 44,
        GroundInfantry => 45,
        SatelliteEo => 60,
        SatelliteSar => 61,
        SatelliteSigint => 62,
    }
}

pub fn base_symbol(b: &BaseSite) -> u16 {
    match b.kind {
        BaseKind::AirBase if b.port => 102,
        BaseKind::AirBase => 100,
        BaseKind::NavalBase => 101,
        BaseKind::RoadStrip => 103,
        BaseKind::Heliport => 104,
        BaseKind::Anchorage => 105,
        BaseKind::ArmyBase => 106,
        BaseKind::SamSite => 107,
        BaseKind::MissileSite => 108,
        BaseKind::Hq => 109,
    }
}

/// Status of a member of `g` with this posture.
pub fn status(g: &Group, posture: Option<&Posture>) -> Status {
    if let Some(Posture::AtBase { ready, .. }) = posture {
        return if *ready {
            Status::Landed
        } else {
            Status::Turnaround
        };
    }
    match g.mode {
        Mode::AtBase { .. } => Status::Landed,
        Mode::Return { .. } => Status::Rtb,
        Mode::ToTanker { .. } | Mode::Refuelling { .. } => Status::Refuel,
        Mode::Task if g.on_station => Status::OnStation,
        Mode::Task if g.task == Task::Idle => Status::Idle,
        Mode::Task => Status::Transit,
    }
}

/// String kinds in the strings section.
pub mod string_kind {
    /// Unit, group or base name.
    pub const NAME: u8 = 0;
    /// Platform type name, or base kind.
    pub const TYPE: u8 = 1;
}

/// One row of the units section.
struct Row {
    id: u32,
    flags: u32,
    group: u32,
    lon: f64,
    lat: f64,
    alt: f32,
    heading: f32,
    speed: f32,
    symbol: u16,
    fuel: u8,
}

/// Builds side views; remembers which strings each client already has.
#[derive(Default)]
pub struct ViewEncoder {
    sent: BTreeMap<(u32, u8), String>,
    buf: Vec<u8>,
}

impl ViewEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget sent strings (the client lost its table, e.g. after a reload).
    pub fn reset(&mut self) {
        self.sent.clear();
    }

    /// Address of the last encoded view (WebAssembly clients read it in place).
    pub fn encode_ptr(&self) -> *const u8 {
        self.buf.as_ptr()
    }

    pub fn encode(&mut self, world: &World, side: u8) -> &[u8] {
        let rules = world.resource::<Rules>();
        let clock = world.resource::<SimClock>();
        let mut rows: Vec<Row> = Vec::new();
        let mut strings: Vec<(u32, u8, String)> = Vec::new();
        let mut want = |sent: &mut BTreeMap<(u32, u8), String>, id: u32, kind: u8, s: &str| {
            if sent.get(&(id, kind)).is_none_or(|old| old != s) {
                sent.insert((id, kind), s.to_owned());
                strings.push((id, kind, s.to_owned()));
            }
        };

        for (&uid, &e) in &world.resource::<UnitIndex>().0 {
            if world.get::<Allegiance>(e).map(|a| a.side) != Some(side) {
                continue;
            }
            let Some(ident) = world.get::<Identity>(e) else {
                continue;
            };
            if let Some(b) = world.get::<BaseSite>(e) {
                want(&mut self.sent, uid.0, string_kind::NAME, &ident.name);
                want(
                    &mut self.sent,
                    uid.0,
                    string_kind::TYPE,
                    crate::bases::kind_name(b.kind),
                );
                rows.push(Row {
                    id: uid.0,
                    flags: flag::SELECTABLE | flag::BASE,
                    group: 0,
                    lon: rad_to_deg(b.at.lon),
                    lat: rad_to_deg(b.at.lat),
                    alt: b.elevation_m as f32,
                    heading: 0.0,
                    speed: 0.0,
                    symbol: base_symbol(b),
                    fuel: 255,
                });
                continue;
            }
            if world.get::<Group>(e).is_some() {
                want(&mut self.sent, uid.0, string_kind::NAME, &ident.name);
                continue;
            }
            let (Some(p), Some(k)) = (world.get::<PlatformRef>(e), world.get::<Kinematics>(e))
            else {
                continue;
            };
            let platform = rules.platform(p.0);
            want(&mut self.sent, uid.0, string_kind::NAME, &ident.name);
            want(
                &mut self.sent,
                uid.0,
                string_kind::TYPE,
                rules.db.str(platform.name),
            );
            let member = world.get::<GroupMember>(e);
            let posture = world.get::<Posture>(e);
            let group = member
                .and_then(|m| entity(world, m.group))
                .and_then(|ge| world.get::<Group>(ge));
            let mut flags = flag::SELECTABLE;
            if matches!(posture, Some(Posture::AtBase { .. })) {
                flags |= flag::AT_BASE;
            }
            if member.is_some_and(|m| m.slot == 0) {
                flags |= flag::GROUP_LEAD;
            }
            let fuel = world.get::<Fuel>(e);
            if let Some(g) = group {
                if matches!(
                    g.mode,
                    Mode::Return {
                        why: ReturnWhy::Bingo,
                        ..
                    }
                ) || fuel.is_some_and(|f| f.bingo_kg > 0.0 && f.kg <= f.bingo_kg)
                {
                    flags |= flag::BINGO;
                }
                flags |= (status(g, posture) as u32) << flag::STATUS_SHIFT;
            }
            rows.push(Row {
                id: uid.0,
                flags,
                group: member.map_or(0, |m| m.group.0),
                lon: rad_to_deg(k.lon),
                lat: rad_to_deg(k.lat),
                alt: k.alt_m as f32,
                heading: rad_to_deg(k.heading) as f32,
                speed: k.speed_mps as f32,
                symbol: platform_symbol(platform.category),
                fuel: fuel.map_or(255, |f| {
                    ((f.kg / f.max_kg.max(1e-9)) * 100.0)
                        .round()
                        .clamp(0.0, 100.0) as u8
                }),
            });
        }

        let n = rows.len();
        let b = &mut self.buf;
        b.clear();
        b.extend_from_slice(&MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.push(side);
        b.push(0);
        b.extend_from_slice(&clock.tick.to_le_bytes());
        b.extend_from_slice(&(clock.sim_time_ms as i64).to_le_bytes());
        b.extend_from_slice(&(n as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes()); // contacts (P4)
        b.extend_from_slice(&0u32.to_le_bytes()); // weapons (P5)
        b.extend_from_slice(&0u32.to_le_bytes()); // effects (P5)
        b.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        let strings_len: usize = strings.iter().map(|s| 7 + s.2.len()).sum();
        b.extend_from_slice(&(strings_len as u32).to_le_bytes());
        debug_assert_eq!(b.len(), HEADER_BYTES);

        fn pad(b: &mut Vec<u8>) {
            while !b.len().is_multiple_of(8) {
                b.push(0);
            }
        }
        for r in &rows {
            b.extend_from_slice(&r.lon.to_le_bytes());
        }
        for r in &rows {
            b.extend_from_slice(&r.lat.to_le_bytes());
        }
        for r in &rows {
            b.extend_from_slice(&r.id.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.extend_from_slice(&r.flags.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.extend_from_slice(&r.group.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.extend_from_slice(&r.alt.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.extend_from_slice(&r.heading.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.extend_from_slice(&r.speed.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.extend_from_slice(&r.symbol.to_le_bytes());
        }
        pad(b);
        for r in &rows {
            b.push(r.fuel);
        }
        pad(b);
        // Contacts, weapons, effects: empty in P3.
        for (id, kind, s) in &strings {
            b.extend_from_slice(&id.to_le_bytes());
            b.push(*kind);
            b.extend_from_slice(&(s.len() as u16).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        }
        &self.buf
    }
}
