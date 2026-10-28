//! `realism`: global tuning constants (doc 02 §17). Every field is required,
//! so a missing constant fails `wb content check`.
//!
//! Sections whose fields are already SI (or dimensionless) are stored in the
//! pack unchanged; the others are converted by the compiler.

use super::common::{Domain, Provenance, Seeker, source_enum, source_type};
use super::platform::PlatformCategory;
use super::weapon::Warhead;
use std::collections::BTreeMap;

source_enum! {
    /// Distribution of the miss distance for CEP rolls (doc 02 §11).
    #[serde(rename_all = "snake_case")]
    pub enum CepDistribution {
        Rayleigh,
    }

    /// Terrain classes for ground combat and visual detection.
    #[serde(rename_all = "snake_case")]
    pub enum TerrainClass {
        Open,
        Forest,
        Urban,
        Mountain,
    }

    /// Base kinds a player can set up (doc 03 §3).
    #[serde(rename_all = "snake_case")]
    pub enum BaseKind {
        AirBase,
        RoadStrip,
        Heliport,
        NavalBase,
        Anchorage,
        ArmyBase,
        SamSite,
        MissileSite,
        Hq,
    }

    /// Classes of the H3 terrain layer (doc 05 §3), for base placement.
    #[serde(rename_all = "snake_case")]
    pub enum CellClass {
        Water,
        Open,
        Forest,
        Jungle,
        Urban,
        Desert,
        Mountain,
        Marsh,
        Arctic,
    }

    /// What a base must be placed on.
    #[serde(rename_all = "snake_case")]
    pub enum SiteRule {
        /// A real airfield (content `base` with an air base) nearby.
        RealAirfield,
        /// A real port (content `base` with a port) nearby.
        RealPort,
        /// Any cell that meets the terrain, territory and slope rules.
        Terrain,
    }
}

