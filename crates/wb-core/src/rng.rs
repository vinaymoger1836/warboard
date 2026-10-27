//! Counter-based random numbers (doc 02 §4, ADR-006).
//!
//! `rng(seed, purpose, entity, tick, n)` is a pure hash of its inputs: a draw
//! never depends on how many other draws happened, in what order, or on which
//! platform. Built from the SplitMix64 finaliser applied word by word.

/// Why a random number is drawn. The discriminants are part of the save and
/// replay format: never renumber, only append.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum RngPurpose {
    /// Tests only.
    Test = 1,
    /// P0 determinism probe system in `wb-sim`.
    Probe = 2,
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64 finaliser (Stafford variant 13): a bijection on u64 with full avalanche.
#[inline]
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 64 random bits for the given counter.
#[inline]
pub const fn rng_u64(seed: u64, purpose: RngPurpose, entity: u64, tick: u64, n: u32) -> u64 {
    let mut h = mix(seed.wrapping_add(GOLDEN));
    h = mix(h.wrapping_add(GOLDEN) ^ purpose as u64);
    h = mix(h.wrapping_add(GOLDEN) ^ entity);
    h = mix(h.wrapping_add(GOLDEN) ^ tick);
    h = mix(h.wrapping_add(GOLDEN) ^ n as u64);
    h
}

