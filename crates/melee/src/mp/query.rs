//! Line queries from `mplib.c`: per-line getters, the surface
//! "offset at x / y" walks, line-chain walkers, and vertex setters.
//!
//! Line ids are `i32` with -1 for "none", as in the C. Functions that use
//! the C `LINEID_CHECK` macro panic where the game would halt.
//!
//! Audit (`dol-inspect`, 1.02): unless a doc comment says otherwise, a
//! function has no fused multiply-adds and computes in single precision;
//! the neighbor tests inlined from `mpLineGetNext`/`Prev` are unfused
//! `fmuls`/`fadds` compared against the `double` 4.0.

// `!(a < b)` forms mirror the C source; they differ from `a >= b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::data::MapLine;
use super::geom::remap_2d;
use super::lib::MpLib;
use super::line_flag;
use super::psvec::ps_vec_normalize;
use crate::math::{sqrtf, Vec2, Vec3};

/// Outputs of the surface walks [`MpLib::floor_offset`] and friends
/// (`mpLib_8004DD90_Floor` etc.): the C writes these through optional
/// out-pointers when the walk succeeds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineOffset {
    /// The line reached by walking from the starting line.
    pub line_id: i32,
    /// `y_out` (floors, ceilings) or `x_out` (walls): the surface position
    /// minus the query position, with the floor/ceiling bias of ±0.0001.
    pub offset: f32,
    /// The line's `lo_flags` (platform, ledge, terrain bits).
    pub flags: u32,
    /// The line normal from `PSVECNormalize` (see [`ps_vec_normalize`]).
    pub normal: Vec3,
}

/// The result of [`MpLib::floor_walk`] (`mpLib_80056C54`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloorWalk {
    /// The function's return value: the distance was covered on floors.
    pub reached: bool,
    /// `line_id_out`: the line the walk ended on, or -1 if it is not a floor.
    pub line_id: i32,
    /// `vec_out`
    pub pos: Vec3,
    /// `flags_out` and `normal_out`, written only when `line_id != -1`.
    pub surface: Option<(u32, Vec3)>,
}

