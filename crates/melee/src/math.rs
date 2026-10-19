//! Vector and matrix types matching the SDK's `Vec`/`Vec2`/`Mtx`/`Quaternion`
//! (`dolphin/mtx.h`): plain `f32`s, row-major like the C.

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

/// The SDK's `Quaternion` (`x, y, z, w`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternion {
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

/// The SDK's `Mtx`: `f32[3][4]`, rows of `[x, y, z, translation]`.
pub type Mtx = [[f32; 4]; 3];

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

/// `sqrtf` from `src/MSL/math_ppc.h` (inlined throughout the game); see
/// [`crate::msl::sqrtf`], which audits it against the 1.02 machine code.
pub use crate::msl::sqrtf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_matches_c_macro() {
        assert_eq!(abs_c(-1.5), 1.5);
        assert_eq!(abs_c(2.0), 2.0);
        assert!(abs_c(-0.0).is_sign_negative());
    }
}
