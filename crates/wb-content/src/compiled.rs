//! Compiled content: what the pack holds and the engine reads. SI units
//! (m, s, kg, rad, m/s; Mach stays a Mach number, dB stays dB), strings
//! interned in [`ContentDb::strings`], references as typed indices into the
//! per-kind lists (each sorted by id).
//!
//! postcard needs every field written in order: no internally tagged enums and
//! no `skip_serializing_if`, so source types with either get their own copy
//! here.

use crate::source::platform::{PlatformCategory, RcsByAspect, StationKind};
use crate::source::realism::{
    AspectFactors, BaseKind, CellClass, EarthRealism, EndgameRealism, EwRealism, FuelRealism,
    GroundRealism, MovementRealism, RadarRealism, SiteRule, TerrainClass, TimeRealism,
    WeaponRealism,
};
use crate::source::sensor::{SensorRole, SensorType};
use crate::source::system::{ComponentRole, DeployAs};
use crate::source::weapon::{
    AltitudeEnvelope, CoastTau, Midcourse, ProfileKind, Warhead, WeaponCategory,
};
use crate::source::world::{BaseType, LaunchDoctrine};
use crate::source::{
    AarSystem, Band, Confidence, CountermeasureType, Domain, Emcon, EwType, Seeker, TargetClass,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! index_type {
    ($($(#[$m:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$m])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub u32);
        )*
    };
}

index_type! {
    /// Index into [`ContentDb::strings`].
    StrId,
    CountryIx,
    PlatformIx,
    SystemIx,
    SensorIx,
    WeaponIx,
    EwSuiteIx,
    CountermeasureIx,
    DatalinkIx,
    LoadoutIx,
    BaseIx,
    InventoryIx,
    DoctrineIx,
}

/// Everything in a pack except the header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentDb {
    pub strings: Vec<String>,
    pub countries: Vec<Country>,
    pub platforms: Vec<Platform>,
    pub systems: Vec<System>,
    pub sensors: Vec<Sensor>,
    pub weapons: Vec<Weapon>,
    pub ew_suites: Vec<EwSuite>,
    pub countermeasures: Vec<Countermeasure>,
    pub datalinks: Vec<Datalink>,
    pub loadouts: Vec<Loadout>,
    pub bases: Vec<Base>,
    pub inventories: Vec<Inventory>,
    pub doctrines: Vec<Doctrine>,
    pub realism: Vec<Realism>,
}

impl ContentDb {
    pub fn str(&self, id: StrId) -> &str {
        &self.strings[id.0 as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub confidence: Confidence,
    pub custom: bool,
    pub estimates: Vec<StrId>,
    pub sources: Vec<Source>,
    pub notes: Option<StrId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub title: StrId,
    pub url: StrId,
    pub accessed: StrId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Country {
    pub id: StrId,
    pub name: StrId,
    pub iso2: StrId,
    pub iso3: StrId,
    pub flag: StrId,
    pub doctrine: DoctrineIx,
    pub datalinks: Vec<DatalinkIx>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Platform {
    pub id: StrId,
    pub name: StrId,
    pub category: PlatformCategory,
    pub domain: Domain,
    pub target_class: TargetClass,
    pub crew: Option<u32>,
    pub mobility: Mobility,
    pub signature: Signature,
    pub fuel: Option<Fuel>,
    pub tanker: Option<Tanker>,
    pub stations: Vec<Station>,
    pub sensors: Vec<SensorMount>,
    pub ew: Vec<EwSuiteIx>,
    pub countermeasures: Vec<(CountermeasureIx, u32)>,
    pub datalinks: Vec<DatalinkIx>,
    pub durability: Durability,
    pub turnaround_s: Option<f64>,
    pub provenance: Provenance,
}

/// Aircraft speeds stay in Mach; surface and ground speeds are m/s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mobility {
    pub cruise_speed_mach: Option<f64>,
    pub max_speed_mach: Option<f64>,
    pub ceiling_m: Option<f64>,
    pub climb_rate_mps: Option<f64>,
    pub sustained_g: Option<f64>,
    pub min_runway_m: Option<f64>,
    pub cruise_speed_mps: Option<f64>,
    pub max_speed_mps: Option<f64>,
    pub turn_radius_m: Option<f64>,
    pub road_speed_mps: Option<f64>,
    pub offroad_speed_mps: Option<f64>,
    pub road_capable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signature {
    pub rcs_m2: RcsByAspect,
    pub external_stores_rcs_multiplier: f64,
    pub ir: Option<IrSignature>,
    pub acoustic_db: Option<f64>,
    pub visual_size_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrSignature {
    pub cruise: f64,
    pub afterburner: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Durability {
    Aircraft { p_kill_on_hit: f64 },
    Ship { damage_points: f64 },
    Ground { damage_points: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fuel {
    pub internal_kg: Option<f64>,
    pub burn_kg_s: Option<FuelBurn>,
    pub range_m: Option<f64>,
    pub aar: AarSystem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuelBurn {
    pub cruise: f64,
    pub military: Option<f64>,
    pub afterburner: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tanker {
    pub offload_kg: f64,
    pub transfer_rate_kg_s: f64,
    pub systems: Vec<AarSystem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub id: StrId,
    pub kind: StationKind,
    pub categories: Vec<StrId>,
    pub max_kg: Option<f64>,
    pub capacity: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorMount {
    pub sensor: SensorIx,
    pub height_m: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct System {
    pub id: StrId,
    pub name: StrId,
    pub deploy_as: DeployAs,
    pub setup_s: f64,
    pub teardown_s: Option<f64>,
    pub components: Vec<SystemComponent>,
    pub emcon_default: Emcon,
    pub iads_capable: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemComponent {
    pub platform: PlatformIx,
    pub count: u32,
    pub role: ComponentRole,
    pub magazine: Vec<(WeaponIx, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sensor {
    pub id: StrId,
    pub name: StrId,
    pub sensor_type: SensorType,
    pub role: Vec<SensorRole>,
    pub radar: Option<Radar>,
    pub esm: Option<Esm>,
    pub irst: Option<Irst>,
    pub eo_ir: Option<EoIr>,
    pub sonar: Option<Sonar>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Radar {
    pub band: Band,
    pub r_ref_m: f64,
    pub sigma_ref_m2: f64,
    pub max_range_m: Option<f64>,
    pub scan_period_s: f64,
    pub field_of_regard_rad: f64,
    pub clutter_rejection: f64,
    pub pulse_doppler: bool,
    pub eccm_db: f64,
    pub erp_class_dbw: f64,
    pub fire_channels: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Esm {
    pub bands: Vec<Band>,
    pub r_ref_m: f64,
    pub erp_ref_dbw: f64,
    pub bearing_sigma_rad: f64,
    pub field_of_regard_rad: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Irst {
    pub r_ref_m: f64,
    pub i_ref: f64,
    pub field_of_regard_rad: f64,
    pub scan_period_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EoIr {
    pub max_range_m: f64,
    pub gsd_at_10km_m: f64,
    pub night_capable: bool,
    pub field_of_regard_rad: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sonar {
    pub passive_r_ref_m: f64,
    pub noise_ref_db: f64,
    pub active_range_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weapon {
    pub id: StrId,
    pub name: StrId,
    pub category: WeaponCategory,
    pub targets: Vec<Domain>,
    pub target_class: Option<TargetClass>,
    pub profile: FlightProfile,
    pub guidance: Guidance,
    pub warhead_kg: f64,
    pub warhead: Option<Warhead>,
    pub pk_base: BTreeMap<TargetClass, f64>,
    pub countermeasure_resistance: BTreeMap<CountermeasureType, f64>,
    pub cep_m: Option<f64>,
    pub lethal_radius_m: Option<f64>,
    pub mass_kg: f64,
    pub station_category: Vec<StrId>,
    pub internal_carriage: bool,
    pub rounds_per_cell: u32,
    pub engage_on_remote: bool,
    pub envelope: Option<AltitudeEnvelope>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightProfile {
    pub kind: ProfileKind,
    pub boost_s: f64,
    pub sustain_s: f64,
    pub max_speed_mach: f64,
    pub coast_tau_s: Option<CoastTau>,
    pub min_speed_mach: Option<f64>,
    pub max_range_m: f64,
    pub min_range_m: f64,
    pub max_g: Option<f64>,
    pub cruise_alt_m: Option<f64>,
    pub terminal_alt_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guidance {
    pub midcourse: Vec<Midcourse>,
    pub terminal: Seeker,
    pub terminal_secondary: Vec<Seeker>,
    pub seeker_range_m: Option<f64>,
    pub seeker_eccm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EwSuite {
    pub id: StrId,
    pub name: StrId,
    pub ew_type: EwType,
    pub bands: Vec<Band>,
    pub j_db: f64,
    pub max_range_m: f64,
    pub simultaneous_targets: u32,
    pub field_of_regard_rad: f64,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Countermeasure {
    pub id: StrId,
    pub name: StrId,
    pub cm_type: CountermeasureType,
    pub effect: f64,
    pub defeats: Vec<Seeker>,
    pub mass_kg: Option<f64>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datalink {
    pub id: StrId,
    pub name: StrId,
    pub latency_s: f64,
    pub capacity_tracks: u32,
    pub range_m: f64,
    pub engage_on_remote: bool,
    pub jam_resistance: f64,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loadout {
    pub id: StrId,
    pub name: StrId,
    pub platform: PlatformIx,
    pub roles: Vec<StrId>,
    pub items: Vec<LoadoutItem>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadoutItem {
    /// Index into the platform's `stations`.
    pub station: u32,
    pub weapon: WeaponIx,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
    pub id: StrId,
    pub name: StrId,
    pub base_type: BaseType,
    pub country: CountryIx,
    pub icao: Option<StrId>,
    pub lat_rad: f64,
    pub lon_rad: f64,
    pub elevation_m: f64,
    pub runway_length_m: Option<f64>,
    pub aircraft_capacity: u32,
    pub ship_berths: u32,
    pub provenance: Provenance,
}

/// A platform or a composite system, as counted in an inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ForceIx {
    Platform(PlatformIx),
    System(SystemIx),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub id: StrId,
    pub country: CountryIx,
    pub as_of: StrId,
    pub platforms: Vec<(ForceIx, u32)>,
    pub munitions: Vec<(WeaponIx, u32)>,
    pub basing: Vec<Basing>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Basing {
    pub force: ForceIx,
    pub base: BaseIx,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Doctrine {
    pub id: StrId,
    pub name: StrId,
    pub aggression: f64,
    pub risk_tolerance: f64,
    pub emcon_discipline: f64,
    pub emcon_default: Emcon,
    pub missile_reserve_fraction: f64,
    pub launch_doctrine: LaunchDoctrine,
    pub shots_per_target: u32,
    pub cap_aircraft: u32,
    pub cap_rotation_s: f64,
    pub reaction_time_s: f64,
    pub iads_layers: Vec<StrId>,
    pub strike_package: Vec<StrId>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Realism {
    pub id: StrId,
    pub name: StrId,
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
    pub bases: BTreeMap<BaseKind, BaseRule>,
    pub ground: GroundRealism,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PassiveRealism {
    pub esm_bearing_sigma_default_rad: f64,
    pub irst_afterburner_factor: f64,
    pub irst_aspect_factor: AspectFactors,
    pub eo_gsd_type_m: f64,
    pub eo_gsd_class_m: f64,
    pub visual_range_m: BTreeMap<TerrainClass, f64>,
    pub visual_night_factor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SatelliteRealism {
    pub eo_min_sun_elevation_rad: f64,
    pub downlink_delay_default_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FusionRealism {
    pub age_out_s: BTreeMap<Domain, f64>,
    pub fixed_sites_kept: bool,
    pub ellipse_growth_mps: BTreeMap<Domain, f64>,
    pub datalink_latency_default_s: f64,
    pub c2_report_delay_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TewaRealism {
    pub ciws_layer_min_m: f64,
    pub ciws_layer_max_m: f64,
    pub soft_kill_window_s: f64,
    pub kill_assessment_s: f64,
    pub iads_autonomous_reaction_factor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageRealism {
    pub ship_out_of_action_fraction: f64,
    pub ship_sink_delay_s: f64,
    pub warhead_multiplier: BTreeMap<Warhead, f64>,
    pub runway_repair_min_s: f64,
    pub runway_repair_max_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogisticsRealism {
    pub fuel_reserve_fraction: f64,
    pub tanker_join_m: f64,
    pub landing_radius_m: f64,
    pub pattern_alt_m: f64,
    pub berth_radius_m: f64,
    pub turnaround_default_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseRule {
    pub site: SiteRule,
    pub match_radius_m: f64,
    pub terrain: Vec<CellClass>,
    pub own_territory: bool,
    pub coast: bool,
    /// Maximum mean cell slope, degrees (the terrain layer stores degrees).
    pub max_slope_deg: Option<f64>,
    pub setup_s: f64,
    pub aircraft_capacity: u32,
    pub ship_berths: u32,
    pub road_capable_only: bool,
    pub rotary_only: bool,
    pub vls_reload: bool,
}
