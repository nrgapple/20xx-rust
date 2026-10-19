//! Port of `src/melee/lb/lbtrigf.c`: the game's `atan2f`, `atanf`, `acosf`,
//! `asinf`, and their private `lb_sqrtf`.
//!
//! Callers in the 1.02 DOL: `atan2f` 245 call sites (the most-used math
//! routine in gameplay code), `atanf` 12 (plus `atan2f`'s), `acosf` 8,
//! `asinf` 8.
//!
//! Unlike MSL's `sqrtf`, `acosf` and `lb_sqrtf` refine the `frsqrte`
//! estimate in single precision (`frsp`, then `fmuls`/`fnmsubs`), so their
//! results can depend on the exact hardware estimate; see
//! `research/notes/math.md`.
//!
//! Constants are the 1.02 DOL's bytes (checked by `tests/math.rs`):
//! `atanf_lookup` at 0x803B7300, `.sdata2` 0x804D7DB0..0x804D7DD4, and
//! `MSL_TrigF_80400770`/`80400774` (a NaN with all mantissa bits set, and
//! +infinity).

use crate::ppc::{fmadds, fnmsubs, frsp, frsqrte};

const NEG_ZERO: f32 = -0.0; // 0x804D7DB0
const NEG_HALF_PI: f32 = f32::from_bits(0xbfc9_0fdb); // 0x804D7DB4
const PI: f32 = f32::from_bits(0x4049_0fdb); // 0x804D7DB8
const ZERO: f32 = 0.0; // 0x804D7DBC
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb); // 0x804D7DC0
const ONE: f32 = 1.0; // 0x804D7DC4
const HALF: f32 = 0.5; // 0x804D7DC8
const THREE: f32 = 3.0; // 0x804D7DCC
const SILVER_RATIO: f32 = f32::from_bits(0x401a_827a); // 2.4142137, 0x804D7DD0
const SILVER_RATIO_CONJUGATE: f32 = f32::from_bits(0x3ed4_13cd); // 0.41421357, 0x804D7DD4
/// `MSL_TrigF_80400770[0]`: the NaN `lb_sqrtf`/`acosf` return (0x7FFFFFFF).
pub const NAN: f32 = f32::from_bits(0x7fff_ffff);
/// `MSL_TrigF_80400774[0]`: +infinity.
const INF: f32 = f32::INFINITY;

const SIGN_BIT: u32 = 0x8000_0000;

/// `atanf_lookup` (0x803B7300, 46 floats). Indices used by `atanf` with
/// `lookup_index` in -1..=4: `[1..=6]` polynomial, `[7 + i]`, `[13 + i]`,
/// `[20 + i]`, `[27 + i]`, `[33 + i]`, `[39 + i]`.
pub const ATANF_LOOKUP: [f32; 46] = {
    const BITS: [u32; 46] = [
        0x3f80_0000,
        0xbeaa_aaaa,
        0x3e4c_cc81,
        0xbe12_3e7d,
        0x3de2_1f95,
        0xbdad_417c,
        0x3d41_186d,
        0x40da_826b,
        0x404f_5958,
        0x4000_0000,
        0x3fb9_25ab,
        0x3f95_f61a,
        0x3f85_1081,
        0x36ef_692f,
        0x355c_1df9,
        0x0000_0000,
        0x3529_1d45,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x3ec9_0eaa,
        0x3f16_cbe4,
        0x3f49_0fda,
        0x3f7b_53c5,
        0x3f96_cbe2,
        0x3faf_edd9,
        0x0000_0000,
        0x3718_5d99,
        0x32c5_9189,
        0x3387_4a9e,
        0x353c_fa83,
        0x3486_37bd,
        0x3554_1063,
        0x401a_8277,
        0x3fbf_90c7,
        0x3f80_0000,
        0x3f2b_0dc1,
        0x3ed4_13cd,
        0x3e4b_afaf,
        0x3516_dc59,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
    ];
    let mut table = [0.0; 46];
    let mut i = 0;
    while i < 46 {
        table[i] = f32::from_bits(BITS[i]);
        i += 1;
    }
    table
};

