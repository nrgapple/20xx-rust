//! Port of `src/melee/mp/mplib.c` (stage collision geometry): loading and
//! line links. Queries are in [`super::query`], segment checks in
//! [`super::check`], and joint (moving-platform) bookkeeping in
//! [`super::joint`].
//!
//! The game keeps this state in file-level globals (`groundCollVtx`,
//! `groundCollLine`, `groundCollJoint`, `jointListStart`, `didCheckBounding`,
//! `mpLib_80458868`, ...); here it is one [`MpLib`] value owned by the
//! simulation. Audited with `dol-inspect`: no fused multiply-adds in the
//! functions below.

use super::data::MapLine;
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

/// `CollJoint`. Its `inner` is always `coll_data.joints[i]` for joint `i`,
/// so it is implicit. `next` links the enabled-joint list that starts at
/// [`MpLib::joint_list_start`]. Not ported yet: `jobj` and the two
/// ground callbacks (`cb_0`/`cb_data_0`, `cb_1`/`cb_data_1`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollJoint {
    pub next: Option<usize>,
    pub flags: u32,
    pub xc: i16,
    pub xe: bool,
    pub bounding_min: Vec2,
    pub bounding_max: Vec2,
}

/// `CollJoint_Enabled` (`mp/forward.h`).
pub const COLL_JOINT_ENABLED: u32 = coll_joint::ENABLED;

/// `enum CollJointFlags` (`mp/forward.h`).
pub mod coll_joint {
    pub const B8: u32 = 1 << 8;
    pub const B9: u32 = 1 << 9;
    pub const B10: u32 = 1 << 10;
    pub const B11: u32 = 1 << 11;
    pub const TOO_FAR: u32 = 1 << 12;
    pub const ENABLED: u32 = 1 << 16;
    pub const HIDDEN: u32 = 1 << 18;
}

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
    /// `groundCollJoint`
    pub joints: Vec<CollJoint>,
    /// `jointListStart`: first joint of the list the segment checks walk.
    pub joint_list_start: Option<usize>,
    /// `jointListEnd`
    pub joint_list_end: Option<usize>,
    /// `didCheckBounding`
    pub did_check_bounding: bool,
    /// `mpLib_80458868[0]`
    pub bounds: MpCollisionBox,
    /// `mpLib_80458868[1]`: the enabled floors' extent, recomputed by
    /// `mpLib_80058614_Floor` ([`MpLib::update_floor_bounds`]). The game
    /// initializes it in `mpLib_80058820` (not ported), not in `mpLibLoad`.
    pub floor_bounds: MpCollisionBox,
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
    /// Joints are enabled and linked in index order, then
    /// `mpUncheckBounding` runs, as in the game. Not yet ported:
    /// `grDynamicAttr_801CA0B4` and island setup (`mpIsland_8005A728`).
    pub fn load(mut coll_data: MapColl, scale: f32, skip_prune: bool) -> Self {
        let joint_count = coll_data.joints.len();
        let joints = coll_data
            .joints
            .iter()
            .enumerate()
            .map(|(i, joint)| CollJoint {
                next: (i + 1 < joint_count).then_some(i + 1),
                flags: coll_joint::ENABLED,
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
        let mut mp = Self {
            coll_data,
            vtx,
            lines,
            joints,
            joint_list_start: (joint_count > 0).then_some(0),
            joint_list_end: joint_count.checked_sub(1),
            did_check_bounding: false,
            bounds,
            floor_bounds: MpCollisionBox::default(),
        };
        mp.uncheck_bounding();
        mp
    }

    /// `groundCollLine[line_id].inner`.
    pub(super) fn inner(&self, line_id: usize) -> &MapLine {
        let inner = self.lines[line_id].inner.expect("line in a line group");
        &self.coll_data.lines[inner]
    }

    pub(super) fn inner_mut(&mut self, line_id: usize) -> &mut MapLine {
        let inner = self.lines[line_id].inner.expect("line in a line group");
        &mut self.coll_data.lines[inner]
    }

    /// The C `LINEID_CHECK` macro: `HSD_ASSERTREPORT` (a halt) when
    /// `line_id` is -1 or at least `line_count`. Returns the index.
    pub(super) fn line_id_check(&self, line_id: i32) -> usize {
        if line_id == -1 || line_id >= self.coll_data.lines.len() as i32 {
            panic!("mplib.c: not found lineID={line_id}");
        }
        line_id as usize
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

    /// Joint indices in list order (`for (joint = jointListStart; joint;
    /// joint = joint->next)`).
    pub fn joint_list(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.joint_list_start, |&joint| self.joints[joint].next)
    }

    /// The lines a segment check visits for one joint: the joint's `group`
    /// range, then its dynamic range. The checks reach the dynamic range
    /// with a `goto` into the loop body that skips the first `i < count`
    /// test, so a nonzero dynamic count always visits at least one line.
    pub(super) fn joint_group_lines(
        &self,
        joint: usize,
        group: usize,
    ) -> impl Iterator<Item = usize> {
        let ranges = self.coll_data.joints[joint].ranges;
        let main = ranges[group];
        let dynamic = ranges[line_group::DYNAMIC];
        let dynamic_count = match dynamic.count {
            0 => 0,
            count => i32::from(count).max(1),
        };
        let start = i32::from(main.start);
        let dynamic_start = i32::from(dynamic.start);
        (0..i32::from(main.count))
            .map(move |i| (start + i) as usize)
            .chain((0..dynamic_count).map(move |i| (dynamic_start + i) as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp::data::{MapJoint, MapLine, MapLineRange};

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

    #[test]
    fn load_links_joints_in_order() {
        let mut coll = chain();
        coll.joints = vec![MapJoint::default(); 3];
        let mp = MpLib::load(coll, 1.0, false);
        assert_eq!(mp.joint_list().collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!((mp.joint_list_start, mp.joint_list_end), (Some(0), Some(2)));
        assert!(mp
            .joints
            .iter()
            .all(|j| j.flags == coll_joint::ENABLED && j.xe));
        assert!(!mp.did_check_bounding);
    }

    #[test]
    fn dynamic_range_visits_at_least_one_line() {
        let mut coll = chain();
        let mut joint = MapJoint::default();
        joint.ranges[line_group::FLOOR] = MapLineRange { start: 0, count: 2 };
        joint.ranges[line_group::DYNAMIC] = MapLineRange {
            start: 2,
            count: -3,
        };
        coll.joints = vec![joint];
        let mp = MpLib::load(coll, 1.0, false);
        let lines: Vec<_> = mp.joint_group_lines(0, line_group::FLOOR).collect();
        assert_eq!(lines, [0, 1, 2]);
    }
}
