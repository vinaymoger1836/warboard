//! Two-line elements and the SGP4 propagator (near-Earth only), on `libm`
//! through [`crate::math`] so a satellite is in the same place natively and in
//! WebAssembly (ADR-024).
//!
//! The algorithm and constants follow Vallado, Crawford, Hujsak and Kelso,
//! *Revisiting Spacetrack Report #3* (AIAA 2006-6753), WGS-72, "improved"
//! mode. Deep-space orbits (period ≥ 225 min) need SDP4 and are refused:
//! imaging and SIGINT satellites fly low.

use crate::math::{atan2, cos, pow, sin, sqrt};
use core::f64::consts::{PI, TAU};

/// WGS-72 Earth radius, km (SGP4's own; positions are converted to the
/// engine's sphere afterwards).
const RE_KM: f64 = 6378.135;
const MU: f64 = 398_600.8;
const J2: f64 = 0.001_082_616;
const J3: f64 = -0.000_002_538_81;
const J4: f64 = -0.000_001_655_97;
const J3OJ2: f64 = J3 / J2;
const X2O3: f64 = 2.0 / 3.0;
const MS_PER_DAY: f64 = 86_400_000.0;
/// Julian date of the Unix epoch.
const JD_UNIX: f64 = 2_440_587.5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TleError {
    /// A line is too short or does not start with its line number.
    Format(&'static str),
    /// Column 69 does not match the line's checksum.
    Checksum(u8),
    /// A field is not a number.
    Field(&'static str),
    /// Period of 225 minutes or more: needs SDP4.
    DeepSpace,
}

impl core::fmt::Display for TleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TleError::Format(m) => write!(f, "{m}"),
            TleError::Checksum(l) => write!(f, "line {l} checksum does not match"),
            TleError::Field(n) => write!(f, "field `{n}` is not a number"),
            TleError::DeepSpace => write!(
                f,
                "period of 225 minutes or more (deep space) is not supported"
            ),
        }
    }
}

/// Mean elements at epoch, as read from a TLE.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tle {
    /// Epoch, Unix milliseconds UTC.
    pub epoch_unix_ms: f64,
    /// Drag term, 1 / Earth radii.
    pub bstar: f64,
    pub incl_rad: f64,
    pub raan_rad: f64,
    pub ecc: f64,
    pub argp_rad: f64,
    pub mean_anomaly_rad: f64,
    /// Kozai mean motion, rad/min.
    pub mean_motion_rad_min: f64,
}

fn checksum(line: &str) -> u8 {
    let sum: u32 = line
        .bytes()
        .take(68)
        .map(|b| match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'-' => 1,
            _ => 0,
        })
        .sum();
    (sum % 10) as u8
}

fn field(line: &str, cols: core::ops::Range<usize>, name: &'static str) -> Result<f64, TleError> {
    line.get(cols)
        .map(str::trim)
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or(TleError::Field(name))
}

/// `±NNNNN±E` with an implied leading decimal point (B* and n̈).
fn implied_exp(line: &str, cols: core::ops::Range<usize>, name: &'static str) -> Result<f64, TleError> {
    let s = line.get(cols).ok_or(TleError::Field(name))?.trim();
    if s.is_empty() {
        return Ok(0.0);
    }
    let (mant, exp) = s.split_at(s.len().saturating_sub(2));
    let sign = if mant.starts_with('-') { -1.0 } else { 1.0 };
    let digits = mant.trim_start_matches(['-', '+', ' ']);
    let m: f64 = format!("0.{digits}").parse().map_err(|_| TleError::Field(name))?;
    let e: i32 = exp.parse().map_err(|_| TleError::Field(name))?;
    Ok(sign * m * pow(10.0, f64::from(e)))
}

