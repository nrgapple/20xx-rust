//! Joint (moving-platform group) bookkeeping from `mplib.c` that needs no
//! HSD joint objects: enabling, hiding, linking joints to each other, and
//! reclassifying dynamic lines.
//!
//! Several of these end by calling `mpIsland_8005B334` (`mpisland.c`, not
//! ported yet). They return that call's arguments as an [`IslandUpdate`],
//! which the caller must apply once islands exist. The island update reads
//! lines and vertices, which are final when these functions return, and
//! does not read the `xE` flag they set afterwards, so applying it after
//! the return is equivalent.
//!
//! Audit (`dol-inspect`, 1.02): no fused multiply-adds in any function
//! here; floating-point details are noted per function.

// `!(a < b)` forms mirror the C source; they differ from `a >= b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::lib::{coll_joint, MpLib};
use super::{line_flag, line_group};

/// The arguments of a pending `mpIsland_8005B334(joint_id, vtx_start,
/// vtx_count, flag)` call.
#[must_use = "apply with mpIsland_8005B334 once islands are ported"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IslandUpdate {
    pub joint_id: i32,
    pub vtx_start: i32,
    pub vtx_count: i32,
    pub flag: bool,
}

/// `TAN60` in `mpJointUpdateDynamics`; the 1.02 constant is
/// `0x3FFBB67AF037935A`.
const TAN60: f64 = 1.7320508368950045;
/// `TAN30`; the 1.02 constant is `0x3FE279A753D75617`.
const TAN30: f64 = 0.577350295784245;

/// The line groups in the order the joint functions visit them.
const GROUPS: [usize; 5] = [
    line_group::FLOOR,
    line_group::CEILING,
    line_group::LEFT_WALL,
    line_group::RIGHT_WALL,
    line_group::DYNAMIC,
];

impl MpLib {
    /// Lines of one group of joint `joint` (`for (count = ...; count > 0;
    /// count--)`; unlike the segment checks, no `goto` into the body).
    fn joint_range(&self, joint: usize, group: usize) -> std::ops::Range<usize> {
        let range = self.coll_data.joints[joint].ranges[group];
        let start = i32::from(range.start);
        let count = i32::from(range.count).max(0);
        start as usize..(start + count) as usize
    }

    fn joint_vertices(&self, joint: usize) -> std::ops::Range<usize> {
        let data = &self.coll_data.joints[joint];
        let start = i32::from(data.vtx_start);
        let count = i32::from(data.vtx_count).max(0);
        start as usize..(start + count) as usize
    }

    fn set_joint_lines(&mut self, joint: usize, set: u32, clear: u32) {
        for group in GROUPS {
            for line in self.joint_range(joint, group) {
                self.lines[line].flags = (self.lines[line].flags | set) & !clear;
            }
        }
    }

    fn island_update(&self, joint: usize, flag: bool) -> IslandUpdate {
        let data = &self.coll_data.joints[joint];
        IslandUpdate {
            joint_id: joint as i32,
            vtx_start: i32::from(data.vtx_start),
            vtx_count: i32::from(data.vtx_count),
            flag,
        }
    }

    /// The `flag` most island updates pass: visible, not `B11`, enabled.
    fn joint_island_flag(&self, joint: usize) -> bool {
        let flags = self.joints[joint].flags;
        flags & (coll_joint::HIDDEN | coll_joint::B11) == 0 && flags & coll_joint::ENABLED != 0
    }

    /// `mpJointHide` (0x8005541C): hide the joint and all its lines.
    pub fn joint_hide(&mut self, joint_id: i32) {
        let joint = joint_id as usize;
        self.joints[joint].flags |= coll_joint::HIDDEN;
        self.set_joint_lines(joint, line_flag::HIDDEN, 0);
    }

    /// `mpJointUnhide` (0x800557D0): show the joint and its lines, and reset
    /// its vertices' previous positions to the current ones.
    pub fn joint_unhide(&mut self, joint_id: i32) {
        let joint = joint_id as usize;
        self.joints[joint].flags &= !coll_joint::HIDDEN;
        self.set_joint_lines(joint, 0, line_flag::HIDDEN);
        for vtx in self.joint_vertices(joint) {
            self.vtx[vtx].prev_pos = self.vtx[vtx].pos;
        }
    }

