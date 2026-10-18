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
