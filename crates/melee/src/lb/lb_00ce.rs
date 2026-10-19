//! Port of the numeric helpers in `src/melee/lb/lb_00CE.c`: series-based
//! `expf`/`powf`, the integer `powi`, an `atan2`-like angle function, and a
//! segment proximity test.
//!
//! Call sites in the 1.02 DOL: `lb_8000D008` 19 (16 in `ftCo_*` code),
//! `powf` 10 (one in fighter code), `powi` 4 (menus), `expf` 2, and
//! `lb_8000D148` 1 (`Fighter_procInput`'s control-stick processing).
//!
//! `expf` sums its Taylor series until the sum stops changing. For a NaN,
//! infinite, or large (overflowing) argument the sum becomes NaN and the
//! loop never ends, on hardware as in this port; `powf` inherits that for
//! NaN.

// `!(a <= b)` mirrors the C source; it differs from `a > b` when a value is NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::lb::trigf::atanf;
use crate::msl::sqrtf;
use crate::ppc::{fmadds, fmsubs, frsp};

/// `M_PI / 2` as a double (0x804D7A68).
const HALF_PI: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);
/// `M_PI` as a double (0x804D7A70).
const PI: f64 = f64::from_bits(0x4009_21fb_5444_2d18);
/// `0.00001f` and `-0.00001f` (0x804D7AA0, 0x804D7AA4).
const SMALL: f32 = f32::from_bits(0x3727_c5ac);
const NEG_SMALL: f32 = f32::from_bits(0xb727_c5ac);

/// `expf` (0x8000CE50): `e^x` from its Taylor series, summed term by term
/// in single precision until the sum repeats; `1/e^|x|` for negative `x`.
pub fn expf(arg: f32) -> f32 {
    let mut x = arg;
    let mut n: i32 = 2;
    let mut factorial: f32 = 1.0;
    let negative = if x < 0.0 {
        x = -x;
        true
    } else {
        false
    };
    let mut power = x;
    let mut sum = 1.0 + x;
    loop {
        let k = n;
        power *= x;
        let previous = sum;
        n += 1;
        factorial *= k as f32;
        sum += power / factorial;
        if sum == previous {
            break;
        }
    }
    if negative {
        sum = 1.0 / sum;
    }
    sum
}

/// `powf` (0x8000CEE0): `expf(y * 2 atanh((x - 1) / (x + 1)))`, i.e.
/// `x^y` via `ln x` from the `atanh` series; 0 for `x == 0`.
pub fn powf(x: f32, y: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    let mut k: i32 = 1;
    let mut term = (x - 1.0) / (1.0 + x);
    let mut sum = term;
    let ratio = term * term;
    loop {
        k += 2;
        term *= ratio;
        let previous = sum;
        sum += term / k as f32;
        if sum == previous {
            break;
        }
    }
    expf(y * (2.0 * sum))
}

/// `powi` (0x8000CF74): `base^exponent` by repeated multiplication (32-bit
/// wrapping); 0 for `base == 0` or a negative exponent.
pub fn powi(base: i32, exponent: i32) -> i32 {
    if base == 0 {
        return 0;
    }
    if exponent >= 0 {
        let mut result: i32 = 1;
        for _ in 0..exponent {
            result = result.wrapping_mul(base);
        }
        return result;
    }
    0
}

/// `lb_8000D008` (0x8000D008): the angle of `(x, y)` like `atan2f(y, x)`,
/// but with a ±1e-5 dead zone around `x == 0` (giving 0 or ±pi/2), and the
/// `x < 0` case computed as `±(pi - atanf(|y / x|))` in double precision.
/// Returns `y` unchanged when `x` is NaN.
pub fn lb_8000d008(y: f32, x: f32) -> f32 {
    let near_zero = |v: f32| v < SMALL && v > NEG_SMALL;
    if near_zero(x) {
        if near_zero(y) {
            return 0.0;
        }
        let sign: i32 = if y < 0.0 { -1 } else { 1 };
        return frsp(HALF_PI * f64::from(sign));
    }
    if x > 0.0 {
        return atanf(y / x);
    }
    if x < 0.0 {
        let mut ratio = y / x;
        if ratio < 0.0 {
            ratio = -ratio;
        }
        let sign: i32 = if y < 0.0 { -1 } else { 1 };
        return frsp(f64::from(sign) * (PI - f64::from(atanf(ratio))));
    }
    y
}