impl Tle {
    pub fn parse(line1: &str, line2: &str) -> Result<Self, TleError> {
        let (l1, l2) = (line1.trim_end(), line2.trim_end());
        if l1.len() < 69 || !l1.starts_with("1 ") {
            return Err(TleError::Format("line 1 must start with `1 ` and have 69 columns"));
        }
        if l2.len() < 69 || !l2.starts_with("2 ") {
            return Err(TleError::Format("line 2 must start with `2 ` and have 69 columns"));
        }
        for (n, l) in [(1u8, l1), (2, l2)] {
            let want = l.as_bytes()[68].wrapping_sub(b'0');
            if checksum(l) != want {
                return Err(TleError::Checksum(n));
            }
        }
        let yy = field(l1, 18..20, "epoch year")?;
        let doy = field(l1, 20..32, "epoch day")?;
        let year = if yy < 57.0 { 2000.0 + yy } else { 1900.0 + yy } as i64;
        let epoch_unix_ms = days_from_civil(year, 1, 1) as f64 * MS_PER_DAY + (doy - 1.0) * MS_PER_DAY;
        let deg = PI / 180.0;
        let ecc: f64 = format!("0.{}", l2.get(26..33).ok_or(TleError::Field("eccentricity"))?.trim())
            .parse()
            .map_err(|_| TleError::Field("eccentricity"))?;
        let tle = Tle {
            epoch_unix_ms,
            bstar: implied_exp(l1, 53..61, "bstar")?,
            incl_rad: field(l2, 8..16, "inclination")? * deg,
            raan_rad: field(l2, 17..25, "raan")? * deg,
            ecc,
            argp_rad: field(l2, 34..42, "argument of perigee")? * deg,
            mean_anomaly_rad: field(l2, 43..51, "mean anomaly")? * deg,
            mean_motion_rad_min: field(l2, 52..63, "mean motion")? * TAU / 1440.0,
        };
        if TAU / tle.mean_motion_rad_min >= 225.0 {
            return Err(TleError::DeepSpace);
        }
        Ok(tle)
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

/// Julian date of a Unix time in milliseconds.
pub fn julian_date(unix_ms: f64) -> f64 {
    unix_ms / MS_PER_DAY + JD_UNIX
}

/// Greenwich mean sidereal time (IAU-82), radians in [0, 2π).
pub fn gmst(unix_ms: f64) -> f64 {
    let t = (julian_date(unix_ms) - 2_451_545.0) / 36_525.0;
    let sec = -6.2e-6 * t * t * t
        + 0.093_104 * t * t
        + (876_600.0 * 3_600.0 + 8_640_184.812_866) * t
        + 67_310.548_41;
    let g = (sec * PI / 180.0 / 240.0) % TAU;
    if g < 0.0 { g + TAU } else { g }
}

/// Position and velocity in the TEME frame, km and km/s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateTeme {
    pub r_km: [f64; 3],
    pub v_kms: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sgp4Error {
    /// Eccentricity left [0, 1).
    Eccentricity,
    /// Mean motion fell to zero or below.
    MeanMotion,
    /// Semi-latus rectum negative.
    SemiLatus,
    /// Below the Earth's surface: the orbit has decayed.
    Decayed,
}

/// SGP4 state initialised from one TLE (`sgp4init`, near-Earth branch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sgp4 {
    tle: Tle,
    xke: f64,
    no: f64,
    isimp: bool,
    con41: f64,
    cc1: f64,
    cc4: f64,
    cc5: f64,
    d2: f64,
    d3: f64,
    d4: f64,
    delmo: f64,
    eta: f64,
    argpdot: f64,
    omgcof: f64,
    sinmao: f64,
    t2cof: f64,
    t3cof: f64,
    t4cof: f64,
    t5cof: f64,
    x1mth2: f64,
    x7thm1: f64,
    mdot: f64,
    nodedot: f64,
    xlcof: f64,
    xmcof: f64,
    nodecf: f64,
    aycof: f64,
}

impl Sgp4 {
    pub fn new(tle: Tle) -> Self {
        let xke = 60.0 / sqrt(RE_KM * RE_KM * RE_KM / MU);
        let ecco = tle.ecc;
        let inclo = tle.incl_rad;
        let argpo = tle.argp_rad;
        let mo = tle.mean_anomaly_rad;
        let bstar = tle.bstar;

        // initl: un-Kozai the mean motion.
        let eccsq = ecco * ecco;
        let omeosq = 1.0 - eccsq;
        let rteosq = sqrt(omeosq);
        let cosio = cos(inclo);
        let cosio2 = cosio * cosio;
        let ak = pow(xke / tle.mean_motion_rad_min, X2O3);
        let d1 = 0.75 * J2 * (3.0 * cosio2 - 1.0) / (rteosq * omeosq);
        let mut del = d1 / (ak * ak);
        let adel = ak * (1.0 - del * del - del * (1.0 / 3.0 + 134.0 * del * del / 81.0));
        del = d1 / (adel * adel);
        let no = tle.mean_motion_rad_min / (1.0 + del);
        let ao = pow(xke / no, X2O3);
        let sinio = sin(inclo);
        let po = ao * omeosq;
        let con42 = 1.0 - 5.0 * cosio2;
        let con41 = -con42 - cosio2 - cosio2;
        let posq = po * po;
        let rp = ao * (1.0 - ecco);

        let isimp = rp < 220.0 / RE_KM + 1.0;
        let mut sfour = 78.0 / RE_KM + 1.0;
        let mut qzms24 = pow((120.0 - 78.0) / RE_KM, 4.0);
        let perige = (rp - 1.0) * RE_KM;
        if perige < 156.0 {
            sfour = if perige < 98.0 { 20.0 } else { perige - 78.0 };
            qzms24 = pow((120.0 - sfour) / RE_KM, 4.0);
            sfour = sfour / RE_KM + 1.0;
        }
        let pinvsq = 1.0 / posq;
        let tsi = 1.0 / (ao - sfour);
        let eta = ao * ecco * tsi;
        let etasq = eta * eta;
        let eeta = ecco * eta;
        let psisq = (1.0 - etasq).abs();
        let coef = qzms24 * pow(tsi, 4.0);
        let coef1 = coef / pow(psisq, 3.5);
        let cc2 = coef1
            * no
            * (ao * (1.0 + 1.5 * etasq + eeta * (4.0 + etasq))
                + 0.375 * J2 * tsi / psisq * con41 * (8.0 + 3.0 * etasq * (8.0 + etasq)));
        let cc1 = bstar * cc2;
        let cc3 = if ecco > 1.0e-4 {
            -2.0 * coef * tsi * J3OJ2 * no * sinio / ecco
        } else {
            0.0
        };
        let x1mth2 = 1.0 - cosio2;
        let cc4 = 2.0
            * no
            * coef1
            * ao
            * omeosq
            * (eta * (2.0 + 0.5 * etasq) + ecco * (0.5 + 2.0 * etasq)
                - J2 * tsi / (ao * psisq)
                    * (-3.0 * con41 * (1.0 - 2.0 * eeta + etasq * (1.5 - 0.5 * eeta))
                        + 0.75 * x1mth2 * (2.0 * etasq - eeta * (1.0 + etasq)) * cos(2.0 * argpo)));
        let cc5 = 2.0 * coef1 * ao * omeosq * (1.0 + 2.75 * (etasq + eeta) + eeta * etasq);
        let cosio4 = cosio2 * cosio2;
        let temp1 = 1.5 * J2 * pinvsq * no;
        let temp2 = 0.5 * temp1 * J2 * pinvsq;
        let temp3 = -0.46875 * J4 * pinvsq * pinvsq * no;
        let mdot = no
            + 0.5 * temp1 * rteosq * con41
            + 0.0625 * temp2 * rteosq * (13.0 - 78.0 * cosio2 + 137.0 * cosio4);
        let argpdot = -0.5 * temp1 * con42
            + 0.0625 * temp2 * (7.0 - 114.0 * cosio2 + 395.0 * cosio4)
            + temp3 * (3.0 - 36.0 * cosio2 + 49.0 * cosio4);
        let xhdot1 = -temp1 * cosio;
        let nodedot =
            xhdot1 + (0.5 * temp2 * (4.0 - 19.0 * cosio2) + 2.0 * temp3 * (3.0 - 7.0 * cosio2)) * cosio;
        let omgcof = bstar * cc3 * cos(argpo);
        let xmcof = if ecco > 1.0e-4 {
            -X2O3 * coef * bstar / eeta
        } else {
            0.0
        };
        let nodecf = 3.5 * omeosq * xhdot1 * cc1;
        let t2cof = 1.5 * cc1;
        let xlcof_den = if (cosio + 1.0).abs() > 1.5e-12 {
            1.0 + cosio
        } else {
            1.5e-12
        };
        let xlcof = -0.25 * J3OJ2 * sinio * (3.0 + 5.0 * cosio) / xlcof_den;
        let aycof = -0.5 * J3OJ2 * sinio;
        let dm = 1.0 + eta * cos(mo);
        let delmo = dm * dm * dm;
        let sinmao = sin(mo);
        let x7thm1 = 7.0 * cosio2 - 1.0;

        let (mut d2, mut d3, mut d4, mut t3cof, mut t4cof, mut t5cof) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        if !isimp {
            let cc1sq = cc1 * cc1;
            d2 = 4.0 * ao * tsi * cc1sq;
            let temp = d2 * tsi * cc1 / 3.0;
            d3 = (17.0 * ao + sfour) * temp;
            d4 = 0.5 * temp * ao * tsi * (221.0 * ao + 31.0 * sfour) * cc1;
            t3cof = d2 + 2.0 * cc1sq;
            t4cof = 0.25 * (3.0 * d3 + cc1 * (12.0 * d2 + 10.0 * cc1sq));
            t5cof = 0.2 * (3.0 * d4 + 12.0 * cc1 * d3 + 6.0 * d2 * d2 + 15.0 * cc1sq * (2.0 * d2 + cc1sq));
        }
        Sgp4 {
            tle,
            xke,
            no,
            isimp,
            con41,
            cc1,
            cc4,
            cc5,
            d2,
            d3,
            d4,
            delmo,
            eta,
            argpdot,
            omgcof,
            sinmao,
            t2cof,
            t3cof,
            t4cof,
            t5cof,
            x1mth2,
            x7thm1,
            mdot,
            nodedot,
            xlcof,
            xmcof,
            nodecf,
            aycof,
        }
    }

