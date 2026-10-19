//! Offline HSD scene conversion, not deterministic simulation code.
//! Descriptor layouts follow jobj.h/dobj.h/pobj.h/mobj.h/tobj.h at the pinned
//! decomp. Loading references: HSD_JObjLoadJoint 0x80370E44,
//! HSD_DObjLoadDesc 0x8035E1B4, HSD_PObjLoadDesc 0x8036BE64.
//! Bind-pose transforms follow HSD_MtxSRT 0x8037A250 and
//! SetupEnvelopeModelMtx 0x8036E4C4 (audited with dol-inspect). This converter
//! uses host trig/f64 math; its results must NOT become collision/rollback state.
use crate::{ensure, texture, Result};
use hsd_dat::Archive;
use std::collections::{BTreeMap, BTreeSet};

pub type Matrix = [[f64; 4]; 3];
const IDENTITY: Matrix = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
#[derive(Clone)]
struct Joint {
    flags: u32,
    world: Matrix,
    inverse: Option<Matrix>,
    display: Option<u32>,
}
pub struct Mesh {
    pub vertices: Vec<[f32; 8]>,
    pub color: [f32; 4],
    pub texture: Option<usize>,
    pub joint: u32,
    pub root: usize,
}
pub struct Scene {
    pub meshes: Vec<Mesh>,
    pub textures: Vec<texture::Texture>,
    pub joints: usize,
    pub warnings: BTreeSet<String>,
}
fn vector(a: &Archive, p: u32) -> Result<[f64; 3]> {
    let v = [
        f64::from(a.f32(p)?),
        f64::from(a.f32(p + 4)?),
        f64::from(a.f32(p + 8)?),
    ];
    ensure(v.iter().all(|v| v.is_finite()), "non-finite vector")?;
    Ok(v)
}
fn matrix(a: &Archive, p: u32) -> Result<Matrix> {
    let mut m = [[0.; 4]; 3];
    for (i, value) in m.iter_mut().flatten().enumerate() {
        *value = f64::from(a.f32(p + i as u32 * 4)?);
    }
    Ok(m)
}
fn concat(a: Matrix, b: Matrix) -> Matrix {
    let mut result = [[0.; 4]; 3];
    for i in 0..3 {
        for j in 0..4 {
            result[i][j] = a[i][0] * b[0][j]
                + a[i][1] * b[1][j]
                + a[i][2] * b[2][j]
                + if j == 3 { a[i][3] } else { 0. };
        }
    }
    result
}
fn transform(m: Matrix, v: [f64; 3], position: bool) -> [f32; 3] {
    std::array::from_fn(|i| {
        (m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] + if position { m[i][3] } else { 0. })
            as f32
    })
}
fn srt(
    scale: [f64; 3],
    rotation: [f64; 3],
    translation: [f64; 3],
    parent_scale: [f64; 3],
) -> Matrix {
    let [sx, sy, sz] = rotation.map(f64::sin);
    let [cx, cy, cz] = rotation.map(f64::cos);
    let rotation = [
        [cz * cy, cz * sx * sy - cx * sz, cz * cx * sy + sx * sz],
        [sz * cy, sz * sx * sy + cx * cz, sz * cx * sy - sx * cz],
        [-sy, cy * sx, cy * cx],
    ];
    let mut m = [[0.; 4]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = rotation[i][j] * scale[j] * parent_scale[j] / parent_scale[i];
        }
        m[i][3] = translation[i];
    }
    m
}

fn read_joints(
    a: &Archive,
    start: u32,
    parent: Matrix,
    parent_scale: [f64; 3],
    joints: &mut BTreeMap<u32, Joint>,
    depth: usize,
) -> Result<()> {
    ensure(depth < 256 && joints.len() < 4096, "joint tree limit")?;
    let mut next = Some(start);
    while let Some(p) = next {
        ensure(
            !joints.contains_key(&p),
            "joint cycle or unsupported instance",
        )?;
        let flags = a.u32(p + 4)?;
        ensure(
            flags & (1 << 17) == 0,
            "quaternion joint requires a dedicated converter",
        )?;
        let scale = vector(a, p + 0x20)?;
        ensure(
            parent_scale.iter().all(|x| x.abs() > 1e-12),
            "zero parent scale",
        )?;
        let world = concat(
            parent,
            srt(
                scale,
                vector(a, p + 0x14)?,
                vector(a, p + 0x2c)?,
                parent_scale,
            ),
        );
        let inverse = a.ptr(p + 0x38)?.map(|x| matrix(a, x)).transpose()?;
        let display = if flags & ((1 << 5) | (1 << 14)) == 0 {
            a.ptr(p + 0x10)?
        } else {
            None
        };
        joints.insert(
            p,
            Joint {
                flags,
                world,
                inverse,
                display,
            },
        );
        let inherited = if flags & 8 != 0 {
            parent_scale
        } else {
            std::array::from_fn(|i| parent_scale[i] * scale[i])
        };
        if let Some(child) = a.ptr(p + 8)? {
            read_joints(a, child, world, inherited, joints, depth + 1)?;
        }
        next = a.ptr(p + 12)?;
    }
    Ok(())
}

