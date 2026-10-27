//! Spherical Earth geometry (doc 02 §5). Angles in radians, distances in metres.
//!
//! Navigation and distances use a sphere of radius [`EARTH_RADIUS_M`]. Local
//! tangent-plane (ENU) maths is for intercept geometry near a reference point.

use crate::math::{asin, atan2, cos, hypot, sin, sqrt};
use core::f64::consts::{PI, TAU};
use serde::{Deserialize, Serialize};

/// Mean Earth radius (IUGG), metres.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// A point on the sphere. `lat` in [-π/2, π/2], `lon` in [-π, π).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// From degrees, as used in content, orders and the UI.
    pub fn from_deg(lat_deg: f64, lon_deg: f64) -> Self {
        Self::new(
            crate::units::deg_to_rad(lat_deg),
            normalize_lon(crate::units::deg_to_rad(lon_deg)),
        )
    }

    pub fn lat_deg(self) -> f64 {
        crate::units::rad_to_deg(self.lat)
    }

    pub fn lon_deg(self) -> f64 {
        crate::units::rad_to_deg(self.lon)
    }
}

/// Wraps a longitude into [-π, π).
pub fn normalize_lon(lon: f64) -> f64 {
    let wrapped = (lon + PI).rem_euclid(TAU) - PI;
    // rem_euclid can return TAU itself for tiny negative inputs.
    if wrapped >= PI {
        wrapped - TAU
    } else {
        wrapped
    }
}

/// Wraps an angle into [0, 2π).
pub fn normalize_bearing(b: f64) -> f64 {
    let wrapped = b.rem_euclid(TAU);
    if wrapped >= TAU { 0.0 } else { wrapped }
}

/// Great-circle distance (haversine), metres.
pub fn distance_m(a: LatLon, b: LatLon) -> f64 {
    let s_dlat = sin((b.lat - a.lat) * 0.5);
    let s_dlon = sin((b.lon - a.lon) * 0.5);
    let h = s_dlat * s_dlat + cos(a.lat) * cos(b.lat) * s_dlon * s_dlon;
    let h = h.clamp(0.0, 1.0);
    2.0 * EARTH_RADIUS_M * atan2(sqrt(h), sqrt(1.0 - h))
}

/// Initial great-circle bearing from `a` to `b`, radians clockwise from true north, [0, 2π).
pub fn initial_bearing(a: LatLon, b: LatLon) -> f64 {
    let dlon = b.lon - a.lon;
    let y = sin(dlon) * cos(b.lat);
    let x = cos(a.lat) * sin(b.lat) - sin(a.lat) * cos(b.lat) * cos(dlon);
    normalize_bearing(atan2(y, x))
}

/// Point reached from `start` after `distance_m` along the great circle with initial `bearing`.
pub fn destination_point(start: LatLon, bearing: f64, distance_m: f64) -> LatLon {
    let delta = distance_m / EARTH_RADIUS_M;
    let (s_lat, c_lat) = (sin(start.lat), cos(start.lat));
    let (s_d, c_d) = (sin(delta), cos(delta));
    let s_lat2 = (s_lat * c_d + c_lat * s_d * cos(bearing)).clamp(-1.0, 1.0);
    let lat2 = asin(s_lat2);
    let lon2 = start.lon + atan2(sin(bearing) * s_d * c_lat, c_d - s_lat * s_lat2);
    LatLon::new(lat2, normalize_lon(lon2))
}

/// Earth-centred, Earth-fixed coordinates on the sphere, metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ecef {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// East-north-up offsets from a reference point, metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enu {
    pub e: f64,
    pub n: f64,
    pub u: f64,
}

/// Position and altitude above the sphere to ECEF.
pub fn to_ecef(p: LatLon, alt_m: f64) -> Ecef {
    let r = EARTH_RADIUS_M + alt_m;
    let c_lat = cos(p.lat);
    Ecef {
        x: r * c_lat * cos(p.lon),
        y: r * c_lat * sin(p.lon),
        z: r * sin(p.lat),
    }
}

/// ECEF to position and altitude above the sphere.
pub fn from_ecef(e: Ecef) -> (LatLon, f64) {
    let horiz = hypot(e.x, e.y);
    let lat = atan2(e.z, horiz);
    let lon = atan2(e.y, e.x);
    let alt = sqrt(e.x * e.x + e.y * e.y + e.z * e.z) - EARTH_RADIUS_M;
    (LatLon::new(lat, normalize_lon(lon)), alt)
}

/// ENU offsets of `p` (with altitude) relative to `reference` (with altitude).
pub fn to_enu(reference: LatLon, ref_alt_m: f64, p: LatLon, alt_m: f64) -> Enu {
    let o = to_ecef(reference, ref_alt_m);
    let q = to_ecef(p, alt_m);
    let (dx, dy, dz) = (q.x - o.x, q.y - o.y, q.z - o.z);
    let (s_lat, c_lat) = (sin(reference.lat), cos(reference.lat));
    let (s_lon, c_lon) = (sin(reference.lon), cos(reference.lon));
    Enu {
        e: -s_lon * dx + c_lon * dy,
        n: -s_lat * c_lon * dx - s_lat * s_lon * dy + c_lat * dz,
        u: c_lat * c_lon * dx + c_lat * s_lon * dy + s_lat * dz,
    }
}

