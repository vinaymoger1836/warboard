//! `weapon`: missiles, bombs, shells, rockets, torpedoes (doc 02 §10–11).

use super::common::{
    CountermeasureType, Domain, Provenance, Seeker, TargetClass, one, source_enum, source_type,
};
use std::collections::BTreeMap;

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum WeaponCategory {
        /// Air-to-air missile.
        Aam,
        /// Surface-to-air missile.
        Sam,
        /// Anti-ship missile.
        Ashm,
        /// Land-attack cruise missile.
        Lacm,
        /// Anti-radiation missile.
        Arm,
        /// Air-to-ground missile.
        Agm,
        Bomb,
        Ballistic,
        Rocket,
        Shell,
        Torpedo,
        Loitering,
        Decoy,
    }

    /// Flight model family.
    #[serde(rename_all = "snake_case")]
    pub enum ProfileKind {
        /// Boost, optional sustain, then exponential coast.
        Rocket,
        /// Holds cruise speed until its range is used up.
        AirBreathing,
        /// Unpowered: range from release altitude and speed.
        Glide,
        Ballistic,
    }

    #[serde(rename_all = "snake_case")]
    pub enum Midcourse {
        Inertial,
        Datalink,
        Gps,
        TerrainMatching,
        Command,
    }

    #[serde(rename_all = "snake_case")]
    pub enum Warhead {
        Blast,
        BlastFragmentation,
        Penetrator,
        HitToKill,
        ShapedCharge,
    }
}

source_type! {
    #[serde(deny_unknown_fields)]
    pub struct Weapon {
        pub id: String,
        pub name: String,
        pub category: WeaponCategory,
        /// Domains it can engage.
        pub targets: Vec<Domain>,
        /// Class when this weapon is itself a target (for interceptors).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub target_class: Option<TargetClass>,
        pub profile: FlightProfile,
        pub guidance: Guidance,
        pub warhead_kg: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub warhead: Option<Warhead>,
        /// Base kill probability by target class (doc 02 §11). Guided weapons only.
        #[serde(default)]
        pub pk_base: BTreeMap<TargetClass, f64>,
        /// 0 = fooled like anything else, 1 = immune.
        #[serde(default)]
        pub countermeasure_resistance: BTreeMap<CountermeasureType, f64>,
        /// Circular error probable for GPS/INS and unguided weapons.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cep_m: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub lethal_radius_m: Option<f64>,
        pub mass_kg: f64,
        /// Station interface categories it fits.
        pub station_category: Vec<String>,
        #[serde(default)]
        pub internal_carriage: bool,
        /// Rounds per VLS cell or launcher tube (4 for quad-packed missiles).
        #[serde(default = "one")]
        pub rounds_per_cell: u32,
        /// May fire on a networked track without its own fire-control track.
        #[serde(default)]
        pub engage_on_remote: bool,
        /// Target altitude limits (surface-to-air and air-to-air).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub envelope: Option<AltitudeEnvelope>,
        pub provenance: Provenance,
    }

    #[serde(deny_unknown_fields)]
    pub struct FlightProfile {
        pub kind: ProfileKind,
        #[serde(default)]
        pub boost_s: f64,
        #[serde(default)]
        pub sustain_s: f64,
        /// Peak speed (burnout) or cruise speed for air-breathing weapons.
        pub max_speed_mach: f64,
        /// Coast decay time constant by altitude (rocket profiles).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub coast_tau_s: Option<CoastTau>,
        /// Below this speed the weapon is out of energy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub min_speed_mach: Option<f64>,
        /// Published range; the calibration target (doc 02 §10).
        pub max_range_km: f64,
        #[serde(default)]
        pub min_range_km: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_g: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cruise_alt_m: Option<f64>,
        /// Terminal altitude, e.g. sea-skimming height.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub terminal_alt_m: Option<f64>,
    }

    #[serde(deny_unknown_fields)]
    pub struct CoastTau {
        pub sea_level: f64,
        pub high_alt: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct Guidance {
        #[serde(default)]
        pub midcourse: Vec<Midcourse>,
        pub terminal: Seeker,
        /// Secondary seekers (dual-mode weapons).
        #[serde(default)]
        pub terminal_secondary: Vec<Seeker>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub seeker_range_km: Option<f64>,
        /// 0..1 resistance of the seeker to jamming.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub seeker_eccm: Option<f64>,
    }

    #[serde(deny_unknown_fields)]
    pub struct AltitudeEnvelope {
        pub min_alt_m: f64,
        pub max_alt_m: f64,
    }
}
