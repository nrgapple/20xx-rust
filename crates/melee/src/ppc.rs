//! Gekko floating-point primitives used by ported code.
//!
//! Fused multiply-adds round once. For `f32` operands the product fits in an
//! `f64`, so an `f64` fma followed by rounding to `f32` matches the
//! hardware's single-precision fused ops (fma in double, then round to
//! single). `f64::mul_add` is a correctly rounded fma on every target,
//! including wasm32, where it is computed in software.
//!
//! The estimate instructions (`frsqrte`, `fres`) are hardware lookup tables.
//! [`frsqrte`] is provisional until its bit-exact behavior is established.

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

/// `fnmadds d, a, c, b`: `-(a * c + b)`.
#[inline]
pub fn fnmadds(a: f32, c: f32, b: f32) -> f32 {
    -fmadds(a, c, b)
}

/// `fnmsubs d, a, c, b`: `-(a * c - b)`, i.e. `b - a * c` rounded once.
#[inline]
pub fn fnmsubs(a: f32, c: f32, b: f32) -> f32 {
    -fmsubs(a, c, b)
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
    -a.mul_add(c, b)
}

/// `fnmsub d, a, c, b` (double): `-(a * c - b)`, i.e. `b - a * c` rounded once.
#[inline]
pub fn fnmsub(a: f64, c: f64, b: f64) -> f64 {
    -a.mul_add(c, -b)
}

/// `frsp`: round a double to single precision (round to nearest even).
#[inline]
pub fn frsp(x: f64) -> f32 {
    x as f32
}

/// `frsqrte`: reciprocal square root estimate.
///
/// PROVISIONAL: the true result comes from a hardware table, which this
/// does not reproduce. It returns `1/sqrt(x)` truncated to 12 mantissa bits,
/// within the documented 1/4096 relative error. Callers that refine the
/// estimate with Newton steps (e.g. `sqrtf`) are usually insensitive to it;
/// see `research/notes/math.md` once written.
pub fn frsqrte(x: f64) -> f64 {
    let estimate = 1.0 / x.sqrt();
    if !estimate.is_finite() || estimate == 0.0 {
        return estimate;
    }
    f64::from_bits(estimate.to_bits() & !((1u64 << 40) - 1))
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
    fn frsqrte_is_close() {
        for x in [0.25, 1.0, 2.0, 1e-10, 3.7e20] {
            let exact = 1.0 / f64::sqrt(x);
            assert!((frsqrte(x) - exact).abs() / exact < 1.0 / 4096.0);
        }
    }
}
