//! `ew_suite`, `countermeasure` and `datalink` (doc 02 §8–9).

use super::common::{Band, CountermeasureType, Design, Seeker, source_enum, source_type};

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum EwType {
        SelfProtection,
        Escort,
        StandOff,
        Comms,
        Gps,
    }

    #[serde(rename_all = "snake_case")]
    pub enum EwEffect {
        Radar,
        Datalink,
        Navigation,
    }
}

source_type! {
    /// 0–10 game stats per mode (doc 02 §9, doc 04 §3.4).
    #[derive(Default)]
    #[serde(deny_unknown_fields)]
    pub struct EwModes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub self_protect: Option<JamMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub escort: Option<JamMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub stand_off: Option<JamMode>,
    }

    #[serde(deny_unknown_fields)]
    pub struct JamMode {
        pub jam_strength: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub coverage_km: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sector_deg: Option<f64>,
    }

    /// A jammer (doc 02 §9, doc 04 §3.4).
    #[serde(deny_unknown_fields)]
    pub struct EwSuite {
        pub id: String,
        pub name: String,
        #[serde(default)]
        pub modes: EwModes,
        #[serde(default)]
        pub effects: Vec<EwEffect>,
        pub design: Design,
        // Legacy fields for backwards compatibility with P1 schema:
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ew_type: Option<EwType>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub bands: Option<Vec<Band>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub j_db: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_range_km: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub simultaneous_targets: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub field_of_regard_deg: Option<f64>,
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
        pub design: Design,
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
        pub design: Design,
    }
}

impl EwSuite {
    /// Modes with fallback to legacy `ew_type`, `j_db`, `max_range_km`.
    pub fn resolved_modes(&self) -> EwModes {
        let mut modes = self.modes.clone();
        if modes.self_protect.is_none()
            && modes.escort.is_none()
            && modes.stand_off.is_none()
            && let Some(ew_type) = &self.ew_type
        {
            let strength = self
                .j_db
                .map(|j| (j * 0.3).round().clamp(0.0, 10.0))
                .unwrap_or(5.0);
            let mode = JamMode {
                jam_strength: strength,
                coverage_km: self.max_range_km,
                sector_deg: self.field_of_regard_deg,
            };
            match ew_type {
                EwType::SelfProtection => {
                    modes.self_protect = Some(JamMode {
                        jam_strength: strength,
                        coverage_km: None,
                        sector_deg: None,
                    })
                }
                EwType::Escort => modes.escort = Some(mode),
                EwType::StandOff => modes.stand_off = Some(mode),
                _ => modes.stand_off = Some(mode),
            }
        }
        modes
    }

    /// Effects with fallback to `[Radar]` when empty.
    pub fn resolved_effects(&self) -> Vec<EwEffect> {
        if self.effects.is_empty() {
            vec![EwEffect::Radar]
        } else {
            self.effects.clone()
        }
    }
}
