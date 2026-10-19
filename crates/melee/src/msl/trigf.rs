//! Port of `src/MSL/trigf.c` (`sinf`, `cosf`) and `tanf` from
//! `src/MSL/math_1.c`, with their tables from `src/MSL/math_data.c`.
//!
//! Callers in the 1.02 DOL: `sinf` 170 call sites, `cosf` 170, `tanf` 31
//! (mostly camera code). `sin__Ff`/`cos__Ff` (0x80326220/0x80326200) only
//! forward to these and are called once each, by `tanf`.
//!
//! The argument reduction works in units of pi/4: `y = (4/pi) x - 2n`, where
//! `4/pi - 1` is split across four `f32`s (`__four_over_pi_m1`, filled at
//! startup by `__sinit_trigf_c` from `tmp_float`) so that each term is added
//! with its own fused multiply-add.
//!
//! Table values are the 1.02 DOL's bytes (checked by `tests/math.rs`):
//! `__sincos_on_quadrant` at 0x803B9358, `__sincos_poly` at 0x803B9378,
//! `tmp_float` at 0x803B8F40, and constants in `.sdata2` at
//! 0x804DE198..0x804DE1A0.

use crate::ppc::{fctiwz, fmadds, fnmadds, fnmsubs};

/// `2.0f / (f32) M_PI` (0x804DE198).
const TWO_OVER_PI: f32 = f32::from_bits(0x3f22_f983);
/// `0.5f` (0x804DE19C).
const HALF: f32 = 0.5;
/// `__epsilon` = 3.45266983e-4f (0x804DE1A0).
const EPSILON: f32 = f32::from_bits(0x39b5_04f3);

/// `__four_over_pi_m1` after `__sinit_trigf_c` copies `tmp_float` into it:
/// 0.25, 0.0232393741608, 1.70555722434e-7, 1.86736494323e-11.
pub const FOUR_OVER_PI_M1: [f32; 4] = [
    f32::from_bits(0x3e80_0000),
    f32::from_bits(0x3cbe_6080),
    f32::from_bits(0x3437_2200),
    f32::from_bits(0x2da4_4152),
];

/// `__sincos_on_quadrant`: `{ 0, 1, 1, 0, 0, -1, -1, 0 }`.
pub const SINCOS_ON_QUADRANT: [f32; 8] = [0.0, 1.0, 1.0, 0.0, 0.0, -1.0, -1.0, 0.0];

/// `__sincos_poly`: even entries are the cosine polynomial, odd entries the
/// sine polynomial (in powers of `y * pi/4`); `[9]` is pi/4.
pub const SINCOS_POLY: [f32; 10] = [
    f32::from_bits(0x366c_cfaa), // 3.5287617e-6
    f32::from_bits(0x34a5_e129), // 3.089747e-7
    f32::from_bits(0xb9aa_e275), // -3.259365e-4
    f32::from_bits(0xb819_6543), // -3.657235e-5
    f32::from_bits(0x3c81_e0ed), // 0.015854323
    f32::from_bits(0x3b23_35dd), // 0.0024903931
    f32::from_bits(0xbe9d_e9e6), // -0.30842513
    f32::from_bits(0xbda5_5de7), // -0.08074551
    f32::from_bits(0x3f80_0000), // 1.0
    f32::from_bits(0x3f49_0fdb), // 0.7853982
];

/// The shared argument reduction: returns `(n & 3, y)`.
///
/// `n = (int)(z -/+ 0.5f)` with `z = (2/pi) x` (`fctiwz` truncates; NaN and
/// overflow saturate). `n * 2` wraps in 32 bits and is converted to `float`
/// with the `xoris`/`lfd`/`fsubs` idiom, which rounds it to single.
#[inline]
fn reduce(x: f32) -> (usize, f32) {
    let z = TWO_OVER_PI * x;
    let n = if x.to_bits() & 0x8000_0000 != 0 {
        fctiwz(f64::from(z - HALF))
    } else {
        fctiwz(f64::from(HALF + z))
    };
    let mut y = x - n.wrapping_mul(2) as f32;
    for term in FOUR_OVER_PI_M1 {
        y = fmadds(term, x, y);
    }
    ((n & 3) as usize, y)
}

