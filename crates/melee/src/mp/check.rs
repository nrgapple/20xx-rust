//! Segment-versus-stage checks from `mplib.c`: the joint bounding pre-pass
//! and the floor/ceiling/wall checks (`mpCheckFloor` and relatives) that
//! environment collision (`mpcoll.c`) and fighter code call.
//!
//! Each check walks the enabled-joint list, skips joints the bounding
//! pre-pass marked too far away, visits the joint's lines of one kind plus
//! its dynamic lines, and keeps the hit closest to the segment start
//! (strictly closer replaces, so the first of equal hits wins). The
//! bounding pre-pass runs only if no caller has run it already, and is
//! undone at the end in that case.
//!
//! Audit (`dol-inspect`, 1.02): the plain checks have no fused ops (the
//! squared distances are `fmuls`/`fadds`). The `*Remap` checks each fuse
//! one op, the direction test `dx * ex + dy * ey` (`fmadds` on the left
//! product), found twice per function (sloped and flat branches).
//! `ABS(d) > 0.0001` compares a single difference with a `double` literal.

use super::geom::{
    exceeds_epsilon, line_intersection, line_intersection_h, line_intersection_v, remap_2d,
    sq_sum_fused,
};
use super::lib::{coll_joint, MpLib};
use super::psvec::ps_vec_normalize;
use super::{line_flag, line_group};
use crate::math::{abs_c, Vec2, Vec3};
use crate::ppc;

/// The outputs of a segment check (`vec_out`, `line_id_out`, `flags_out`,
/// `normal_out` in the C), all from the same hit.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollHit {
    pub pos: Vec3,
    pub line_id: i32,
    /// The line's `lo_flags`.
    pub flags: u32,
    pub normal: Vec3,
}

/// The fighter callback `mpCheckFloor` takes (`bool (*)(Fighter_GObj*,
/// int)`): given the stage and a line id, whether to consider the line.
/// The fighter is captured by the closure.
pub type FloorFilter<'a> = &'a mut dyn FnMut(&MpLib, i32) -> bool;

/// Which line group and kind bit a check covers.
#[derive(Clone, Copy)]
enum Surface {
    Floor,
    Ceiling,
    LeftWall,
    RightWall,
}

impl Surface {
    fn group(self) -> usize {
        match self {
            Surface::Floor => line_group::FLOOR,
            Surface::Ceiling => line_group::CEILING,
            Surface::LeftWall => line_group::LEFT_WALL,
            Surface::RightWall => line_group::RIGHT_WALL,
        }
    }

    fn kind(self) -> u32 {
        match self {
            Surface::Floor => line_flag::FLOOR,
            Surface::Ceiling => line_flag::CEILING,
            Surface::LeftWall => line_flag::LEFT_WALL,
            Surface::RightWall => line_flag::RIGHT_WALL,
        }
    }
}

fn vec3(p: Vec2) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0)
}

impl MpLib {
    /// `mpCheckedBounding` (0x800588C8).
    pub fn checked_bounding(&self) -> bool {
        self.did_check_bounding
    }

    /// `mpBoundingCheck` (0x800588D0): mark each listed joint `TOO_FAR`
    /// unless it is enabled, visible, and its bounding box overlaps the
    /// given box (`B10` joints are never too far).
    pub fn bounding_check(&mut self, left: f32, bottom: f32, right: f32, top: f32) {
        let mut cur = self.joint_list_start;
        while let Some(index) = cur {
            let joint = &mut self.joints[index];
            if joint.flags & coll_joint::ENABLED != 0 && joint.flags & coll_joint::HIDDEN == 0 {
                if joint.flags & coll_joint::B10 != 0 {
                    joint.flags &= !coll_joint::TOO_FAR;
                } else if left > joint.bounding_max.x
                    || right < joint.bounding_min.x
                    || bottom > joint.bounding_max.y
                    || top < joint.bounding_min.y
                {
                    joint.flags |= coll_joint::TOO_FAR;
                } else {
                    joint.flags &= !coll_joint::TOO_FAR;
                }
            } else {
                joint.flags |= coll_joint::TOO_FAR;
            }
            cur = joint.next;
        }
        self.did_check_bounding = true;
    }

