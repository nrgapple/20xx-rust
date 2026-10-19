//! Port of `src/melee/lb/lbvector.c`, the game's 3D vector helpers.
//!
//! Call sites in the 1.02 DOL (see `research/notes/math.md`):
//! `lbVector_Normalize` 98, `Add` 85, `Diff` 50, `Angle` 38, `Rotate` 35,
//! `Sub` 34, `AngleXY` 33, `Mirror` 29, `RotateAboutUnitAxis` 26, and fewer
//! for the rest. `lbVector_WorldToScreen` (camera projection, which needs
//! `HSD_CObj`) is not ported; it is also the only function here that 20XX
//! changes (one byte).
//!
//! Fusing is not uniform: the inline `lbVector_Len` compiles to
//! `(x*x + y*y) + z*z` with no fused ops in `lbVector_Normalize` and
//! `lbVector_Angle`, but to fused `fmadds` chains in `lbVector_8000E838`.
//! Expressions of the form `a*b ± c*d` round `c*d` and fuse the rest. Each
//! function's doc comment states what its machine code does; all are
//! checked against the DOL by `tests/math.rs`.
//!
//! Pointer-returning C functions return nothing (or a value) here. Where the
//! C writes through an output pointer that might alias an input, the port
//! takes inputs by value; the notes say where the original would differ
//! under aliasing.

// The C clamps with `if` chains; kept as written rather than `f32::clamp`.
#![allow(clippy::manual_clamp)]

use crate::lb::trigf::{acosf, asinf, atan2f};
use crate::math::{Mtx, Vec3};
use crate::msl::{sqrtf, sqrtf_accurate};
use crate::ppc::{fmadds, fmsubs, frsp};
use crate::sdk::vec::ps_vec_cross_product;

/// `M_PI` as the `double` the C compares against (0x804D7AD0).
const PI: f64 = f64::from_bits(0x4009_21fb_5444_2d18);
/// `M_TAU` (0x804D7AD8).
const TAU: f64 = f64::from_bits(0x4019_21fb_5444_2d18);
/// `-M_PI` (0x804D7AE0).
const NEG_PI: f64 = f64::from_bits(0xc009_21fb_5444_2d18);
/// `M_PI / 2` (0x804D7AF8).
const HALF_PI: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);
/// The quintic's coefficients: 0.98786199f, 0.15527099f, 0.005643f
/// (0x804D7AE8..0x804D7AF0).
const SIN_C1: f32 = f32::from_bits(0x3f7c_e486);
const SIN_C3: f32 = f32::from_bits(0x3e1e_ff5c);
const SIN_C5: f32 = f32::from_bits(0x3bb8_e8ea);
/// `0.0000000001f` (0x804D7AC4).
const TINY: f32 = f32::from_bits(0x2edb_e6ff);
/// `9.9999997473787516e-06f` (0x804D7B28).
const SMALL: f32 = f32::from_bits(0x3727_c5ac);
const NEG_SMALL: f32 = f32::from_bits(0xb727_c5ac);
/// `1.5707963705062866f` (0x804D7B10).
const HALF_PI_F: f32 = f32::from_bits(0x3fc9_0fdb);

/// Rotation axis for [`rotate`]: the C passes 1, 2, or 4.
///
/// The original leaves `x`, `y`, `z` uninitialized for any other value and
/// stores whatever the caller left in `f0`, `f2`, `f3`; this enum makes
/// that undefined case unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X = 1,
    Y = 2,
    Z = 4,
}

/// The quintic `c1 a - c3 a^3 + c5 a^5` as compiled:
/// `fmadds(a, (((c5 a) a) a) a, fmsubs(c1, a, ((c3 a) a) a))`.
#[inline]
fn quintic(a: f32) -> f32 {
    let t5 = SIN_C5 * a * a * a * a;
    let t3 = SIN_C3 * a * a * a;
    fmadds(a, t5, fmsubs(SIN_C1, a, t3))
}

/// Wrap once into (-pi, pi], comparing and subtracting in double precision.
#[inline]
fn wrap(angle: f32) -> f32 {
    let a = f64::from(angle);
    if a > PI {
        frsp(a - TAU)
    } else if a < NEG_PI {
        frsp(a + TAU)
    } else {
        angle
    }
}

