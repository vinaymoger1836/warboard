//! SI unit helpers. The engine works in m, s, kg, rad; content and UI use the
//! units below and convert at the edges.

use core::f64::consts::PI;

pub const M_PER_KM: f64 = 1_000.0;
/// International nautical mile.
pub const M_PER_NM: f64 = 1_852.0;
/// International foot.
pub const M_PER_FT: f64 = 0.3048;
/// Speed of sound at sea level, ISA (15 °C).
pub const MACH_SEA_LEVEL_MPS: f64 = 340.294;

pub const fn deg_to_rad(deg: f64) -> f64 {
    deg * (PI / 180.0)
}

pub const fn rad_to_deg(rad: f64) -> f64 {
    rad * (180.0 / PI)
}

pub const fn km_to_m(km: f64) -> f64 {
    km * M_PER_KM
}

pub const fn m_to_km(m: f64) -> f64 {
    m / M_PER_KM
}

pub const fn nm_to_m(nm: f64) -> f64 {
    nm * M_PER_NM
}

pub const fn m_to_nm(m: f64) -> f64 {
    m / M_PER_NM
}

pub const fn ft_to_m(ft: f64) -> f64 {
    ft * M_PER_FT
}

pub const fn m_to_ft(m: f64) -> f64 {
    m / M_PER_FT
}

/// Knots to m/s.
pub const fn kt_to_mps(kt: f64) -> f64 {
    kt * M_PER_NM / 3_600.0
}

/// m/s to knots.
pub const fn mps_to_kt(mps: f64) -> f64 {
    mps * 3_600.0 / M_PER_NM
}

pub const fn kmh_to_mps(kmh: f64) -> f64 {
    kmh / 3.6
}

pub const fn mps_to_kmh(mps: f64) -> f64 {
    mps * 3.6
}

/// Mach number to m/s at sea level. Altitude-dependent speed of sound is a
/// model choice and belongs to the realism profile, not here.
pub const fn mach_sl_to_mps(mach: f64) -> f64 {
    mach * MACH_SEA_LEVEL_MPS
}

pub const fn min_to_s(min: f64) -> f64 {
    min * 60.0
}

pub const fn h_to_s(h: f64) -> f64 {
    h * 3_600.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_round_trip() {
        assert_eq!(nm_to_m(12.0), 22_224.0);
        assert!((mps_to_kt(kt_to_mps(450.0)) - 450.0).abs() < 1e-12);
        assert!((rad_to_deg(deg_to_rad(57.3)) - 57.3).abs() < 1e-12);
        assert!((ft_to_m(1_000.0) - 304.8).abs() < 1e-9);
        assert_eq!(kmh_to_mps(36.0), 10.0);
    }
}
