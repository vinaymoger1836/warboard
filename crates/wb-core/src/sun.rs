//! Solar position for day and night (doc 02 §3, §7.2–7.3): the low-precision
//! formulas of the Astronomical Almanac (about 0.01° until 2050), on `libm`.

use crate::geo::LatLon;
use crate::math::{asin, atan2, cos, sin};
use crate::orbit::julian_date;
use core::f64::consts::PI;

const DEG: f64 = PI / 180.0;

/// Sun elevation above the horizon at `p`, radians (negative at night).
/// Refraction is ignored.
pub fn sun_elevation(p: LatLon, unix_ms: f64) -> f64 {
    let n = julian_date(unix_ms) - 2_451_545.0;
    let l = (280.460 + 0.985_647_4 * n) % 360.0;
    let g = ((357.528 + 0.985_600_3 * n) % 360.0) * DEG;
    let lambda = (l + 1.915 * sin(g) + 0.020 * sin(2.0 * g)) * DEG;
    let eps = (23.439 - 0.000_000_4 * n) * DEG;
    let ra = atan2(cos(eps) * sin(lambda), cos(lambda));
    let dec = asin(sin(eps) * sin(lambda));
    let gmst_deg = (280.460_618_37 + 360.985_647_366_29 * n) % 360.0;
    let ha = gmst_deg * DEG + p.lon - ra;
    asin(sin(p.lat) * sin(dec) + cos(p.lat) * cos(dec) * cos(ha))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-06-21 (solstice) noon and midnight UTC on the meridian at the equator.
    #[test]
    fn solstice_on_the_equator() {
        let noon = 1_782_043_200_000.0;
        let el = sun_elevation(LatLon::from_deg(0.0, 0.0), noon) / DEG;
        assert!((el - 66.56).abs() < 0.5, "noon {el}");
        let midnight = noon - 12.0 * 3_600_000.0;
        let el = sun_elevation(LatLon::from_deg(0.0, 0.0), midnight) / DEG;
        assert!((el + 66.56).abs() < 0.5, "midnight {el}");
    }

    /// Baltic in late October: day at noon, night at 22:00 UTC.
    #[test]
    fn baltic_october() {
        let kaliningrad = LatLon::from_deg(54.7, 20.5);
        let noon = 1_793_102_400_000.0; // 2026-10-27T12:00:00Z, 1.5 h after local noon
        let el = sun_elevation(kaliningrad, noon) / DEG;
        assert!((15.0..25.0).contains(&el), "noon {el}");
        let night = noon + 10.0 * 3_600_000.0;
        assert!(sun_elevation(kaliningrad, night) < -30.0 * DEG);
    }
}