    /// `mpJointUpdateDynamics` (0x80055C5C): reclassify the joint's dynamic
    /// lines by slope. Rising left to right: floor up to 60°, left wall
    /// beyond. Running right to left: ceiling up to 30°, walls beyond. On
    /// enabled joints, lines with `lo_flags & 0x400` become pass-through
    /// platforms while they are floors and are disabled otherwise.
    ///
    /// Audited: the slope is a single `fdivs` compared with the `double`
    /// constants [`TAN60`]/[`TAN30`]. A zero-length line is an assertion
    /// failure in the game.
    pub fn joint_update_dynamics(&mut self, joint_id: i32) {
        let joint = joint_id as usize;
        for line in self.joint_range(joint, line_group::DYNAMIC) {
            let inner = *self.inner(line);
            let v1 = self.vtx[inner.v1_idx as usize].pos;
            let v0 = self.vtx[inner.v0_idx as usize].pos;
            let dx = v1.x - v0.x;
            let dy = v1.y - v0.y;
            let kind = if dx > 0.0 {
                let slope = f64::from(dy / dx);
                if slope > TAN60 {
                    line_flag::LEFT_WALL
                } else if slope < -TAN60 {
                    line_flag::RIGHT_WALL
                } else {
                    line_flag::FLOOR
                }
            } else if dx < 0.0 {
                let slope = f64::from(dy / dx);
                if slope > TAN30 {
                    line_flag::RIGHT_WALL
                } else if slope < -TAN30 {
                    line_flag::LEFT_WALL
                } else {
                    line_flag::CEILING
                }
            } else if dy > 0.0 {
                line_flag::LEFT_WALL
            } else if dy < 0.0 {
                line_flag::RIGHT_WALL
            } else {
                panic!("mplib.c:4884: zero-length dynamic line {line}");
            };
            let flags = &mut self.lines[line].flags;
            *flags = (*flags & !line_flag::KIND) | kind;
            if self.joints[joint].flags & coll_joint::ENABLED != 0 && inner.lo_flags & 0x400 != 0 {
                if kind & line_flag::FLOOR != 0 {
                    self.lines[line].flags |= line_flag::ENABLED | line_flag::PLATFORM;
                    self.inner_mut(line).lo_flags |= line_flag::PLATFORM as u16;
                } else {
                    self.lines[line].flags &= !line_flag::ENABLED;
                }
            }
        }
    }

    /// `mpLib_80055E24` (0x80055E24): [`Self::joint_update_dynamics`], then
    /// the island update.
    pub fn joint_update_dynamics_islands(&mut self, joint_id: i32) -> IslandUpdate {
        self.joint_update_dynamics(joint_id);
        let joint = joint_id as usize;
        self.island_update(joint, self.joint_island_flag(joint))
    }

    /// `mpJointUpdateBounding` (0x800565DC): grow the joint's bounding box
    /// to hold each of its vertices with a 30-unit margin (single-precision
    /// `pos ± 30.0F`).
    pub fn joint_update_bounding(&mut self, joint_id: i32) {
        let joint = joint_id as usize;
        for vtx in self.joint_vertices(joint) {
            let pos = self.vtx[vtx].pos;
            let j = &mut self.joints[joint];
            if j.bounding_min.x > pos.x - 30.0 {
                j.bounding_min.x = pos.x - 30.0;
            }
            if j.bounding_max.x < pos.x + 30.0 {
                j.bounding_max.x = pos.x + 30.0;
            }
            if j.bounding_min.y > pos.y - 30.0 {
                j.bounding_min.y = pos.y - 30.0;
            }
            if j.bounding_max.y < pos.y + 30.0 {
                j.bounding_max.y = pos.y + 30.0;
            }
        }
    }

    /// `mpLib_8005667C` (0x8005667C): only the island update.
    pub fn joint_islands(&self, joint_id: i32) -> IslandUpdate {
        let joint = joint_id as usize;
        self.island_update(joint, self.joint_island_flag(joint))
    }

    /// `mpLib_80057424` (0x80057424): reset the joint's vertices' previous
    /// positions to the current ones.
    pub fn joint_save_prev_pos(&mut self, joint_id: i32) {
        for vtx in self.joint_vertices(joint_id as usize) {
            self.vtx[vtx].prev_pos = self.vtx[vtx].pos;
        }
    }

    /// `mpLib_80057528` (0x80057528): enable one line and update its joint's
    /// islands. `None` if no joint holds the line.
    pub fn line_enable(&mut self, line_id: i32) -> Option<IslandUpdate> {
        self.line_set_enabled(line_id, true)
    }

    /// `mpLib_800575B0` (0x800575B0): disable one line; see
    /// [`Self::line_enable`].
    pub fn line_disable(&mut self, line_id: i32) -> Option<IslandUpdate> {
        self.line_set_enabled(line_id, false)
    }

