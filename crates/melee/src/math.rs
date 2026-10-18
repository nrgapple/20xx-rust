//! Vector types matching the SDK's `Vec`/`Vec2` (`dolphin/mtx.h`): plain `f32`s.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// The decomp's `ABS(x)`: `((x) < 0 ? -(x) : (x))`. MWCC emits a compare and
/// `fneg`, not `fabs`, so `-0.0` stays `-0.0`; ported code uses this, not
/// [`f32::abs`], wherever the C source uses `ABS`.
#[inline]
pub fn abs_c(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// `sqrtf` from `src/MSL/math_ppc.h` (inlined throughout the game): an
/// `frsqrte` estimate refined by three Newton steps in double precision.
///
/// PROVISIONAL: the operation order and fusing must be confirmed against the
/// 1.02 machine code, and [`crate::ppc::frsqrte`] is not yet bit-exact.
pub fn sqrtf(x: f32) -> f32 {
    if x > 0.0 {
        let x64 = f64::from(x);
        let mut guess = crate::ppc::frsqrte(x64);
        for _ in 0..3 {
            guess = 0.5 * guess * (3.0 - guess * guess * x64);
        }
        (x64 * guess) as f32
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_matches_c_macro() {
        assert_eq!(abs_c(-1.5), 1.5);
        assert_eq!(abs_c(2.0), 2.0);
        assert!(abs_c(-0.0).is_sign_negative());
    }

    #[test]
    fn sqrtf_is_close_and_passes_nonpositive_through() {
        assert_eq!(sqrtf(4.0), 2.0);
        assert!((sqrtf(2.0) - std::f32::consts::SQRT_2).abs() <= f32::EPSILON);
        assert_eq!(sqrtf(-1.0), -1.0);
        assert_eq!(sqrtf(0.0), 0.0);
    }
}