/// `atanf_lookup[offset + lookup_index]`, where `lookup_index` may be -1.
#[inline]
fn lookup(offset: usize, index: i32) -> f32 {
    ATANF_LOOKUP[(offset as i32 + index) as usize]
}

/// `atanf` (0x80022E68).
///
/// Works on `|x|` (sign bit cleared), reduces it with `1/x` above the
/// silver ratio or with one of five table intervals above its conjugate,
/// evaluates an odd polynomial, adds two correction terms, and restores the
/// sign.
///
/// Audit: 8 fused ops (2 `fnmsubs` in the interval reduction, 6 `fmadds` in
/// the polynomial, the last being `(r * r^2) * poly + r`). The two
/// correction terms are added separately, each rounded (stored to the
/// stack). Interval thresholds compare raw bit patterns as signed integers.
/// Checked against the DOL in `tests/math.rs`.
pub fn atanf(x: f32) -> f32 {
    let mut lookup_index: i32 = -1;
    let mut x_ge_ratio = false;
    let sign_bit_x = x.to_bits() & SIGN_BIT;
    let x = f32::from_bits(x.to_bits() & !SIGN_BIT);
    let bits = x.to_bits() as i32;

    let mut result;
    if x >= SILVER_RATIO {
        x_ge_ratio = true;
        result = ONE / x;
    } else if SILVER_RATIO_CONJUGATE < x {
        lookup_index = 0;
        match x.to_bits() & 0x7f80_0000 {
            0x3f00_0000 => {
                if !(bits < 0x3f08_d5b9) {
                    lookup_index = 1;
                }
                if !(bits < 0x3f52_1801) {
                    lookup_index += 1;
                }
            }
            0x3f80_0000 => {
                lookup_index = 2;
                if !(bits < 0x3f9b_f7ec) {
                    lookup_index = 3;
                }
                if !(bits < 0x3fef_789e) {
                    lookup_index += 1;
                }
            }
            0x4000_0000 => lookup_index = 4,
            _ => {}
        }
        let offset_39 = lookup(39, lookup_index);
        let offset_33 = lookup(33, lookup_index);
        result = ONE / (offset_33 + (x + offset_39));
        result = fnmsubs(result, lookup(7, lookup_index), offset_33)
            + fnmsubs(result, lookup(13, lookup_index), offset_39);
    } else {
        result = x;
    }

    let result_squared = result * result;
    let t = &ATANF_LOOKUP;
    let poly = fmadds(result_squared, t[6], t[5]);
    let cube = result * result_squared;
    let poly = fmadds(result_squared, poly, t[4]);
    let poly = fmadds(result_squared, poly, t[3]);
    let poly = fmadds(result_squared, poly, t[2]);
    let poly = fmadds(result_squared, poly, t[1]);
    result = fmadds(cube, poly, result);
    result += lookup(27, lookup_index);
    result += lookup(20, lookup_index);

    if x_ge_ratio {
        result -= HALF_PI;
        return if sign_bit_x != 0 { result } else { -result };
    }
    f32::from_bits(result.to_bits() | sign_bit_x)
}

/// `atan2f` (0x80022C30).
///
/// Dispatches on the operands' sign bits (so `-0.0` counts as negative).
/// With `x == 0` and differing signs it returns pi/2 with `y`'s sign bit,
/// built by integer addition of the bit patterns.
///
/// Audit: no fused ops; every branch and constant matches the disassembly
/// (the `-0.0f == x` test uses `fcmpu`, so it is also true for `+0.0`).
/// Checked against the DOL in `tests/math.rs`.
pub fn atan2f(y: f32, x: f32) -> f32 {
    let sign_x = x.to_bits() & SIGN_BIT;
    let sign_y = y.to_bits() & SIGN_BIT;
    if sign_x == sign_y {
        if sign_x != 0 {
            return if NEG_ZERO == x {
                NEG_HALF_PI
            } else {
                atanf(y / x) - PI
            };
        }
        return if x != ZERO { atanf(y / x) } else { HALF_PI };
    }
    if x < ZERO {
        return PI + atanf(y / x);
    }
    if x != ZERO {
        return atanf(y / x);
    }
    f32::from_bits(sign_y.wrapping_add(HALF_PI.to_bits()))
}

