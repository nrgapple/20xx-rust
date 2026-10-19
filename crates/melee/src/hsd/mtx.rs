//! HSD Euler transforms. `HSD_MtxSRT` 0x8037A250, audited against 1.02.
//! Four fused operations at 0x8037A388/38C/3B4/3B8; parent reciprocals are
//! double division followed by frsp. Trig is the game's MSL implementation.
use crate::{
    math::Mtx,
    msl::{cosf, sinf},
    ppc::{fmadds, fmsubs},
};

pub fn srt(
    scale: [f32; 3],
    rotation: [f32; 3],
    translation: [f32; 3],
    parent_scale: Option<[f32; 3]>,
) -> Mtx {
    let [sx, sy, sz] = rotation.map(sinf);
    let [cx, cy, cz] = rotation.map(cosf);
    let [mut x2, mut y2, mut z2] = scale;
    let [mut x1, y1, mut z1] = scale;
    let [mut x, mut y, z] = scale;
    if let Some(p) = parent_scale {
        let inv = p.map(|v| (1.0f64 / f64::from(v)) as f32);
        y2 *= p[1] * inv[0];
        z2 *= p[2] * inv[0];
        x1 *= p[0] * inv[1];
        z1 *= p[2] * inv[1];
        x *= p[0] * inv[2];
        y *= p[1] * inv[2];
    }
    // Keep the register-level operand order, including the separate products
    // feeding fma and fmsub. x2 is deliberately not parent-compensated.
    x2 = cz * (x2 * cy);
    [
        [
            x2,
            y2 * fmsubs(cz, sx * sy, cx * sz),
            z2 * fmadds(cz, cx * sy, sx * sz),
            translation[0],
        ],
        [
            sz * (x1 * cy),
            y1 * fmadds(sz, sx * sy, cx * cz),
            z1 * fmsubs(sz, cx * sy, sx * cz),
            translation[1],
        ],
        [-x * sy, cy * (y * sx), cy * (z * cx), translation[2]],
    ]
}