/// `lbvector_sin` (static inline in `lbvector.c`): a quintic approximation
/// after one wrap into (-pi, pi].
pub fn lbvector_sin(angle: f32) -> f32 {
    quintic(wrap(angle))
}

/// `lbvector_cos` (static inline): `lbvector_sin` of `angle + pi/2`, with
/// the shift done in double precision and rounded before wrapping.
pub fn lbvector_cos(angle: f32) -> f32 {
    quintic(wrap(frsp(f64::from(angle) + HALF_PI)))
}

/// `lbVector_Normalize` (0x8000D2EC): scales `vec` to unit length and returns
/// its original length, or returns 0 (leaving `vec` alone) when the length
/// is 0.
///
/// Audit: length is `sqrtf((x*x + y*y) + z*z)` with no fused ops; then
/// `inv = 1.0f / len` and three `fmuls`.
pub fn normalize(vec: &mut Vec3) -> f32 {
    let len = sqrtf(vec.x * vec.x + vec.y * vec.y + vec.z * vec.z);
    if len == 0.0 {
        return 0.0;
    }
    let inv = 1.0 / len;
    vec.x *= inv;
    vec.y *= inv;
    vec.z *= inv;
    len
}

/// `lbVector_NormalizeXY` (0x8000D3B0): like [`normalize`] on `x`/`y` only,
/// using `sqrtf_accurate` (four Newton steps).
pub fn normalize_xy(a: &mut Vec3) -> f32 {
    let len = sqrtf_accurate(a.x * a.x + a.y * a.y);
    if len == 0.0 {
        return 0.0;
    }
    let inv = 1.0 / len;
    a.x *= inv;
    a.y *= inv;
    len
}

/// `lbVector_Add` (0x8000D46C): `a += b`.
pub fn add(a: &mut Vec3, b: &Vec3) {
    a.x += b.x;
    a.y += b.y;
    a.z += b.z;
}

/// `lbVector_Add_xy` (0x8000D4A0): `a.xy += b.xy`.
pub fn add_xy(a: &mut Vec3, b: &Vec3) {
    a.x += b.x;
    a.y += b.y;
}

/// `lbVector_Sub` (0x8000D4C4): `a -= b`.
pub fn sub(a: &mut Vec3, b: &Vec3) {
    a.x -= b.x;
    a.y -= b.y;
    a.z -= b.z;
}