/// Three single-precision Newton steps for `1/sqrt(x)` from the `frsqrte`
/// estimate rounded to single, as inlined in `acosf` and `lb_sqrtf`.
#[inline]
fn rsqrt_single(x: f32) -> f32 {
    let mut guess = frsp(frsqrte(f64::from(x)));
    for _ in 0..3 {
        let squared = guess * guess;
        let half = HALF * guess;
        guess = half * fnmsubs(x, squared, THREE);
    }
    guess
}

/// `acosf` (0x80022D1C): `pi/2 - atanf(x / sqrt(1 - x^2))`, computing
/// `1/sqrt(1 - x^2)` inline in single precision.
///
/// Audit: `1 - x * x` is a fused `fnmsubs`; the three Newton steps are
/// `fmuls`/`fmuls`/`fnmsubs`/`fmuls` after `frsp` of the estimate. For
/// `1 - x^2 <= 0` it uses NaN (if nonzero) or +infinity. Checked against
/// the DOL in `tests/math.rs` (with the provisional `frsqrte`).
pub fn acosf(x: f32) -> f32 {
    let mut result = fnmsubs(x, x, ONE);
    if result > ZERO {
        result = rsqrt_single(result);
    } else if result != ZERO {
        result = NAN;
    } else {
        result = INF;
    }
    HALF_PI - atanf(x * result)
}

/// `asinf` (0x80022DBC): `atanf(x * lb_sqrtf(-(x * x - 1)))`.
///
/// Audit: `-(x * x - 1)` is a fused `fnmsubs`. Checked against the DOL.
pub fn asinf(x: f32) -> f32 {
    atanf(x * lb_sqrtf(fnmsubs(x, x, ONE)))
}

/// `lb_sqrtf` (0x80022DF8, static): despite its name, returns the
/// reciprocal square root `1/sqrt(x)` for `x > 0`, NaN for other nonzero
/// `x` (including NaN), and +infinity for zero.
pub fn lb_sqrtf(x: f32) -> f32 {
    if x > ZERO {
        return rsqrt_single(x);
    }
    if x != ZERO {
        return NAN;
    }
    INF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_to_libm() {
        let mut x = -20.0f32;
        while x < 20.0 {
            assert!((atanf(x) - x.atan()).abs() < 1e-6, "atan {x}");
            x += 0.003;
        }
        let mut x = -1.0f32;
        while x <= 1.0 {
            assert!((acosf(x) - x.acos()).abs() < 1e-5, "acos {x}");
            assert!((asinf(x) - x.asin()).abs() < 1e-5, "asin {x}");
            x += 0.001;
        }
        for (y, x) in [
            (1.0f32, 1.0f32),
            (1.0, -1.0),
            (-1.0, -1.0),
            (-1.0, 1.0),
            (0.3, -2.0),
        ] {
            assert!((atan2f(y, x) - y.atan2(x)).abs() < 1e-6, "atan2 {y} {x}");
        }
    }

    #[test]
    fn atan2f_zero_cases_follow_sign_bits() {
        assert_eq!(atan2f(1.0, 0.0), HALF_PI);
        assert_eq!(atan2f(-1.0, -0.0), NEG_HALF_PI);
        assert_eq!(atan2f(-1.0, 0.0), -HALF_PI);
        assert_eq!(atan2f(1.0, -0.0), HALF_PI);
        assert_eq!(atan2f(0.0, 0.0), HALF_PI);
    }

    #[test]
    fn lb_sqrtf_is_a_reciprocal_square_root() {
        assert_eq!(lb_sqrtf(4.0), 0.5);
        assert_eq!(lb_sqrtf(0.0), f32::INFINITY);
        assert_eq!(lb_sqrtf(-1.0).to_bits(), 0x7fff_ffff);
    }
}
