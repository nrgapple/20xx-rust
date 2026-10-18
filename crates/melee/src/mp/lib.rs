//! Port of `src/melee/mp/mplib.c` (stage collision geometry).
//!
//! The game keeps this state in file-level globals (`groundCollVtx`,
//! `groundCollLine`, `groundCollJoint`, `mpLib_80458868`, ...); here it is
//! one [`MpLib`] value owned by the simulation. Audited with `dol-inspect`:
//! no fused multiply-adds in the functions below.

use super::{line_flag, line_group, MapColl};
use crate::math::Vec2;

/// `CollVtx`: a vertex's data-file position and its scaled current/previous
/// positions (they differ from `base_pos` once moving platforms animate).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollVtx {
    pub base_pos: Vec2,
    pub pos: Vec2,
    pub prev_pos: Vec2,
}

/// `CollLine`. `inner` indexes [`MapColl::lines`]; `None` is a slot the game
/// never initializes (lines outside every line group).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollLine {
    pub inner: Option<usize>,
    pub flags: u32,
}

/// `CollJoint` (callbacks and `jobj` are not ported yet).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollJoint {
    pub flags: u32,
    pub xc: i16,
    pub xe: bool,
    pub bounding_min: Vec2,
    pub bounding_max: Vec2,
}

/// `CollJoint_Enabled` (`mp/forward.h`).
pub const COLL_JOINT_ENABLED: u32 = 1 << 16;

/// `mpCollisionBox`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MpCollisionBox {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

/// The `mplib.c` globals for a loaded stage.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MpLib {
    /// `mpLib_804D64B4`
    pub coll_data: MapColl,
    /// `groundCollVtx`
    pub vtx: Vec<CollVtx>,
    /// `groundCollLine`
    pub lines: Vec<CollLine>,
    /// `groundCollJoint`; the game links them in order
    /// (`jointListStart`..`jointListEnd`).
    pub joints: Vec<CollJoint>,
    /// `mpLib_80458868[0]`
    pub bounds: MpCollisionBox,
}

/// `mpPruneEmptyLines` (0x8004D184): unlink zero-length lines. The game skips
/// this on Poké Floats (`Gr_Kind_Pura`); pass `skip` for that stage.
pub fn prune_empty_lines(coll: &mut MapColl, skip: bool) {
    if skip {
        return;
    }
    for i in 0..coll.lines.len() {
        let line = coll.lines[i];
        let v0 = coll.verts[line.v0_idx as usize];
        let v1 = coll.verts[line.v1_idx as usize];
        if v0.x != v1.x || v0.y != v1.y {
            continue;
        }
        // The C reads `line->...` through a pointer on every iteration, so
        // updates to this line itself (self links) are seen immediately.
        for j in 0..coll.lines.len() {
            let cur = coll.lines[i];
            let id = i as i16;
            let other = &mut coll.lines[j];
            if other.prev_id0 == id {
                other.prev_id0 = cur.prev_id0;
            }
            if other.next_id0 == id {
                other.next_id0 = cur.next_id0;
            }
            if other.prev_id1 == id {
                other.prev_id1 = cur.prev_id0;
            }
            if other.next_id1 == id {
                other.next_id1 = cur.next_id0;
            }
        }
        let line = &mut coll.lines[i];
        line.hi_flags |= line_flag::EMPTY as u16;
        line.prev_id0 = -1;
        line.next_id0 = -1;
        line.prev_id1 = -1;
        line.next_id1 = -1;
    }
}

impl MpLib {
    /// `mpLibLoad` (0x8004D288), with `scale` from `Ground_801C0498`.
    ///
    /// Not yet ported: `grDynamicAttr_801CA0B4`, island setup
    /// (`mpIsland_8005A728`), and `mpUncheckBounding`.
    pub fn load(mut coll_data: MapColl, scale: f32, skip_prune: bool) -> Self {
        let joints = coll_data
            .joints
            .iter()
            .map(|joint| CollJoint {
                flags: COLL_JOINT_ENABLED,
                xc: 0,
                xe: true,
                bounding_min: Vec2::new(scale * joint.left_bound, scale * joint.bottom_bound),
                bounding_max: Vec2::new(scale * joint.right_bound, scale * joint.top_bound),
            })
            .collect();
        prune_empty_lines(&mut coll_data, skip_prune);

        let mut lines = vec![CollLine::default(); coll_data.lines.len()];
        for group in [
            line_group::FLOOR,
            line_group::CEILING,
            line_group::RIGHT_WALL,
            line_group::LEFT_WALL,
            line_group::DYNAMIC,
        ] {
            let range = coll_data.ranges[group];
            for index in range.start..range.start + range.count {
                let index = index as usize;
                lines[index] = CollLine {
                    inner: Some(index),
                    flags: u32::from(coll_data.lines[index].hi_flags) | line_flag::ENABLED,
                };
            }
        }

        // The game starts each bound at the wrong extreme (top/right at
        // +F32_MAX, bottom/left at -F32_MAX), so these comparisons never
        // update it and the box stays unbounded. Ported as-is.
        let mut bounds = MpCollisionBox {
            top: f32::MAX,
            bottom: -f32::MAX,
            left: -f32::MAX,
            right: f32::MAX,
        };
        let vtx = coll_data
            .verts
            .iter()
            .map(|raw| {
                let (x, y) = (scale * raw.x, scale * raw.y);
                if bounds.top < y {
                    bounds.top = y;
                }
                if bounds.bottom > y {
                    bounds.bottom = y;
                }
                if bounds.right < x {
                    bounds.right = x;
                }
                if bounds.left > x {
                    bounds.left = x;
                }
                CollVtx {
                    base_pos: *raw,
                    pos: Vec2::new(x, y),
                    prev_pos: Vec2::new(x, y),
                }
            })
            .collect();
        Self {
            coll_data,
            vtx,
            lines,
            joints,
            bounds,
        }
    }