    pub fn tle(&self) -> &Tle {
        &self.tle
    }

    /// Position and velocity `t_min` minutes after the TLE epoch.
    pub fn propagate(&self, t_min: f64) -> Result<StateTeme, Sgp4Error> {
        let s = self;
        let e = &s.tle;
        let bstar = e.bstar;
        let t = t_min;
        let xmdf = e.mean_anomaly_rad + s.mdot * t;
        let argpdf = e.argp_rad + s.argpdot * t;
        let nodedf = e.raan_rad + s.nodedot * t;
        let mut argpm = argpdf;
        let mut mm = xmdf;
        let t2 = t * t;
        let mut nodem = nodedf + s.nodecf * t2;
        let mut tempa = 1.0 - s.cc1 * t;
        let mut tempe = bstar * s.cc4 * t;
        let mut templ = s.t2cof * t2;
        if !s.isimp {
            let delomg = s.omgcof * t;
            let dm = 1.0 + s.eta * cos(xmdf);
            let delm = s.xmcof * (dm * dm * dm - s.delmo);
            let temp = delomg + delm;
            mm = xmdf + temp;
            argpm = argpdf - temp;
            let t3 = t2 * t;
            let t4 = t3 * t;
            tempa = tempa - s.d2 * t2 - s.d3 * t3 - s.d4 * t4;
            tempe += bstar * s.cc5 * (sin(mm) - s.sinmao);
            templ = templ + s.t3cof * t3 + t4 * (s.t4cof + t * s.t5cof);
        }
        let mut nm = s.no;
        let mut em = e.ecc;
        if nm <= 0.0 {
            return Err(Sgp4Error::MeanMotion);
        }
        let am = pow(s.xke / nm, X2O3) * tempa * tempa;
        nm = s.xke / pow(am, 1.5);
        em -= tempe;
        if !(-0.001..1.0).contains(&em) {
            return Err(Sgp4Error::Eccentricity);
        }
        if em < 1.0e-6 {
            em = 1.0e-6;
        }
        mm += s.no * templ;
        let mut xlm = mm + argpm + nodem;
        nodem %= TAU;
        argpm %= TAU;
        xlm %= TAU;
        mm = (xlm - argpm - nodem) % TAU;
        let inclm = e.incl_rad;
        let (sinim, cosim) = (sin(inclm), cos(inclm));

        let ep = em;
        let axnl = ep * cos(argpm);
        let temp = 1.0 / (am * (1.0 - ep * ep));
        let aynl = ep * sin(argpm) + temp * s.aycof;
        let xl = mm + argpm + nodem + temp * s.xlcof * axnl;

        // Kepler's equation.
        let u = (xl - nodem) % TAU;
        let mut eo1 = u;
        let mut tem5: f64 = 9999.9;
        let mut ktr = 1;
        let (mut sineo1, mut coseo1) = (0.0, 0.0);
        while tem5.abs() >= 1.0e-12 && ktr <= 10 {
            sineo1 = sin(eo1);
            coseo1 = cos(eo1);
            tem5 = 1.0 - coseo1 * axnl - sineo1 * aynl;
            tem5 = (u - aynl * coseo1 + axnl * sineo1 - eo1) / tem5;
            if tem5.abs() >= 0.95 {
                tem5 = if tem5 > 0.0 { 0.95 } else { -0.95 };
            }
            eo1 += tem5;
            ktr += 1;
        }

        let ecose = axnl * coseo1 + aynl * sineo1;
        let esine = axnl * sineo1 - aynl * coseo1;
        let el2 = axnl * axnl + aynl * aynl;
        let pl = am * (1.0 - el2);
        if pl < 0.0 {
            return Err(Sgp4Error::SemiLatus);
        }
        let rl = am * (1.0 - ecose);
        let rdotl = sqrt(am) * esine / rl;
        let rvdotl = sqrt(pl) / rl;
        let betal = sqrt(1.0 - el2);
        let temp = esine / (1.0 + betal);
        let sinu = am / rl * (sineo1 - aynl - axnl * temp);
        let cosu = am / rl * (coseo1 - axnl + aynl * temp);
        let mut su = atan2(sinu, cosu);
        let sin2u = (cosu + cosu) * sinu;
        let cos2u = 1.0 - 2.0 * sinu * sinu;
        let temp = 1.0 / pl;
        let temp1 = 0.5 * J2 * temp;
        let temp2 = temp1 * temp;

        let mrt = rl * (1.0 - 1.5 * temp2 * betal * s.con41) + 0.5 * temp1 * s.x1mth2 * cos2u;
        su -= 0.25 * temp2 * s.x7thm1 * sin2u;
        let xnode = nodem + 1.5 * temp2 * cosim * sin2u;
        let xinc = inclm + 1.5 * temp2 * cosim * sinim * cos2u;
        let mvt = rdotl - nm * temp1 * s.x1mth2 * sin2u / s.xke;
        let rvdot = rvdotl + nm * temp1 * (s.x1mth2 * cos2u + 1.5 * s.con41) / s.xke;

        let (sinsu, cossu) = (sin(su), cos(su));
        let (snod, cnod) = (sin(xnode), cos(xnode));
        let (sini, cosi) = (sin(xinc), cos(xinc));
        let xmx = -snod * cosi;
        let xmy = cnod * cosi;
        let ux = xmx * sinsu + cnod * cossu;
        let uy = xmy * sinsu + snod * cossu;
        let uz = sini * sinsu;
        let vx = xmx * cossu - cnod * sinsu;
        let vy = xmy * cossu - snod * sinsu;
        let vz = sini * cossu;
        if mrt < 1.0 {
            return Err(Sgp4Error::Decayed);
        }
        let vkms = RE_KM * s.xke / 60.0;
        Ok(StateTeme {
            r_km: [mrt * ux * RE_KM, mrt * uy * RE_KM, mrt * uz * RE_KM],
            v_kms: [
                (mvt * ux + rvdot * vx) * vkms,
                (mvt * uy + rvdot * vy) * vkms,
                (mvt * uz + rvdot * vz) * vkms,
            ],
        })
    }

