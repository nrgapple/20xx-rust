//! Ports from `src/melee/mp` (map collision).

pub mod data;
pub mod lib;

use crate::dat::DatField;
use crate::math::Vec2;
use data::{MapCollData, MapJoint, MapLine, MapLineRange};
use hsd_dat::{Archive, Result};

/// `enum MapLineGroup`: indices into `MapCollData::ranges`.
pub mod line_group {
    pub const FLOOR: usize = 0;
    pub const CEILING: usize = 1;
    pub const RIGHT_WALL: usize = 2;
    pub const LEFT_WALL: usize = 3;
    pub const DYNAMIC: usize = 4;
}

/// Line flags (`mp/forward.h`). `KIND`..`EMPTY`, `ENABLED`, and `HIDDEN` apply
/// to `MapLine::hi_flags` and the runtime `CollLine::flags`; `PLATFORM` and
/// `LEDGE` apply to `MapLine::lo_flags`, which collision queries return.
pub mod line_flag {
    pub const KIND: u32 = 0xf;
    pub const FLOOR: u32 = 1 << 0;
    pub const CEILING: u32 = 1 << 1;
    pub const RIGHT_WALL: u32 = 1 << 2;
    pub const LEFT_WALL: u32 = 1 << 3;
    pub const EMPTY: u32 = 1 << 7;
    pub const PLATFORM: u32 = 1 << 8;
    pub const LEDGE: u32 = 1 << 9;
    pub const ENABLED: u32 = 1 << 16;
    pub const HIDDEN: u32 = 1 << 18;
}

/// A stage's `MapCollData` with its arrays copied out of the archive. The
/// game edits these in place (`mpPruneEmptyLines`), so they are owned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MapColl {
    pub verts: Vec<Vec2>,
    pub lines: Vec<MapLine>,
    pub ranges: [MapLineRange; 5],
    pub joints: Vec<MapJoint>,
    pub x2c: i32,
}

fn read_array<T: DatField>(
    archive: &Archive,
    ptr: crate::dat::DatPtr,
    count: i32,
) -> Result<Vec<T>> {
    let count = u32::try_from(count).map_err(|_| hsd_dat::Error::OutOfBounds {
        offset: ptr.0.unwrap_or(0),
        len: 0,
    })?;
    match ptr.0 {
        None if count == 0 => Ok(Vec::new()),
        None => Err(hsd_dat::Error::NullPointer(0)),
        Some(start) => (0..count)
            .map(|i| T::read(archive, start + i * T::SIZE))
            .collect(),
    }
}

/// Load the stage archive's `coll_data` public symbol
/// (`stage_info.coll_data`, set in `grDatFiles_801C6038`).
pub fn load_map_coll(archive: &Archive) -> Result<MapColl> {
    let raw = MapCollData::read(archive, archive.public("coll_data")?)?;
    Ok(MapColl {
        verts: read_array(archive, raw.verts, raw.vert_count)?,
        lines: read_array(archive, raw.lines, raw.line_count)?,
        ranges: raw.ranges,
        joints: read_array(archive, raw.joints, raw.joint_count)?,
        x2c: raw.x2c,
    })
}
