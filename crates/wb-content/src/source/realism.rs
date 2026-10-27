//! `realism`: global tuning constants (doc 02 §17). Every field is required,
//! so a missing constant fails `wb content check`.
//!
//! Sections whose fields are already SI (or dimensionless) are stored in the
//! pack unchanged; the others are converted by the compiler.

use super::common::{Domain, Provenance, Seeker, source_enum, source_type};
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
        pub logistics: LogisticsRealism,
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

    /// Fuel and logistics (doc 02 §6).
    #[serde(deny_unknown_fields)]
    pub struct LogisticsRealism {
        /// Bingo reserve as a fraction of the fuel to reach base.
        pub fuel_reserve_fraction: f64,
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