    fn line_set_enabled(&mut self, line_id: i32, enabled: bool) -> Option<IslandUpdate> {
        let joint_id = self.joint_from_line(line_id);
        if joint_id == -1 {
            return None;
        }
        let joint = joint_id as usize;
        let flags = &mut self.lines[line_id as usize].flags;
        if enabled {
            *flags |= line_flag::ENABLED;
        } else {
            *flags &= !line_flag::ENABLED;
        }
        let update = self.island_update(joint, self.joints[joint].flags & coll_joint::B11 == 0);
        self.joints[joint].xe = true;
        Some(update)
    }

    /// `mpJointListAdd` (0x80057638): enable a joint, append it to the joint
    /// list, enable its lines, and reset its vertices' previous positions.
    /// `None` if it was already enabled (the C returns early).
    pub fn joint_list_add(&mut self, joint_id: i32) -> Option<IslandUpdate> {
        let joint = joint_id as usize;
        if self.joints[joint].flags & coll_joint::ENABLED != 0 {
            return None;
        }
        self.joints[joint].flags |= coll_joint::ENABLED;
        if self.joint_list_start.is_none() {
            self.joint_list_start = Some(joint);
        } else {
            let end = self.joint_list_end.expect("joint list end");
            self.joints[end].next = Some(joint);
        }
        self.joint_list_end = Some(joint);
        self.joints[joint].next = None;
        self.set_joint_lines(joint, line_flag::ENABLED, 0);
        self.joint_save_prev_pos(joint_id);
        let update = self.island_update(joint, self.joints[joint].flags & coll_joint::B11 == 0);
        self.joints[joint].xe = true;
        Some(update)
    }

    /// `mpJointListUnlink` (0x80057B4C): remove a joint from the list. The
    /// removed joint keeps its `next` link, as in the C.
    pub fn joint_list_unlink(&mut self, joint: usize) {
        if self.joint_list_start == Some(joint) {
            if self.joint_list_end == Some(joint) {
                self.joint_list_end = None;
                self.joint_list_start = None;
            } else {
                self.joint_list_start = self.joints[joint].next;
            }
            return;
        }
        let mut cur = self.joint_list_start;
        while let Some(index) = cur {
            if self.joints[index].next == Some(joint) {
                if self.joint_list_end == Some(joint) {
                    self.joint_list_end = Some(index);
                }
                self.joints[index].next = self.joints[joint].next;
                return;
            }
            cur = self.joints[index].next;
        }
    }

    /// `mpLib_80057BC0` (0x80057BC0): disable a joint, unlink it, and
    /// disable its lines. `None` if it was already disabled.
    pub fn joint_list_remove(&mut self, joint_id: i32) -> Option<IslandUpdate> {
        let joint = joint_id as usize;
        if self.joints[joint].flags & coll_joint::ENABLED == 0 {
            return None;
        }
        self.joints[joint].flags &= !coll_joint::ENABLED;
        self.joint_list_unlink(joint);
        self.set_joint_lines(joint, 0, line_flag::ENABLED);
        let update = self.island_update(joint, false);
        self.joints[joint].xe = true;
        Some(update)
    }

    /// `mpLib_80057FDC` (0x80057FDC): clear `B11` and update islands.
    pub fn joint_clear_b11(&mut self, joint_id: i32) -> IslandUpdate {
        let joint = joint_id as usize;
        self.joints[joint].flags &= !coll_joint::B11;
        self.island_update(joint, self.joint_island_flag(joint))
    }

    /// `mpLib_80058044` (0x80058044): set `B11` and update islands (the
    /// island flag is then always false).
    pub fn joint_set_b11(&mut self, joint_id: i32) -> IslandUpdate {
        let joint = joint_id as usize;
        self.joints[joint].flags |= coll_joint::B11;
        self.island_update(joint, self.joint_island_flag(joint))
    }

    /// `mpJointSetB10` (0x800580AC).
    pub fn joint_set_b10(&mut self, joint_id: i32) {
        self.joints[joint_id as usize].flags |= coll_joint::B10;
    }

