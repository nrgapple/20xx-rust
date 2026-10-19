//! Port of `src/MSL/math_ppc.h`: the inline `sqrtf` and `sqrtf_accurate`.
//!
//! `sqrtf` is `extern inline`, so MWCC expands it at most call sites; 268 of
//! the DOL's 278 `frsqrte` instructions are these expansions (see
//! `research/notes/math.md`). An out-of-line copy, `sqrtf__Ff` (0x8000D5BC),
//! has 55 callers. Both compile to the same sequence:
//!
//! ```text
//! frsqrte f2, x             ; estimate, double
//! 3x: fmul    f0, f2, f2    ; g * g
//!     fmul    f2, 0.5, f2   ; 0.5 * g
//!     fnmsub  f0, x, f0, 3.0; 3.0 - x * (g * g), fused, double
//!     fmul    f2, f2, f0    ; g = (0.5 * g) * (...)
//! fmul    f0, x, f2         ; x * g
//! frsp    f0, f0            ; (float), then a volatile store/load
//! ```
//!
//! Everything before the final `frsp` is double precision, so the result
//! is essentially independent of the exact `frsqrte` estimate; the
//! measured sensitivity is in `research/notes/math.md`.

use crate::ppc::{fnmsub, frsp, frsqrte};

/// One Newton step of the inline `sqrtf`: `0.5 * g * (3.0 - g * g * x)`,
/// in the machine code's order (`g * g` first, then the fused
/// `3.0 - x * (g * g)`).
#[inline]
fn newton_step(guess: f64, x: f64) -> f64 {
    let squared = guess * guess;
    let half = 0.5 * guess;
    half * fnmsub(x, squared, 3.0)
}

/// `sqrtf` (`math_ppc.h`, inline; out-of-line copy `sqrtf__Ff` at
/// 0x8000D5BC): `frsqrte` plus three double-precision Newton steps, then
/// `(float)(x * guess)`. Returns `x` unchanged unless `x > 0` (so `-0.0`,
/// negative values, and NaN pass through).
///
/// Audit: matches `sqrtf__Ff` instruction for instruction (3 `fnmsub`, all
/// other ops double, one final `frsp`), and the expanded copies (e.g. in
/// `lbVector_Normalize`, 0x8000D2EC). Checked against the DOL in
/// `tests/math.rs`.
pub fn sqrtf(x: f32) -> f32 {
    if x > 0.0 {
        let x64 = f64::from(x);
        let mut guess = frsqrte(x64);
        for _ in 0..3 {
            guess = newton_step(guess, x64);
        }
        frsp(x64 * guess)
    } else {
        x
    }
}

/// `sqrtf_accurate` (`math_ppc.h`, static inline; out-of-line as
/// `lbVector_sqrtf_accurate` at 0x8000E19C): like [`sqrtf`] with a fourth
/// Newton step.
///
/// Audit: matches `lbVector_sqrtf_accurate` (4 `fnmsub`) and its expansions
/// in `lbVector_NormalizeXY`/`lbVector_AngleXY`. Checked against the DOL.
pub fn sqrtf_accurate(x: f32) -> f32 {
    if x > 0.0 {
        let x64 = f64::from(x);
        let mut guess = frsqrte(x64);
        for _ in 0..4 {
            guess = newton_step(guess, x64);
        }
        frsp(x64 * guess)
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqrtf_is_close_and_passes_nonpositive_through() {
        assert_eq!(sqrtf(4.0), 2.0);
        assert_eq!(sqrtf(2.0), std::f32::consts::SQRT_2);
        assert_eq!(sqrtf(-1.0), -1.0);
        assert_eq!(sqrtf(0.0), 0.0);
        assert!(sqrtf(-0.0).is_sign_negative());
        assert!(sqrtf(f32::NAN).is_nan());
        assert_eq!(sqrtf_accurate(9.0), 3.0);
    }

    #[test]
    fn sqrtf_is_correctly_rounded_on_a_sample() {
        // Not a fidelity check (the game's result is what it is), but the
        // double-precision refinement makes it agree with IEEE sqrt here.
        let mut x = 1.0e-30f32;
        while x < 1.0e30 {
            assert_eq!(sqrtf(x), x.sqrt(), "{x}");
            x *= 1.618;
        }
    }
}