/// `normal_out = (-(y1 - y0), x1 - x0, 0)`, normalized.
fn line_normal_of(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec3 {
    ps_vec_normalize(Vec3::new(-(y1 - y0), x1 - x0, 0.0))
}

impl MpLib {
    fn v0(&self, line_id: usize) -> Vec2 {
        self.vtx[self.inner(line_id).v0_idx as usize].pos
    }

    fn v1(&self, line_id: usize) -> Vec2 {
        self.vtx[self.inner(line_id).v1_idx as usize].pos
    }

    fn line_has(&self, line_id: i32, kind: u32) -> bool {
        self.lines[line_id as usize].flags & kind != 0
    }

    /// `mpLib_8004DD90_Floor` (0x8004DD90): starting on floor `line_id`, walk
    /// left/right along connected floors to the one under `pos.x`, and return
    /// the floor's height there relative to `pos.y`, plus 0.0001.
    ///
    /// Past the end of the floor chain, `x` clamps to the end if within
    /// 0.1, otherwise the result is `None` (the C returns -1). Quirk: walking
    /// right never sets the direction flag, so a walk can turn back once.
    ///
    /// Audited: no fused ops; `x - x0 < -0.1` and `x - x1 > 0.1` compare a
    /// single difference with a `double` literal, and the offset is single
    /// math with the `double` 0.0001 added before `frsp`.
    pub fn floor_offset(&self, mut line_id: i32, pos: Vec3) -> Option<LineOffset> {
        self.line_id_check(line_id);
        let mut dir = 0;
        let (mut x0, mut x1, mut x);
        let y = pos.y;
        loop {
            let id = line_id as usize;
            x0 = self.v0(id).x;
            x1 = self.v1(id).x;
            x = pos.x;
            if x < x0 {
                if dir != 1 {
                    let new_id = self.line_get_prev(id);
                    if new_id == -1 || !self.line_has(new_id, line_flag::FLOOR) {
                        if f64::from(x - x0) < -0.1 {
                            return None;
                        }
                        x = x0;
                        break;
                    }
                    line_id = new_id;
                    dir = -1;
                } else {
                    x = x0;
                    break;
                }
            } else if x > x1 {
                if dir != -1 {
                    let new_id = self.line_get_next(id);
                    if new_id == -1 || !self.line_has(new_id, line_flag::FLOOR) {
                        if f64::from(x - x1) > 0.1 {
                            return None;
                        }
                        x = x1;
                        break;
                    }
                    line_id = new_id;
                } else {
                    x = x1;
                    break;
                }
            } else {
                break;
            }
        }
        let id = line_id as usize;
        let (y0, y1) = (self.v0(id).y, self.v1(id).y);
        Some(LineOffset {
            line_id,
            offset: (f64::from((y1 - y0) * (x - x0) / (x1 - x0) + y0 - y) + 0.0001) as f32,
            flags: u32::from(self.inner(id).lo_flags),
            normal: line_normal_of(x0, y0, x1, y1),
        })
    }

    /// `mpLib_8004E090_Ceiling` (0x8004E090): the ceiling counterpart of
    /// [`Self::floor_offset`]. Ceilings run right to left (`v0` is the right
    /// end), so the walk follows `next` leftward; the offset subtracts
    /// 0.0001 instead of adding it.
    ///
    /// Audited: no fused ops; same `double` compares and bias as the floor.
    pub fn ceiling_offset(&self, mut line_id: i32, pos: Vec3) -> Option<LineOffset> {
        self.line_id_check(line_id);
        let mut dir = 0;
        let (mut x0, mut x1);
        let mut x = pos.x;
        loop {
            let id = line_id as usize;
            x0 = self.v0(id).x;
            x1 = self.v1(id).x;
            if pos.x < x1 {
                if dir != 1 {
                    let new_id = self.line_get_next(id);
                    if new_id == -1 || !self.line_has(new_id, line_flag::CEILING) {
                        if f64::from(pos.x - x1) < -0.1 {
                            return None;
                        }
                        x = x1;
                        break;
                    }
                    line_id = new_id;
                    dir = -1;
                    continue;
                }
                x = x1;
            } else if pos.x > x0 {
                if dir != -1 {
                    let new_id = self.line_get_prev(id);
                    if new_id == -1 || !self.line_has(new_id, line_flag::CEILING) {
                        if f64::from(pos.x - x0) > 0.1 {
                            return None;
                        }
                        x = x0;
                        break;
                    }
                    line_id = new_id;
                    dir = 1;
                    continue;
                }
                x = x0;
            }
            break;
        }
        let id = line_id as usize;
        let (y0, y1) = (self.v0(id).y, self.v1(id).y);
        Some(LineOffset {
            line_id,
            offset: (f64::from((y1 - y0) * (x - x0) / (x1 - x0) + y0 - pos.y) - 0.0001) as f32,
            flags: u32::from(self.inner(id).lo_flags),
            normal: line_normal_of(x0, y0, x1, y1),
        })
    }

    /// `mpLib_8004E398_LeftWall` (0x8004E398): walk along left walls
    /// (bottom to top) to the one level with `pos.y`; the offset is the
    /// wall's x there minus `pos.x`.
    ///
    /// Quirk kept from the C: after turning around (`dir` set), `y` is not
    /// clamped to the line's end, unlike the right-wall version.
    ///
    /// Audited: no fused ops; `double` compares for the 0.1 tolerance, the
    /// offset is single precision with no bias.
    pub fn left_wall_offset(&self, mut line_id: i32, pos: Vec3) -> Option<LineOffset> {
        self.line_id_check(line_id);
        let mut dir = 0;
        let (mut y0, mut y1);
        let mut y = pos.y;
        loop {
            let id = line_id as usize;
            y0 = self.v0(id).y;
            y1 = self.v1(id).y;
            if pos.y < y0 {
                if dir != 1 {
                    let new_id = self.line_get_prev(id);
                    if new_id == -1 || !self.line_has(new_id, line_flag::LEFT_WALL) {
                        if f64::from(pos.y - y0) < -0.1 {
                            return None;
                        }
                        y = y0;
                        break;
                    }
                    line_id = new_id;
                    dir = -1;
                    continue;
                }
            } else if pos.y > y1 && dir != -1 {
                let new_id = self.line_get_next(id);
                if new_id == -1 || !self.line_has(new_id, line_flag::LEFT_WALL) {
                    if f64::from(pos.y - y1) > 0.1 {
                        return None;
                    }
                    y = y1;
                    break;
                }
                line_id = new_id;
                dir = 1;
                continue;
            }
            break;
        }
        let id = line_id as usize;
        let (x0, x1) = (self.v0(id).x, self.v1(id).x);
        Some(LineOffset {
            line_id,
            offset: x0 + (x1 - x0) * (y - y0) / (y1 - y0) - pos.x,
            flags: u32::from(self.inner(id).lo_flags),
            normal: line_normal_of(x0, y0, x1, y1),
        })
    }

    /// `mpLib_8004E684_RightWall` (0x8004E684): walk along right walls (top
    /// to bottom) to the one level with `pos.y`; the offset is the wall's x
    /// there minus `pos.x`.
    ///
    /// Audited: no fused ops; as [`Self::left_wall_offset`].
    pub fn right_wall_offset(&self, mut line_id: i32, pos: Vec3) -> Option<LineOffset> {
        self.line_id_check(line_id);
        let mut dir = 0;
        let (mut y0, mut y1);
        let mut y = pos.y;
        loop {
            let id = line_id as usize;
            y0 = self.v0(id).y;
            y1 = self.v1(id).y;
            if pos.y > y0 {
                if dir != -1 {
                    let new_id = self.line_get_prev(id);
                    if new_id == -1 || !self.line_has(new_id, line_flag::RIGHT_WALL) {
                        if f64::from(pos.y - y0) > 0.1 {
                            return None;
                        }
                        y = y0;
                        break;
                    }
                    line_id = new_id;
                    dir = 1;
                    continue;
                }
                y = y0;
            } else if pos.y < y1 {
                if dir != 1 {
                    let next_id = self.line_get_next(id);
                    if next_id == -1 || !self.line_has(next_id, line_flag::RIGHT_WALL) {
                        if f64::from(pos.y - y1) < -0.1 {
                            return None;
                        }
                        y = y1;
                        break;
                    }
                    line_id = next_id;
                    dir = -1;
                    continue;
                }
                y = y1;
            }
            break;
        }
        let id = line_id as usize;
        let (x0, x1) = (self.v0(id).x, self.v1(id).x);
        Some(LineOffset {
            line_id,
            offset: x0 + ((x1 - x0) * (y - y0)) / (y1 - y0) - pos.x,
            flags: u32::from(self.inner(id).lo_flags),
            normal: line_normal_of(x0, y0, x1, y1),
        })
    }

    /// `mpLib_8004ED5C` (0x8004ED5C): the line's endpoints `(x0, y0, x1,
    /// y1)`, each pushed out by one unit along the line where a neighbor
    /// exists. Floor and ceiling checks use this so that segments crossing
    /// a joint between lines are not missed.
    ///
    /// Quirk kept from the C: the `v1` extension uses the already-extended
    /// `x0`/`y0`, so it is slightly more than one unit
    /// (`(x1 - x0) / distance` with the old `distance`).
    ///
    /// Audited: the only fused ops (6) are the three `fnmsub` Newton steps
    /// in each inlined `sqrtf`, which [`sqrtf`] owns. `distance > 0.001F`
    /// is a single-precision compare.
    pub fn line_extended(&self, line_id: i32) -> (f32, f32, f32, f32) {
        let id = line_id as usize;
        let (v0, v1) = (self.v0(id), self.v1(id));
        let (mut x0, mut y0, mut x1, mut y1) = (v0.x, v0.y, v1.x, v1.y);
        let mut calculated_distance = false;
        // Only read after being computed below.
        let mut distance = 0.0;
        if self.line_get_prev(id) != -1 {
            distance = sqrtf((x0 - x1) * (x0 - x1) + (y0 - y1) * (y0 - y1));
            if distance > 0.001 {
                x0 += (x0 - x1) / distance;
                y0 += (y0 - y1) / distance;
            }
            calculated_distance = true;
        }
        if self.line_get_next(id) != -1 {
            if !calculated_distance {
                distance = sqrtf((x0 - x1) * (x0 - x1) + (y0 - y1) * (y0 - y1));
            }
            if distance > 0.001 {
                x1 += (x1 - x0) / distance;
                y1 += (y1 - y0) / distance;
            }
        }
        (x0, y0, x1, y1)
    }

    /// Shared body of `mpLineNextNonFloor` and its seven siblings: follow
    /// `mpLineGetNext` (or `Prev`) while lines have `kind`; return the first
    /// line without it, or -1 at a dead end or on looping back to the start.
    fn line_walk_non(&self, line_id: i32, kind: u32, next: bool) -> i32 {
        let start = self.line_id_check(line_id);
        let step = |id: usize| {
            if next {
                self.line_get_next(id)
            } else {
                self.line_get_prev(id)
            }
        };
        let mut new_id = step(start);
        while new_id != -1 && new_id != line_id && self.line_has(new_id, kind) {
            new_id = step(new_id as usize);
        }
        if new_id != -1 && new_id != line_id {
            new_id
        } else {
            -1
        }
    }

    /// `mpLineNextNonFloor` (0x80052534): see [`Self::line_walk_non`].
    pub fn line_next_non_floor(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::FLOOR, true)
    }

    /// `mpLinePrevNonFloor` (0x80052700).
    pub fn line_prev_non_floor(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::FLOOR, false)
    }

    /// `mpLinePrevNonCeiling` (0x800528CC).
    pub fn line_prev_non_ceiling(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::CEILING, false)
    }

    /// `mpLineNextNonCeiling` (0x80052A98).
    pub fn line_next_non_ceiling(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::CEILING, true)
    }

    /// `mpLineNextNonLeftWall` (0x80052C64).
    pub fn line_next_non_left_wall(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::LEFT_WALL, true)
    }

    /// `mpLinePrevNonLeftWall` (0x80052E30).
    pub fn line_prev_non_left_wall(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::LEFT_WALL, false)
    }

    /// `mpLinePrevNonRightWall` (0x80052FFC).
    pub fn line_prev_non_right_wall(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::RIGHT_WALL, false)
    }

    /// `mpLineNextNonRightWall` (0x800531C8).
    pub fn line_next_non_right_wall(&self, line_id: i32) -> i32 {
        self.line_walk_non(line_id, line_flag::RIGHT_WALL, true)
    }

    /// Shared body of `mpLib_80053394_Floor` and siblings: follow only the
    /// primary links (`next_id0`/`prev_id0`, ignoring the alternates and the
    /// enabled/hidden flags) while lines have `kind`; return the first line
    /// without it, or -1.
    fn line_walk_non_raw(&self, line_id: i32, kind: u32, next: bool) -> i32 {
        let id = self.line_id_check(line_id);
        let link = |line: &MapLine| {
            if next {
                line.next_id0
            } else {
                line.prev_id0
            }
        };
        let mut new_id = i32::from(link(self.inner(id)));
        while new_id != -1 && self.line_has(new_id, kind) {
            new_id = i32::from(link(self.inner(new_id as usize)));
        }
        new_id
    }

    /// `mpLib_80053394_Floor` (0x80053394): see [`Self::line_walk_non_raw`].
    pub fn line_next0_non_floor(&self, line_id: i32) -> i32 {
        self.line_walk_non_raw(line_id, line_flag::FLOOR, true)
    }

    /// `mpLib_80053448_Floor` (0x80053448).
    pub fn line_prev0_non_floor(&self, line_id: i32) -> i32 {
        self.line_walk_non_raw(line_id, line_flag::FLOOR, false)
    }

    /// `mpLib_8005389C_Ceiling` (0x8005389C).
    pub fn line_prev0_non_ceiling(&self, line_id: i32) -> i32 {
        self.line_walk_non_raw(line_id, line_flag::CEILING, false)
    }

    /// `mpLib_80053950_Ceiling` (0x80053950).
    pub fn line_next0_non_ceiling(&self, line_id: i32) -> i32 {
        self.line_walk_non_raw(line_id, line_flag::CEILING, true)
    }

    /// Shared body of `mpLib_800534FC_Floor` and siblings: follow
    /// `mpLineGetNext` (or `Prev`) along lines of `kind`. Returns -1 on
    /// reaching a line without `kind` or a dead end, and returns the line
    /// as soon as a step arrives through the current line's alternate link
    /// (`next_id1`/`prev_id1`), i.e. across a junction.
    fn line_walk_to_alt(&self, line_id: i32, kind: u32, next: bool) -> i32 {
        let mut line_id = self.line_id_check(line_id);
        let step = |id: usize| {
            if next {
                self.line_get_next(id)
            } else {
                self.line_get_prev(id)
            }
        };
        let alt = |id: usize| {
            let line = self.inner(id);
            i32::from(if next { line.next_id1 } else { line.prev_id1 })
        };
        let mut new_id = step(line_id);
        while new_id != -1 {
            if !self.line_has(new_id, kind) {
                new_id = -1;
            } else if new_id != alt(line_id) {
                line_id = new_id as usize;
                new_id = step(line_id);
                continue;
            }
            break;
        }
        new_id
    }

    /// `mpLib_800534FC_Floor` (0x800534FC): see [`Self::line_walk_to_alt`].
    pub fn floor_next_via_alt(&self, line_id: i32) -> i32 {
        self.line_walk_to_alt(line_id, line_flag::FLOOR, true)
    }

    /// `mpLib_800536CC_Floor` (0x800536CC).
    pub fn floor_prev_via_alt(&self, line_id: i32) -> i32 {
        self.line_walk_to_alt(line_id, line_flag::FLOOR, false)
    }

    /// `mpLib_80053A04_Ceiling` (0x80053A04).
    pub fn ceiling_prev_via_alt(&self, line_id: i32) -> i32 {
        self.line_walk_to_alt(line_id, line_flag::CEILING, false)
    }

    /// `mpLib_80053BD4_Ceiling` (0x80053BD4).
    pub fn ceiling_next_via_alt(&self, line_id: i32) -> i32 {
        self.line_walk_to_alt(line_id, line_flag::CEILING, true)
    }

    /// `mpLib_80053DA4_Floor` (0x80053DA4): follow primary `next_id0` links
    /// while they lead to floors; return the last floor's `v1` position.
    pub fn floor_next0_end(&self, line_id: i32) -> Vec3 {
        let mut id = self.line_id_check(line_id);
        loop {
            let next_id = i32::from(self.inner(id).next_id0);
            if next_id != -1 && self.line_has(next_id, line_flag::FLOOR) {
                id = next_id as usize;
            } else {
                break;
            }
        }
        self.line_id_check(id as i32);
        let v = self.v1(id);
        Vec3::new(v.x, v.y, 0.0)
    }

    /// `mpLib_80053ECC_Floor` (0x80053ECC): the `prev_id0` mirror of
    /// [`Self::floor_next0_end`], returning the first floor's `v0`.
    pub fn floor_prev0_end(&self, line_id: i32) -> Vec3 {
        let mut id = self.line_id_check(line_id);
        loop {
            let prev_id = i32::from(self.inner(id).prev_id0);
            if prev_id != -1 && self.line_has(prev_id, line_flag::FLOOR) {
                id = prev_id as usize;
            } else {
                break;
            }
        }
        self.line_id_check(id as i32);
        let v = self.v0(id);
        Vec3::new(v.x, v.y, 0.0)
    }

    /// Shared body of `mpFloorGetRight` and siblings: follow `mpLineGetNext`
    /// (or `Prev`) while the line kind (`flags & 0xF`) stays the same, and
    /// return the last line's `v1` (or `v0`).
    fn line_chain_end(&self, line_id: i32, next: bool) -> Vec3 {
        let mut w = self.line_id_check(line_id);
        let kind = self.lines[w].flags & line_flag::KIND;
        loop {
            let id = if next {
                self.line_get_next(w)
            } else {
                self.line_get_prev(w)
            };
            if id == -1 || kind != self.lines[id as usize].flags & line_flag::KIND {
                break;
            }
            w = id as usize;
        }
        let v = if next { self.v1(w) } else { self.v0(w) };
        Vec3::new(v.x, v.y, 0.0)
    }

    /// `mpFloorGetRight` (0x80053FF4): right end of the floor chain.
    pub fn floor_get_right(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, true)
    }

    /// `mpFloorGetLeft` (0x80054158): left end of the floor chain.
    pub fn floor_get_left(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, false)
    }

    /// `mpCeilingGetRight` (0x800542BC): ceilings run right to left, so this
    /// follows `prev` to the first `v0`.
    pub fn ceiling_get_right(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, false)
    }

    /// `mpCeilingGetLeft` (0x80054420).
    pub fn ceiling_get_left(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, true)
    }

    /// `mpLeftWallGetTop` (0x80054584): left walls run bottom to top.
    pub fn left_wall_get_top(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, true)
    }

    /// `mpLeftWallGetBottom` (0x800546E8).
    pub fn left_wall_get_bottom(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, false)
    }

    /// `mpRightWallGetTop` (0x8005484C): right walls run top to bottom.
    pub fn right_wall_get_top(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, false)
    }

    /// `mpRightWallGetBottom` (0x800549B0).
    pub fn right_wall_get_bottom(&self, line_id: i32) -> Vec3 {
        self.line_chain_end(line_id, true)
    }

    /// `mpLineGetV1Pos` (0x80054B14).
    pub fn line_v1_pos(&self, line_id: i32) -> Vec3 {
        let v = self.v1(self.line_id_check(line_id));
        Vec3::new(v.x, v.y, 0.0)
    }

    /// `mpLineGetV0Pos` (0x80054BC0).
    pub fn line_v0_pos(&self, line_id: i32) -> Vec3 {
        let v = self.v0(self.line_id_check(line_id));
        Vec3::new(v.x, v.y, 0.0)
    }

    /// `mpLineGetKind` (0x80054C6C): the runtime kind bits (`flags & 0xF`).
    pub fn line_kind(&self, line_id: i32) -> u32 {
        self.lines[self.line_id_check(line_id)].flags & line_flag::KIND
    }

    /// `mpLineGetFlags` (0x80054CEC): the line data's `lo_flags`.
    pub fn line_flags(&self, line_id: i32) -> u32 {
        u32::from(self.inner(self.line_id_check(line_id)).lo_flags)
    }

    /// `mpLib_80054D68` (0x80054D68): replace the low byte of `lo_flags`
    /// (`(old & ~0xFF) | flags`, stored as a halfword).
    pub fn line_set_flags_low(&mut self, line_id: i32, flags: u32) {
        let id = self.line_id_check(line_id);
        let line = self.inner_mut(id);
        line.lo_flags = ((u32::from(line.lo_flags) & !0xFF) | flags) as u16;
    }

    /// `mpLineGetNormal` (0x80054DFC).
    pub fn line_normal(&self, line_id: i32) -> Vec3 {
        let id = self.line_id_check(line_id);
        let (v0, v1) = (self.v0(id), self.v1(id));
        line_normal_of(v0.x, v0.y, v1.x, v1.y)
    }

    /// `mpLib_80054ED8` (0x80054ED8): `line_id` is a line that is enabled and
    /// not hidden. -1 gives `false`; other out-of-range ids hang the game
    /// (`OSReport` then an infinite loop), which panics here.
    pub fn line_active(&self, line_id: i32) -> bool {
        if line_id == -1 {
            return false;
        }
        if line_id < 0 || line_id >= self.coll_data.lines.len() as i32 {
            panic!("mplib.c:4636:not found lineID={line_id}");
        }
        let flags = self.lines[line_id as usize].flags;
        !(flags & line_flag::ENABLED == 0 || flags & line_flag::HIDDEN != 0)
    }

    /// `mpLinesConnected` (0x80054F68): `target_id` is reachable from
    /// `start_id` by `mpLineGetNext` or `mpLineGetPrev` steps through lines
    /// of the same kind.
    pub fn lines_connected(&self, start_id: i32, target_id: i32) -> bool {
        let start = self.line_id_check(start_id);
        self.line_id_check(target_id);
        if start_id == target_id {
            return true;
        }
        let kind = self.lines[start].flags & line_flag::KIND;
        let same_kind = |id: i32| kind == self.lines[id as usize].flags & line_flag::KIND;
        let mut line_id = self.line_get_next(start);
        while line_id != -1 && same_kind(line_id) {
            if line_id == target_id {
                return true;
            }
            line_id = self.line_get_next(line_id as usize);
        }
        line_id = self.line_get_prev(start);
        while line_id != -1 && same_kind(line_id) {
            if line_id == target_id {
                return true;
            }
            line_id = self.line_get_prev(line_id as usize);
        }
        false
    }

    /// `mpVtxGetPos` (0x800566D8).
    pub fn vtx_pos(&self, vtx_id: i32) -> Vec2 {
        self.vtx[vtx_id as usize].pos
    }

    /// `mpVtxSetPos` (0x800566F8).
    pub fn vtx_set_pos(&mut self, vtx_id: i32, x: f32, y: f32) {
        self.vtx[vtx_id as usize].pos = Vec2::new(x, y);
    }

    /// `mpLineSetPos` (0x80056710).
    pub fn line_set_pos(&mut self, line_id: i32, x0: f32, y0: f32, x1: f32, y1: f32) {
        let line = *self.inner(line_id as usize);
        self.vtx_set_pos(i32::from(line.v0_idx), x0, y0);
        self.vtx_set_pos(i32::from(line.v1_idx), x1, y1);
    }

    /// `mpLib_80056758` (0x80056758): set the line's vertices to their
    /// data-file (unscaled) positions plus the given offsets.
    pub fn line_set_pos_from_base(&mut self, line_id: i32, x0: f32, y0: f32, x1: f32, y1: f32) {
        let line = *self.inner(line_id as usize);
        let vtx = &mut self.vtx[line.v0_idx as usize];
        vtx.pos = Vec2::new(vtx.base_pos.x + x0, vtx.base_pos.y + y0);
        let vtx = &mut self.vtx[line.v1_idx as usize];
        vtx.pos = Vec2::new(vtx.base_pos.x + x1, vtx.base_pos.y + y1);
    }

    /// `mpGetSpeed` (0x800567C0): how far `pos` is carried this frame by its
    /// line's motion: [`remap_2d`] from the line's previous to its current
    /// position, minus `pos`. `None` if the line is not active.
    ///
    /// The 1.02 code also reports and asserts when the speed exceeds 10000,
    /// but only when `DbLevel >= DbLKind_DebugRom`; that check is omitted.
    ///
    /// Audited: no fused ops in this function (they are in [`remap_2d`]).
    pub fn line_speed(&self, line_id: i32, pos: Vec3) -> Option<Vec3> {
        if !self.line_active(line_id) {
            return None;
        }
        let line = self.inner(line_id as usize);
        let v0 = self.vtx[line.v0_idx as usize];
        let v1 = self.vtx[line.v1_idx as usize];
        let new = remap_2d(
            v0.prev_pos,
            v1.prev_pos,
            v0.pos,
            v1.pos,
            Vec2::new(pos.x, pos.y),
        );
        Some(Vec3::new(new.x - pos.x, new.y - pos.y, 0.0))
    }

    /// `mpJointFromLine` (0x80056B6C): the joint whose vertex range holds
    /// the line's `v0`, searching all joints in index order; -1 if none.
    pub fn joint_from_line(&self, line_id: i32) -> i32 {
        if line_id != -1 {
            let id = self.line_id_check(line_id);
            let v0_idx = i32::from(self.inner(id).v0_idx);
            for (i, joint) in self.coll_data.joints.iter().enumerate() {
                let start = i32::from(joint.vtx_start);
                if start <= v0_idx && v0_idx < start + i32::from(joint.vtx_count) {
                    return i as i32;
                }
            }
        }
        -1
    }

    /// `mpLib_80056C54` (0x80056C54): from `pos` on floor `line_id`, walk
    /// `distance` units along the surface (rightward via `v1`/next for a
    /// positive distance, leftward via `v0`/prev otherwise), crossing walls
    /// for at most `wall_limit` in total and stopping at ceilings. `None`
    /// is the early `false` return (inactive line, or no floor under `pos`),
    /// where the C writes no outputs.
    ///
    /// Known divergence: the C never initializes `total_dist` before the
    /// first wall line. Both callers (`ftdonkey.c`, `itleadead.c`) start on
    /// a floor, which zeroes it before any wall is reached; the port starts
    /// it at 0.0, so a walk that begins on a wall line may differ from the
    /// game (which would read a stale register).
    ///
    /// Audited: 9 fused ops — the inlined `sqrtf` steps (owned by
    /// [`sqrtf`]) and three `fmadds` for the final interpolation
    /// (`t * (vtx - cur) + cur`). `dist < 0.0001` compares in double.
    pub fn floor_walk(
        &self,
        mut line_id: i32,
        pos: Vec3,
        mut distance: f32,
        wall_limit: f32,
    ) -> Option<FloorWalk> {
        if !self.line_active(line_id) {
            return None;
        }
        let y = self.floor_offset(line_id, pos)?.offset;
        let mut cur_pos = pos;
        cur_pos.y += y;
        let mut result = true;
        let mut total_dist = 0.0f32;
        let mut dist;
        let mut vtx_pos;
        let forward = distance > 0.0;
        if !forward {
            distance = -distance;
        }
        loop {
            vtx_pos = if forward {
                self.line_v1_pos(line_id)
            } else {
                self.line_v0_pos(line_id)
            };
            let dx2 = (cur_pos.x - vtx_pos.x) * (cur_pos.x - vtx_pos.x);
            let dy2 = (cur_pos.y - vtx_pos.y) * (cur_pos.y - vtx_pos.y);
            dist = sqrtf(dx2 + dy2);
            let flags = self.line_kind(line_id);
            if flags & (line_flag::RIGHT_WALL | line_flag::LEFT_WALL) != 0 {
                total_dist += dist;
                if total_dist > wall_limit {
                    result = false;
                    break;
                }
            } else if flags & line_flag::CEILING != 0 {
                result = false;
                break;
            } else {
                total_dist = 0.0;
            }
            if !(dist > distance) {
                let new_id = if forward {
                    self.line_get_next(line_id as usize)
                } else {
                    self.line_get_prev(line_id as usize)
                };
                if new_id == -1 {
                    result = false;
                } else {
                    distance -= dist;
                    line_id = new_id;
                    cur_pos = vtx_pos;
                    continue;
                }
            }
            break;
        }
        if line_id != -1 && self.line_kind(line_id) & line_flag::FLOOR == 0 {
            line_id = -1;
        }
        let surface =
            (line_id != -1).then(|| (self.line_flags(line_id), self.line_normal(line_id)));
        let pos = if result {
            if f64::from(dist) < 0.0001 {
                cur_pos
            } else {
                let t = distance / dist;
                Vec3::new(
                    crate::ppc::fmadds(t, vtx_pos.x - cur_pos.x, cur_pos.x),
                    crate::ppc::fmadds(t, vtx_pos.y - cur_pos.y, cur_pos.y),
                    crate::ppc::fmadds(t, vtx_pos.z - cur_pos.z, cur_pos.z),
                )
            }
        } else {
            vtx_pos
        };
        Some(FloorWalk {
            reached: result,
            line_id,
            pos,
            surface,
        })
    }
}