    /// Position at a Unix time, Earth-fixed (TEME rotated by GMST; polar
    /// motion ignored), metres.
    pub fn ecef_m(&self, unix_ms: f64) -> Result<[f64; 3], Sgp4Error> {
        let st = self.propagate((unix_ms - self.tle.epoch_unix_ms) / 60_000.0)?;
        let g = gmst(unix_ms);
        let (sg, cg) = (sin(g), cos(g));
        let [x, y, z] = st.r_km;
        Ok([
            (cg * x + sg * y) * 1_000.0,
            (-sg * x + cg * y) * 1_000.0,
            z * 1_000.0,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: [f64; 3], want: [f64; 3], tol: f64) {
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() < tol,
                "component {i}: got {} want {}",
                got[i],
                want[i]
            );
        }
    }

    /// Test case 00005 of Vallado et al. 2006 (SGP4-VER.TLE / tcppver.out).
    #[test]
    fn vanguard_00005() {
        let tle = Tle::parse(
            "1 00005U 58002B   00179.78495062  .00000023  00000-0  28098-4 0  4753",
            "2 00005  34.2682 348.7242 1859667 331.7664  19.3264 10.82419157413667",
        )
        .unwrap();
        let s = Sgp4::new(tle);
        let p0 = s.propagate(0.0).unwrap();
        close(p0.r_km, [7022.46529266, -1400.08296755, 0.03995155], 1e-4);
        close(p0.v_kms, [1.893841015, 6.405893759, 4.534807250], 1e-7);
        let p360 = s.propagate(360.0).unwrap();
        close(p360.r_km, [-7154.03120202, -3783.17682504, -3536.19412294], 1e-4);
        close(p360.v_kms, [4.741887409, -4.151817765, -2.093935425], 1e-7);
    }

