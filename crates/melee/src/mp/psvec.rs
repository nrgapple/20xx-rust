//! `PSVECNormalize` from the SDK (`libs/dolphin/src/dolphin/mtx/vec.c`),
//! which every `mplib.c` surface-normal query calls. It lives here until
//! the port has a shared SDK vector module; move it there then.

use crate::math::Vec3;
use crate::ppc;

/// A single-precision multiply (`fmuls`) whose operands come from
/// `frsqrte`, which returns a double. For an estimate with at most 26
/// significant bits the `f64` product is exact, so rounding it once to
/// `f32` is the `fmuls` result.
///
/// PROVISIONAL with [`ppc::frsqrte`]: once the bit-exact estimate is known,
/// confirm how Gekko's single multiply treats a `frC` operand that is not
/// a single-precision value.
fn fmuls_estimate(a: f64, c: f64) -> f32 {
    (a * c) as f32
}

/// `PSVECNormalize` (0x80342DB8): scale `v` to unit length with one Newton
/// step from `frsqrte`, all in paired-single arithmetic.
///
/// Audited against the 1.02 machine code, op by op:
/// `ps_mul` (x², y²), `ps_madd` (z·z + x², fused), `ps_sum0` (+ y²),
/// `frsqrte`, `fmuls` r·r, `fmuls` r·0.5, `fnmsubs` 3 − r²·sum (fused),
/// `fmuls`, then `ps_muls0` scales each lane. The SDK was built with
/// `-fp_contract off`, but these are hand-written paired-single
/// instructions, so the two fused ops are explicit.
///
/// PROVISIONAL: the result depends on the exact `frsqrte` estimate (only one
/// Newton step follows), which [`ppc::frsqrte`] does not reproduce yet, so
/// low bits may differ from hardware. A zero vector gives NaNs, as on
/// hardware (`frsqrte(0)` is +inf).
pub fn ps_vec_normalize(v: Vec3) -> Vec3 {
    let xx = v.x * v.x;
    let yy = v.y * v.y;
    let xx_zz = ppc::fmadds(v.z, v.z, xx);
    let sqsum = xx_zz + yy;
    let rsqrt = ppc::frsqrte(f64::from(sqsum));
    let nwork0 = fmuls_estimate(rsqrt, rsqrt);
    let nwork1 = fmuls_estimate(rsqrt, 0.5);
    let nwork0 = ppc::fnmsubs(nwork0, sqsum, 3.0);
    let rsqrt = nwork0 * nwork1;
    Vec3::new(v.x * rsqrt, v.y * rsqrt, v.z * rsqrt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_to_unit_length() {
        let n = ps_vec_normalize(Vec3::new(3.0, 4.0, 0.0));
        // Exact bits await a bit-exact frsqrte; one Newton step from a
        // 12-bit estimate is within a few ulps.
        assert!((n.x - 0.6).abs() < 4.0 * f32::EPSILON);
        assert!((n.y - 0.8).abs() < 4.0 * f32::EPSILON);
        assert_eq!(n.z, 0.0);
    }

    #[test]
    fn keeps_signed_zero_and_propagates_degenerate_input() {
        // Floors produce normals like (-(y1 - y0), x1 - x0, 0) = (-0.0, w, 0).
        let n = ps_vec_normalize(Vec3::new(-0.0, 150.0, 0.0));
        assert!(n.x == 0.0 && n.x.is_sign_negative());
        assert!((n.y - 1.0).abs() < 4.0 * f32::EPSILON);
        let zero = ps_vec_normalize(Vec3::new(0.0, 0.0, 0.0));
        assert!(zero.x.is_nan() && zero.y.is_nan() && zero.z.is_nan());
    }
}