    /// `mpLib_800581DC` (0x800581DC): link two joints' lines where their
    /// vertices meet. First, every alternate link (`prev_id1`/`next_id1`)
    /// of either joint that is not reciprocated is cleared. Then for each
    /// pair of vertices within 2 units on both axes (`ABS(d) < 2.0`,
    /// compared in double), lines of the first joint ending or starting
    /// there get alternate links to lines of the second joint starting or
    /// ending at the other vertex, in both directions.
    pub fn joint_connect(&mut self, joint_id0: i32, joint_id1: i32) {
        let (j0, j1) = (joint_id0 as usize, joint_id1 as usize);
        for group in 0..5 {
            for joint in [j0, j1] {
                for idx in self.joint_range(joint, group) {
                    let prev = self.inner(idx).prev_id1;
                    if prev != -1 {
                        let back = self.inner(prev as usize).next_id1;
                        if back != -1 && idx as i32 != i32::from(back) {
                            self.inner_mut(idx).prev_id1 = -1;
                        }
                    }
                    let next = self.inner(idx).next_id1;
                    if next != -1 {
                        let back = self.inner(next as usize).prev_id1;
                        if back != -1 && idx as i32 != i32::from(back) {
                            self.inner_mut(idx).next_id1 = -1;
                        }
                    }
                }
            }
        }

        for vstart0 in self.joint_vertices(j0) {
            let vtx0 = self.vtx[vstart0].pos;
            for vid in self.joint_vertices(j1) {
                let v1 = self.vtx[vid].pos;
                if !(f64::from(crate::math::abs_c(vtx0.x - v1.x)) < 2.0)
                    || !(f64::from(crate::math::abs_c(vtx0.y - v1.y)) < 2.0)
                {
                    continue;
                }
                for group in 0..5 {
                    for line0 in self.joint_range(j0, group) {
                        let l0 = *self.inner(line0);
                        if vstart0 == usize::from(l0.v0_idx) {
                            for g1 in 0..5 {
                                for line1 in self.joint_range(j1, g1) {
                                    if vid == usize::from(self.inner(line1).v1_idx) {
                                        self.inner_mut(line0).prev_id1 = line1 as i16;
                                        self.inner_mut(line1).next_id1 = line0 as i16;
                                    }
                                }
                            }
                        } else if vstart0 == usize::from(l0.v1_idx) {
                            for g1 in 0..5 {
                                for line1 in self.joint_range(j1, g1) {
                                    if vid == usize::from(self.inner(line1).v0_idx) {
                                        self.inner_mut(line0).next_id1 = line1 as i16;
                                        self.inner_mut(line1).prev_id1 = line0 as i16;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// `mpLib_80058560` (0x80058560): [`Self::joint_connect`] every pair of
    /// enabled, visible joints, in index order.
    pub fn joint_connect_all(&mut self) {
        let count = self.coll_data.joints.len();
        let usable = |mp: &Self, joint: usize| {
            let flags = mp.joints[joint].flags;
            flags & coll_joint::ENABLED != 0 && flags & coll_joint::HIDDEN == 0
        };
        for i in 0..count.saturating_sub(1) {
            for j in i + 1..count {
                if usable(self, i) && usable(self, j) {
                    self.joint_connect(i as i32, j as i32);
                }
            }
        }
    }

    /// `mpLib_80058614_Floor` (0x80058614): if any joint is marked changed
    /// (`xE`), clear every joint's mark and recompute
    /// [`MpLib::floor_bounds`] from the enabled floors of enabled, visible
    /// joints (empty lines included; this pass does not test `EMPTY`).
    pub fn update_floor_bounds(&mut self) {
        if !self.joints.iter().any(|joint| joint.xe) {
            return;
        }
        let mut bounds = super::lib::MpCollisionBox {
            right: -f32::MAX,
            top: -f32::MAX,
            left: f32::MAX,
            bottom: f32::MAX,
        };
        for joint in 0..self.joints.len() {
            self.joints[joint].xe = false;
            let flags = self.joints[joint].flags;
            if flags & coll_joint::ENABLED == 0 || flags & coll_joint::HIDDEN != 0 {
                continue;
            }
            for line in self.joint_group_lines(joint, line_group::FLOOR) {
                let flags = self.lines[line].flags;
                if flags & line_flag::FLOOR == 0 || flags & line_flag::ENABLED == 0 {
                    continue;
                }
                let inner = self.inner(line);
                let v0 = self.vtx[inner.v0_idx as usize].pos;
                let v1 = self.vtx[inner.v1_idx as usize].pos;
                for v in [v0, v1] {
                    if bounds.top < v.y {
                        bounds.top = v.y;
                    }
                    if bounds.bottom > v.y {
                        bounds.bottom = v.y;
                    }
                    if bounds.right < v.x {
                        bounds.right = v.x;
                    }
                    if bounds.left > v.x {
                        bounds.left = v.x;
                    }
                }
            }
        }
        self.floor_bounds = bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slope_constants_match_the_dol() {
        assert_eq!(TAN60.to_bits(), 0x3FFB_B67A_F037_935A);
        assert_eq!(TAN30.to_bits(), 0x3FE2_79A7_53D7_5617);
    }
}