    fn inner(&self, line_id: usize) -> &super::data::MapLine {
        let inner = self.lines[line_id].inner.expect("line in a line group");
        &self.coll_data.lines[inner]
    }

    fn connects(&self, line: i16) -> bool {
        let flags = self.lines[line as usize].flags;
        flags & line_flag::ENABLED != 0 && flags & line_flag::HIDDEN == 0
    }

    /// `mpLineGetNext` (0x8004DB78): the line after `line_id`, preferring the
    /// alternate link (`next_id1`) when it is enabled and its start lies
    /// within 2 units of this line's end.
    pub fn line_get_next(&self, line_id: usize) -> i32 {
        let inner = self.inner(line_id);
        let alt = inner.next_id1;
        if alt != -1 && self.connects(alt) {
            let v1 = self.vtx[inner.v1_idx as usize].pos;
            let v0 = self.vtx[self.inner(alt as usize).v0_idx as usize].pos;
            let (dx, dy) = (v1.x - v0.x, v1.y - v0.y);
            if dx * dx + dy * dy < 4.0 {
                return i32::from(alt);
            }
        }
        i32::from(inner.next_id0)
    }

    /// `mpLineGetPrev` (0x8004DC04): mirror of [`Self::line_get_next`].
    pub fn line_get_prev(&self, line_id: usize) -> i32 {
        let inner = self.inner(line_id);
        let alt = inner.prev_id1;
        if alt != -1 && self.connects(alt) {
            let v0 = self.vtx[inner.v0_idx as usize].pos;
            let v1 = self.vtx[self.inner(alt as usize).v1_idx as usize].pos;
            let (dx, dy) = (v0.x - v1.x, v0.y - v1.y);
            if dx * dx + dy * dy < 4.0 {
                return i32::from(alt);
            }
        }
        i32::from(inner.prev_id0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp::data::{MapLine, MapLineRange};

    fn line(v0: u16, v1: u16, prev: i16, next: i16) -> MapLine {
        MapLine {
            v0_idx: v0,
            v1_idx: v1,
            prev_id0: prev,
            next_id0: next,
            prev_id1: -1,
            next_id1: -1,
            hi_flags: line_flag::FLOOR as u16,
            lo_flags: 0,
        }
    }

    /// Floors 0 -> 1 -> 2 where line 1 has zero length.
    fn chain() -> MapColl {
        let mut ranges = [MapLineRange::default(); 5];
        ranges[line_group::FLOOR] = MapLineRange { start: 0, count: 3 };
        MapColl {
            verts: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(20.0, 0.0),
            ],
            lines: vec![line(0, 1, -1, 1), line(1, 2, 0, 2), line(2, 3, 1, -1)],
            ranges,
            joints: Vec::new(),
            x2c: 0,
        }
    }

    #[test]
    fn prune_unlinks_zero_length_lines() {
        let mp = MpLib::load(chain(), 2.0, false);
        let lines = &mp.coll_data.lines;
        assert_eq!((lines[0].next_id0, lines[2].prev_id0), (2, 0));
        assert_eq!(
            lines[1].hi_flags & line_flag::EMPTY as u16,
            line_flag::EMPTY as u16
        );
        assert_eq!([lines[1].prev_id0, lines[1].next_id0], [-1, -1]);
        assert_eq!(mp.line_get_next(0), 2);
        // Scale applies to current positions, not base positions.
        assert_eq!(mp.vtx[3].pos, Vec2::new(40.0, 0.0));
        assert_eq!(mp.vtx[3].base_pos, Vec2::new(20.0, 0.0));
        assert_eq!(mp.lines[0].flags, line_flag::FLOOR | line_flag::ENABLED);
    }

    #[test]
    fn prune_can_be_skipped_and_bounds_stay_unbounded() {
        let mp = MpLib::load(chain(), 1.0, true);
        assert_eq!(mp.coll_data.lines[0].next_id0, 1);
        // mpLibLoad's inverted initial bounds never tighten (see `load`).
        assert_eq!(mp.bounds.top, f32::MAX);
        assert_eq!(mp.bounds.left, -f32::MAX);
    }
}
