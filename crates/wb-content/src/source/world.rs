//! `nation`, `world`, `archetype`, `base`, `inventory` and `doctrine`.

use super::common::{Design, Emcon, source_enum, source_type};
use std::collections::BTreeMap;

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum BaseType {
        AirBase,
        Port,
        AirBaseAndPort,
    }

    /// Flag layouts (doc 04 §3.0).
    #[serde(rename_all = "snake_case")]
    pub enum FlagPattern {
        /// Off-centre cross.
        NordicCross,
        /// Two or three horizontal bands.
        Horizontal,
        /// Two or three vertical bands.
        Vertical,
        /// Diagonal split.
        Diagonal,
        /// Plain field with a centred disc.
        Disc,
        /// Plain field with a triangle at the hoist.
        Chevron,
    }

    /// What a label in `places` names (doc 11 §2.4).
    #[serde(rename_all = "snake_case")]
    pub enum PlaceKind {
        /// A nation's name, drawn over its land.
        Nation,
        Capital,
        City,
        Sea,
        Gulf,
        Strait,
        /// An island or island group.
        Islands,
        /// A land region (peninsula, plain, mountain range).
        Region,
    }

    /// When a unit fires by default (doc 03 §5).
    #[serde(rename_all = "snake_case")]
    pub enum LaunchDoctrine {
        MaxRange,
        NoEscapeZone,
        Close,
    }
}

source_type! {
    /// A fictional nation (doc 11 §2.1, doc 04 §3.0).
    #[serde(deny_unknown_fields)]
    pub struct Nation {
        /// Three lower-case letters, e.g. `ald`; shown upper-case (`ALD`). Also
        /// the territory code in the region packs.
        pub id: String,
        pub name: String,
        /// Map tint when neutral, `#RRGGBB`; coalition colours override it.
        pub colour: String,
        pub flag: Flag,
        /// Capital's name (a place in `world/`); optional until places exist.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub capital: Option<String>,
        pub doctrine: String,
        /// Data link families its forces use.
        #[serde(default)]
        pub datalinks: Vec<String>,
        pub design: Design,
    }

    /// A generated flag: a simple pattern in two or three colours.
    #[serde(deny_unknown_fields)]
    pub struct Flag {
        pub pattern: FlagPattern,
        /// `#RRGGBB`, two or three of them.
        pub colours: Vec<String>,
    }

    /// A theatre: real terrain with generated borders (doc 11 §2.2).
    #[serde(deny_unknown_fields)]
    pub struct World {
        pub id: String,
        pub name: String,
        /// Region pack whose terrain the theatre uses, e.g. `europe`.
        pub terrain_pack: String,
        /// Fixes the border growth and the generated names.
        pub seed: u64,
        /// `[west, south, east, north]` in degrees: the window borders grow in.
        /// Land outside it is unclaimed.
        pub extent: [f64; 4],
        /// How far a nation of weight 1 grows, in km of open flat land; rougher
        /// terrain and sea count for more. Land no nation reaches is unclaimed.
        pub reach_km: f64,
        pub nations: Vec<NationSeed>,
        /// Hand-edits after growth: cells given to a nation (or unclaimed).
        /// Not `overrides`, which is the meta key for `extends` (doc 04 §3.6).
        #[serde(default)]
        pub border_overrides: Vec<CellOverride>,
    }

    #[serde(deny_unknown_fields)]
    pub struct NationSeed {
        pub nation: String,
        /// `[lon, lat]` in degrees, on land.
        pub seed_at: [f64; 2],
        /// Relative growth weight; larger grows a larger region.
        pub weight: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct CellOverride {
        /// Nation id; none makes the cells unclaimed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub nation: Option<String>,
        /// H3 cell indexes as hex strings.
        pub cells: Vec<String>,
    }

    /// Map labels of a world (doc 11 §2.4): `content/world/places.yaml`.
    #[serde(deny_unknown_fields)]
    pub struct Places {
        pub id: String,
        /// The world these labels belong to.
        pub world: String,
        pub places: Vec<Place>,
    }

    #[serde(deny_unknown_fields)]
    pub struct Place {
        pub name: String,
        pub kind: PlaceKind,
        /// Owner nation for nation, capital and city labels.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub nation: Option<String>,
        /// `[lon, lat]` in degrees.
        pub at: [f64; 2],
        /// Proposed by `geodata places`; a re-run replaces generated entries
        /// and keeps the rest. Remove the flag to keep an edited entry.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pub generated: bool,
    }

    /// Class bands for the stats that decide engagements (doc 04 §4).
    #[serde(deny_unknown_fields)]
    pub struct Archetype {
        pub id: String,
        /// Class name shown in the UI, e.g. `Stealth multirole fighter`.
        pub name: String,
        /// Field path (as in `overrides`) → `[min, max]` in the entry's units.
        /// Values outside a band are warnings, not errors.
        #[serde(default)]
        pub bands: BTreeMap<String, [f64; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub note: Option<String>,
    }

    #[serde(deny_unknown_fields)]
    pub struct Base {
        pub id: String,
        pub name: String,
        pub base_type: BaseType,
        /// Owner nation id.
        pub nation: String,
        pub lat_deg: f64,
        pub lon_deg: f64,
        #[serde(default)]
        pub elevation_m: f64,
        /// Longest runway.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub runway_length_m: Option<f64>,
        /// Aircraft that can be stationed (parking and shelters).
        #[serde(default)]
        pub aircraft_capacity: u32,
        #[serde(default)]
        pub ship_berths: u32,
        pub design: Design,
    }

    /// A nation's forces at war start (doc 04 §3.7).
    #[serde(deny_unknown_fields)]
    pub struct Inventory {
        pub id: String,
        pub nation: String,
        /// Platform or system id → quantity.
        #[serde(default)]
        pub platforms: BTreeMap<String, u32>,
        /// Weapon id → rounds in stock.
        #[serde(default)]
        pub munitions: BTreeMap<String, u32>,
        /// Peacetime basing.
        #[serde(default)]
        pub basing: Vec<Basing>,
        /// Where the munitions are stored: each base's stock at war start.
        #[serde(default)]
        pub munition_basing: Vec<MunitionBasing>,
        pub design: Design,
    }

    #[serde(deny_unknown_fields)]
    pub struct MunitionBasing {
        pub base: String,
        /// Weapon id → rounds stored at this base.
        pub munitions: BTreeMap<String, u32>,
    }

    #[serde(deny_unknown_fields)]
    pub struct Basing {
        pub platform: String,
        pub base: String,
        pub count: u32,
    }

    /// AI profile per nation (doc 03 §11). Weights are 0..1.
    #[serde(deny_unknown_fields)]
    pub struct Doctrine {
        pub id: String,
        pub name: String,
        pub aggression: f64,
        pub risk_tolerance: f64,
        /// How strictly emitters stay off until needed.
        pub emcon_discipline: f64,
        pub emcon_default: Emcon,
        /// Fraction of SAM and AAM stocks held back except for self-defence.
        pub missile_reserve_fraction: f64,
        pub launch_doctrine: LaunchDoctrine,
        pub shots_per_target: u32,
        /// Aircraft per CAP station and rotation period.
        pub cap_aircraft: u32,
        pub cap_rotation_min: f64,
        /// Delay before the AI reacts to a new threat.
        pub reaction_time_s: f64,
        /// IADS layers, longest-range first, as free-text template tags.
        #[serde(default)]
        pub iads_layers: Vec<String>,
        /// Strike package template, e.g. `[sead, escort, strike, jammer]`.
        #[serde(default)]
        pub strike_package: Vec<String>,
        pub design: Design,
    }
}