    /// `mpBoundingCheck2` (0x80058970): [`Self::bounding_check`] on the box
    /// spanned by two points.
    pub fn bounding_check2(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        let (left, right) = if x1 > x2 { (x2, x1) } else { (x1, x2) };
        let (bottom, top) = if y1 > y2 { (y2, y1) } else { (y1, y2) };
        self.bounding_check(left, bottom, right, top);
    }

    /// `mpBoundingCheck3` (0x800589D0): [`Self::bounding_check`] on the box
    /// spanned by four points (the third and fourth only extend one side
    /// each per axis, as the C's `if`/`else if` does).
    #[allow(clippy::too_many_arguments)]
    pub fn bounding_check3(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    ) {
        let mut left = x1;
        let mut bottom = y1;
        let mut right;
        let mut top;
        if x0 > left {
            right = x0;
        } else {
            right = left;
            left = x0;
        }
        if y0 > bottom {
            top = y0;
        } else {
            top = bottom;
            bottom = y0;
        }
        if right < x2 {
            right = x2;
        } else if left > x2 {
            left = x2;
        }
        if top < y2 {
            top = y2;
        } else if bottom > y2 {
            bottom = y2;
        }
        if right < x3 {
            right = x3;
        } else if left > x3 {
            left = x3;
        }
        if top < y3 {
            top = y3;
        } else if bottom > y3 {
            bottom = y3;
        }
        self.bounding_check(left, bottom, right, top);
    }

    /// `mpUncheckBounding` (0x80058AA0): clear `TOO_FAR` on listed joints.
    pub fn uncheck_bounding(&mut self) {
        let mut cur = self.joint_list_start;
        while let Some(index) = cur {
            let joint = &mut self.joints[index];
            joint.flags &= !coll_joint::TOO_FAR;
            cur = joint.next;
        }
        self.did_check_bounding = false;
    }

    /// Run `body` between the checks' usual bounding prologue/epilogue.
    fn with_bounding<T>(
        &mut self,
        bounding: impl FnOnce(&mut Self),
        body: impl FnOnce(&Self) -> T,
    ) -> T {
        let already_checked = self.checked_bounding();
        if !already_checked {
            bounding(self);
        }
        let result = body(self);
        if !already_checked {
            self.uncheck_bounding();
        }
        result
    }

    /// The joints a check visits: listed, not `TOO_FAR`, not
    /// `joint_id_skip`, and `joint_id_only` if that is not -1.
    fn checked_joints(
        &self,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> impl Iterator<Item = usize> + '_ {
        self.joint_list().filter(move |&joint| {
            let id = joint as i32;
            self.joints[joint].flags & coll_joint::TOO_FAR == 0
                && joint_id_skip != id
                && (joint_id_only == -1 || joint_id_only == id)
        })
    }

    /// The line passes the checks' flag test: of `kind`, enabled, not empty.
    fn line_usable(&self, line: usize, kind: u32) -> bool {
        let flags = self.lines[line].flags;
        flags & kind != 0 && flags & line_flag::ENABLED != 0 && flags & line_flag::EMPTY == 0
    }

    fn lo_flags(&self, line: usize) -> u32 {
        u32::from(self.inner(line).lo_flags)
    }

