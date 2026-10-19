//! Gekko floating-point primitives used by ported code.
//!
//! Fused multiply-adds round once. For `f32` operands the product fits in an
//! `f64`, so an `f64` fma followed by rounding to `f32` matches the
//! hardware's single-precision fused ops (fma in double, then round to
//! single). `f64::mul_add` is a correctly rounded fma on every target,
//! including wasm32, where it is computed in software. (Whether Gekko's
//! single-precision fused ops also round twice in the rare cases where the
//! exact `a * c + b` needs more than 53 bits is unverified; see
//! `research/notes/math.md`.)
//!
//! Plain single-precision ops (`fadds`, `fmuls`, ...) on `f32` values are
//! exactly Rust's `f32` operators. The negated fused forms leave a NaN
//! result's sign alone, as the PowerPC architecture specifies.
//!
//! The estimate instructions (`frsqrte`, `fres`) are hardware lookup tables.
//! [`frsqrte`] and [`fres`] are provisional until their bit-exact behavior is
//! established.
//!
//! [`Ps`] models a paired-single register for the SDK's `PSVEC`/`PSMTX`
//! routines: each lane rounds to single on every operation.

/// `fmadds d, a, c, b`: `a * c + b`, rounded once to single.
#[inline]
pub fn fmadds(a: f32, c: f32, b: f32) -> f32 {
    f64::from(a).mul_add(f64::from(c), f64::from(b)) as f32
}

/// `fmsubs d, a, c, b`: `a * c - b`, rounded once to single.
#[inline]
pub fn fmsubs(a: f32, c: f32, b: f32) -> f32 {
    f64::from(a).mul_add(f64::from(c), -f64::from(b)) as f32
}

/// Negate a fused result as `fnmadd(s)`/`fnmsub(s)` do: NaNs keep their sign.
#[inline]
fn negate_fused<T: Copy + std::ops::Neg<Output = T> + PartialEq>(x: T) -> T {
    #[allow(clippy::eq_op)]
    if x != x {
        x
    } else {
        -x
    }
}

/// `fnmadds d, a, c, b`: `-(a * c + b)`.
#[inline]
pub fn fnmadds(a: f32, c: f32, b: f32) -> f32 {
    negate_fused(fmadds(a, c, b))
}

/// `fnmsubs d, a, c, b`: `-(a * c - b)`, i.e. `b - a * c` rounded once.
#[inline]
pub fn fnmsubs(a: f32, c: f32, b: f32) -> f32 {
    negate_fused(fmsubs(a, c, b))
}

/// `fmadd d, a, c, b` (double): `a * c + b`, rounded once.
#[inline]
pub fn fmadd(a: f64, c: f64, b: f64) -> f64 {
    a.mul_add(c, b)
}

/// `fmsub d, a, c, b` (double): `a * c - b`, rounded once.
#[inline]
pub fn fmsub(a: f64, c: f64, b: f64) -> f64 {
    a.mul_add(c, -b)
}

/// `fnmadd d, a, c, b` (double): `-(a * c + b)`.
#[inline]
pub fn fnmadd(a: f64, c: f64, b: f64) -> f64 {
    negate_fused(a.mul_add(c, b))
}

/// `fnmsub d, a, c, b` (double): `-(a * c - b)`, i.e. `b - a * c` rounded once.
#[inline]
pub fn fnmsub(a: f64, c: f64, b: f64) -> f64 {
    negate_fused(a.mul_add(c, -b))
}

/// `frsp`: round a double to single precision (round to nearest even).
#[inline]
pub fn frsp(x: f64) -> f32 {
    x as f32
}

/// `fctiwz`: convert to `i32`, truncating toward zero. Out-of-range values
/// saturate and NaN gives `i32::MIN` (`0x8000_0000`), as the architecture
/// specifies; Rust's `as i32` would give 0 for NaN.
#[inline]
pub fn fctiwz(x: f64) -> i32 {
    if x.is_nan() {
        i32::MIN
    } else {
        x as i32
    }
}

