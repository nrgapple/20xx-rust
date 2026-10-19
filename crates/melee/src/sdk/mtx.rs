//! Port of the matrix routines in `libs/dolphin/src/dolphin/mtx/mtx.c` and
//! `mtxvec.c` that Melee calls (`MTX*` macros resolve to the `PS*` forms).
//!
//! Call sites in the 1.02 DOL: `PSMTXConcat` 133, `PSMTXIdentity` 71,
//! `PSMTXMultVec` 53, `PSMTXCopy` 36, `PSMTXScale` 15, `PSMTXRotAxisRad` 13,
//! `PSMTXTrans` 11, `PSMTXTranspose` 10, `PSMTXMultVecSR` 10,
//! `C_MTXLookAt` 6, `PSMTXInverse` 5, `MTXRotRad` 5, `PSMTXQuat` 4,
//! `PSMTXRotTrig` 1. The projection builders (`MTXFrustum`,
//! `MTXPerspective`, `MTXOrtho`, `MTXLight*`) are rendering-only and not
//! ported.
//!
//! Every routine here loads all of its inputs before its first store, so
//! value semantics match the original even when the C passes the same
//! matrix as input and output (e.g. `MTXConcat(temp, m, m)`).
//!
//! `PSMTXInverse` and `PSMTXQuat` refine a `fres` estimate with one Newton
//! step, and `PSMTXRotAxisRad` normalizes its axis with `PSVECNormalize`;
//! all three depend on exact hardware estimates (provisional here).

use crate::math::{Mtx, Quaternion, Vec3};
use crate::msl::{cosf, sinf};
use crate::ppc::{
    fmadds, fnmsubs, fres, ps_add, ps_madd, ps_madds0, ps_madds1, ps_merge00, ps_merge01,
    ps_merge10, ps_merge11, ps_msub, ps_mul, ps_muls0, ps_muls1, ps_neg, ps_nmadd, ps_nmsub,
    ps_sub, ps_sum0, ps_sum1, Ps,
};
use crate::sdk::vec::{ps_vec_cross_product, ps_vec_normalize};

/// `Unit01` (`.sbss`-adjacent data at 0x804D5C00): `{ 0.0f, 1.0f }`.
const UNIT01: Ps = Ps(0.0, 1.0);

/// The identity matrix.
pub const IDENTITY: Mtx = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// `PSMTXIdentity` (0x803421A4).
pub fn ps_mtx_identity() -> Mtx {
    IDENTITY
}

/// `PSMTXCopy` (0x803421D0): a bitwise copy (`psq_l`/`psq_st` with `qr0`).
pub fn ps_mtx_copy(src: &Mtx) -> Mtx {
    *src
}

/// `PSMTXConcat` (0x80342204): `a * b`.
///
/// Per row of `a`, the four results are built in two paired-single
/// accumulators: `b0*a0` (`ps_muls0`), then `+ b1*a1`, `+ b2*a2` (fused
/// `ps_madds1`/`ps_madds0`), and for the right pair a final fused
/// `+ Unit01 * a3`, i.e. `0*a3` into column 2 and `1*a3` into column 3.
pub fn ps_mtx_concat(a: &Mtx, b: &Mtx) -> Mtx {
    let pair = |row: &[f32; 4], i: usize| Ps(row[i], row[i + 1]);
    let (b0l, b0r) = (pair(&b[0], 0), pair(&b[0], 2));
    let (b1l, b1r) = (pair(&b[1], 0), pair(&b[1], 2));
    let (b2l, b2r) = (pair(&b[2], 0), pair(&b[2], 2));
    let row = |r: &[f32; 4]| {
        let (lo, hi) = (pair(r, 0), pair(r, 2));
        let mut left = ps_muls0(b0l, lo);
        let mut right = ps_muls0(b0r, lo);
        left = ps_madds1(b1l, lo, left);
        right = ps_madds1(b1r, lo, right);
        left = ps_madds0(b2l, hi, left);
        right = ps_madds0(b2r, hi, right);
        right = ps_madds1(UNIT01, hi, right);
        [left.0, left.1, right.0, right.1]
    };
    [row(&a[0]), row(&a[1]), row(&a[2])]
}

/// `PSMTXTranspose` (0x803422D0): transposes the 3x3 part; the translation
/// column becomes zero.
pub fn ps_mtx_transpose(src: &Mtx) -> Mtx {
    [
        [src[0][0], src[1][0], src[2][0], 0.0],
        [src[0][1], src[1][1], src[2][1], 0.0],
        [src[0][2], src[1][2], src[2][2], 0.0],
    ]
}