    /// Body shared by `mpCheckFloor`, `mpCheckCeiling`, `mpCheckLeftWall`
    /// and `mpCheckRightWall`, which differ only in the parameters below.
    #[allow(clippy::too_many_arguments)]
    fn check_lines(
        &self,
        surface: Surface,
        a: Vec2,
        b: Vec2,
        y_offset: f32,
        line_id_skip: i32,
        joint_id_skip: i32,
        joint_id_only: i32,
        mut cb: Option<FloorFilter>,
    ) -> Option<CollHit> {
        let mut min_dist2 = f32::MAX;
        let mut hit = None;
        for joint in self.checked_joints(joint_id_skip, joint_id_only) {
            for line in self.joint_group_lines(joint, surface.group()) {
                let id = line as i32;
                if let Some(cb) = cb.as_mut() {
                    if !cb(self, id) {
                        continue;
                    }
                }
                if line_id_skip == id || !self.line_usable(line, surface.kind()) {
                    continue;
                }
                let (x0, mut y0, x1, mut y1) = match surface {
                    Surface::Floor | Surface::Ceiling => self.line_extended(id),
                    Surface::LeftWall | Surface::RightWall => {
                        let line = self.inner(line);
                        let v0 = self.vtx[line.v0_idx as usize].pos;
                        let v1 = self.vtx[line.v1_idx as usize].pos;
                        (v0.x, v0.y, v1.x, v1.y)
                    }
                };
                // Only `mpCheckFloor` has `y_offset`; adding 0.0 elsewhere
                // would turn a -0.0 vertex into +0.0.
                if let Surface::Floor = surface {
                    y0 += y_offset;
                    y1 += y_offset;
                }
                let sloped = match surface {
                    Surface::Floor | Surface::Ceiling => exceeds_epsilon(y0 - y1),
                    Surface::LeftWall | Surface::RightWall => exceeds_epsilon(x0 - x1),
                };
                let (int, normal) = if sloped {
                    let Some(int) = line_intersection(Vec2::new(x0, y0), Vec2::new(x1, y1), a, b)
                    else {
                        continue;
                    };
                    (int, None)
                } else {
                    let (allowed, normal) = match surface {
                        Surface::Floor => (a.y >= b.y, Vec3::new(0.0, 1.0, 0.0)),
                        Surface::Ceiling => (a.y <= b.y, Vec3::new(0.0, -1.0, 0.0)),
                        Surface::LeftWall => (a.x <= b.x, Vec3::new(-1.0, 0.0, 0.0)),
                        Surface::RightWall => (a.x >= b.x, Vec3::new(1.0, 0.0, 0.0)),
                    };
                    if !allowed {
                        continue;
                    }
                    let int = match surface {
                        Surface::Floor | Surface::Ceiling => line_intersection_h(x0, y0, x1, a, b),
                        Surface::LeftWall | Surface::RightWall => {
                            line_intersection_v(x0, y0, y1, a, b)
                        }
                    };
                    let Some(int) = int else {
                        continue;
                    };
                    (int, Some(normal))
                };
                let dist2 = (int.x - a.x) * (int.x - a.x) + (int.y - a.y) * (int.y - a.y);
                if min_dist2 > dist2 {
                    min_dist2 = dist2;
                    hit = Some(CollHit {
                        pos: vec3(int),
                        line_id: id,
                        flags: self.lo_flags(line),
                        normal: normal.unwrap_or_else(|| {
                            ps_vec_normalize(Vec3::new(-(y1 - y0), x1 - x0, 0.0))
                        }),
                    });
                }
            }
        }
        hit
    }