source_type! {
    #[serde(deny_unknown_fields)]
    pub struct Realism {
        pub id: String,
        pub name: String,
        pub time: TimeRealism,
        pub earth: EarthRealism,
        pub radar: RadarRealism,
        pub passive: PassiveRealism,
        pub satellites: SatelliteRealism,
        pub fusion: FusionRealism,
        pub ew: EwRealism,
        pub weapons: WeaponRealism,
        pub endgame: EndgameRealism,
        pub tewa: TewaRealism,
        pub damage: DamageRealism,
        pub movement: MovementRealism,
        pub fuel: FuelRealism,
        pub logistics: LogisticsRealism,
        /// Placement rules per base kind (doc 03 §3). Every kind must be listed.
        pub bases: BTreeMap<BaseKind, BaseRule>,
        pub ground: GroundRealism,
        pub provenance: Provenance,
    }

    /// Scheduler (doc 02 §3).
    #[serde(deny_unknown_fields)]
    pub struct TimeRealism {
        /// Tick while anything is engaging.
        pub tick_fine_ms: u32,
        /// Tick otherwise.
        pub tick_coarse_ms: u32,
        pub esm_period_s: f64,
        pub satellite_period_s: f64,
        pub fusion_period_s: f64,
        pub tewa_period_s: f64,
        pub logistics_period_s: f64,
        pub ground_combat_period_s: f64,
        pub ai_tactical_period_s: f64,
        pub ai_operational_period_s: f64,
        pub ai_strategic_period_s: f64,
        /// State hash checkpoint interval.
        pub hash_checkpoint_ticks: u32,
    }

    /// Earth model (doc 02 §5, §7.1).
    #[serde(deny_unknown_fields)]
    pub struct EarthRealism {
        pub radius_m: f64,
        /// Effective-Earth factor for the radio horizon (4/3).
        pub radio_k_factor: f64,
        /// `horizon_km = c × (√h_radar_m + √h_target_m)`.
        pub horizon_coefficient: f64,
    }

    /// Radar detection and tracking (doc 02 §7.1).
    #[serde(deny_unknown_fields)]
    pub struct RadarRealism {
        /// Exponent `k` of `Pd = 1 / (1 + (d / R50)^k)`.
        pub pd_steepness_k: f64,
        /// A track forms after this many detections ...
        pub track_confirm_hits: u32,
        /// ... within this many scans.
        pub track_confirm_window_scans: u32,
        /// A track drops after this many consecutive misses.
        pub track_drop_misses: u32,
        /// Look-down clutter applies below these target heights.
        pub clutter_alt_land_m: f64,
        pub clutter_alt_sea_m: f64,
        /// Pulse-Doppler notch: radial speed below which a look-down target is
        /// hard to see, and the Pd factor applied.
        pub doppler_notch_radial_mps: f64,
        pub doppler_notch_pd_factor: f64,
        /// Line of sight is sampled every `max(DEM cell, d / divisor)`.
        pub los_sample_divisor: f64,
    }

    /// Passive sensors (doc 02 §7.2).
    #[serde(deny_unknown_fields)]
    pub struct PassiveRealism {
        /// Bearing error used when an ESM entry gives none.
        pub esm_bearing_sigma_default_deg: f64,
        /// IR multiplier with afterburner when a platform gives none.
        pub irst_afterburner_factor: f64,
        /// IR intensity factor by aspect (rear is hot).
        pub irst_aspect_factor: AspectFactors,
        /// EO ground sample distance needed for type and count ...
        pub eo_gsd_type_m: f64,
        /// ... and for class.
        pub eo_gsd_class_m: f64,
        /// Visual / recon detection range by terrain.
        pub visual_range_km: BTreeMap<TerrainClass, f64>,
        /// Visual range factor at night without night vision.
        pub visual_night_factor: f64,
    }

    /// Satellites (doc 02 §7.3).
    #[serde(deny_unknown_fields)]
    pub struct SatelliteRealism {
        /// EO imaging needs the sun above this elevation at the target.
        pub eo_min_sun_elevation_deg: f64,
        /// Report delay when a payload gives none.
        pub downlink_delay_default_min: f64,
    }

    /// Contacts, ageing and sharing (doc 02 §8).
    #[serde(deny_unknown_fields)]
    pub struct FusionRealism {
        /// A contact not refreshed for this long becomes a last-known marker.
        pub age_out_min: BTreeMap<Domain, f64>,
        /// Fixed sites never age out.
        pub fixed_sites_kept: bool,
        /// `v_max(class)` in the ellipse growth `σ(t) = σ0 + v_max × age`.
        pub ellipse_growth_kt: BTreeMap<Domain, f64>,
        /// Data link latency when a link entry gives none.
        pub datalink_latency_default_s: f64,
        /// Delay for units not on the network (C2 reports).
        pub c2_report_delay_s: f64,
    }

    /// Electronic warfare (doc 02 §9).
    #[serde(deny_unknown_fields)]
    pub struct EwRealism {
        /// Default sidelobe gain `G_sl` for stand-off jamming.
        pub sidelobe_gain_db: f64,
        /// Gain in the main beam.
        pub main_beam_gain_db: f64,
        /// CEP multiplier for GPS weapons in a GPS-jammed area (no anti-jam).
        pub gps_jam_cep_factor: f64,
        /// Network latency multiplier in a comms-jammed area.
        pub comms_jam_latency_factor: f64,
        /// `jam_effect` in the seeker-jamming endgame factor.
        pub seeker_jam_effect: f64,
        /// Pk multiplier for anti-radiation weapons in memory mode.
        pub arm_memory_pk_factor: f64,
    }

    /// Weapons in flight (doc 02 §10).
    #[serde(deny_unknown_fields)]
    pub struct WeaponRealism {
        /// Proportional navigation constant range.
        pub pn_gain_min: f64,
        pub pn_gain_max: f64,
        /// Sea-skimming altitude band.
        pub sea_skim_min_alt_m: f64,
        pub sea_skim_max_alt_m: f64,
        /// Calibration: published range must be met within ± this fraction.
        pub range_calibration_tolerance: f64,
    }

    /// Endgame Pk (doc 02 §11).
    #[serde(deny_unknown_fields)]
    pub struct EndgameRealism {
        /// `f_maneuver = min(1, G_missile / (ratio × G_target))`.
        pub maneuver_g_ratio: f64,
        pub maneuver_floor: f64,
        pub pk_min: f64,
        pub pk_max: f64,
        /// `f_aspect` by seeker type.
        pub aspect: BTreeMap<Seeker, AspectFactors>,
        pub cep_distribution: CepDistribution,
    }

    #[serde(deny_unknown_fields)]
    pub struct AspectFactors {
        pub head_on: f64,
        pub beam: f64,
        pub tail: f64,
    }

    /// Threat evaluation and weapon assignment (doc 02 §12).
    #[serde(deny_unknown_fields)]
    pub struct TewaRealism {
        /// CIWS layer: last 2–4 km.
        pub ciws_layer_min_km: f64,
        pub ciws_layer_max_km: f64,
        /// Soft kill for radar-homing threats inside this time to impact.
        pub soft_kill_window_s: f64,
        /// Time to assess a shot before shooting again (shoot-look-shoot).
        pub kill_assessment_s: f64,
        /// IADS sites without C2 react this many times slower.
        pub iads_autonomous_reaction_factor: f64,
    }

    /// Damage (doc 02 §13).
    #[serde(deny_unknown_fields)]
    pub struct DamageRealism {
        /// A ship at or below this fraction of damage points is out of action.
        pub ship_out_of_action_fraction: f64,
        pub ship_sink_delay_min: f64,
        /// Damage per kg of warhead by type.
        pub warhead_multiplier: BTreeMap<Warhead, f64>,
        pub runway_repair_min_h: f64,
        pub runway_repair_max_h: f64,
    }

    /// Aircraft and ship movement (doc 02 §5). The aircraft and ship values
    /// are defaults for platforms that give none.
    #[serde(deny_unknown_fields)]
    pub struct MovementRealism {
        /// ISA troposphere, for the speed of sound at altitude (Mach → m/s).
        pub isa_sea_level_temp_k: f64,
        pub isa_lapse_rate_k_per_m: f64,
        /// Above this the temperature stays at the tropopause value.
        pub isa_tropopause_m: f64,
        /// Ratio of specific heats × specific gas constant of dry air, J/(kg·K).
        pub isa_gamma_r: f64,
        pub aircraft_climb_rate_mps: f64,
        pub aircraft_descent_rate_mps: f64,
        /// Speed change rate.
        pub aircraft_accel_mps2: f64,
        pub aircraft_sustained_g: f64,
        /// Cruise altitude for tasks that give none ...
        pub aircraft_cruise_alt_m: f64,
        /// ... unless the category has its own.
        #[serde(default)]
        pub cruise_alt_by_category_m: BTreeMap<PlatformCategory, f64>,
        pub ship_accel_mps2: f64,
        /// Distance between aircraft in a flight.
        pub formation_spacing_m: f64,
        /// A waypoint counts as reached within this distance.
        pub waypoint_radius_m: f64,
    }

    /// Burn model (doc 02 §6).
    #[serde(deny_unknown_fields)]
    pub struct FuelRealism {
        /// Burn multiplier by altitude: the first band whose `below_m` is above
        /// the altitude applies; above the last band its factor applies.
        pub burn_altitude_factor: Vec<AltitudeFactor>,
        /// Ship fuel per metre scales with `(v / v_cruise)^exponent`.
        pub ship_burn_speed_exponent: f64,
        /// Burn in a descent as a fraction of the cruise burn (throttle back).
        pub descent_burn_fraction: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct AltitudeFactor {
        pub below_m: f64,
        pub factor: f64,
    }

    /// Fuel and logistics (doc 02 §6).
    #[serde(deny_unknown_fields)]
    pub struct LogisticsRealism {
        /// Bingo reserve as a fraction of the fuel to reach base.
        pub fuel_reserve_fraction: f64,
        /// A receiver joins its tanker within this distance.
        pub tanker_join_km: f64,
        /// An aircraft lands within this distance of its base ...
        pub landing_radius_km: f64,
        /// ... once down to this height above the base.
        pub pattern_alt_m: f64,
        /// A ship berths within this distance of its port.
        pub berth_radius_km: f64,
        /// Turnaround for platforms that give none.
        pub turnaround_default_min: f64,
    }

    /// Placement rule of one base kind (doc 03 §3).
    #[serde(deny_unknown_fields)]
    pub struct BaseRule {
        pub site: SiteRule,
        /// For `real_airfield` and `real_port`: the real base must lie within
        /// this distance.
        #[serde(default)]
        pub match_radius_km: f64,
        /// Allowed terrain classes; empty means any.
        #[serde(default)]
        pub terrain: Vec<CellClass>,
        /// Must be in own coalition territory (land or 12 nm waters).
        #[serde(default)]
        pub own_territory: bool,
        /// Must be a coastal cell.
        #[serde(default)]
        pub coast: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_slope_deg: Option<f64>,
        /// Activation time after placement.
        pub setup_min: f64,
        /// Capacity of a placed base (real bases use their own figures).
        #[serde(default)]
        pub aircraft_capacity: u32,
        #[serde(default)]
        pub ship_berths: u32,
        /// Only aircraft flagged `road_capable`.
        #[serde(default)]
        pub road_capable_only: bool,
        /// Only helicopters and drones.
        #[serde(default)]
        pub rotary_only: bool,
        /// VLS cells can be reloaded here.
        #[serde(default)]
        pub vls_reload: bool,
    }

    /// Ground combat (doc 02 §14, Phase 11). Placeholders until then.
    #[serde(deny_unknown_fields)]
    pub struct GroundRealism {
        /// Lanchester attrition coefficient per resolution step.
        pub lanchester_coefficient: f64,
        /// Bounded random variation (± fraction) of each step's losses.
        pub random_variation: f64,
        /// Defender combat value multiplier by terrain.
        pub defender_terrain_factor: BTreeMap<TerrainClass, f64>,
    }
}