#[derive(Debug)]
struct Attr {
    kind: u32,
    mode: u32,
    count: u32,
    format: u32,
    frac: u8,
    stride: u32,
    data: Option<u32>,
}
fn attrs(a: &Archive, p: u32) -> Result<Vec<Attr>> {
    let mut out = Vec::new();
    for i in 0..32 {
        let q = p + i * 24;
        let kind = a.u32(q)?;
        if kind == 255 {
            return Ok(out);
        }
        let attr = Attr {
            kind,
            mode: a.u32(q + 4)?,
            count: a.u32(q + 8)?,
            format: a.u32(q + 12)?,
            frac: a.u8(q + 16)?,
            stride: u32::from(a.u16(q + 18)?),
            data: a.ptr(q + 20)?,
        };
        ensure(attr.mode <= 3, "invalid vertex attribute mode")?;
        out.push(attr);
    }
    Err("unterminated vertex descriptor".into())
}
fn components(attr: &Attr) -> usize {
    match attr.kind {
        0..=8 => 1,
        9 => {
            if attr.count == 0 {
                2
            } else {
                3
            }
        }
        10 | 25 => {
            if attr.count == 0 {
                3
            } else {
                9
            }
        }
        11 | 12 => 4,
        _ => {
            if attr.count == 0 {
                1
            } else {
                2
            }
        }
    }
}
fn width(attr: &Attr) -> Result<usize> {
    if attr.kind <= 8 {
        return Ok(1);
    }
    if matches!(attr.kind, 11 | 12) {
        return match attr.format {
            0 | 3 => Ok(2),
            1 | 4 => Ok(3),
            2 | 5 => Ok(4),
            _ => Err("color format".into()),
        };
    }
    Ok(components(attr)
        * match attr.format {
            0 | 1 => 1,
            2 | 3 => 2,
            4 => 4,
            _ => return Err("vertex component format".into()),
        })
}
fn values(attr: &Attr, bytes: &[u8]) -> Result<Vec<f64>> {
    if attr.kind <= 8 {
        return Ok(vec![f64::from(bytes[0])]);
    }
    if matches!(attr.kind, 11 | 12) {
        return Ok(Vec::new());
    }
    let size = match attr.format {
        0 | 1 => 1,
        2 | 3 => 2,
        4 => 4,
        _ => return Err("component format".into()),
    };
    let frac = if matches!(attr.kind, 10 | 25) {
        match attr.format {
            1 => 6,
            3 => 14,
            _ => 0,
        }
    } else {
        attr.frac
    };
    ensure(frac < 32, "vertex fraction")?;
    let out: Vec<f64> = bytes
        .chunks_exact(size)
        .map(|b| match attr.format {
            0 => f64::from(b[0]) / (1u64 << frac) as f64,
            1 => f64::from(b[0] as i8) / (1u64 << frac) as f64,
            2 => f64::from(u16::from_be_bytes([b[0], b[1]])) / (1u64 << frac) as f64,
            3 => f64::from(i16::from_be_bytes([b[0], b[1]])) / (1u64 << frac) as f64,
            _ => f64::from(f32::from_be_bytes(b.try_into().unwrap())),
        })
        .collect();
    ensure(out.iter().all(|v| v.is_finite()), "nonfinite vertex")?;
    Ok(out)
}
fn take<'a>(data: &mut &'a [u8], count: usize) -> Result<&'a [u8]> {
    ensure(data.len() >= count, "truncated display list")?;
    let (v, tail) = data.split_at(count);
    *data = tail;
    Ok(v)
}
fn vertex(
    a: &Archive,
    attributes: &[Attr],
    data: &mut &[u8],
    matrices: &[Matrix],
) -> Result<[f32; 8]> {
    let mut pos = [0.; 3];
    let mut normal = [0., 0., 1.];
    let mut uv = [0.; 2];
    let mut slot = 0;
    for attr in attributes {
        if attr.mode == 0 {
            continue;
        }
        // NBT3 uses three indices; this first converter rejects it explicitly.
        ensure(
            !(matches!(attr.kind, 10 | 25) && attr.count == 2),
            "NBT3 not yet supported",
        )?;
        let n = width(attr)?;
        let bytes = if attr.mode == 1 {
            take(data, n)?
        } else {
            let index = if attr.mode == 2 {
                u32::from(take(data, 1)?[0])
            } else {
                u32::from(u16::from_be_bytes(take(data, 2)?.try_into().unwrap()))
            };
            let offset = attr
                .data
                .ok_or("indexed attribute missing array")?
                .checked_add(index.checked_mul(attr.stride).ok_or("index overflow")?)
                .ok_or("index overflow")?;
            a.bytes(offset, n as u32)?
        };
        let v = values(attr, bytes)?;
        match attr.kind {
            0 => {
                ensure(
                    (v[0] as usize).is_multiple_of(3),
                    "matrix index not divisible by 3",
                )?;
                slot = v[0] as usize / 3;
            }
            9 => {
                for (i, value) in v.iter().take(3).enumerate() {
                    pos[i] = *value;
                }
            }
            10 | 25 => normal.copy_from_slice(&v[..3]),
            13 => {
                uv[0] = v[0];
                uv[1] = *v.get(1).unwrap_or(&0.);
            }
            _ => {}
        }
    }
    let m = *matrices.get(slot).ok_or("matrix index out of bounds")?;
    let p = transform(m, pos, true);
    let n = transform(m, normal, false);
    Ok([
        p[0],
        p[1],
        p[2],
        n[0],
        n[1],
        n[2],
        uv[0] as f32,
        uv[1] as f32,
    ])
}
fn triangles(a: &Archive, p: u32, matrices: &[Matrix]) -> Result<Vec<[f32; 8]>> {
    let attributes = attrs(a, a.non_null_ptr(p + 8)?)?;
    ensure(
        attributes.iter().any(|v| v.kind == 9 && v.mode != 0),
        "missing position attribute",
    )?;
    let mut data = a.bytes(a.non_null_ptr(p + 16)?, u32::from(a.u16(p + 14)?) * 32)?;
    let mut out = Vec::new();
    while !data.is_empty() {
        let command = take(&mut data, 1)?[0];
        if command == 0 {
            continue;
        }
        let primitive = command & 0xf8;
        ensure(
            matches!(primitive, 0x80 | 0x90 | 0x98 | 0xa0),
            &format!("unsupported GX primitive {command:#x}"),
        )?;
        ensure(command & 7 == 0, "nonzero VAT format")?;
        let count = u16::from_be_bytes(take(&mut data, 2)?.try_into().unwrap());
        let vertices = (0..count)
            .map(|_| vertex(a, &attributes, &mut data, matrices))
            .collect::<Result<Vec<_>>>()?;
        let mut emit = |i: usize, j: usize, k: usize| {
            out.extend([vertices[i], vertices[j], vertices[k]]);
        };
        match primitive {
            0x80 => {
                ensure(vertices.len().is_multiple_of(4), "quad count")?;
                for i in (0..vertices.len()).step_by(4) {
                    emit(i, i + 1, i + 2);
                    emit(i, i + 2, i + 3);
                }
            }
            0x90 => {
                ensure(vertices.len().is_multiple_of(3), "triangle count")?;
                out.extend(vertices);
            }
            0x98 => {
                for i in 2..vertices.len() {
                    if i % 2 == 0 {
                        emit(i - 2, i - 1, i)
                    } else {
                        emit(i - 1, i - 2, i)
                    }
                }
            }
            0xa0 => {
                for i in 2..vertices.len() {
                    emit(0, i - 1, i)
                }
            }
            _ => unreachable!(),
        }
        ensure(out.len() < 2_000_000, "mesh vertex limit")?;
    }
    Ok(out)
}
fn matrices(
    a: &Archive,
    p: u32,
    joint: &Joint,
    joints: &BTreeMap<u32, Joint>,
) -> Result<Vec<Matrix>> {
    let mode = a.u16(p + 12)? & 0x3000;
    if mode == 0 {
        let mut m = vec![joint.world];
        if let Some(other) = a.ptr(p + 20)? {
            m.push(joints.get(&other).ok_or("shared joint missing")?.world);
        }
        return Ok(m);
    }
    ensure(mode == 0x2000, "shape animation mesh is unsupported")?;
    let list = a.non_null_ptr(p + 20)?;
    let mut result = Vec::new();
    for slot in 0..10 {
        let Some(envelope) = a.ptr(list + slot * 4)? else {
            break;
        };
        let mut m = [[0.; 4]; 3];
        let mut total = 0.;
        let mut terminated = false;
        for i in 0..64 {
            let Some(j) = a.ptr(envelope + i * 8)? else {
                terminated = true;
                break;
            };
            let j = joints.get(&j).ok_or("envelope joint missing")?;
            let w = f64::from(a.f32(envelope + i * 8 + 4)?);
            ensure(w.is_finite() && (0.0..=1.0).contains(&w), "envelope weight")?;
            // Single-weight envelopes use joint-local coordinates; blended ones
            // use the bind-space inverse, matching SetupEnvelopeModelMtx.
            let jm = if i == 0 && w >= 1.0 - f64::from(f32::EPSILON) {
                j.world
            } else {
                concat(j.world, j.inverse.ok_or("missing inverse bind matrix")?)
            };
            for r in 0..3 {
                for c in 0..4 {
                    m[r][c] += jm[r][c] * w;
                }
            }
            total += w;
        }
        ensure(terminated && (total - 1.).abs() < 0.001, "invalid envelope")?;
        result.push(m);
    }
    ensure(!result.is_empty(), "empty matrix palette")?;
    Ok(result)
}

