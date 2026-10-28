//! `platform`: anything that moves or stands on the map, built from components
//! (doc 04 §1–3.3).

use super::common::{
    AarSystem, Domain, Provenance, TargetClass, no_aar, one, source_enum, source_type,
};
use std::collections::BTreeMap;

source_enum! {
    /// Platform category; the first segment is the folder under
    /// `content/platforms/` and fixes the domain.
    pub enum PlatformCategory {
        #[serde(rename = "aircraft/fighter")]
        AircraftFighter,
        #[serde(rename = "aircraft/bomber")]
        AircraftBomber,
        #[serde(rename = "aircraft/attack")]
        AircraftAttack,
        #[serde(rename = "aircraft/tanker")]
        AircraftTanker,
        #[serde(rename = "aircraft/aew")]
        AircraftAew,
        #[serde(rename = "aircraft/ew")]
        AircraftEw,
        #[serde(rename = "aircraft/transport")]
        AircraftTransport,
        #[serde(rename = "aircraft/maritime_patrol")]
        AircraftMaritimePatrol,
        #[serde(rename = "aircraft/uav")]
        AircraftUav,
        #[serde(rename = "helicopter/attack")]
        HelicopterAttack,
        #[serde(rename = "helicopter/utility")]
        HelicopterUtility,
        #[serde(rename = "ship/destroyer")]
        ShipDestroyer,
        #[serde(rename = "ship/frigate")]
        ShipFrigate,
        #[serde(rename = "ship/corvette")]
        ShipCorvette,
        #[serde(rename = "ship/patrol")]
        ShipPatrol,
        #[serde(rename = "ship/auxiliary")]
        ShipAuxiliary,
        #[serde(rename = "submarine/attack")]
        SubmarineAttack,
        #[serde(rename = "ground/radar")]
        GroundRadar,
        #[serde(rename = "ground/launcher")]
        GroundLauncher,
        #[serde(rename = "ground/c2")]
        GroundC2,
        #[serde(rename = "ground/armour")]
        GroundArmour,
        #[serde(rename = "ground/artillery")]
        GroundArtillery,
        #[serde(rename = "ground/infantry")]
        GroundInfantry,
        #[serde(rename = "satellite/eo")]
        SatelliteEo,
        #[serde(rename = "satellite/sar")]
        SatelliteSar,
        #[serde(rename = "satellite/sigint")]
        SatelliteSigint,
    }

    /// How a station carries weapons.
    #[serde(rename_all = "snake_case")]
    pub enum StationKind {
        /// Internal bay: weapons must allow internal carriage.
        Internal,
        /// External pylon: raises RCS by the stores multiplier.
        External,
        /// Vertical launch cells.
        Vls,
        /// Deck or vehicle launcher (canisters, tubes, rails).
        Launcher,
    }
}

impl PlatformCategory {
    /// The category string, e.g. `aircraft/fighter`.
    pub fn as_str(self) -> &'static str {
        use PlatformCategory::*;
        match self {
            AircraftFighter => "aircraft/fighter",
            AircraftBomber => "aircraft/bomber",
            AircraftAttack => "aircraft/attack",
            AircraftTanker => "aircraft/tanker",
            AircraftAew => "aircraft/aew",
            AircraftEw => "aircraft/ew",
            AircraftTransport => "aircraft/transport",
            AircraftMaritimePatrol => "aircraft/maritime_patrol",
            AircraftUav => "aircraft/uav",
            HelicopterAttack => "helicopter/attack",
            HelicopterUtility => "helicopter/utility",
            ShipDestroyer => "ship/destroyer",
            ShipFrigate => "ship/frigate",
            ShipCorvette => "ship/corvette",
            ShipPatrol => "ship/patrol",
            ShipAuxiliary => "ship/auxiliary",
            SubmarineAttack => "submarine/attack",
            GroundRadar => "ground/radar",
            GroundLauncher => "ground/launcher",
            GroundC2 => "ground/c2",
            GroundArmour => "ground/armour",
            GroundArtillery => "ground/artillery",
            GroundInfantry => "ground/infantry",
            SatelliteEo => "satellite/eo",
            SatelliteSar => "satellite/sar",
            SatelliteSigint => "satellite/sigint",
        }
    }

    /// The folder under `content/platforms/`.
    pub fn folder(self) -> &'static str {
        self.as_str().split('/').next().unwrap_or_default()
    }

    pub fn domain(self) -> Domain {
        match self.folder() {
            "aircraft" | "helicopter" => Domain::Air,
            "ship" => Domain::Surface,
            "submarine" => Domain::Subsurface,
            "satellite" => Domain::Space,
            _ => Domain::Ground,
        }
    }
}