    /// Body shared by the four `*Remap` checks. For joints that move
    /// (`B8`/`B9`/`B10`), the segment start is first carried along with the
    /// line ([`remap_2d`] from the line's previous to current position);
    /// distances are measured from the original start and are negative
    /// for hits behind it.
    #[allow(clippy::too_many_arguments)]
    fn check_lines_remap(
        &self,
        surface: Surface,
        old: Vec2,
        b: Vec2,
        y_offset: f32,
        line_id_skip: i32,
        joint_id_skip: i32,
        joint_id_only: i32,
        mut cb: Option<FloorFilter>,
    ) -> Option<CollHit> {
        let mut min_dist2 = f32::MAX;
        let mut hit = None;
        for joint in self.checked_joints(joint_id_skip, joint_id_only) {
            let moving =
                self.joints[joint].flags & (coll_joint::B10 | coll_joint::B9 | coll_joint::B8) != 0;
            for line in self.joint_group_lines(joint, surface.group()) {
                let id = line as i32;
                if let Some(cb) = cb.as_mut() {
                    if !cb(self, id) {
                        continue;
                    }
                }
                if line_id_skip == id || !self.line_usable(line, surface.kind()) {
                    continue;
                }
                let inner = self.inner(line);
                let (v0, v1) = (
                    self.vtx[inner.v0_idx as usize],
                    self.vtx[inner.v1_idx as usize],
                );
                let (x0, y0, x1, y1) = match surface {
                    // `y_offset + pos.y`, from the raw vertices.
                    Surface::Floor => {
                        (v0.pos.x, y_offset + v0.pos.y, v1.pos.x, y_offset + v1.pos.y)
                    }
                    Surface::Ceiling => self.line_extended(id),
                    Surface::LeftWall | Surface::RightWall => {
                        (v0.pos.x, v0.pos.y, v1.pos.x, v1.pos.y)
                    }
                };
                let a = if moving {
                    remap_2d(
                        v0.prev_pos,
                        v1.prev_pos,
                        Vec2::new(x0, y0),
                        Vec2::new(x1, y1),
                        old,
                    )
                } else {
                    old
                };
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let sloped = match surface {
                    Surface::Floor | Surface::Ceiling => exceeds_epsilon(y0 - y1),
                    Surface::LeftWall | Surface::RightWall => exceeds_epsilon(x0 - x1),
                };
                let (int, normal) = if sloped {
                    let Some(int) = line_intersection(Vec2::new(x0, y0), Vec2::new(x1, y1), a, b)
                    else {
                        continue;
                    };
                    (int, None)
                } else {
                    let (allowed, normal) = match surface {
                        Surface::Floor => (a.y >= b.y, Vec3::new(0.0, 1.0, 0.0)),
                        Surface::Ceiling => (a.y <= b.y, Vec3::new(0.0, -1.0, 0.0)),
                        Surface::LeftWall => (a.x <= b.x, Vec3::new(-1.0, 0.0, 0.0)),
                        Surface::RightWall => (a.x >= b.x, Vec3::new(1.0, 0.0, 0.0)),
                    };
                    if !allowed {
                        continue;
                    }
                    let int = match surface {
                        Surface::Floor | Surface::Ceiling => line_intersection_h(x0, y0, x1, a, b),
                        Surface::LeftWall | Surface::RightWall => {
                            line_intersection_v(x0, y0, y1, a, b)
                        }
                    };
                    let Some(int) = int else {
                        continue;
                    };
                    (int, Some(normal))
                };
                let (ex, ey) = (int.x - old.x, int.y - old.y);
                let mut dist2 = ex * ex + ey * ey;
                if ppc::fmadds(dx, ex, dy * ey) < 0.0 {
                    dist2 = -dist2;
                }
                if min_dist2 > dist2 {
                    min_dist2 = dist2;
                    hit = Some(CollHit {
                        pos: vec3(int),
                        line_id: id,
                        flags: self.lo_flags(line),
                        normal: normal.unwrap_or_else(|| {
                            ps_vec_normalize(Vec3::new(-(y1 - y0), x1 - x0, 0.0))
                        }),
                    });
                }
            }
        }
        hit
    }