pub fn load(a: &Archive, roots: &[u32]) -> Result<Scene> {
    let mut scene = Scene {
        meshes: Vec::new(),
        textures: Vec::new(),
        joints: 0,
        warnings: BTreeSet::new(),
    };
    let mut texture_ids = BTreeMap::new();
    for (root_index, root) in roots.iter().enumerate() {
        let mut joints = BTreeMap::new();
        read_joints(a, *root, IDENTITY, [1.; 3], &mut joints, 0)?;
        scene.joints += joints.len();
        for (&joint_offset, joint) in &joints {
            if joint.flags & (1 << 4) != 0 {
                continue;
            }
            let mut next = joint.display;
            let mut seen = BTreeSet::new();
            while let Some(d) = next {
                ensure(seen.insert(d), "display object cycle")?;
                let mut color = [1.; 4];
                let mut tex = None;
                if let Some(m) = a.ptr(d + 8)? {
                    if let Some(material) = a.ptr(m + 12)? {
                        for i in 0..3 {
                            color[i as usize] = f32::from(a.u8(material + 4 + i)?) / 255.;
                        }
                        color[3] = a.f32(material + 12)?.clamp(0., 1.);
                    }
                    if let Some(t) = a.ptr(m + 8)? {
                        if a.ptr(t + 4)?.is_some() {
                            scene
                                .warnings
                                .insert("Additional texture layers/TEV are not reproduced".into());
                        }
                        // Use only a texture sourced from UV0, not reflection coordinates.
                        if a.u32(t + 12)? == 4 {
                            if let Some(image) = a.ptr(t + 76)? {
                                let palette = a.ptr(t + 80)?;
                                let key = (image, palette);
                                let id = if let Some(id) = texture_ids.get(&key) {
                                    *id
                                } else {
                                    let id = scene.textures.len();
                                    scene.textures.push(texture::load(a, image, palette)?);
                                    texture_ids.insert(key, id);
                                    id
                                };
                                tex = Some(id);
                            }
                        }
                    }
                }
                let mut pobj = a.ptr(d + 12)?;
                let mut seen_p = BTreeSet::new();
                while let Some(p) = pobj {
                    ensure(seen_p.insert(p), "polygon object cycle")?;
                    let vertices = triangles(a, p, &matrices(a, p, joint, &joints)?)?;
                    if !vertices.is_empty() {
                        scene.meshes.push(Mesh {
                            vertices,
                            color,
                            texture: tex,
                            joint: joint_offset,
                            root: root_index,
                        });
                    }
                    pobj = a.ptr(p + 4)?;
                }
                next = a.ptr(d + 4)?;
            }
        }
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_and_scale_compensation() {
        let parent = srt([2., 3., 4.], [0.; 3], [1., 2., 3.], [1.; 3]);
        let child = srt(
            [1.; 3],
            [0., 0., std::f64::consts::FRAC_PI_2],
            [2., 0., 0.],
            [2., 3., 4.],
        );
        let p = transform(concat(parent, child), [1., 0., 0.], true);
        assert!((p[0] - 5.).abs() < 0.00001);
        assert!((p[1] - 4.).abs() < 0.00001);
        assert_eq!(p[2], 3.);
    }
    #[test]
    fn checked_display_reads_and_fixed_point() {
        let mut data = &[1u8][..];
        assert!(take(&mut data, 2).is_err());
        let attr = Attr {
            kind: 9,
            mode: 3,
            count: 1,
            format: 3,
            frac: 4,
            stride: 6,
            data: None,
        };
        assert_eq!(
            values(&attr, &[0, 16, 255, 240, 0, 8]).unwrap(),
            [1., -1., 0.5]
        );
    }
}