/// Position and altitude of ENU offsets relative to `reference` (with altitude).
pub fn from_enu(reference: LatLon, ref_alt_m: f64, enu: Enu) -> (LatLon, f64) {
    let o = to_ecef(reference, ref_alt_m);
    let (s_lat, c_lat) = (sin(reference.lat), cos(reference.lat));
    let (s_lon, c_lon) = (sin(reference.lon), cos(reference.lon));
    let dx = -s_lon * enu.e - s_lat * c_lon * enu.n + c_lat * c_lon * enu.u;
    let dy = c_lon * enu.e - s_lat * s_lon * enu.n + c_lat * s_lon * enu.u;
    let dz = c_lat * enu.n + s_lat * enu.u;
    from_ecef(Ecef {
        x: o.x + dx,
        y: o.y + dy,
        z: o.z + dz,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{deg_to_rad, rad_to_deg};

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn one_degree_of_latitude() {
        let d = distance_m(LatLon::from_deg(0.0, 10.0), LatLon::from_deg(1.0, 10.0));
        assert!(close(d, EARTH_RADIUS_M * deg_to_rad(1.0), 1e-6), "{d}");
    }

    #[test]
    fn london_to_paris() {
        // Reference great-circle distance on a 6371.0088 km sphere ≈ 343.9 km.
        let london = LatLon::from_deg(51.5074, -0.1278);
        let paris = LatLon::from_deg(48.8566, 2.3522);
        let d = distance_m(london, paris);
        assert!(close(d, 343_900.0, 1_000.0), "{d}");
        let b = rad_to_deg(initial_bearing(london, paris));
        assert!(close(b, 148.1, 0.5), "{b}");
    }

    #[test]
    fn cardinal_bearings() {
        let o = LatLon::from_deg(0.0, 0.0);
        assert!(close(
            initial_bearing(o, LatLon::from_deg(1.0, 0.0)),
            0.0,
            1e-12
        ));
        let east = rad_to_deg(initial_bearing(o, LatLon::from_deg(0.0, 1.0)));
        assert!(close(east, 90.0, 1e-9));
        let west = rad_to_deg(initial_bearing(o, LatLon::from_deg(0.0, -1.0)));
        assert!(close(west, 270.0, 1e-9));
    }

    #[test]
    fn destination_round_trips_distance_and_bearing() {
        let start = LatLon::from_deg(59.33, 18.07);
        for i in 0..36 {
            let bearing = deg_to_rad(f64::from(i) * 10.0);
            for &dist in &[1.0, 1_000.0, 250_000.0, 2_000_000.0] {
                let end = destination_point(start, bearing, dist);
                assert!(close(distance_m(start, end), dist, dist * 1e-9 + 1e-6));
                if dist > 1.0 {
                    let diff = normalize_lon(initial_bearing(start, end) - bearing);
                    assert!(diff.abs() < 1e-7, "bearing {i} dist {dist}: {diff}");
                }
            }
        }
    }

    #[test]
    fn crosses_antimeridian() {
        let start = LatLon::from_deg(0.0, 179.9);
        let end = destination_point(start, deg_to_rad(90.0), 50_000.0);
        assert!(end.lon < 0.0 && end.lon >= -PI, "{}", end.lon_deg());
        let d = distance_m(
            LatLon::from_deg(10.0, 179.5),
            LatLon::from_deg(10.0, -179.5),
        );
        assert!(close(
            d,
            distance_m(LatLon::from_deg(10.0, 0.0), LatLon::from_deg(10.0, 1.0)),
            1e-6
        ));
    }

    #[test]
    fn handles_poles() {
        let pole = LatLon::from_deg(90.0, 0.0);
        let p = destination_point(pole, deg_to_rad(180.0), 111_195.0);
        assert!(close(p.lat_deg(), 89.0, 1e-3));
        let d = distance_m(LatLon::from_deg(-90.0, 0.0), LatLon::from_deg(90.0, 0.0));
        assert!(close(d, PI * EARTH_RADIUS_M, 1e-6));
    }

    #[test]
    fn normalize_lon_range() {
        for &x in &[-10.0, -PI, -1e-18, 0.0, PI, 3.5, 10.0] {
            let n = normalize_lon(x);
            assert!((-PI..PI).contains(&n), "{x} -> {n}");
        }
    }

    #[test]
    fn enu_round_trip() {
        let reference = LatLon::from_deg(54.7, 20.5);
        let target = LatLon::from_deg(55.1, 21.3);
        let enu = to_enu(reference, 30.0, target, 9_000.0);
        assert!(enu.e > 0.0 && enu.n > 0.0);
        let (back, alt) = from_enu(reference, 30.0, enu);
        assert!(close(back.lat, target.lat, 1e-12));
        assert!(close(back.lon, target.lon, 1e-12));
        assert!(close(alt, 9_000.0, 1e-6));
    }

    #[test]
    fn enu_up_is_up() {
        let reference = LatLon::from_deg(-33.0, 151.0);
        let enu = to_enu(reference, 0.0, reference, 1_000.0);
        assert!(close(enu.u, 1_000.0, 1e-6) && enu.e.abs() < 1e-6 && enu.n.abs() < 1e-6);
    }
}
