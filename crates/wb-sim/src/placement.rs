//! Base placement validator (doc 03 §3). The rules per base kind come from the
//! realism profile (`bases`); this file only applies them and explains
//! rejections in plain English.
//!
//! Real airfields and ports are content bases, so those rules need no region
//! pack. Terrain, territory, coast and slope rules read the packs; without
//! them such placements are rejected rather than guessed.

use wb_content::compiled::{BaseIx, CountryIx};
use wb_content::source::realism::{BaseKind, CellClass, SiteRule};
use wb_core::geo::{LatLon, distance_m};
use wb_geo::{Geo, TerrainClass};

use crate::bases::{BaseInfo, content_kind, kind_name};
use crate::components::BaseSite;
use crate::rules::Rules;
use crate::world::Sides;

/// A placement that passed every rule.
#[derive(Clone, Debug)]
pub struct Placed {
    pub site: BaseSite,
    pub country: CountryIx,
    /// Name of the real base it sits on, if any.
    pub real_name: Option<String>,
}

fn cell_class(c: TerrainClass) -> CellClass {
    match c {
        TerrainClass::Water => CellClass::Water,
        TerrainClass::Open => CellClass::Open,
        TerrainClass::Forest => CellClass::Forest,
        TerrainClass::Jungle => CellClass::Jungle,
        TerrainClass::Urban => CellClass::Urban,
        TerrainClass::Desert => CellClass::Desert,
        TerrainClass::Mountain => CellClass::Mountain,
        TerrainClass::Marsh => CellClass::Marsh,
        TerrainClass::Arctic => CellClass::Arctic,
    }
}

/// Checks a placement of `kind` at `at` for `side`.
#[allow(clippy::too_many_arguments)]
pub fn validate(
    rules: &Rules,
    sides: &Sides,
    side: u8,
    kind: BaseKind,
    at: LatLon,
    active: &[BaseInfo],
    geo: Option<&Geo>,
    now_ms: u64,
) -> Result<Placed, String> {
    let name = kind_name(kind);
    let rule = rules
        .realism()
        .bases
        .get(&kind)
        .ok_or_else(|| format!("{name}: no placement rule in the realism profile"))?;
    let own = &sides.country_ix[usize::from(side)];
    let setup_ms = (rule.setup_s * 1_000.0) as u64;

    match rule.site {
        SiteRule::RealAirfield | SiteRule::RealPort => {
            let want_air = rule.site == SiteRule::RealAirfield;
            let db = &rules.db;
            let nearest = db
                .bases
                .iter()
                .enumerate()
                .filter(|(_, b)| {
                    let (_, air, port) = content_kind(b);
                    if want_air { air } else { port }
                })
                .map(|(i, b)| (distance_m(at, LatLon::new(b.lat_rad, b.lon_rad)), i, b))
                .filter(|(d, _, _)| *d <= rule.match_radius_m)
                .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            let Some((_, i, b)) = nearest else {
                let what = if want_air {
                    "a real airfield"
                } else {
                    "a port"
                };
                return Err(format!(
                    "{name} must be at {what} (none within {:.0} km)",
                    rule.match_radius_m / 1_000.0
                ));
            };
            let real = db.str(b.name).to_owned();
            if own.binary_search(&b.country).is_err() {
                return Err(format!("{real} belongs to another country, not your side"));
            }
            let ix = BaseIx(i as u32);
            if active.iter().any(|a| a.content == Some(ix)) {
                return Err(format!("{real} is already one of your bases"));
            }
            let (_, air, port) = content_kind(b);
            Ok(Placed {
                site: BaseSite {
                    content: Some(ix),
                    kind,
                    air,
                    port,
                    at: LatLon::new(b.lat_rad, b.lon_rad),
                    elevation_m: b.elevation_m,
                    runway_m: b.runway_length_m,
                    aircraft_capacity: b.aircraft_capacity,
                    ship_berths: b.ship_berths,
                    active_at_ms: now_ms + setup_ms,
                    active: setup_ms == 0,
                    stationed: Vec::new(),
                },
                country: b.country,
                real_name: Some(real),
            })
        }
        SiteRule::Terrain => {
            let Some(geo) = geo else {
                return Err(format!(
                    "Placing a {} needs the region pack for this area",
                    name.to_lowercase()
                ));
            };
            if geo.pack_at(at).is_none() {
                return Err("No terrain data here; install the region pack".to_owned());
            }
            // Inside a pack, a cell the terrain file does not list is open
            // water (as `Geo::terrain_class` reads it).
            let cell = geo
                .cell_info(at)
                .map_err(|e| format!("Cannot read the terrain here: {e}"))?;
            let terrain = cell.map_or(TerrainClass::Water, |c| c.class);
            if !rule.terrain.is_empty() && !rule.terrain.contains(&cell_class(terrain)) {
                return Err(format!("{name} cannot stand on {}", terrain.as_str()));
            }
            let slope = cell.map_or(0.0, |c| c.slope_deg);
            if let Some(max) = rule.max_slope_deg
                && slope > max
            {
                return Err(format!(
                    "{name} needs a slope under {max:.1}° (here {slope:.1}°)"
                ));
            }
            if rule.coast && !cell.is_some_and(|c| c.is_coast()) {
                return Err(format!("{name} must be on the coast"));
            }
            let owner = geo
                .territory_owner(at)
                .map_err(|e| format!("Cannot read the territory here: {e}"))?;
            let owner_ix = owner.and_then(|c| {
                rules
                    .db
                    .countries
                    .iter()
                    .position(|k| rules.db.str(k.id) == c.as_str())
                    .map(|i| CountryIx(i as u32))
            });
            let in_own = owner_ix.filter(|c| own.binary_search(c).is_ok());
            if rule.own_territory && in_own.is_none() {
                return Err("Outside own territory".to_owned());
            }
            let country = in_own
                .or_else(|| own.first().copied())
                .ok_or_else(|| "Your side has no country to own this base".to_owned())?;
            let air = rule.aircraft_capacity > 0;
            let port = rule.ship_berths > 0;
            Ok(Placed {
                site: BaseSite {
                    content: None,
                    kind,
                    air,
                    port,
                    at,
                    elevation_m: geo.elevation_m(at).unwrap_or(0.0),
                    runway_m: None,
                    aircraft_capacity: rule.aircraft_capacity,
                    ship_berths: rule.ship_berths,
                    active_at_ms: now_ms + setup_ms,
                    active: setup_ms == 0,
                    stationed: Vec::new(),
                },
                country,
                real_name: None,
            })
        }
    }
}