    /// `mpCheckFloor` (0x8004F008): the floor hit closest to `a` on segment
    /// `a`–`b`, with floors raised by `y_offset`, skipping `line_id_skip`
    /// and lines `cb` rejects (`cb` runs first, on every visited line).
    ///
    /// Floors are extended at linked ends ([`MpLib::line_extended`]).
    /// Sloped floors use [`line_intersection`] and a normalized normal; flat
    /// ones (`|y0 - y1| <= 0.0001`) only catch downward segments
    /// (`a.y >= b.y`), via [`line_intersection_h`], with normal exactly
    /// (0, 1, 0).
    ///
    /// Audited: no fused ops.
    #[allow(clippy::too_many_arguments)]
    pub fn check_floor(
        &mut self,
        a: Vec2,
        b: Vec2,
        y_offset: f32,
        line_id_skip: i32,
        joint_id_skip: i32,
        joint_id_only: i32,
        cb: Option<FloorFilter>,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines(
                    Surface::Floor,
                    a,
                    b,
                    y_offset,
                    line_id_skip,
                    joint_id_skip,
                    joint_id_only,
                    cb,
                )
            },
        )
    }

    /// `mpCheckFloorRemap` (0x8004F400): [`Self::check_floor`] with moving
    /// floors handled by remapping the start point (see
    /// [`Self::check_lines_remap`]). Unlike `mpCheckFloor`, the floors are
    /// not extended at their ends.
    ///
    /// Audited: 2 fused ops, the `fmadds` direction test in each branch.
    #[allow(clippy::too_many_arguments)]
    pub fn check_floor_remap(
        &mut self,
        a: Vec2,
        b: Vec2,
        y_offset: f32,
        line_id_skip: i32,
        joint_id_skip: i32,
        joint_id_only: i32,
        cb: Option<FloorFilter>,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines_remap(
                    Surface::Floor,
                    a,
                    b,
                    y_offset,
                    line_id_skip,
                    joint_id_skip,
                    joint_id_only,
                    cb,
                )
            },
        )
    }

    /// `mpCheckCeiling` (0x8004F8A4): the ceiling hit closest to `a`.
    /// Ceilings are extended like floors; flat ones only catch upward
    /// segments (`a.y <= b.y`), with normal exactly (0, -1, 0).
    ///
    /// Audited: no fused ops.
    pub fn check_ceiling(
        &mut self,
        a: Vec2,
        b: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines(
                    Surface::Ceiling,
                    a,
                    b,
                    0.0,
                    -1,
                    joint_id_skip,
                    joint_id_only,
                    None,
                )
            },
        )
    }

    /// `mpCheckCeilingRemap` (0x8004FC2C). Ceilings stay extended here.
    ///
    /// Audited: 2 fused ops, the `fmadds` direction tests.
    pub fn check_ceiling_remap(
        &mut self,
        a: Vec2,
        b: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines_remap(
                    Surface::Ceiling,
                    a,
                    b,
                    0.0,
                    -1,
                    joint_id_skip,
                    joint_id_only,
                    None,
                )
            },
        )
    }

    /// `mpCheckLeftWall` (0x800501CC): the left-wall hit closest to `a`.
    /// Walls are not extended. Vertical walls (`|x0 - x1| <= 0.0001`) only
    /// catch rightward segments (`a.x <= b.x`), via
    /// [`line_intersection_v`], with normal exactly (-1, 0, 0).
    ///
    /// Audited: no fused ops.
    pub fn check_left_wall(
        &mut self,
        a: Vec2,
        b: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines(
                    Surface::LeftWall,
                    a,
                    b,
                    0.0,
                    -1,
                    joint_id_skip,
                    joint_id_only,
                    None,
                )
            },
        )
    }

    /// `mpCheckLeftWallRemap` (0x8005057C).
    ///
    /// Audited: 2 fused ops, the `fmadds` direction tests.
    pub fn check_left_wall_remap(
        &mut self,
        a: Vec2,
        b: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines_remap(
                    Surface::LeftWall,
                    a,
                    b,
                    0.0,
                    -1,
                    joint_id_skip,
                    joint_id_only,
                    None,
                )
            },
        )
    }

    /// `mpCheckRightWall` (0x800509B8): as [`Self::check_left_wall`];
    /// vertical walls catch leftward segments (`a.x >= b.x`), with normal
    /// exactly (1, 0, 0).
    ///
    /// Audited: no fused ops.
    pub fn check_right_wall(
        &mut self,
        a: Vec2,
        b: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines(
                    Surface::RightWall,
                    a,
                    b,
                    0.0,
                    -1,
                    joint_id_skip,
                    joint_id_only,
                    None,
                )
            },
        )
    }

    /// `mpCheckRightWallRemap` (0x80050D68).
    ///
    /// Audited: 2 fused ops, the `fmadds` direction tests.
    pub fn check_right_wall_remap(
        &mut self,
        a: Vec2,
        b: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        self.with_bounding(
            |mp| mp.bounding_check2(a.x, a.y, b.x, b.y),
            |mp| {
                mp.check_lines_remap(
                    Surface::RightWall,
                    a,
                    b,
                    0.0,
                    -1,
                    joint_id_skip,
                    joint_id_only,
                    None,
                )
            },
        )
    }

    /// Body of `mpLib_800511A4_RightWall` / `mpLib_800515A0_LeftWall`:
    /// whether a wall vertex crossed an ECB edge as the edge moved from
    /// `a0`–`a1` to `b0`–`b1`. Each vertex's previous position is remapped
    /// into the moved edge's frame, and the path from there to the
    /// vertex's current position is intersected with the edge `b0`–`b1`.
    #[allow(clippy::too_many_arguments)]
    fn check_wall_vertices(
        &self,
        surface: Surface,
        a0: Vec2,
        a1: Vec2,
        b0: Vec2,
        b1: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<i32> {
        let mut min_dist2 = f32::MAX;
        let mut hit = None;
        for joint in self.checked_joints(joint_id_skip, joint_id_only) {
            for line in self.joint_group_lines(joint, surface.group()) {
                if !self.line_usable(line, surface.kind()) {
                    continue;
                }
                let inner = *self.inner(line);
                for vtx in [inner.v0_idx, inner.v1_idx] {
                    let vtx = self.vtx[vtx as usize];
                    let (cur, prev) = (vtx.pos, vtx.prev_pos);
                    let p = remap_2d(a0, a1, b0, b1, prev);
                    let vdx = cur.x - p.x;
                    let vdy = cur.y - p.y;
                    if sq_sum_fused(vdx, vdy) > 0.001 {
                        if let Some(int) = line_intersection(b0, b1, p, cur) {
                            let (ex, ey) = (int.x - prev.x, int.y - prev.y);
                            let mut dist2 = ex * ex + ey * ey;
                            if ppc::fmadds(vdx, ex, vdy * ey) < 0.0 {
                                dist2 = -dist2;
                            }
                            if min_dist2 > dist2 {
                                min_dist2 = dist2;
                                hit = Some(line as i32);
                            }
                        }
                    }
                }
            }
        }
        hit
    }

    /// `mpLib_800511A4_RightWall` (0x800511A4): see
    /// [`Self::check_wall_vertices`]; returns the line of the closest
    /// crossing.
    ///
    /// Audited: 4 fused ops — per vertex, `SQ(vdx) + SQ(vdy)` as
    /// `fmadds vdx, vdx, vdy*vdy` and the `fmadds` direction test. The
    /// distance itself is unfused.
    pub fn check_right_wall_vertices(
        &mut self,
        a0: Vec2,
        a1: Vec2,
        b0: Vec2,
        b1: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<i32> {
        self.with_bounding(
            |mp| mp.bounding_check3(a0.x, a0.y, a1.x, a1.y, b0.x, b0.y, b1.x, b1.y),
            |mp| {
                mp.check_wall_vertices(
                    Surface::RightWall,
                    a0,
                    a1,
                    b0,
                    b1,
                    joint_id_skip,
                    joint_id_only,
                )
            },
        )
    }

    /// `mpLib_800515A0_LeftWall` (0x800515A0): left-wall mirror of
    /// [`Self::check_right_wall_vertices`].
    ///
    /// Audited: 4 fused ops, as the right-wall version.
    pub fn check_left_wall_vertices(
        &mut self,
        a0: Vec2,
        a1: Vec2,
        b0: Vec2,
        b1: Vec2,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<i32> {
        self.with_bounding(
            |mp| mp.bounding_check3(a0.x, a0.y, a1.x, a1.y, b0.x, b0.y, b1.x, b1.y),
            |mp| {
                mp.check_wall_vertices(
                    Surface::LeftWall,
                    a0,
                    a1,
                    b0,
                    b1,
                    joint_id_skip,
                    joint_id_only,
                )
            },
        )
    }

    /// `mpLib_8005199C_Floor` (0x8005199C): the first floor, in joint and
    /// line order, that lies under `pos` (`x0 <= x <= x1` and `pos.y` on or
    /// above the line). Not necessarily the nearest floor. -1 if none. The
    /// bounding pre-pass uses a segment 30000 units straight down.
    ///
    /// Audited: 1 fused op, `y >= fmadds(dy / dx, x - x0, y0)` (the
    /// quotient is `fdivs`).
    pub fn floor_below(&mut self, pos: Vec3, joint_id_skip: i32, joint_id_only: i32) -> i32 {
        let (x, y) = (pos.x, pos.y);
        self.with_bounding(
            |mp| mp.bounding_check2(x, y, x, y - 30000.0),
            |mp| {
                for joint in mp.checked_joints(joint_id_skip, joint_id_only) {
                    for line in mp.joint_group_lines(joint, line_group::FLOOR) {
                        if !mp.line_usable(line, line_flag::FLOOR) {
                            continue;
                        }
                        let inner = mp.inner(line);
                        let v0 = mp.vtx[inner.v0_idx as usize].pos;
                        let v1 = mp.vtx[inner.v1_idx as usize].pos;
                        let (x0, y0, x1, y1) = (v0.x, v0.y, v1.x, v1.y);
                        if x >= x0 && x <= x1 {
                            if y >= y0 && y >= y1 {
                                return line as i32;
                            }
                            if exceeds_epsilon(x1 - x0) {
                                let dx = x1 - x0;
                                let dy = y1 - y0;
                                if y >= ppc::fmadds(dy / dx, x - x0, y0) {
                                    return line as i32;
                                }
                            }
                        }
                    }
                }
                -1
            },
        )
    }

    /// `mpLib_80051BA8_Floor` (0x80051BA8): find a ledge floor
    /// (`lo_flags & LEDGE`) whose bounding box overlaps the given box,
    /// skipping `line_id_skip`. For `dir > 0` it keeps the leftmost `v0`,
    /// for `dir < 0` the rightmost `v1`. Returns the line (-1 if none) and,
    /// when found, that endpoint with x clamped into the box. `dir == 0`
    /// is an assertion failure in the game.
    ///
    /// Audited: no fused ops; the box sums/differences are hoisted single
    /// ops (`right + left`, `right - left`, `top + bottom`, `top - bottom`).
    #[allow(clippy::too_many_arguments)]
    pub fn find_ledge(
        &mut self,
        line_id_skip: i32,
        joint_id_skip: i32,
        joint_id_only: i32,
        dir: i32,
        left: f32,
        bottom: f32,
        right: f32,
        top: f32,
    ) -> (i32, Option<Vec3>) {
        let mut min = if dir > 0 {
            f32::MAX
        } else if dir < 0 {
            -f32::MAX
        } else {
            panic!("mplib.c:3821: dir != 0");
        };
        let (mut ledge_id, mut out) = (-1, Vec2::default());
        self.with_bounding(
            |mp| mp.bounding_check(left, bottom, right, top),
            |mp| {
                for joint in mp.checked_joints(joint_id_skip, joint_id_only) {
                    for line in mp.joint_group_lines(joint, line_group::FLOOR) {
                        let new_id = line as i32;
                        if line_id_skip == new_id || !mp.line_usable(line, line_flag::FLOOR) {
                            continue;
                        }
                        let inner = mp.inner(line);
                        if u32::from(inner.lo_flags) & line_flag::LEDGE == 0 {
                            continue;
                        }
                        let v0 = mp.vtx[inner.v0_idx as usize].pos;
                        let v1 = mp.vtx[inner.v1_idx as usize].pos;
                        let (x0, y0, x1, y1) = (v0.x, v0.y, v1.x, v1.y);
                        let (line_left, line_right) = if x0 > x1 { (x1, x0) } else { (x0, x1) };
                        let (line_bottom, line_top) = if y0 > y1 { (y1, y0) } else { (y0, y1) };
                        // distance between the midpoints of the two boxes
                        let dist_h = abs_c((line_right + line_left) - (right + left));
                        if dist_h < (line_right - line_left) + (right - left) {
                            let dist_v = abs_c((line_top + line_bottom) - (top + bottom));
                            if dist_v < (line_top - line_bottom) + (top - bottom) {
                                if dir > 0 {
                                    if min > x0 {
                                        min = x0;
                                        ledge_id = new_id;
                                        out = Vec2::new(x0, y0);
                                    }
                                } else if dir < 0 && min < x1 {
                                    min = x1;
                                    ledge_id = new_id;
                                    out = Vec2::new(x1, y1);
                                }
                            }
                        }
                    }
                }
            },
        );
        if ledge_id == -1 {
            return (-1, None);
        }
        if out.x > right {
            out.x = right;
        } else if out.x < left {
            out.x = left;
        }
        (ledge_id, Some(vec3(out)))
    }

    /// `mpCheckMultiple` (0x80051EC8): run the checks selected by `checks`
    /// (1 floor, 2 ceiling, 4 left wall, 8 right wall; 0x10 selects the
    /// `*Remap` versions) on segment `p0`–`p1` and return the hit closest
    /// to `p0`. Ties keep the earlier check (floor, ceiling, left, right).
    ///
    /// Audited: no fused ops.
    pub fn check_multiple(
        &mut self,
        p0: Vec2,
        p1: Vec2,
        checks: u32,
        joint_id_skip: i32,
        joint_id_only: i32,
    ) -> Option<CollHit> {
        let already_checked = self.checked_bounding();
        if !already_checked {
            self.bounding_check2(p0.x, p0.y, p1.x, p1.y);
        }
        let remap = checks & 0x10 != 0;
        let mut min_dist2 = f32::MAX;
        let mut best = None;
        let dist2 = |hit: &CollHit| {
            let dx = (hit.pos.x - p0.x) * (hit.pos.x - p0.x);
            let dy = (hit.pos.y - p0.y) * (hit.pos.y - p0.y);
            dx + dy
        };
        if checks & 1 != 0 {
            let hit = if remap {
                self.check_floor_remap(p0, p1, 0.0, -1, joint_id_skip, joint_id_only, None)
            } else {
                self.check_floor(p0, p1, 0.0, -1, joint_id_skip, joint_id_only, None)
            };
            // The floor result is taken unconditionally.
            if let Some(hit) = hit {
                min_dist2 = dist2(&hit);
                best = Some(hit);
            }
        }
        type Check = fn(&mut MpLib, Vec2, Vec2, i32, i32) -> Option<CollHit>;
        let others: [(u32, Check, Check); 3] = [
            (2, MpLib::check_ceiling, MpLib::check_ceiling_remap),
            (4, MpLib::check_left_wall, MpLib::check_left_wall_remap),
            (8, MpLib::check_right_wall, MpLib::check_right_wall_remap),
        ];
        for (bit, plain, remapped) in others {
            if checks & bit == 0 {
                continue;
            }
            let check = if remap { remapped } else { plain };
            if let Some(hit) = check(self, p0, p1, joint_id_skip, joint_id_only) {
                if min_dist2 > dist2(&hit) {
                    min_dist2 = dist2(&hit);
                    best = Some(hit);
                }
            }
        }
        if !already_checked {
            self.uncheck_bounding();
        }
        if min_dist2 < f32::MAX {
            best
        } else {
            None
        }
    }

    /// `mpCheckAllRemap` (0x800524DC): [`Self::check_multiple`] with 0x1F.
    pub fn check_all_remap(
        &mut self,
        joint_id_skip: i32,
        joint_id_only: i32,
        p0: Vec2,
        p1: Vec2,
    ) -> Option<CollHit> {
        self.check_multiple(p0, p1, 0x1F, joint_id_skip, joint_id_only)
    }

    /// `mpCheckAll` (0x80052508): [`Self::check_multiple`] with 0xF.
    pub fn check_all(
        &mut self,
        joint_id_skip: i32,
        joint_id_only: i32,
        p0: Vec2,
        p1: Vec2,
    ) -> Option<CollHit> {
        self.check_multiple(p0, p1, 0xF, joint_id_skip, joint_id_only)
    }
}
