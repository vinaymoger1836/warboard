//! Source format: the YAML entries in `content/` (doc 04). Field names carry
//! their units (`range_km`, `speed_mach`); the compiler converts to SI.
//!
//! Every file is one entry. Besides the fields of its kind, an entry may have
//! the meta keys of [`EntryMeta`]: `kind` (required), `custom`, `extends` and
//! `overrides`. They are consumed by the loader before the typed fields are
//! read.

pub(crate) mod common;
pub mod ew;
pub mod platform;
pub mod realism;
pub mod sensor;
pub mod system;
pub mod weapon;
pub mod world;

pub use common::{
    AarSystem, Band, Confidence, CountermeasureType, Domain, Emcon, Provenance, Seeker, Source,
    TargetClass,
};
pub use ew::{Countermeasure, Datalink, EwSuite, EwType};
pub use platform::{Orbit, Platform, PlatformCategory, SensorMount, Station, StationKind};
pub use realism::Realism;
pub use sensor::{PayloadKind, Sensor, SensorRole, SensorType};
pub use system::{Loadout, System};
pub use weapon::{Weapon, WeaponCategory};
pub use world::{Base, Country, Doctrine, Inventory};

use common::{source_enum, source_type};
use std::collections::BTreeMap;

source_enum! {
    /// Entry kind; each lives in its own folder under `content/`.
    #[serde(rename_all = "snake_case")]
    pub enum Kind {
        Country,
        Platform,
        System,
        Sensor,
        Weapon,
        EwSuite,
        Countermeasure,
        Datalink,
        Loadout,
        Base,
        Inventory,
        Doctrine,
        Realism,
    }
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Country,
        Kind::Platform,
        Kind::System,
        Kind::Sensor,
        Kind::Weapon,
        Kind::EwSuite,
        Kind::Countermeasure,
        Kind::Datalink,
        Kind::Loadout,
        Kind::Base,
        Kind::Inventory,
        Kind::Doctrine,
        Kind::Realism,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Country => "country",
            Kind::Platform => "platform",
            Kind::System => "system",
            Kind::Sensor => "sensor",
            Kind::Weapon => "weapon",
            Kind::EwSuite => "ew_suite",
            Kind::Countermeasure => "countermeasure",
            Kind::Datalink => "datalink",
            Kind::Loadout => "loadout",
            Kind::Base => "base",
            Kind::Inventory => "inventory",
            Kind::Doctrine => "doctrine",
            Kind::Realism => "realism",
        }
    }

    /// Top-level folder under `content/` (doc 04 §2).
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Country => "countries",
            Kind::Platform => "platforms",
            Kind::System => "systems",
            Kind::Sensor => "sensors",
            Kind::Weapon => "weapons",
            Kind::EwSuite => "ew",
            Kind::Countermeasure => "countermeasures",
            Kind::Datalink => "datalinks",
            Kind::Loadout => "loadouts",
            Kind::Base => "bases",
            Kind::Inventory => "inventories",
            Kind::Doctrine => "doctrine",
            Kind::Realism => "realism",
        }
    }

    pub fn from_folder(folder: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.folder() == folder)
    }
}

source_type! {
    /// Keys every entry may carry besides the fields of its kind.
    pub struct EntryMeta {
        pub kind: Kind,
        /// A user-made variant rather than a real system; may have no sources.
        #[serde(default)]
        pub custom: bool,
        /// Id of the entry this one is based on (same kind). Its fields are
        /// copied, then this entry's own top-level keys replace them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub extends: Option<String>,
        /// Field path (`mobility.max_speed_mach`, `stations[1].max_kg`) →
        /// new value, applied after `extends`. The path must exist.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
        #[cfg_attr(
            feature = "schema",
            schemars(with = "BTreeMap<String, serde_json::Value>")
        )]
        pub overrides: BTreeMap<String, serde_yaml::Value>,
    }

    /// `content/manifest.yaml`.
    #[serde(deny_unknown_fields)]
    pub struct Manifest {
        /// Semantic version of the content set, stored in the pack.
        pub content_version: String,
    }
}

/// A typed, resolved entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Country(Country),
    Platform(Platform),
    System(System),
    Sensor(Sensor),
    Weapon(Weapon),
    EwSuite(EwSuite),
    Countermeasure(Countermeasure),
    Datalink(Datalink),
    Loadout(Loadout),
    Base(Base),
    Inventory(Inventory),
    Doctrine(Doctrine),
    // Boxed: by far the largest entry, and there is one per pack.
    Realism(Box<Realism>),
}

impl Entry {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Entry::Country(e) => &e.provenance,
            Entry::Platform(e) => &e.provenance,
            Entry::System(e) => &e.provenance,
            Entry::Sensor(e) => &e.provenance,
            Entry::Weapon(e) => &e.provenance,
            Entry::EwSuite(e) => &e.provenance,
            Entry::Countermeasure(e) => &e.provenance,
            Entry::Datalink(e) => &e.provenance,
            Entry::Loadout(e) => &e.provenance,
            Entry::Base(e) => &e.provenance,
            Entry::Inventory(e) => &e.provenance,
            Entry::Doctrine(e) => &e.provenance,
            Entry::Realism(e) => &e.provenance,
        }
    }
}