/// `lb_8000D148` (0x8000D148): a proximity test of `point2` against the
/// segment `point0`-`point1` within `threshold`. Returns false for a
/// degenerate segment (squared length < 1e-5).
///
/// As compiled (and as the decomp's C reads), the "distance to the line" is
/// `|p0x*p1y - p0y*p1x + (p1x - p0x)*p2x + (p0y - p1y)*p2y| / |p1 - p0|`,
/// which is the perpendicular distance only when `point2` is the origin.
/// The game's single caller (`Fighter_procInput`, `fighter.c`) passes the
/// previous and current control-stick positions and `point2 = (0, 0)`, to
/// detect the stick sweeping past the center within
/// `horizontal_stick_smash_deadzone`; for other `point2` the formula is not
/// a distance. The port keeps the machine code's arithmetic.
///
/// Audit: the segment's squared length, its cross term, and the numerator
/// are fused (`fmadds`/`fmsubs`); the endpoint squared distances are not.
#[allow(clippy::too_many_arguments)]
pub fn lb_8000d148(
    point0_x: f32,
    point0_y: f32,
    point1_x: f32,
    point1_y: f32,
    point2_x: f32,
    point2_y: f32,
    threshold: f32,
) -> bool {
    let diff_01_y = point0_y - point1_y;
    let cross = point0_y * point1_x;
    let diff_01_x = point1_x - point0_x;
    let cross = fmsubs(point0_x, point1_y, cross);
    let dist_01 = fmadds(diff_01_x, diff_01_x, diff_01_y * diff_01_y);
    if dist_01 < SMALL {
        return false;
    }
    let dist_01 = sqrtf(dist_01);
    let mut numerator = cross + fmadds(diff_01_x, point2_x, diff_01_y * point2_y);
    if numerator < 0.0 {
        numerator = -numerator;
    }
    if !(numerator / dist_01 <= threshold) {
        return false;
    }
    let (dx0, dy0) = (point0_x - point2_x, point0_y - point2_y);
    let (dx1, dy1) = (point1_x - point2_x, point1_y - point2_y);
    let dist_squared_02 = dx0 * dx0 + dy0 * dy0;
    let threshold_squared = threshold * threshold;
    let dist_squared_12 = dx1 * dx1 + dy1 * dy1;
    if dist_squared_02 < threshold_squared {
        if dist_squared_12 > threshold_squared {
            return true;
        }
        if dist_squared_12 < threshold_squared {
            return false;
        }
        return true;
    }
    if dist_squared_02 > threshold_squared {
        if dist_squared_12 > threshold_squared {
            return (point0_x > point2_x && point1_x < point2_x)
                || (point0_x < point2_x && point1_x > point2_x)
                || (point0_y > point2_y && point1_y < point2_y)
                || (point0_y < point2_y && point1_y > point2_y);
        }
        return true;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_and_pow_are_close() {
        for x in [-3.0f32, -0.5, 0.0, 0.25, 1.0, 2.0, 5.0] {
            assert!(
                (expf(x) - x.exp()).abs() <= 1e-5 * x.exp().max(1.0),
                "exp {x}"
            );
        }
        assert!((powf(2.0, 3.0) - 8.0).abs() < 1e-4);
        assert!((powf(0.5, 0.5) - 0.5f32.sqrt()).abs() < 1e-5);
        assert_eq!(powf(0.0, 2.0), 0.0);
        assert_eq!(powi(3, 4), 81);
        assert_eq!(powi(3, -1), 0);
    }

    #[test]
    fn angle_dead_zone() {
        assert_eq!(lb_8000d008(0.0, 0.0), 0.0);
        assert_eq!(lb_8000d008(1.0, 0.0), std::f32::consts::FRAC_PI_2);
        assert_eq!(lb_8000d008(-1.0, 0.0), -std::f32::consts::FRAC_PI_2);
        assert!((lb_8000d008(1.0, -1.0) - 0.75 * std::f32::consts::PI).abs() < 1e-6);
        assert!((lb_8000d008(1.0, 1.0) - std::f32::consts::FRAC_PI_4).abs() < 1e-6);
    }

    #[test]
    fn segment_proximity() {
        // "Line distance" |10 * 0.5 + 0 * 5| / 10 = 0.5; endpoints straddle.
        assert!(lb_8000d148(0.0, 0.0, 10.0, 0.0, 0.5, 5.0, 1.0));
        // |10 * 5| / 10 = 5 > 1, although (5, 0.5) is 0.5 from the segment.
        assert!(!lb_8000d148(0.0, 0.0, 10.0, 0.0, 5.0, 0.5, 1.0));
        assert!(!lb_8000d148(0.0, 0.0, 10.0, 0.0, 20.0, 0.0, 1.0));
        assert!(!lb_8000d148(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0));
    }
}
