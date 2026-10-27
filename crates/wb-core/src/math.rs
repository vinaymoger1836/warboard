//! The only door to transcendental maths in the engine.
//!
//! Everything goes through `libm` (pure Rust), so results are bit-identical on
//! every platform. Clippy bans the std `f64` methods (see `clippy.toml`).

#[inline]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

#[inline]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

#[inline]
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

#[inline]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// Natural logarithm.
#[inline]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

#[inline]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

#[inline]
pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

#[inline]
pub fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

/// IEEE square root is correctly rounded everywhere; `libm` keeps it uniform.
#[inline]
pub fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}