/// `PSMTXInverse` (0x80342320): the inverse of an affine matrix, or `None`
/// (the C returns 0 and leaves `inv` untouched) if the determinant compares
/// equal to zero.
///
/// The cofactors, determinant, and translation are computed in paired
/// singles exactly as in the machine code; `1/det` is a `fres` estimate
/// refined by one fused Newton step (`2r - det*r*r`).
pub fn ps_mtx_inverse(src: &Mtx) -> Option<Mtx> {
    let f0 = Ps::single(src[0][0]);
    let f1 = Ps(src[0][1], src[0][2]);
    let f2 = Ps::single(src[1][0]);
    let mut f6 = ps_merge10(f1, f0);
    let f3 = Ps(src[1][1], src[1][2]);
    let f4 = Ps::single(src[2][0]);
    let mut f7 = ps_merge10(f3, f2);
    let f5 = Ps(src[2][1], src[2][2]);
    let mut f11 = ps_mul(f3, f6);
    let mut f13 = ps_mul(f5, f7);
    let mut f8 = ps_merge10(f5, f4);
    f11 = ps_msub(f1, f7, f11);
    let mut f12 = ps_mul(f1, f8);
    f13 = ps_msub(f3, f8, f13);
    let mut f10 = ps_mul(f3, f4);
    f12 = ps_msub(f5, f6, f12);
    let mut f9 = ps_mul(f0, f5);
    f8 = ps_mul(f1, f2);
    f6 = ps_sub(f6, f6);
    f10 = ps_msub(f2, f5, f10);
    f7 = ps_mul(f0, f13);
    f9 = ps_msub(f1, f4, f9);
    f7 = ps_madd(f2, f12, f7);
    f8 = ps_msub(f0, f3, f8);
    f7 = ps_madd(f4, f11, f7);
    // ps_cmpo0: equal only if both ps0 values are ordered and equal.
    if f7.0 == f6.0 {
        return None;
    }
    let estimate = Ps::splat(fres(f64::from(f7.0)));
    let twice = ps_add(estimate, estimate);
    let squared = ps_mul(estimate, estimate);
    let inv_det = ps_nmsub(f7, squared, twice);
    let (t0, t1, t2) = (
        Ps::splat(src[0][3]),
        Ps::splat(src[1][3]),
        Ps::splat(src[2][3]),
    );
    f13 = ps_muls0(f13, inv_det);
    f12 = ps_muls0(f12, inv_det);
    f11 = ps_muls0(f11, inv_det);
    let row0_lo = ps_merge00(f13, f12);
    f10 = ps_muls0(f10, inv_det);
    let row1_lo = ps_merge11(f13, f12);
    f9 = ps_muls0(f9, inv_det);
    let mut t = ps_mul(f13, t0);
    f8 = ps_muls0(f8, inv_det);
    t = ps_madd(f12, t1, t);
    t = ps_nmadd(f11, t2, t);
    let mut t_row2 = ps_mul(f10, t0);
    let row0_hi = ps_merge00(f11, t);
    let row1_hi = ps_merge11(f11, t);
    t_row2 = ps_madd(f9, t1, t_row2);
    t_row2 = ps_nmadd(f8, t2, t_row2);
    Some([
        [row0_lo.0, row0_lo.1, row0_hi.0, row0_hi.1],
        [row1_lo.0, row1_lo.1, row1_hi.0, row1_hi.1],
        [f10.0, f9.0, f8.0, t_row2.0],
    ])
}

/// Axis selector for [`ps_mtx_rot_trig`]: the C takes a `char`, accepting
/// `'x'`/`'X'`, `'y'`/`'Y'`, `'z'`/`'Z'` (compared after `| 0x20`).
pub fn rot_axis_char(axis: u8) -> Option<u8> {
    match axis | 0x20 {
        b'x' | b'y' | b'z' => Some(axis | 0x20),
        _ => None,
    }
}

