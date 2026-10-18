//! Port of `struct Fighter` (`src/melee/ft/types.h`), grown field by field as
//! functions that use them are ported. Field names are the decomp's,
//! lowercased; each documents its original `fp+` offset.

use super::data::FighterAttributes;
use crate::math::{Vec2, Vec3};

/// `GroundOrAir` (`ft/forward.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroundOrAir {
    #[default]
    Ground = 0,
    Air = 1,
}

/// `struct Fighter_input` at `fp+620`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FighterInput {
    /// `fp+620`: control stick, current frame first.
    pub lstick: [Vec2; 3],
}

/// `SurfaceData` (`lb/types.h`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SurfaceData {
    pub index: i32,
    pub flags: u32,
    pub normal: Vec3,
}

/// `CollData` at `fp+6F0` (`lb/types.h`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollData {
    /// `fp+83C`
    pub floor: SurfaceData,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fighter {
    /// `fp+74`
    pub x74_self_accel: Vec3,
    /// `fp+80`
    pub self_vel: Vec3,
    /// `fp+8C`
    pub x8c_kb_vel: Vec3,
    /// `fp+E0`
    pub ground_or_air: GroundOrAir,
    /// `fp+E4`
    pub xe4_ground_accel_1: f32,
    /// `fp+EC`
    pub gr_vel: f32,
    /// `fp+F0`
    pub xf0_ground_kb_vel: f32,
    /// `fp+F4`
    pub xf4_ground_attacker_shield_kb_vel: f32,
    /// `fp+110`: this fighter's copy of its `ftCo_DatAttrs`.
    pub co_attrs: FighterAttributes,
    /// `fp+620`
    pub input: FighterInput,
    /// `fp+6F0`
    pub coll_data: CollData,
}