    /// Test case 06251 (low perigee, drag) of the same set.
    #[test]
    fn delta_06251() {
        let tle = Tle::parse(
            "1 06251U 62025E   06176.82412014  .00008885  00000-0  12808-3 0  3985",
            "2 06251  58.0579  54.0425 0030035 139.1568 221.1854 15.56387291  6774",
        )
        .unwrap();
        let p0 = Sgp4::new(tle).propagate(0.0).unwrap();
        close(p0.r_km, [3988.31022699, 5498.96657235, 0.90055879], 1e-4);
        close(p0.v_kms, [-3.290032738, 2.357652820, 6.496623475], 1e-7);
    }

    #[test]
    fn rejects_bad_checksum_and_deep_space() {
        let l2 = "2 00005  34.2682 348.7242 1859667 331.7664  19.3264 10.82419157413667";
        assert_eq!(
            Tle::parse(
                "1 00005U 58002B   00179.78495062  .00000023  00000-0  28098-4 0  4754",
                l2
            ),
            Err(TleError::Checksum(1))
        );
        // A geostationary orbit (1 rev/day).
        let geo2 = "2 99999   0.0100 100.0000 0001000   0.0000   0.0000  1.00270000    00";
        let fixed = format!("{}{}", &geo2[..68], checksum(geo2));
        assert_eq!(
            Tle::parse(
                "1 00005U 58002B   00179.78495062  .00000023  00000-0  28098-4 0  4753",
                &fixed
            ),
            Err(TleError::DeepSpace)
        );
    }

    #[test]
    fn epoch_and_gmst() {
        let tle = Tle::parse(
            "1 00005U 58002B   00179.78495062  .00000023  00000-0  28098-4 0  4753",
            "2 00005  34.2682 348.7242 1859667 331.7664  19.3264 10.82419157413667",
        )
        .unwrap();
        // 2000-06-27 18:50:19.73 UTC.
        let want = 962_131_819_733.568;
        assert!((tle.epoch_unix_ms - want).abs() < 1.0, "{}", tle.epoch_unix_ms);
        // GMST at J2000.0 (2000-01-01 12:00 UT1) is 280.46061837°.
        let g = gmst(946_728_000_000.0) * 180.0 / PI;
        assert!((g - 280.460_618_37).abs() < 1e-6, "{g}");
    }
}
