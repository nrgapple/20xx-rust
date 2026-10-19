//! Port of the paired-single vector routines in
//! `libs/dolphin/src/dolphin/mtx/vec.c` that Melee calls.
//!
//! Call sites in the 1.02 DOL: `PSVECCrossProduct` 66, `PSVECNormalize` 55,
//! `PSVECAdd` 44, `PSVECScale` 39, `PSVECSubtract` 26, `PSVECDotProduct` 23,
//! `PSVECMag` 19.
//!
//! All loads happen before any store in each routine, so taking inputs by
//! value matches the original even when the C caller passes the same vector
//! as input and output.
//!
//! `PSVECNormalize` and `PSVECMag` refine `frsqrte` with a single
//! single-precision Newton step, so their results depend on the exact
//! hardware estimate; with the provisional [`frsqrte`] they are not
//! bit-exact. See `research/notes/math.md`.

use crate::math::Vec3;
use crate::ppc::{
    fnmsubs, frsqrte, ps_madd, ps_merge01, ps_merge10, ps_merge11, ps_msub, ps_mul, ps_muls0,
    ps_neg, ps_sum0, Ps,
};

/// `0.5f`, `3.0f` (0x804DE340, 0x804DE344).
const HALF: f32 = 0.5;
const THREE: f32 = 3.0;

/// `fmuls` with a double operand (the raw `frsqrte` estimate): the `f64`
/// product rounded to single. Exact when the estimate has at most 24
/// significant bits, as the provisional one does. If the hardware estimate
/// has more, Gekko's handling of a non-single `fmuls` operand also matters
/// (unverified).
#[inline]
fn fmuls_d(a: f64, c: f64) -> f32 {
    (a * c) as f32
}

/// `PSVECAdd` (0x80342D54): `a + b`, per component.
pub fn ps_vec_add(a: &Vec3, b: &Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// `PSVECSubtract` (0x80342D78): `a - b`, per component.
pub fn ps_vec_subtract(a: &Vec3, b: &Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// `PSVECScale` (0x80342D9C): `src * scale` (`ps_muls0`).
pub fn ps_vec_scale(src: &Vec3, scale: f32) -> Vec3 {
    Vec3::new(src.x * scale, src.y * scale, src.z * scale)
}

/// `(z*z + x*x) + y*y`, as `PSVECNormalize`/`PSVECMag` compute it
/// (`ps_mul`, `ps_madd`, `ps_sum0`).
#[inline]
fn square_mag(v: &Vec3) -> f32 {
    let xy = Ps(v.x, v.y);
    let xx_yy = ps_mul(xy, xy);
    let z = Ps::single(v.z);
    let xx_zz = ps_madd(z, z, xx_yy);
    ps_sum0(xx_zz, z, xx_yy).0
}

/// One single-precision Newton step on the raw estimate:
/// `(3 - r*r*sq) * (r * 0.5)`, with `3 - (r*r)*sq` fused.
#[inline]
fn rsqrt_one_step(sqsum: f32) -> f32 {
    let rsqrt = frsqrte(f64::from(sqsum));
    let nwork0 = fmuls_d(rsqrt, rsqrt);
    let nwork1 = fmuls_d(rsqrt, f64::from(HALF));
    let nwork0 = fnmsubs(nwork0, sqsum, THREE);
    nwork0 * nwork1
}

/// `PSVECNormalize` (0x80342DB8): `v / |v|` with a one-step reciprocal
/// square root. A zero vector gives NaNs (the estimate is infinite).
pub fn ps_vec_normalize(v: &Vec3) -> Vec3 {
    let rsqrt = rsqrt_one_step(square_mag(v));
    let xy = ps_muls0(Ps(v.x, v.y), Ps::splat(rsqrt));
    Vec3::new(xy.0, xy.1, v.z * rsqrt)
}

/// `PSVECMag` (0x80342DFC): `sq * rsqrt(sq)`, where `fsel` substitutes `sq`
/// for a negative or NaN refined estimate (so a zero vector gives 0).
pub fn ps_vec_mag(v: &Vec3) -> f32 {
    let sq = square_mag(v);
    let rsqrt = rsqrt_one_step(sq);
    let selected = if rsqrt >= 0.0 { rsqrt } else { sq };
    sq * selected
}

/// `PSVECDotProduct` (0x80342E38): `fmadds(ax, bx, ay*by) + az*bz`.
pub fn ps_vec_dot_product(a: &Vec3, b: &Vec3) -> f32 {
    let yz = ps_mul(Ps(a.y, a.z), Ps(b.y, b.z));
    let sum = ps_madd(Ps(a.x, a.y), Ps(b.x, b.y), yz);
    ps_sum0(sum, yz, yz).0
}

/// `PSVECCrossProduct` (0x80342E58): `a x b`.
///
/// Each component is `p*q - (r*s)` with the first product fused; `y` and `z`
/// are computed as `a.x*b.z - b.x*a.z` and `a.y*b.x - b.y*a.x` and then
/// negated (`ps_neg`), so a zero there comes out as `-0.0`.
pub fn ps_vec_cross_product(a: &Vec3, b: &Vec3) -> Vec3 {
    let b_xy = Ps(b.x, b.y);
    let a_z = Ps::splat(a.z);
    let a_xy = Ps(a.x, a.y);
    let b_yx = ps_merge10(b_xy, b_xy);
    let b_z = Ps::splat(b.z);
    let f4 = ps_mul(b_xy, a_z);
    let f7 = ps_muls0(b_xy, a_xy);
    let f5 = ps_msub(a_xy, b_z, f4);
    let f8 = ps_msub(a_xy, b_yx, f7);
    let x = ps_merge11(f5, f5);
    let yz = ps_neg(ps_merge01(f5, f8));
    Vec3::new(x.0, yz.0, yz.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_product_signs() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = ps_vec_cross_product(&x, &y);
        assert_eq!(z, Vec3::new(0.0, 0.0, 1.0));
        // The negated components produce -0.0 where the product is zero.
        assert!(z.y.is_sign_negative());
    }

    #[test]
    fn normalize_and_mag_are_close() {
        let v = Vec3::new(3.0, 4.0, 12.0);
        assert!((ps_vec_mag(&v) - 13.0).abs() < 1e-4);
        let n = ps_vec_normalize(&v);
        assert!((n.z - 12.0 / 13.0).abs() < 1e-5);
        assert_eq!(ps_vec_mag(&Vec3::default()), 0.0);
        assert_eq!(ps_vec_dot_product(&v, &v), 169.0);
    }
}