/// The cosine polynomial in `ysq`: `(((p0 ysq + p2) ysq + p4) ysq + p6) ysq + p8`.
#[inline]
fn cos_poly(ysq: f32) -> f32 {
    let p = &SINCOS_POLY;
    let z = fmadds(p[0], ysq, p[2]);
    let z = fmadds(ysq, z, p[4]);
    let z = fmadds(ysq, z, p[6]);
    fmadds(ysq, z, p[8])
}

/// The sine polynomial up to (not including) the final fused term with `p9`.
#[inline]
fn sin_poly_head(ysq: f32) -> f32 {
    let p = &SINCOS_POLY;
    let z = fmadds(p[1], ysq, p[3]);
    let z = fmadds(ysq, z, p[5]);
    fmadds(ysq, z, p[7])
}

/// `sinf` (0x803263D4).
///
/// Audit: 13 fused ops in the DOL (4 in the reduction, 1 in the small-`y`
/// path, 4 in each polynomial), all ported with [`fmadds`]; operand order
/// matches the disassembly. Checked against the DOL in `tests/math.rs`.
pub fn sinf(x: f32) -> f32 {
    let (n, y) = reduce(x);
    let q = &SINCOS_ON_QUADRANT;
    let i = n << 1;
    if y.abs() < EPSILON {
        return fmadds(SINCOS_POLY[9], y * q[i + 1], q[i]);
    }
    let ysq = y * y;
    if n & 1 != 0 {
        cos_poly(ysq) * q[i]
    } else {
        let z = fmadds(ysq, sin_poly_head(ysq), SINCOS_POLY[9]);
        (y * z) * q[i + 1]
    }
}

/// `cosf` (0x80326240).
///
/// Audit: 13 fused ops in the DOL, including `fnmsubs` in the small-`y`
/// path and `fnmadds` for the negated sine polynomial. Checked against the
/// DOL in `tests/math.rs`.
pub fn cosf(x: f32) -> f32 {
    let (n, y) = reduce(x);
    let q = &SINCOS_ON_QUADRANT;
    let i = n << 1;
    if y.abs() < EPSILON {
        return fnmsubs(y, q[i], q[i + 1]);
    }
    let ysq = y * y;
    if n & 1 != 0 {
        let z = fnmadds(ysq, sin_poly_head(ysq), SINCOS_POLY[9]);
        (y * z) * q[i]
    } else {
        cos_poly(ysq) * q[i + 1]
    }
}

/// `tanf` (0x803261BC): `sin__Ff(x) / cos__Ff(x)` (the machine code calls
/// `cos__Ff` first; both are pure).
pub fn tanf(x: f32) -> f32 {
    let cos = cosf(x);
    let sin = sinf(x);
    sin / cos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_to_libm_on_game_ranges() {
        let mut x = -10.0f32;
        while x < 10.0 {
            assert!((sinf(x) - x.sin()).abs() < 2e-6, "sin {x}");
            assert!((cosf(x) - x.cos()).abs() < 2e-6, "cos {x}");
            x += 0.01;
        }
        assert_eq!(sinf(0.0), 0.0);
        assert_eq!(cosf(0.0), 1.0);
        assert!((tanf(0.5) - 0.5f32.tan()).abs() < 1e-6);
    }

    #[test]
    fn four_over_pi_split_sums_to_four_over_pi() {
        let sum: f64 = FOUR_OVER_PI_M1.iter().map(|&t| f64::from(t)).sum();
        assert!((sum + 1.0 - 4.0 / std::f64::consts::PI).abs() < 1e-15);
    }
}
