//! `ew_suite`, `countermeasure` and `datalink` (doc 02 §8–9).

use super::common::{Band, CountermeasureType, Provenance, Seeker, source_enum, source_type};

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum EwType {
        SelfProtection,
        Escort,
        StandOff,
        Comms,
        Gps,
    }
}

source_type! {
    /// A jammer.
    #[serde(deny_unknown_fields)]
    pub struct EwSuite {
        pub id: String,
        pub name: String,
        pub ew_type: EwType,
        pub bands: Vec<Band>,
        /// Jamming power class `J_dB` used in doc 02 §9.
        pub j_db: f64,
        pub max_range_km: f64,
        /// Radars or links it can jam at once.
        pub simultaneous_targets: u32,
        pub field_of_regard_deg: f64,
        pub provenance: Provenance,
    }

    /// Expendable or active countermeasure.
    #[serde(deny_unknown_fields)]
    pub struct Countermeasure {
        pub id: String,
        pub name: String,
        pub cm_type: CountermeasureType,
        /// `cm_effect` in the endgame: chance a single use defeats a seeker
        /// with zero resistance.
        pub effect: f64,
        /// Seekers it works against.
        pub defeats: Vec<Seeker>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub mass_kg: Option<f64>,
        pub provenance: Provenance,
    }

    #[serde(deny_unknown_fields)]
    pub struct Datalink {
        pub id: String,
        pub name: String,
        /// Detection-to-recipient delay.
        pub latency_s: f64,
        /// Tracks the network can carry at once.
        pub capacity_tracks: u32,
        /// Line-of-sight range between terminals (relays extend it).
        pub range_km: f64,
        /// The network can provide engage-on-remote tracks.
        pub engage_on_remote: bool,
        /// 0..1 resistance to comms jamming.
        pub jam_resistance: f64,
        pub provenance: Provenance,
    }
}