/// `lbVector_Diff` (0x8000D4F8): `a - b`.
pub fn diff(a: &Vec3, b: &Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// `lbVector_CrossprodNormalized` (0x8000D530): `PSVECCrossProduct(a, b)`,
/// then [`normalize`] (inlined in the original, calling `sqrtf__Ff`).
pub fn crossprod_normalized(a: &Vec3, b: &Vec3) -> Vec3 {
    let mut result = ps_vec_cross_product(a, b);
    normalize(&mut result);
    result
}

/// `lbVector_Angle` (0x8000D620): the angle between `a` and `b` via
/// [`acosf`], or 0 if `|a| |b| <= 1e-10`.
///
/// Audit: lengths unfused as in [`normalize`]; the dot product is fused,
/// `fmadds(az, bz, fmadds(ax, bx, ay * by))`; the cosine is clamped to
/// [-1, 1] before `acosf`.
pub fn angle(a: &Vec3, b: &Vec3) -> f32 {
    let len_a = sqrtf(a.x * a.x + a.y * a.y + a.z * a.z);
    let len_b = sqrtf(b.x * b.x + b.y * b.y + b.z * b.z);
    let lena_lenb = len_a * len_b;
    if lena_lenb > TINY {
        let dot = fmadds(a.z, b.z, fmadds(a.x, b.x, a.y * b.y));
        let mut cosine = dot / lena_lenb;
        if cosine > 1.0 {
            cosine = 1.0;
        }
        if cosine < -1.0 {
            cosine = -1.0;
        }
        return acosf(cosine);
    }
    0.0
}

/// `lbVector_AngleXY` (0x8000D790): the angle between `a.xy` and `b.xy`,
/// with `sqrtf_accurate` lengths; 0 if the product of lengths is 0.
pub fn angle_xy(a: &Vec3, b: &Vec3) -> f32 {
    let len_a = sqrtf_accurate(a.x * a.x + a.y * a.y);
    let len_b = sqrtf_accurate(b.x * b.x + b.y * b.y);
    let lena_lenb = len_a * len_b;
    if lena_lenb != 0.0 {
        let mut cosine = fmadds(a.x, b.x, a.y * b.y) / lena_lenb;
        if cosine > 1.0 {
            cosine = 1.0;
        }
        if cosine < -1.0 {
            cosine = -1.0;
        }
        return acosf(cosine);
    }
    0.0
}

/// `lbVector_RotateAboutUnitAxis` (0x8000D8F4): rotates `v` by `angle`
/// (radians) about the unit vector `axis`, by first rotating the axis onto
/// the z axis.
///
/// Audit: 14 fused ops plus 4 in the inlined sine/cosine, matched one by
/// one (each `a*b ± c*d` fuses `a*b`). Checked against the DOL.
pub fn rotate_about_unit_axis(v: &mut Vec3, axis: &Vec3, angle: f32) {
    let len_axis_yz = sqrtf(axis.y * axis.y + axis.z * axis.z);
    let s = lbvector_sin(angle);
    let c = lbvector_cos(angle);
    let rotate_yz = len_axis_yz > TINY;

    let (mut x, mut y, mut z) = (v.x, v.y, v.z);
    let (mut unit_axis_yz_y, mut unit_axis_yz_z) = (0.0, 0.0);
    if rotate_yz {
        unit_axis_yz_y = axis.z / len_axis_yz;
        unit_axis_yz_z = axis.y / len_axis_yz;
        y = fmsubs(v.y, unit_axis_yz_y, v.z * unit_axis_yz_z);
        z = fmadds(v.y, unit_axis_yz_z, v.z * unit_axis_yz_y);
    }

    let x2 = fmsubs(x, len_axis_yz, z * axis.x);
    let z2 = fmadds(x, axis.x, z * len_axis_yz);
    let x3 = fmsubs(x2, c, y * s);
    let y3 = fmadds(x2, s, y * c);

    x = fmadds(x3, len_axis_yz, z2 * axis.x);
    y = y3;
    z = fmadds(-x3, axis.x, z2 * len_axis_yz);

    if rotate_yz {
        v.x = x;
        v.y = fmadds(y, unit_axis_yz_y, z * unit_axis_yz_z);
        v.z = fmadds(-y, unit_axis_yz_z, z * unit_axis_yz_y);
    } else {
        v.x = x;
        v.y = y;
        v.z = z;
    }
}

/// `lbVector_Rotate` (0x8000DB00): rotates `v` by `angle` about one
/// coordinate axis using the approximate `lbvector_sin`/`lbvector_cos`.
pub fn rotate(v: &mut Vec3, axis: Axis, angle: f32) {
    let s = lbvector_sin(angle);
    let c = lbvector_cos(angle);
    let (x, y, z) = match axis {
        Axis::X => (v.x, fmsubs(v.y, c, v.z * s), fmadds(v.y, s, v.z * c)),
        Axis::Y => (fmadds(v.x, c, v.z * s), v.y, fmsubs(v.z, c, v.x * s)),
        Axis::Z => (fmsubs(v.x, c, v.y * s), fmadds(v.x, s, v.y * c), v.z),
    };
    v.x = x;
    v.y = y;
    v.z = z;
}

/// `lbVector_Mirror` (0x8000DC6C): `a.xy -= 2 <a.xy, b.xy> b.xy` (z is
/// untouched).
///
/// Audit: `f = fmadds(bx, ax, by * ay) * -2.0f`; each update fused.
pub fn mirror(a: &mut Vec3, unit_mirror_axis: &Vec3) {
    let b = unit_mirror_axis;
    let f = fmadds(b.x, a.x, b.y * a.y) * -2.0;
    a.x = fmadds(b.x, f, a.x);
    a.y = fmadds(b.y, f, a.y);
}

/// `lbVector_CosAngle` (0x8000DCA8): `<a.xy, b.xy> / (|a.xy| |b.xy|)`.
pub fn cos_angle(a: &Vec3, b: &Vec3) -> f32 {
    let len_a = sqrtf(a.x * a.x + a.y * a.y);
    let len_b = sqrtf(b.x * b.x + b.y * b.y);
    fmadds(a.x, b.x, a.y * b.y) / (len_a * len_b)
}

/// `lbVector_Lerp` (0x8000DDAC): `(b - a) * f + a`, each step rounded.
///
/// The original writes `result` between steps and rereads `a`; a caller
/// passing `result == a` would get `((b - a) * f) * 2` style results. The
/// port assumes no aliasing (its single caller has none to our knowledge).
pub fn lerp(a: &Vec3, b: &Vec3, f: f32) -> Vec3 {
    let d = diff(b, a);
    let scaled = Vec3::new(d.x * f, d.y * f, d.z * f);
    Vec3::new(scaled.x + a.x, scaled.y + a.y, scaled.z + a.z)
}

/// `lbVector_8000DE38` (0x8000DE38): the quadratic through `points` at
/// `c = 0, 0.5, 1`, with `c` clamped to [0, 1].
///
/// Audit: per component, `var1 = fmadds(2, p2, fmsubs(2, p0, 4 * p1))`,
/// `var2 = fmadds(-3, p0, 4 * p1) - p2`, result
/// `p0 + fmadds(c, var1 * c, var2 * c)`.
pub fn lbvector_8000de38(points: &[Vec3; 3], mut c: f32) -> Vec3 {
    if c > 1.0 {
        c = 1.0;
    } else if c < 0.0 {
        c = 0.0;
    }
    let component = |p0: f32, p1: f32, p2: f32| {
        let p1_4 = 4.0 * p1;
        let var1 = fmadds(2.0, p2, fmsubs(2.0, p0, p1_4));
        let var2 = fmadds(-3.0, p0, p1_4) - p2;
        p0 + fmadds(c, var1 * c, var2 * c)
    };
    let [p0, p1, p2] = points;
    Vec3::new(
        component(p0.x, p1.x, p2.x),
        component(p0.y, p1.y, p2.y),
        component(p0.z, p1.z, p2.z),
    )
}

/// `lbVector_EulerAnglesFromONB` (0x8000DF0C): Euler angles rotating the
/// standard basis onto `(b, c, a)` (x, then y, then z).
pub fn euler_angles_from_onb(a: &Vec3, b: &Vec3, c: &Vec3) -> Vec3 {
    if b.z == -1.0 || b.z == 1.0 {
        if b.z == -1.0 {
            Vec3::new(atan2f(c.x, c.y), HALF_PI_F, 0.0)
        } else {
            Vec3::new(atan2f(-c.x, c.y), -HALF_PI_F, 0.0)
        }
    } else {
        let y = asinf(-b.z);
        let x = atan2f(c.z, a.z);
        let z = atan2f(b.y, b.x);
        Vec3::new(x, y, z)
    }
}

/// `lbVector_EulerAnglesFromPartialONB` (0x8000DFF4):
/// [`euler_angles_from_onb`] with `b = normalize(c x a)`.
pub fn euler_angles_from_partial_onb(a: &Vec3, c: &Vec3) -> Vec3 {
    let mut b = ps_vec_cross_product(c, a);
    normalize(&mut b);
    euler_angles_from_onb(a, &b, c)
}

/// `lbVector_ApplyEulerRotation` (0x8000E138): [`rotate`] about x, y, then z.
pub fn apply_euler_rotation(v: &mut Vec3, angles: &Vec3) {
    rotate(v, Axis::X, angles.x);
    rotate(v, Axis::Y, angles.y);
    rotate(v, Axis::Z, angles.z);
}

/// `lbVector_sqrtf_accurate` (0x8000E19C).
pub fn lbvector_sqrtf_accurate(x: f32) -> f32 {
    sqrtf_accurate(x)
}

/// `lbVector_CreateEulerMatrix` (0x8000E530): `Rz(z) Ry(y) Rx(x)` from the
/// first three components of `angles` (a `Quaternion*` in the C), using the
/// approximate sine/cosine; the translation column is zero.
pub fn create_euler_matrix(angles: &Vec3) -> Mtx {
    let sx = lbvector_sin(angles.x);
    let cx = lbvector_cos(angles.x);
    let sy = lbvector_sin(angles.y);
    let cy = lbvector_cos(angles.y);
    let sz = lbvector_sin(angles.z);
    let cz = lbvector_cos(angles.z);

    let sxsy = sx * sy;
    let cxsy = cx * sy;
    [
        [
            cy * cz,
            fmsubs(cz, sxsy, cx * sz),
            fmadds(cz, cxsy, sx * sz),
            0.0,
        ],
        [
            cy * sz,
            fmadds(sz, sxsy, cx * cz),
            fmsubs(sz, cxsy, sx * cz),
            0.0,
        ],
        [-sy, sx * cy, cx * cy, 0.0],
    ]
}

/// `lbVector_8000E838` (0x8000E838): projects `c` onto the line through `a`
/// with direction `b - a`, returning the foot point `d` and `|c - d|`. If
/// `|b - a|^2` is within ±1e-5, `d = a` and the result is `|c - a|`.
///
/// Audit: squared lengths and dot products here are fused `fmadds` chains
/// (unlike [`normalize`]); `d = fmadds(ba, f1, a)` per component. The
/// original rereads `a` and `c` after writing `d`, so it differs if `d`
/// aliases them; the port assumes it does not.
pub fn lbvector_8000e838(a: &Vec3, b: &Vec3, c: &Vec3) -> (Vec3, f32) {
    let len = |v: Vec3| sqrtf(fmadds(v.z, v.z, fmadds(v.x, v.x, v.y * v.y)));
    let b_a = diff(b, a);
    let sqrlen_b_a = fmadds(b_a.z, b_a.z, fmadds(b_a.x, b_a.x, b_a.y * b_a.y));
    let c_a = diff(c, a);
    if sqrlen_b_a < SMALL && sqrlen_b_a > NEG_SMALL {
        return (*a, len(c_a));
    }
    let f1 = fmadds(b_a.z, c_a.z, fmadds(b_a.x, c_a.x, b_a.y * c_a.y)) / sqrlen_b_a;
    let d = Vec3::new(
        fmadds(b_a.x, f1, a.x),
        fmadds(b_a.y, f1, a.y),
        fmadds(b_a.z, f1, a.z),
    );
    (d, len(diff(c, &d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_lengths() {
        let mut v = Vec3::new(3.0, 4.0, 12.0);
        assert_eq!(normalize(&mut v), 13.0);
        // Multiplied by the rounded reciprocal, not divided.
        let inv = 1.0f32 / 13.0;
        assert_eq!(v, Vec3::new(3.0 * inv, 4.0 * inv, 12.0 * inv));
        assert_ne!(v.x, 3.0 / 13.0);
        let mut zero = Vec3::default();
        assert_eq!(normalize(&mut zero), 0.0);
        let mut w = Vec3::new(3.0, 4.0, 9.0);
        assert_eq!(normalize_xy(&mut w), 5.0);
        assert_eq!(w.z, 9.0);
    }

    #[test]
    fn approximate_trig_is_close() {
        let mut a = -6.0f32;
        while a < 6.0 {
            assert!((lbvector_sin(a) - a.sin()).abs() < 0.02, "sin {a}");
            assert!((lbvector_cos(a) - a.cos()).abs() < 0.02, "cos {a}");
            a += 0.01;
        }
    }

    #[test]
    fn rotations_match_geometry() {
        let mut v = Vec3::new(1.0, 0.0, 0.0);
        rotate(&mut v, Axis::Z, std::f32::consts::FRAC_PI_2);
        assert!((v.x).abs() < 0.02 && (v.y - 1.0).abs() < 0.02);
        let mut w = Vec3::new(1.0, 0.0, 0.0);
        rotate_about_unit_axis(
            &mut w,
            &Vec3::new(0.0, 0.0, 1.0),
            std::f32::consts::FRAC_PI_2,
        );
        assert!((w.x).abs() < 0.02 && (w.y - 1.0).abs() < 0.02, "{w:?}");
        let angle = angle(&Vec3::new(1.0, 0.0, 0.0), &Vec3::new(0.0, 2.0, 0.0));
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn quadratic_passes_through_points() {
        let p = [
            Vec3::new(0.0, 1.0, 2.0),
            Vec3::new(1.0, 3.0, 2.0),
            Vec3::new(4.0, 1.0, -2.0),
        ];
        assert_eq!(lbvector_8000de38(&p, 0.0), p[0]);
        assert_eq!(lbvector_8000de38(&p, 0.5), p[1]);
        assert_eq!(lbvector_8000de38(&p, 2.0), p[2]);
    }
}
