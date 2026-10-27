//! Scenario files (`scenarios/*.yaml`). P0 format: name, seed, start time,
//! optional determinism probe and a list of bare units.

use crate::clock::{EngagementState, RngSeed, SimClock, TickLen};
use crate::components::{Identity, Kinematics};
use crate::probe::ProbeState;
use bevy_ecs::world::World;
use serde::Deserialize;
use std::collections::BTreeSet;
use wb_core::UnitId;
use wb_core::geo::LatLon;
use wb_core::units::deg_to_rad;

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("scenario YAML is invalid: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("start_utc '{0}' is not a UTC time like 2026-10-27T06:00:00Z")]
    StartTime(String),
    #[error("unit id {0} is used more than once")]
    DuplicateUnit(u32),
    #[error("unit {id} ({name}): {reason}")]
    InvalidUnit {
        id: u32,
        name: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub seed: u64,
    /// War start, `YYYY-MM-DDTHH:MM:SSZ`.
    pub start_utc: String,
    /// Enables the P0 determinism probe system (see `probe.rs`).
    #[serde(default)]
    pub determinism_probe: bool,
    #[serde(default)]
    pub units: Vec<ScenarioUnit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioUnit {
    pub id: u32,
    pub name: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    #[serde(default)]
    pub alt_m: f64,
    #[serde(default)]
    pub heading_deg: f64,
    #[serde(default)]
    pub speed_mps: f64,
}

impl Scenario {
    /// Parses and validates a scenario.
    pub fn from_yaml(yaml: &str) -> Result<Self, ScenarioError> {
        let scenario: Scenario = serde_yaml::from_str(yaml)?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn validate(&self) -> Result<(), ScenarioError> {
        parse_utc_ms(&self.start_utc)?;
        let mut seen = BTreeSet::new();
        for u in &self.units {
            if !seen.insert(u.id) {
                return Err(ScenarioError::DuplicateUnit(u.id));
            }
            let bad = |reason: &str| ScenarioError::InvalidUnit {
                id: u.id,
                name: u.name.clone(),
                reason: reason.to_owned(),
            };
            if !(-90.0..=90.0).contains(&u.lat_deg) {
                return Err(bad("lat_deg must be between -90 and 90"));
            }
            if !(-180.0..=180.0).contains(&u.lon_deg) {
                return Err(bad("lon_deg must be between -180 and 180"));
            }
            if !u.alt_m.is_finite() || !u.heading_deg.is_finite() {
                return Err(bad("alt_m and heading_deg must be finite numbers"));
            }
            if !u.speed_mps.is_finite() || u.speed_mps < 0.0 {
                return Err(bad("speed_mps must be zero or positive"));
            }
        }
        Ok(())
    }

    /// Inserts resources and spawns units, in `UnitId` order.
    pub(crate) fn spawn_into(&self, world: &mut World) {
        let start_utc_ms = parse_utc_ms(&self.start_utc).expect("validated scenario");
        world.insert_resource(SimClock {
            tick: 0,
            sim_time_ms: 0,
            tick_len: TickLen::Coarse,
            start_utc_ms,
        });
        world.insert_resource(RngSeed(self.seed));
        world.insert_resource(EngagementState::default());
        if self.determinism_probe {
            world.insert_resource(ProbeState::new());
        }

        let mut units: Vec<&ScenarioUnit> = self.units.iter().collect();
        units.sort_by_key(|u| u.id);
        for u in units {
            let p = LatLon::from_deg(u.lat_deg, u.lon_deg);
            world.spawn((
                Identity {
                    unit: UnitId(u.id),
                    name: u.name.clone(),
                },
                Kinematics {
                    lat: p.lat,
                    lon: p.lon,
                    alt_m: u.alt_m,
                    heading: wb_core::geo::normalize_bearing(deg_to_rad(u.heading_deg)),
                    speed_mps: u.speed_mps,
                },
            ));
        }
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` to Unix epoch milliseconds.
pub fn parse_utc_ms(s: &str) -> Result<i64, ScenarioError> {
    let err = || ScenarioError::StartTime(s.to_owned());
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return Err(err());
    }
    let num = |range: std::ops::Range<usize>| -> Result<i64, ScenarioError> {
        s[range].parse::<u32>().map(i64::from).map_err(|_| err())
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || se > 59
    {
        return Err(err());
    }
    let days = days_from_civil(y, mo, d);
    Ok(((days * 24 + h) * 60 + mi) * 60_000 + se * 1_000)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc() {
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(
            parse_utc_ms("2000-03-01T00:00:00Z").unwrap(),
            951_868_800_000
        );
        assert_eq!(
            parse_utc_ms("2026-10-27T06:30:15Z").unwrap(),
            1_793_082_615_000
        );
        assert!(parse_utc_ms("2026-02-29T00:00:00Z").is_err());
        assert!(parse_utc_ms("2026-10-27 06:30:15").is_err());
    }

    #[test]
    fn rejects_duplicate_units() {
        let yaml = "name: t\nseed: 1\nstart_utc: 2026-10-27T00:00:00Z\nunits:\n  - {id: 1, name: a, lat_deg: 0, lon_deg: 0}\n  - {id: 1, name: b, lat_deg: 0, lon_deg: 0}\n";
        let err = Scenario::from_yaml(yaml).unwrap_err();
        assert_eq!(err.to_string(), "unit id 1 is used more than once");
    }
}