/// `PSMTXRotTrig` (0x80342488): rotation about a coordinate axis from a
/// sine and cosine. Returns `None` for an unknown axis (the C leaves `m`
/// untouched). `-sin` is `ps_neg`, so it flips the sign of zero.
pub fn ps_mtx_rot_trig(axis: u8, sin_a: f32, cos_a: f32) -> Option<Mtx> {
    let (s, c) = (sin_a, cos_a);
    Some(match rot_axis_char(axis)? {
        b'x' => [[1.0, 0.0, 0.0, 0.0], [0.0, c, -s, 0.0], [0.0, s, c, 0.0]],
        b'y' => [[c, 0.0, s, 0.0], [0.0, 1.0, 0.0, 0.0], [-s, 0.0, c, 0.0]],
        _ => [[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
    })
}

/// `MTXRotRad` (0x80342418): [`ps_mtx_rot_trig`] with `sinf(rad)`,
/// `cosf(rad)`.
pub fn mtx_rot_rad(axis: u8, rad: f32) -> Option<Mtx> {
    let s = sinf(rad);
    let c = cosf(rad);
    ps_mtx_rot_trig(axis, s, c)
}

/// `PSMTXRotAxisRad` (0x80342530): rotation by `rad` about `axis`, which is
/// first normalized with [`ps_vec_normalize`].
///
/// Audit: `t = 1 - cos`; products in paired singles; `t*x*y -/+ s*z` are a
/// fused `fnmsubs`/`fmadds` pair; the rest are `ps_sum0`/`ps_sum1` adds.
pub fn ps_mtx_rot_axis_rad(axis: &Vec3, rad: f32) -> Mtx {
    let zero = Ps::splat(0.0);
    let s = sinf(rad);
    let c = cosf(rad);
    let t = 1.0 - c;
    let n = ps_vec_normalize(axis);
    let mut xy = Ps(n.x, n.y);
    let z = Ps::splat(n.z);
    let cc = Ps(c, c);
    let mut txy = ps_muls0(xy, Ps::splat(t));
    let mut tz = ps_muls0(z, Ps::splat(t));
    let f3 = ps_muls1(txy, xy);
    let f2 = ps_muls0(txy, xy);
    xy = ps_muls0(xy, Ps::splat(s));
    txy = ps_muls0(txy, z);
    let f6 = Ps::splat(fnmsubs(z.0, s, f3.0));
    let f7 = Ps::splat(fmadds(z.0, s, f3.0));
    let mut neg = ps_neg(xy);
    let f8 = ps_sum0(txy, zero, xy);
    let f2 = ps_sum0(f2, f6, cc);
    let f3 = ps_sum1(cc, f7, f3);
    let f6 = ps_sum0(neg, zero, txy);
    neg = ps_sum0(txy, txy, neg);
    tz = ps_muls0(tz, z);
    let f4 = ps_sum1(xy, neg, txy);
    tz = ps_sum0(tz, zero, cc);
    [
        [f2.0, f2.1, f8.0, f8.1],
        [f3.0, f3.1, f6.0, f6.1],
        [f4.0, f4.1, tz.0, tz.1],
    ]
}

/// `PSMTXTrans` (0x80342634): a translation matrix.
pub fn ps_mtx_trans(x: f32, y: f32, z: f32) -> Mtx {
    [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]]
}

/// `PSMTXScale` (0x80342668): a scale matrix.
pub fn ps_mtx_scale(x: f32, y: f32, z: f32) -> Mtx {
    [[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0]]
}

/// `PSMTXQuat` (0x80342690): rotation matrix from a quaternion, scaled by
/// `2 / |q|^2` where `1 / |q|^2` is a `fres` estimate refined by one fused
/// Newton step.
pub fn ps_mtx_quat(q: &Quaternion) -> Mtx {
    let one = Ps::splat(1.0);
    let xy = Ps(q.x, q.y);
    let zw = Ps(q.z, q.w);
    let zero = Ps::splat(0.0); // fsubs f0, 1.0, 1.0
    let two = Ps::splat(2.0); // fadds f2, 1.0, 1.0
    let mut sq_xy = ps_mul(xy, xy);
    let yx = ps_merge10(xy, xy);
    let mut f8 = ps_madd(zw, zw, sq_xy);
    let sq_zw = ps_mul(zw, zw);
    let mut norm = ps_sum0(f8, f8, f8);
    let mut yw_xw = ps_muls1(yx, zw);
    let estimate = Ps::splat(fres(f64::from(norm.0)));
    f8 = ps_sum1(sq_zw, f8, sq_xy);
    norm = ps_nmsub(norm, estimate, two);
    let zw_ww = ps_muls1(zw, zw);
    norm = ps_mul(estimate, norm);
    sq_xy = ps_sum0(sq_xy, sq_xy, sq_xy);
    let scale = Ps::splat(norm.0 * two.0);
    let mut f11 = ps_madd(xy, yx, zw_ww);
    let mut f7 = ps_msub(xy, yx, zw_ww);
    let m22 = ps_nmsub(sq_xy, scale, one);
    f8 = ps_nmsub(f8, scale, one);
    f11 = ps_mul(f11, scale);
    f7 = ps_mul(f7, scale);
    let mut f9 = ps_madds0(xy, zw, yw_xw);
    let row1_lo = ps_merge00(f11, f8);
    yw_xw = ps_nmsub(yw_xw, two, f9);
    let row0_lo = ps_merge10(f8, f7);
    f9 = ps_mul(f9, scale);
    yw_xw = ps_mul(yw_xw, scale);
    let row1_hi = ps_merge10(yw_xw, zero);
    let row2_lo = ps_merge01(yw_xw, f9);
    [
        [row0_lo.0, row0_lo.1, f9.0, zero.0],
        [row1_lo.0, row1_lo.1, row1_hi.0, row1_hi.1],
        [row2_lo.0, row2_lo.1, m22.0, zero.0],
    ]
}

/// `PSMTXMultVec` (0x80342AA8): `m * (v, 1)`.
///
/// Per row: `(m0*x, m1*y)`, fused with `(m2*z, m3*1)`, then the two lanes
/// added (`ps_sum0`).
pub fn ps_mtx_mult_vec(m: &Mtx, v: &Vec3) -> Vec3 {
    let xy = Ps(v.x, v.y);
    let z1 = Ps::single(v.z);
    let row = |r: &[f32; 4]| {
        let p = ps_mul(Ps(r[0], r[1]), xy);
        let q = ps_madd(Ps(r[2], r[3]), z1, p);
        q.0 + q.1
    };
    Vec3::new(row(&m[0]), row(&m[1]), row(&m[2]))
}

/// `PSMTXMultVecSR` (0x80342AFC): `m * (v, 0)` (rotation and scale only):
/// `fmadds(m2, z, m0*x + m1*y)` per row.
pub fn ps_mtx_mult_vec_sr(m: &Mtx, v: &Vec3) -> Vec3 {
    let row = |r: &[f32; 4]| fmadds(r[2], v.z, r[0] * v.x + r[1] * v.y);
    Vec3::new(row(&m[0]), row(&m[1]), row(&m[2]))
}

/// `C_MTXLookAt` (0x80342734): view matrix from camera position, up vector,
/// and target. Normalizes with [`ps_vec_normalize`]; the translation dot
/// products are unfused C (`-((px*rx + py*ry) + pz*rz)`).
pub fn c_mtx_look_at(cam_pos: &Vec3, cam_up: &Vec3, target: &Vec3) -> Mtx {
    let look = Vec3::new(
        cam_pos.x - target.x,
        cam_pos.y - target.y,
        cam_pos.z - target.z,
    );
    let look = ps_vec_normalize(&look);
    let right = ps_vec_cross_product(cam_up, &look);
    let right = ps_vec_normalize(&right);
    let up = ps_vec_cross_product(&look, &right);
    let dot = |v: &Vec3| -(cam_pos.x * v.x + cam_pos.y * v.y + cam_pos.z * v.z);
    [
        [right.x, right.y, right.z, dot(&right)],
        [up.x, up.y, up.z, dot(&up)],
        [look.x, look.y, look.z, dot(&look)],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_with_identity_is_exact() {
        let m: Mtx = [
            [1.5, -2.0, 0.25, 7.0],
            [0.0, 3.0, -1.0, -4.5],
            [2.0, 0.5, 1.0, 0.125],
        ];
        assert_eq!(ps_mtx_concat(&IDENTITY, &m), m);
        assert_eq!(ps_mtx_concat(&m, &IDENTITY), m);
    }

    #[test]
    fn inverse_of_scale_translate() {
        let m = ps_mtx_concat(&ps_mtx_trans(1.0, 2.0, 3.0), &ps_mtx_scale(2.0, 4.0, 8.0));
        let inv = ps_mtx_inverse(&m).unwrap();
        let p = ps_mtx_mult_vec(&inv, &ps_mtx_mult_vec(&m, &Vec3::new(1.0, 1.0, 1.0)));
        assert!((p.x - 1.0).abs() < 1e-3 && (p.y - 1.0).abs() < 1e-3 && (p.z - 1.0).abs() < 1e-3);
        assert_eq!(ps_mtx_inverse(&ps_mtx_scale(0.0, 1.0, 1.0)), None);
    }

    #[test]
    fn rotations_are_rotations() {
        let r = mtx_rot_rad(b'Z', std::f32::consts::FRAC_PI_2).unwrap();
        let v = ps_mtx_mult_vec(&r, &Vec3::new(1.0, 0.0, 0.0));
        assert!(v.x.abs() < 1e-6 && (v.y - 1.0).abs() < 1e-6);
        let a = ps_mtx_rot_axis_rad(&Vec3::new(0.0, 0.0, 2.0), std::f32::consts::FRAC_PI_2);
        let w = ps_mtx_mult_vec(&a, &Vec3::new(1.0, 0.0, 0.0));
        assert!(w.x.abs() < 1e-3 && (w.y - 1.0).abs() < 1e-3, "{w:?}");
        let q = ps_mtx_quat(&Quaternion::new(0.0, 0.0, 0.0, 1.0));
        assert!((q[0][0] - 1.0).abs() < 1e-3 && (q[1][1] - 1.0).abs() < 1e-3);
        assert_eq!(mtx_rot_rad(b'w', 1.0), None);
    }
}