/// `frsqrte`: reciprocal square root estimate.
///
/// PROVISIONAL: the true result comes from a hardware table, which this
/// does not reproduce. It returns `1/sqrt(x)` truncated to 12 mantissa bits,
/// within the documented 1/4096 relative error. Special cases follow the
/// architecture: `+0 -> +inf`, `-0 -> -inf`, negative or NaN `-> NaN`,
/// `+inf -> +0`. See `research/notes/math.md` for which callers' results
/// depend on the exact estimate (the game's `sqrtf` does not in practice;
/// `PSVECNormalize`, `PSVECMag`, and the single-precision `acosf`/`asinf`
/// paths do).
pub fn frsqrte(x: f64) -> f64 {
    let estimate = 1.0 / x.sqrt();
    if !estimate.is_finite() || estimate == 0.0 {
        return estimate;
    }
    f64::from_bits(estimate.to_bits() & !((1u64 << 40) - 1))
}

/// `fres`: reciprocal estimate, single precision.
///
/// PROVISIONAL, like [`frsqrte`]: `1/x` truncated to 12 mantissa bits
/// (within the 750CL's documented 1/4096 relative error). Used only by
/// `PSMTXInverse` and `PSMTXQuat`, which refine it with one Newton step, so
/// their results depend on the exact hardware estimate.
pub fn fres(x: f64) -> f32 {
    let estimate = (1.0 / x) as f32;
    if !estimate.is_finite() || estimate == 0.0 {
        return estimate;
    }
    f32::from_bits(estimate.to_bits() & !((1u32 << 11) - 1))
}

/// A paired-single register: lanes `ps0` and `ps1`.
///
/// Every arithmetic operation rounds each lane to single. `ps_madd` and
/// friends are fused per lane (modeled like [`fmadds`]). The `s0`/`s1`
/// forms use one lane of `c` for both lanes. Moves, merges, and sign
/// operations copy values unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ps(pub f32, pub f32);

impl Ps {
    /// `psq_l ..., 1, qr0` (single load): `ps0 = x`, `ps1 = 1.0`.
    #[inline]
    pub fn single(x: f32) -> Self {
        Self(x, 1.0)
    }

    /// Both lanes `x`, as `lfs` leaves a register.
    #[inline]
    pub fn splat(x: f32) -> Self {
        Self(x, x)
    }
}

/// `ps_add d, a, b`.
#[inline]
pub fn ps_add(a: Ps, b: Ps) -> Ps {
    Ps(a.0 + b.0, a.1 + b.1)
}

/// `ps_sub d, a, b`.
#[inline]
pub fn ps_sub(a: Ps, b: Ps) -> Ps {
    Ps(a.0 - b.0, a.1 - b.1)
}

/// `ps_mul d, a, c`.
#[inline]
pub fn ps_mul(a: Ps, c: Ps) -> Ps {
    Ps(a.0 * c.0, a.1 * c.1)
}

/// `ps_div d, a, b`.
#[inline]
pub fn ps_div(a: Ps, b: Ps) -> Ps {
    Ps(a.0 / b.0, a.1 / b.1)
}

/// `ps_muls0 d, a, c`: both lanes times `c.ps0`.
#[inline]
pub fn ps_muls0(a: Ps, c: Ps) -> Ps {
    Ps(a.0 * c.0, a.1 * c.0)
}

/// `ps_muls1 d, a, c`: both lanes times `c.ps1`.
#[inline]
pub fn ps_muls1(a: Ps, c: Ps) -> Ps {
    Ps(a.0 * c.1, a.1 * c.1)
}

/// `ps_madd d, a, c, b`: `a * c + b` per lane, fused.
#[inline]
pub fn ps_madd(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(fmadds(a.0, c.0, b.0), fmadds(a.1, c.1, b.1))
}

/// `ps_msub d, a, c, b`: `a * c - b` per lane, fused.
#[inline]
pub fn ps_msub(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(fmsubs(a.0, c.0, b.0), fmsubs(a.1, c.1, b.1))
}

/// `ps_nmadd d, a, c, b`: `-(a * c + b)` per lane, fused.
#[inline]
pub fn ps_nmadd(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(fnmadds(a.0, c.0, b.0), fnmadds(a.1, c.1, b.1))
}

/// `ps_nmsub d, a, c, b`: `-(a * c - b)` per lane, fused.
#[inline]
pub fn ps_nmsub(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(fnmsubs(a.0, c.0, b.0), fnmsubs(a.1, c.1, b.1))
}

/// `ps_madds0 d, a, c, b`: `a * c.ps0 + b` per lane, fused.
#[inline]
pub fn ps_madds0(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(fmadds(a.0, c.0, b.0), fmadds(a.1, c.0, b.1))
}

