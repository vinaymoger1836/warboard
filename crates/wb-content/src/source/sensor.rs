//! `sensor`: radars, ESM, IRST, EO/IR, visual and sonar (doc 02 §7).

use super::common::{Band, Provenance, source_enum, source_type};

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum SensorType {
        Radar,
        Esm,
        Irst,
        EoIr,
        Sonar,
        Visual,
        /// A satellite payload (doc 02 §7.3).
        SatellitePayload,
    }

    /// What a satellite payload collects.
    #[serde(rename_all = "snake_case")]
    pub enum PayloadKind {
        /// Electro-optical imaging: needs daylight at the target.
        Optical,
        /// Synthetic aperture radar: day and night.
        Sar,
        /// Signals intelligence: locates emitting radars.
        Sigint,
    }

    #[serde(rename_all = "snake_case")]
    pub enum SensorRole {
        AirSearch,
        SurfaceSearch,
        FireControl,
        GroundMapping,
        Aew,
        HeightFinding,
        MissileGuidance,
        Esm,
        Elint,
        Irst,
        Targeting,
        Sonar,
        Lookout,
        Imaging,
    }
}

source_type! {
    /// One sensor type. Exactly the block that matches `sensor_type` is present.
    #[serde(deny_unknown_fields)]
    pub struct Sensor {
        pub id: String,
        pub name: String,
        pub sensor_type: SensorType,
        pub role: Vec<SensorRole>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub radar: Option<Radar>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub esm: Option<Esm>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub irst: Option<Irst>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub eo_ir: Option<EoIr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sonar: Option<Sonar>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub visual: Option<Visual>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub payload: Option<Payload>,
        pub provenance: Provenance,
    }

    /// Radar model of doc 02 §7.1.
    #[serde(deny_unknown_fields)]
    pub struct Radar {
        pub band: Band,
        /// 50% single-scan detection range against `sigma_ref_m2`.
        pub r_ref_km: f64,
        pub sigma_ref_m2: f64,
        /// Instrumented range; nothing is detected beyond it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub max_range_km: Option<f64>,
        pub scan_period_s: f64,
        /// Full width of the field of regard (360 for rotating radars).
        pub field_of_regard_deg: f64,
        /// 0 = none, 1 = perfect look-down / clutter rejection.
        pub clutter_rejection: f64,
        pub pulse_doppler: bool,
        pub eccm_db: f64,
        /// Effective radiated power class, seen by enemy ESM.
        pub erp_class_dbw: f64,
        /// Simultaneous guided engagements (0 = search only).
        #[serde(default)]
        pub fire_channels: u32,
        /// 1-sigma bearing error; the realism default when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub bearing_sigma_deg: Option<f64>,
    }

    /// Passive emitter detection (doc 02 §7.2).
    #[serde(deny_unknown_fields)]
    pub struct Esm {
        pub bands: Vec<Band>,
        /// Detection range against an emitter of `erp_ref_dbw`.
        pub r_ref_km: f64,
        pub erp_ref_dbw: f64,
        /// 1-sigma bearing error.
        pub bearing_sigma_deg: f64,
        pub field_of_regard_deg: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct Irst {
        /// Detection range against a target of relative intensity `i_ref`.
        pub r_ref_km: f64,
        pub i_ref: f64,
        pub field_of_regard_deg: f64,
        pub scan_period_s: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct EoIr {
        pub max_range_km: f64,
        /// Ground sample distance at 10 km slant range.
        pub gsd_at_10km_m: f64,
        /// Has a thermal channel, so works at night.
        pub night_capable: bool,
        pub field_of_regard_deg: f64,
    }

    #[serde(deny_unknown_fields)]
    pub struct Sonar {
        /// Passive detection range against the reference noise level.
        pub passive_r_ref_km: f64,
        pub noise_ref_db: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub active_range_km: Option<f64>,
    }

    /// Eyes and optics of a recon team or lookout (doc 02 §7.2).
    #[serde(deny_unknown_fields)]
    pub struct Visual {
        /// Night vision: no night range penalty.
        pub night_vision: bool,
    }

    /// A satellite payload (doc 02 §7.3).
    #[serde(deny_unknown_fields)]
    pub struct Payload {
        pub kind: PayloadKind,
        /// Width of the strip seen along the ground track.
        pub swath_km: f64,
        /// Ground sample distance (optical and SAR).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub gsd_m: Option<f64>,
        /// Bands it can hear (SIGINT).
        #[serde(default)]
        pub bands: Vec<Band>,
        /// 1-sigma position error of a SIGINT fix.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub location_sigma_km: Option<f64>,
        /// Time from collection to report; the realism default when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub downlink_delay_min: Option<f64>,
    }
}
