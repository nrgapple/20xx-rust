//! Euler-joint subset of HSD_JObjMakeMatrix 0x8036F1F8 and
//! JObjUpdateFunc 0x8036FDC0. Audited before porting. Matrix concatenation uses
//! the ported SDK paired-single operation sequence. IK, constraints, quaternion
//! joints and object/path callbacks are deliberately not implemented here.
use super::{aobj::Animation, mtx::srt};
use crate::{math::Mtx, sdk::mtx::ps_mtx_concat};
#[derive(Debug, Clone)]
pub struct Joint {
    pub parent: Option<usize>,
    pub flags: u32,
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub inverse: Mtx,
}
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
    pub animations: Vec<Option<Animation>>,
    pub world: Vec<Mtx>,
    pub palette: Vec<f32>,
}
impl Skeleton {
    pub fn new(joints: Vec<Joint>) -> Result<Self, String> {
        if joints.is_empty() || joints.len() > 256 {
            return Err("invalid joint count".into());
        }
        for (i, j) in joints.iter().enumerate() {
            if j.parent.is_some_and(|p| p >= i) {
                return Err("joint parents must precede children".into());
            }
            if j.flags & (1 << 17) != 0 {
                return Err("quaternion joint unsupported".into());
            }
        }
        let n = joints.len();
        let mut s = Self {
            joints,
            animations: vec![None; n],
            world: vec![[[0.; 4]; 3]; n],
            palette: vec![0.; n * 24],
        };
        s.update_matrices();
        Ok(s)
    }
    /// Scalar transform channels of JObjUpdateFunc. Scale clamp intentionally
    /// turns tiny negative scales into positive 0.001, exactly as the original.
    pub fn apply(&mut self, index: usize, channel: u8, value: f32) -> Result<(), String> {
        let j = &mut self.joints[index];
        match channel {
            1..=3 => j.rotation[usize::from(channel - 1)] = value,
            5..=7 => j.translation[usize::from(channel - 5)] = value,
            8..=10 => {
                j.scale[usize::from(channel - 8)] = if value.abs() < 0.001 { 0.001 } else { value }
            }
            11 => {
                if value > 0.5 {
                    j.flags &= !16;
                } else {
                    j.flags |= 16;
                }
            }
            12 => {
                let visible = value > 0.5;
                let hidden = !visible;
                if hidden {
                    j.flags |= 16;
                } else {
                    j.flags &= !16;
                }
                for k in index + 1..self.joints.len() {
                    let mut parent = self.joints[k].parent;
                    while let Some(p) = parent {
                        if p == index {
                            if hidden {
                                self.joints[k].flags |= 16;
                            } else {
                                self.joints[k].flags &= !16;
                            }
                            break;
                        }
                        parent = self.joints[p].parent;
                    }
                }
            }
            _ => return Err(format!("unsupported joint animation channel {channel}")),
        }
        Ok(())
    }
    pub fn advance(&mut self) -> Result<(), String> {
        for i in 0..self.joints.len() {
            let values = match &mut self.animations[i] {
                Some(a) => a.advance().map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            for (channel, value) in values {
                self.apply(i, channel, value)?;
            }
        }
        self.update_matrices();
        Ok(())
    }
    pub fn update_matrices(&mut self) {
        let mut scales: Vec<Option<[f32; 3]>> = vec![None; self.joints.len()];
        for (i, j) in self.joints.iter().enumerate() {
            let inherited = j.parent.and_then(|p| scales[p]);
            scales[i] = if j.flags & 8 != 0 {
                inherited
            } else {
                Some(match inherited {
                    Some(p) => std::array::from_fn(|k| j.scale[k] * p[k]),
                    None => j.scale,
                })
            };
            let local = srt(j.scale, j.rotation, j.translation, inherited);
            let world = match j.parent {
                Some(p) => ps_mtx_concat(&self.world[p], &local),
                None => local,
            };
            self.world[i] = world;
            let envelope = ps_mtx_concat(&world, &j.inverse);
            self.palette[i * 24..i * 24 + 12].copy_from_slice(world.as_flattened());
            self.palette[i * 24 + 12..i * 24 + 24].copy_from_slice(envelope.as_flattened());
        }
    }
}
