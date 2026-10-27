//! `country`, `base`, `inventory` and `doctrine`.

use super::common::{Emcon, Provenance, source_enum, source_type};
use std::collections::BTreeMap;

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum BaseType {
        AirBase,
        Port,
        AirBaseAndPort,
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
    #[serde(deny_unknown_fields)]
    pub struct Country {
        /// Lower-case ISO 3166-1 alpha-3, e.g. `ita`.
        pub id: String,
        pub name: String,
        pub iso2: String,
        pub iso3: String,
        /// Flag asset key (packages/symbols), e.g. `it`.
        pub flag: String,
        pub doctrine: String,
        /// Data link families its forces use.
        #[serde(default)]
        pub datalinks: Vec<String>,
        pub provenance: Provenance,
    }

    #[serde(deny_unknown_fields)]
    pub struct Base {
        pub id: String,
        pub name: String,
        pub base_type: BaseType,
        /// Owner country id.
        pub country: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub icao: Option<String>,
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
        pub provenance: Provenance,
    }

    /// A country's forces at a date (doc 04 §3.6).
    #[serde(deny_unknown_fields)]
    pub struct Inventory {
        pub id: String,
        pub country: String,
        /// `YYYY-MM-DD`.
        pub as_of: String,
        /// Platform or system id → quantity.
        #[serde(default)]
        pub platforms: BTreeMap<String, u32>,
        /// Weapon id → rounds in stock.
        #[serde(default)]
        pub munitions: BTreeMap<String, u32>,
        /// Peacetime basing.
        #[serde(default)]
        pub basing: Vec<Basing>,
        pub provenance: Provenance,
    }

    #[serde(deny_unknown_fields)]
    pub struct Basing {
        pub platform: String,
        pub base: String,
        pub count: u32,
    }

    /// AI profile per country (doc 03 §11). Weights are 0..1.
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
        pub provenance: Provenance,
    }
}
