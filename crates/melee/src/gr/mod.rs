//! Ports from `src/melee/gr` (grounds/stages).

pub mod data;

use crate::dat::DatField;
use data::GroundParam;
use hsd_dat::{Archive, Result};

/// The stage archive's `grGroundParam` (`stage_info.param`, set in
/// `grDatFiles_801C6038`).
pub fn load_ground_param(archive: &Archive) -> Result<GroundParam> {
    GroundParam::read(archive, archive.public("grGroundParam")?)
}

/// `Ground_801C0498` (0x801C0498): the stage's collision scale, `param->y`,
/// or 1.0 without params.
pub fn ground_scale(param: Option<&GroundParam>) -> f32 {
    param.map_or(1.0, |param| param.y)
}