source_type! {
    #[serde(deny_unknown_fields)]
    pub struct Platform {
        pub id: String,
        pub name: String,
        pub category: PlatformCategory,
        /// Class used when this platform is a target (weapon `pk_base` key).
        pub target_class: TargetClass,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub crew: Option<u32>,
        pub mobility: Mobility,
        pub signature: Signature,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub fuel: Option<Fuel>,
        /// Present only on tankers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub tanker: Option<Tanker>,
        #[serde(default)]
        pub stations: Vec<Station>,
        #[serde(default)]
        pub sensors: Vec<SensorMount>,
        /// EW suite ids.
        #[serde(default)]
        pub ew: Vec<String>,
        /// Countermeasure id → count carried.
        #[serde(default)]
        pub countermeasures: BTreeMap<String, u32>,
        /// Data link ids.
        #[serde(default)]
        pub datalinks: Vec<String>,
        pub durability: Durability,
        /// Refuel and rearm time at base.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub turnaround_min: Option<f64>,
        pub provenance: Provenance,
    }

    /// Speed and envelope. Which fields are required depends on the domain:
    /// aircraft need the Mach speeds and ceiling, ships and submarines the knot
    /// speeds, ground platforms the road speed.
    #[serde(deny_unknown_fields)]
    pub struct Mobility {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cruise_speed_mach: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_speed_mach: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ceiling_m: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub climb_rate_mps: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sustained_g: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub min_runway_m: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cruise_speed_kt: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_speed_kt: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub turn_radius_m: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub road_speed_kmh: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub offroad_speed_kmh: Option<f64>,
        /// Can operate from a road strip (doc 03 §3).
        #[serde(default)]
        pub road_capable: bool,
    }

    #[serde(deny_unknown_fields)]
    pub struct Signature {
        pub rcs_m2: RcsByAspect,
        /// RCS multiplier with external stores (1 = none).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub external_stores_rcs_multiplier: Option<f64>,
        /// Relative IR intensity (1 = reference fighter at cruise).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ir: Option<IrSignature>,
        /// Radiated noise at cruise, for sonar.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub acoustic_db: Option<f64>,
        /// Largest dimension, for visual and EO detection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub visual_size_m: Option<f64>,
    }

    #[serde(deny_unknown_fields)]
    pub struct RcsByAspect {
        pub front: f64,
        pub side: f64,
        pub rear: f64,
        pub top: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct IrSignature {
        pub cruise: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub afterburner: Option<f64>,
    }

    #[serde(deny_unknown_fields)]
    pub struct Fuel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub internal_kg: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub burn_kg_h: Option<FuelBurn>,
        /// Range at cruise speed (ships).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub range_nm: Option<f64>,
        /// Receiving system for air-to-air refuelling.
        #[serde(default = "no_aar")]
        pub aar: AarSystem,
    }

    #[serde(deny_unknown_fields)]
    pub struct FuelBurn {
        pub cruise: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub military: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub afterburner: Option<f64>,
    }

    #[serde(deny_unknown_fields)]
    pub struct Tanker {
        /// Fuel that can be given away on a typical sortie.
        pub offload_kg: f64,
        pub transfer_rate_kg_min: f64,
        pub systems: Vec<AarSystem>,
    }

    /// A weapon station: pylon, bay, launcher or group of VLS cells.
    #[serde(deny_unknown_fields)]
    pub struct Station {
        pub id: String,
        pub kind: StationKind,
        /// Interface categories this station accepts (e.g. `aam_medium`,
        /// `sylver_a50`). A weapon fits if it lists one of them.
        pub categories: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_kg: Option<f64>,
        /// Rounds (pylons, bays) or cells (VLS, launchers).
        #[serde(default = "one")]
        pub capacity: u32,
    }

    /// A sensor on a platform: a plain sensor id, or the id with a mounting.
    #[serde(untagged)]
    pub enum SensorMount {
        Id(String),
        Mounted(MountedSensor),
    }

    #[serde(deny_unknown_fields)]
    pub struct MountedSensor {
        pub sensor: String,
        /// Antenna height above the platform's reference (deck, ground).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub height_m: Option<f64>,
    }

    /// How a platform takes damage (doc 02 §13).
    #[serde(tag = "model", rename_all = "snake_case")]
    pub enum Durability {
        /// A hit destroys it with `p_kill_on_hit`, otherwise damages it.
        Aircraft(KillOnHit),
        /// Damage points scaled by displacement.
        Ship(DamagePoints),
        Ground(DamagePoints),
    }

    #[serde(deny_unknown_fields)]
    pub struct KillOnHit {
        pub p_kill_on_hit: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct DamagePoints {
        pub damage_points: f64,
    }
}

impl SensorMount {
    pub fn sensor(&self) -> &str {
        match self {
            SensorMount::Id(id) => id,
            SensorMount::Mounted(m) => &m.sensor,
        }
    }

    pub fn height_m(&self) -> Option<f64> {
        match self {
            SensorMount::Id(_) => None,
            SensorMount::Mounted(m) => m.height_m,
        }
    }
}