/// Uniform `f64` in [0, 1) for the given counter (53 random bits).
#[inline]
pub fn rng(seed: u64, purpose: RngPurpose, entity: u64, tick: u64, n: u32) -> f64 {
    const SCALE: f64 = 1.0 / (1_u64 << 53) as f64;
    (rng_u64(seed, purpose, entity, tick, n) >> 11) as f64 * SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u64 = 0x5EED_0000_0000_0042;

    /// Chi-square statistic of `samples` over `bins` equal bins of [0, 1).
    fn chi_square(samples: impl Iterator<Item = f64>, bins: usize) -> f64 {
        let mut counts = vec![0_u64; bins];
        let mut total = 0_u64;
        for u in samples {
            assert!((0.0..1.0).contains(&u), "out of range: {u}");
            counts[(u * bins as f64) as usize] += 1;
            total += 1;
        }
        let expected = total as f64 / bins as f64;
        counts
            .iter()
            .map(|&c| {
                let d = c as f64 - expected;
                d * d / expected
            })
            .sum()
    }

    /// 99 degrees of freedom: mean 99, sd ≈ 14. 170 is beyond p = 1e-5.
    const CHI2_LIMIT_99: f64 = 170.0;

    #[test]
    fn uniform_over_n() {
        let chi = chi_square(
            (0..200_000).map(|n| rng(SEED, RngPurpose::Test, 7, 100, n)),
            100,
        );
        assert!(chi < CHI2_LIMIT_99, "chi2 {chi}");
    }

    #[test]
    fn uniform_over_tick() {
        let chi = chi_square(
            (0..200_000).map(|t| rng(SEED, RngPurpose::Test, 7, t, 0)),
            100,
        );
        assert!(chi < CHI2_LIMIT_99, "chi2 {chi}");
    }

    #[test]
    fn uniform_over_entity() {
        let chi = chi_square(
            (0..200_000).map(|e| rng(SEED, RngPurpose::Test, e, 0, 0)),
            100,
        );
        assert!(chi < CHI2_LIMIT_99, "chi2 {chi}");
    }

    #[test]
    fn uniform_over_seed() {
        let chi = chi_square((0..200_000).map(|s| rng(s, RngPurpose::Test, 0, 0, 0)), 100);
        assert!(chi < CHI2_LIMIT_99, "chi2 {chi}");
    }

    #[test]
    fn mean_and_variance() {
        let n = 400_000_u32;
        let (mut sum, mut sum_sq) = (0.0, 0.0);
        for i in 0..n {
            let u = rng(SEED, RngPurpose::Test, 1, u64::from(i / 64), i % 64);
            sum += u;
            sum_sq += u * u;
        }
        let mean = sum / f64::from(n);
        let var = sum_sq / f64::from(n) - mean * mean;
        assert!((mean - 0.5).abs() < 0.002, "mean {mean}");
        assert!((var - 1.0 / 12.0).abs() < 0.001, "var {var}");
    }

    fn correlation(pairs: &[(f64, f64)]) -> f64 {
        let n = pairs.len() as f64;
        let (mx, my) = pairs
            .iter()
            .fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
        let (mx, my) = (mx / n, my / n);
        let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
        for (x, y) in pairs {
            sxy += (x - mx) * (y - my);
            sxx += (x - mx) * (x - mx);
            syy += (y - my) * (y - my);
        }
        sxy / crate::math::sqrt(sxx * syy)
    }

    #[test]
    fn neighbouring_counters_are_uncorrelated() {
        // sd of r for 100k pairs ≈ 0.0032; 0.015 is ~5 sd.
        let lag_n: Vec<_> = (0..100_000)
            .map(|n| {
                (
                    rng(SEED, RngPurpose::Test, 3, 9, n),
                    rng(SEED, RngPurpose::Test, 3, 9, n + 1),
                )
            })
            .collect();
        let lag_entity: Vec<_> = (0..100_000)
            .map(|e| {
                (
                    rng(SEED, RngPurpose::Test, e, 9, 0),
                    rng(SEED, RngPurpose::Test, e + 1, 9, 0),
                )
            })
            .collect();
        let lag_purpose: Vec<_> = (0..100_000)
            .map(|t| {
                (
                    rng(SEED, RngPurpose::Test, 3, t, 0),
                    rng(SEED, RngPurpose::Probe, 3, t, 0),
                )
            })
            .collect();
        let lag_seed: Vec<_> = (0..100_000)
            .map(|t| {
                (
                    rng(SEED, RngPurpose::Test, 3, t, 0),
                    rng(SEED + 1, RngPurpose::Test, 3, t, 0),
                )
            })
            .collect();
        for (name, pairs) in [
            ("n", lag_n),
            ("entity", lag_entity),
            ("purpose", lag_purpose),
            ("seed", lag_seed),
        ] {
            let r = correlation(&pairs);
            assert!(r.abs() < 0.015, "{name}: r = {r}");
        }
    }

    #[test]
    fn independent_of_call_order() {
        let keys: Vec<(u64, u64, u32)> = (0..64_u32)
            .map(|i| (u64::from(i % 5), u64::from(i / 5), i % 3))
            .collect();
        let forward: Vec<f64> = keys
            .iter()
            .map(|&(e, t, n)| rng(SEED, RngPurpose::Test, e, t, n))
            .collect();

        // Reverse order, with unrelated draws interleaved.
        let mut reverse = vec![0.0; keys.len()];
        for (i, &(e, t, n)) in keys.iter().enumerate().rev() {
            let _noise = rng(SEED, RngPurpose::Probe, 999, t, n);
            reverse[i] = rng(SEED, RngPurpose::Test, e, t, n);
        }

        // A fixed scramble of the order.
        let mut scrambled = vec![0.0; keys.len()];
        for k in 0..keys.len() {
            let i = (k * 37 + 11) % keys.len();
            let (e, t, n) = keys[i];
            scrambled[i] = rng(SEED, RngPurpose::Test, e, t, n);
        }

        assert_eq!(forward, reverse);
        assert_eq!(forward, scrambled);
    }

    #[test]
    fn every_input_matters() {
        let base = rng_u64(SEED, RngPurpose::Test, 1, 2, 3);
        assert_ne!(base, rng_u64(SEED + 1, RngPurpose::Test, 1, 2, 3));
        assert_ne!(base, rng_u64(SEED, RngPurpose::Probe, 1, 2, 3));
        assert_ne!(base, rng_u64(SEED, RngPurpose::Test, 2, 2, 3));
        assert_ne!(base, rng_u64(SEED, RngPurpose::Test, 1, 3, 3));
        assert_ne!(base, rng_u64(SEED, RngPurpose::Test, 1, 2, 4));
        // Swapping fields must not collide.
        assert_ne!(
            rng_u64(0, RngPurpose::Test, 1, 2, 3),
            rng_u64(0, RngPurpose::Test, 2, 1, 3)
        );
    }

    /// Pinned outputs. If this fails, every saved war and golden hash changes:
    /// that needs an explicit decision, not a test update.
    #[test]
    fn known_answers() {
        let got = [
            rng_u64(0, RngPurpose::Test, 0, 0, 0),
            rng_u64(SEED, RngPurpose::Test, 1, 2, 3),
            rng_u64(u64::MAX, RngPurpose::Probe, u64::MAX, u64::MAX, u32::MAX),
        ];
        assert_eq!(got, KNOWN, "got {got:#018x?}");
        assert_eq!(
            rng(0, RngPurpose::Test, 0, 0, 0),
            (KNOWN[0] >> 11) as f64 / (1_u64 << 53) as f64
        );
    }

    const KNOWN: [u64; 3] = [
        0xf192_64ec_81b7_7bd5,
        0x7a8b_9bbb_a5d4_745f,
        0x9a1c_7584_a9ab_cfc0,
    ];
}