/// `ps_madds1 d, a, c, b`: `a * c.ps1 + b` per lane, fused.
#[inline]
pub fn ps_madds1(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(fmadds(a.0, c.1, b.0), fmadds(a.1, c.1, b.1))
}

/// `ps_sum0 d, a, c, b`: `(a.ps0 + b.ps1, c.ps1)`.
#[inline]
pub fn ps_sum0(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(a.0 + b.1, c.1)
}

/// `ps_sum1 d, a, c, b`: `(c.ps0, a.ps0 + b.ps1)`.
#[inline]
pub fn ps_sum1(a: Ps, c: Ps, b: Ps) -> Ps {
    Ps(c.0, a.0 + b.1)
}

/// `ps_merge00 d, a, b`: `(a.ps0, b.ps0)`.
#[inline]
pub fn ps_merge00(a: Ps, b: Ps) -> Ps {
    Ps(a.0, b.0)
}

/// `ps_merge01 d, a, b`: `(a.ps0, b.ps1)`.
#[inline]
pub fn ps_merge01(a: Ps, b: Ps) -> Ps {
    Ps(a.0, b.1)
}

/// `ps_merge10 d, a, b`: `(a.ps1, b.ps0)`.
#[inline]
pub fn ps_merge10(a: Ps, b: Ps) -> Ps {
    Ps(a.1, b.0)
}

/// `ps_merge11 d, a, b`: `(a.ps1, b.ps1)`.
#[inline]
pub fn ps_merge11(a: Ps, b: Ps) -> Ps {
    Ps(a.1, b.1)
}

/// `ps_neg d, b`: flip both sign bits.
#[inline]
pub fn ps_neg(b: Ps) -> Ps {
    Ps(-b.0, -b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fused_ops_round_once() {
        // a * c is not representable in f32; fused and unfused results differ.
        let (a, c, b) = (1.0 + f32::EPSILON, 1.0 - f32::EPSILON, -1.0f32);
        assert_eq!(fmadds(a, c, b), -(f32::EPSILON * f32::EPSILON));
        assert_eq!(a * c + b, 0.0);
        assert_eq!(fnmsubs(a, c, 1.0), f32::EPSILON * f32::EPSILON);
    }

    #[test]
    fn negated_fused_ops_keep_nan_sign() {
        let nan = f32::from_bits(0x7fc0_0001);
        assert_eq!(fnmadds(nan, 1.0, 0.0).to_bits(), 0x7fc0_0001);
        assert_eq!(fnmsub(f64::NAN, 1.0, 0.0).to_bits(), f64::NAN.to_bits());
        assert!(fnmadds(1.0, 1.0, -1.0).is_sign_negative());
    }

    #[test]
    fn fctiwz_saturates() {
        assert_eq!(fctiwz(-2.75), -2);
        assert_eq!(fctiwz(3e9), i32::MAX);
        assert_eq!(fctiwz(-3e9), i32::MIN);
        assert_eq!(fctiwz(f64::NAN), i32::MIN);
    }

    #[test]
    fn estimates_are_close_and_handle_special_values() {
        for x in [0.25, 1.0, 2.0, 1e-10, 3.7e20] {
            let exact = 1.0 / f64::sqrt(x);
            assert!((frsqrte(x) - exact).abs() / exact < 1.0 / 4096.0);
            let exact = 1.0 / x;
            assert!((f64::from(fres(x)) - exact).abs() / exact < 1.0 / 4096.0);
        }
        assert_eq!(frsqrte(0.0), f64::INFINITY);
        assert_eq!(frsqrte(-0.0), f64::NEG_INFINITY);
        assert_eq!(frsqrte(f64::INFINITY), 0.0);
        assert!(frsqrte(-1.0).is_nan());
    }

    #[test]
    fn paired_singles_round_per_lane() {
        let a = Ps(1.0 + f32::EPSILON, 3.0);
        let c = Ps(1.0 - f32::EPSILON, 0.5);
        let b = Ps(-1.0, 1.0);
        assert_eq!(ps_madd(a, c, b), Ps(-(f32::EPSILON * f32::EPSILON), 2.5));
        assert_eq!(ps_muls1(a, c), Ps((1.0 + f32::EPSILON) * 0.5, 1.5));
        // (1 + eps) + 1 is a tie in f32 and rounds to even.
        assert_eq!(ps_sum0(a, c, b), Ps(2.0, 0.5));
        assert_eq!(ps_merge10(a, b), Ps(3.0, -1.0));
    }
}
