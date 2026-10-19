//! Stage collision queries (`mplib.c`): synthetic geometry with expected
//! values worked out by hand from the C, and real stages from the vanilla
//! disc (`MELEE_VANILLA_ISO`, or `originals/melee-ntsc-1.02.iso`; without
//! it the disc tests print a note and pass).
//!
//! Normals that go through `PSVECNormalize` are compared with a tolerance:
//! their low bits depend on the hardware `frsqrte` estimate, which the port
//! does not reproduce yet. Normals the C writes as constants are exact.

use gc_disc::Disc;
use hsd_dat::Archive;
use melee::math::{Vec2, Vec3};
use melee::mp::check::CollHit;
use melee::mp::data::{MapJoint, MapLine, MapLineRange};
use melee::mp::lib::{coll_joint, MpLib};
use melee::mp::{line_flag, line_group, MapColl};
use std::fs::File;
use std::path::PathBuf;

const FLOOR: u32 = line_flag::FLOOR;
const CEILING: u32 = line_flag::CEILING;
const RIGHT_WALL: u32 = line_flag::RIGHT_WALL;
const LEFT_WALL: u32 = line_flag::LEFT_WALL;
const LEDGE: u16 = line_flag::LEDGE as u16;
const PLATFORM: u16 = line_flag::PLATFORM as u16;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn v3(x: f32, y: f32) -> Vec3 {
    Vec3::new(x, y, 0.0)
}

/// A line for [`build`]: `(v0, v1, prev, next, kind, lo_flags)`.
type Line = (u16, u16, i16, i16, u32, u16);

fn map_line(&(v0, v1, prev, next, kind, lo): &Line) -> MapLine {
    MapLine {
        v0_idx: v0,
        v1_idx: v1,
        prev_id0: prev,
        next_id0: next,
        prev_id1: -1,
        next_id1: -1,
        hi_flags: kind as u16,
        lo_flags: lo,
    }
}

/// Ranges for lines listed in group order (floor, ceiling, right wall,
/// left wall, dynamic) with the given counts.
fn ranges(counts: [i16; 5]) -> [MapLineRange; 5] {
    let mut start = 0;
    counts.map(|count| {
        let range = MapLineRange { start, count };
        start += count;
        range
    })
}

fn joint(ranges: [MapLineRange; 5], vtx_start: i16, vtx_count: i16) -> MapJoint {
    MapJoint {
        ranges,
        left_bound: -1000.0,
        bottom_bound: -1000.0,
        right_bound: 1000.0,
        top_bound: 1000.0,
        vtx_start,
        vtx_count,
    }
}

/// Load a one-joint stage at scale 1.
fn build(verts: &[(f32, f32)], lines: &[Line], counts: [i16; 5]) -> MpLib {
    let ranges = ranges(counts);
    let coll = MapColl {
        verts: verts.iter().map(|&(x, y)| v(x, y)).collect(),
        lines: lines.iter().map(map_line).collect(),
        ranges,
        joints: vec![joint(ranges, 0, verts.len() as i16)],
        x2c: 0,
    };
    MpLib::load(coll, 1.0, false)
}

/// A floating box (an island like Final Destination) plus a sloped
/// pass-through platform:
///
/// ```text
///  line 0: floor      (-10, 0) -> (0, 0)     ledge,  prev 5, next 1
///  line 1: floor      (0, 0) -> (10, 0)      ledge,  prev 0, next 4
///  line 2: floor      (20, 0) -> (30, 5)     platform, unlinked
///  line 3: ceiling    (10, -10) -> (-10, -10)        prev 4, next 5
///  line 4: right wall (10, 0) -> (10, -10)           prev 1, next 3
///  line 5: left wall  (-10, -10) -> (-10, 0)         prev 3, next 0
/// ```
fn island() -> MpLib {
    build(
        &[
            (-10.0, 0.0),
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, -10.0),
            (-10.0, -10.0),
            (20.0, 0.0),
            (30.0, 5.0),
        ],
        &[
            (0, 1, 5, 1, FLOOR, LEDGE),
            (1, 2, 0, 4, FLOOR, LEDGE),
            (5, 6, -1, -1, FLOOR, PLATFORM),
            (3, 4, 4, 5, CEILING, 0),
            (2, 3, 1, 3, RIGHT_WALL, 0),
            (4, 0, 3, 0, LEFT_WALL, 0),
        ],
        [3, 1, 1, 1, 0],
    )
}

/// Within a few ulps (for `PSVECNormalize` results).
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 4.0 * f32::EPSILON * b.abs().max(1.0)
}

fn close3(a: Vec3, b: Vec3) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
}

#[test]
fn floor_offset_walks_to_the_floor_under_x() {
    let mp = island();
    // Start on line 0; x = 5 is past its end, so the walk moves to line 1.
    // (0 - 0) * (5 - 0) / (10 - 0) + 0 - 3 = -3, plus the double 0.0001.
    let hit = mp.floor_offset(0, v3(5.0, 3.0)).unwrap();
    assert_eq!(hit.line_id, 1);
    assert_eq!(hit.offset, (-3.0f64 + 0.0001) as f32);
    assert_eq!(u32::from(LEDGE), hit.flags);
    // (-(0 - 0), 10 - 0, 0) normalized: x keeps the sign of -0.0.
    assert!(hit.normal.x == 0.0 && hit.normal.x.is_sign_negative());
    assert!(close(hit.normal.y, 1.0) && hit.normal.z == 0.0);

    // Sloped platform: 5 * (24 - 20) / 10 + 0 - 0 = 2.
    let hit = mp.floor_offset(2, v3(24.0, 0.0)).unwrap();
    assert_eq!(hit.offset, (2.0f64 + 0.0001) as f32);
    assert_eq!(hit.flags, u32::from(PLATFORM));
    let n = 1.0 / 125f32.sqrt();
    assert!(close3(hit.normal, v3(-5.0 * n, 10.0 * n)));
}

#[test]
fn floor_offset_clamps_within_a_tenth_past_the_chain() {
    let mp = island();
    // Past the right end by 0.05 (next is a wall): x clamps to 10.
    let hit = mp.floor_offset(1, v3(10.05, 0.0)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (1, 0.0001f64 as f32));
    // Past by 0.2: no floor.
    assert_eq!(mp.floor_offset(1, v3(10.2, 0.0)), None);
    // Walking left from line 1 onto line 0, then past its end by 0.05.
    let hit = mp.floor_offset(1, v3(-10.05, 0.0)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (0, 0.0001f64 as f32));
    assert_eq!(mp.floor_offset(1, v3(-10.5, 0.0)), None);
}

#[test]
fn floor_offset_can_turn_back_once() {
    // A (0..10) links to B (10.5..20). x = 10.2 is past A's end and before
    // B's start: the walk goes right to B (the floor walk never marks the
    // rightward direction), left back to A, and then clamps to A's end.
    let mp = build(
        &[(0.0, 0.0), (10.0, 0.0), (10.5, 1.0), (20.0, 1.0)],
        &[(0, 1, -1, 1, FLOOR, 0), (2, 3, 0, -1, FLOOR, 0)],
        [2, 0, 0, 0, 0],
    );
    let hit = mp.floor_offset(0, v3(10.2, 0.0)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (0, 0.0001f64 as f32));
}

#[test]
fn ceiling_and_wall_offsets() {
    let mp = island();
    // Ceiling: 0 * (0 - 10) / (-10 - 10) + -10 - -12 = 2, minus 0.0001.
    let hit = mp.ceiling_offset(3, v3(0.0, -12.0)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (3, (2.0f64 - 0.0001) as f32));
    assert!(hit.normal.x.is_sign_negative() && close(hit.normal.y, -1.0));
    // Within 0.1 past the left end (the next line is a wall).
    let hit = mp.ceiling_offset(3, v3(-10.05, -12.0)).unwrap();
    assert_eq!(hit.offset, (2.0f64 - 0.0001) as f32);
    assert_eq!(mp.ceiling_offset(3, v3(-10.5, -12.0)), None);

    // Left wall: -10 + 0 * 5 / 10 - -12 = 2, no bias.
    let hit = mp.left_wall_offset(5, v3(-12.0, -5.0)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (5, 2.0));
    assert!(close3(hit.normal, v3(-1.0, 0.0)));
    // Right wall: 10 + (0 * -5) / -10 - 12 = -2.
    let hit = mp.right_wall_offset(4, v3(12.0, -5.0)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (4, -2.0));
    assert!(close3(hit.normal, v3(1.0, 0.0)));
    assert_eq!(mp.right_wall_offset(4, v3(12.0, 0.5)), None);
}

#[test]
fn left_wall_offset_does_not_clamp_after_turning() {
    // Left walls W1 (-10,-10)->(-10,-5) and W2 (-10,-4)->(-12,0), linked.
    // y = -4.5 is above W1, so the walk moves up to W2 (dir = 1); there it
    // is below W2's start, and with dir == 1 the C breaks without clamping
    // y to -4. The offset extrapolates W2:
    // -10 + (-2) * (-4.5 - -4) / 4 - -20 = 10.25 (10 if y were clamped).
    let mp = build(
        &[(-10.0, -10.0), (-10.0, -5.0), (-10.0, -4.0), (-12.0, 0.0)],
        &[(0, 1, -1, 1, LEFT_WALL, 0), (2, 3, 0, -1, LEFT_WALL, 0)],
        [0, 0, 0, 2, 0],
    );
    let hit = mp.left_wall_offset(0, v3(-20.0, -4.5)).unwrap();
    assert_eq!((hit.line_id, hit.offset), (1, 10.25));
}

#[test]
fn line_extended_pushes_linked_ends_out() {
    let mp = island();
    // Line 1 has both neighbors. distance = 10; x0 = 0 + (0 - 10) / 10;
    // then x1 uses the moved x0: 10 + (10 - -1) / 10.
    assert_eq!(mp.line_extended(1), (-1.0, 0.0, 10.0 + 11.0f32 / 10.0, 0.0));
    // The platform has no neighbors.
    assert_eq!(mp.line_extended(2), (20.0, 0.0, 30.0, 5.0));
}

#[test]
fn check_floor_finds_the_nearest_floor_below() {
    let mut mp = island();
    // Straight down through line 1 (flat): constant normal (0, 1, 0).
    let hit = mp.check_floor(v(5.0, 5.0), v(5.0, -5.0), 0.0, -1, -1, -1, None);
    let expected = CollHit {
        pos: v3(5.0, 0.0),
        line_id: 1,
        flags: u32::from(LEDGE),
        normal: v3(0.0, 1.0),
    };
    assert_eq!(hit, Some(expected));
    assert!(!mp.did_check_bounding);
    // Flat floors only stop downward segments.
    assert_eq!(
        mp.check_floor(v(5.0, -5.0), v(5.0, 5.0), 0.0, -1, -1, -1, None),
        None
    );
    // y_offset raises every floor.
    let hit = mp.check_floor(v(5.0, 5.0), v(5.0, -5.0), 2.0, -1, -1, -1, None);
    assert_eq!(hit.unwrap().pos, v3(5.0, 2.0));
    // Skips by line and joint.
    assert_eq!(
        mp.check_floor(v(5.0, 5.0), v(5.0, -5.0), 0.0, 1, -1, -1, None),
        None
    );
    assert_eq!(
        mp.check_floor(v(5.0, 5.0), v(5.0, -5.0), 0.0, -1, 0, -1, None),
        None
    );
    assert_eq!(
        mp.check_floor(v(5.0, 5.0), v(5.0, -5.0), 0.0, -1, -1, 0, None),
        Some(expected)
    );
    assert_eq!(
        mp.check_floor(v(5.0, 5.0), v(5.0, -5.0), 0.0, -1, -1, 1, None),
        None
    );
}

#[test]
fn check_floor_on_a_slope() {
    let mut mp = island();
    // Through the platform (20,0)-(30,5) at x = 24: hs_b0 = 80,
    // hs_b1 = -120, det = 0, area = 200, t = 0.4, so (24, 2).
    let hit = mp
        .check_floor(v(24.0, 10.0), v(24.0, -10.0), 0.0, -1, -1, -1, None)
        .unwrap();
    assert_eq!((hit.pos, hit.line_id), (v3(24.0, 2.0), 2));
    assert_eq!(hit.flags, u32::from(PLATFORM));
    let n = 1.0 / 125f32.sqrt();
    assert!(close3(hit.normal, v3(-5.0 * n, 10.0 * n)));
}

#[test]
fn check_floor_callback_sees_every_visited_line_first() {
    let mut mp = island();
    let mut seen = Vec::new();
    let mut reject_ledges = |mp: &MpLib, line: i32| {
        seen.push(line);
        mp.line_flags(line) & u32::from(LEDGE) == 0
    };
    let hit = mp.check_floor(
        v(5.0, 5.0),
        v(5.0, -5.0),
        0.0,
        -1,
        -1,
        -1,
        Some(&mut reject_ledges),
    );
    assert_eq!(hit, None);
    assert_eq!(seen, [0, 1, 2]);
}

#[test]
fn check_floor_extends_linked_ends_but_remap_does_not() {
    let mut mp = island();
    // x = 10.5 is past line 1's end but within its one-unit extension.
    let a = v(10.5, 5.0);
    let b = v(10.5, -5.0);
    let hit = mp.check_floor(a, b, 0.0, -1, -1, -1, None).unwrap();
    assert_eq!((hit.pos, hit.line_id), (v3(10.5, 0.0), 1));
    assert_eq!(mp.check_floor_remap(a, b, 0.0, -1, -1, -1, None), None);
    // Inside the line both agree (static joints: no remapping).
    let a = v(5.0, 5.0);
    let b = v(5.0, -5.0);
    assert_eq!(
        mp.check_floor(a, b, 0.0, -1, -1, -1, None),
        mp.check_floor_remap(a, b, 0.0, -1, -1, -1, None)
    );
}

#[test]
fn check_floor_keeps_the_closest_hit() {
    // Two flat platforms over x in [0, 10] at y = 0 (line 0) and y = 5.
    let mut mp = build(
        &[(0.0, 0.0), (10.0, 0.0), (0.0, 5.0), (10.0, 5.0)],
        &[(0, 1, -1, -1, FLOOR, 0), (2, 3, -1, -1, FLOOR, 0)],
        [2, 0, 0, 0, 0],
    );
    let hit = mp.check_floor(v(5.0, 10.0), v(5.0, -10.0), 0.0, -1, -1, -1, None);
    assert_eq!(hit.map(|h| (h.line_id, h.pos)), Some((1, v3(5.0, 5.0))));
}

#[test]
fn ceiling_and_wall_checks() {
    let mut mp = island();
    // Up into the ceiling (flat, extended to [-11.05, 11]).
    let hit = mp.check_ceiling(v(0.0, -12.0), v(0.0, -8.0), -1, -1);
    assert_eq!(
        hit,
        Some(CollHit {
            pos: v3(0.0, -10.0),
            line_id: 3,
            flags: 0,
            normal: v3(0.0, -1.0),
        })
    );
    assert_eq!(mp.check_ceiling(v(0.0, -8.0), v(0.0, -12.0), -1, -1), None);

    // Rightward into the (vertical) left wall.
    let hit = mp.check_left_wall(v(-12.0, -5.0), v(-8.0, -5.0), -1, -1);
    assert_eq!(
        hit,
        Some(CollHit {
            pos: v3(-10.0, -5.0),
            line_id: 5,
            flags: 0,
            normal: v3(-1.0, 0.0),
        })
    );
    assert_eq!(
        mp.check_left_wall(v(-8.0, -5.0), v(-12.0, -5.0), -1, -1),
        None
    );

    // Leftward into the right wall: new_y = -0.0 * -2 + -5 = -5.
    let hit = mp.check_right_wall(v(12.0, -5.0), v(8.0, -5.0), -1, -1);
    assert_eq!(
        hit,
        Some(CollHit {
            pos: v3(10.0, -5.0),
            line_id: 4,
            flags: 0,
            normal: v3(1.0, 0.0),
        })
    );
    assert_eq!(
        mp.check_right_wall(v(8.0, -5.0), v(12.0, -5.0), -1, -1),
        None
    );
    // The remap versions agree on static geometry.
    assert_eq!(
        mp.check_right_wall_remap(v(12.0, -5.0), v(8.0, -5.0), -1, -1),
        hit
    );
}

#[test]
fn check_multiple_selects_and_breaks_ties_toward_floors() {
    let mut mp = island();
    // (12, -3) -> (8, -7) crosses only the right wall, at (10, -5).
    let hit = mp.check_all(-1, -1, v(12.0, -3.0), v(8.0, -7.0)).unwrap();
    assert_eq!((hit.line_id, hit.pos), (4, v3(10.0, -5.0)));
    assert_eq!(
        mp.check_multiple(v(12.0, -3.0), v(8.0, -7.0), 0x7, -1, -1),
        None
    );
    // (12, 2) -> (8, -2) reaches the floor's extension and the right wall
    // at the same point (10, 0), both at squared distance 8. The floor is
    // checked first and a tie does not replace it.
    let hit = mp.check_all(-1, -1, v(12.0, 2.0), v(8.0, -2.0)).unwrap();
    assert_eq!((hit.line_id, hit.pos), (1, v3(10.0, 0.0)));
    let hit = mp
        .check_all_remap(-1, -1, v(12.0, 2.0), v(8.0, -2.0))
        .unwrap();
    // Remap floors are not extended (line 1 ends at x = 10): new_x = 10
    // is exactly the end, so it still hits.
    assert_eq!((hit.line_id, hit.pos), (1, v3(10.0, 0.0)));
}

#[test]
fn remap_carries_the_start_with_a_moving_floor() {
    let mut mp = island();
    // Joint 0 moves (B8): line 1 was at y = -1 last frame and is at 0 now.
    mp.joints[0].flags |= coll_joint::B8;
    for vtx in [1, 2] {
        mp.vtx[vtx].prev_pos.y = -1.0;
    }
    // A segment from (5, -0.5) to (5, -3) starts below the floor, but in
    // the floor's previous frame it was above it: remapped, the start is
    // (5, 0.5), so the floor is hit. The hit is behind the original start
    // in the segment's direction, so its squared distance is negated.
    let hit = mp
        .check_floor_remap(v(5.0, -0.5), v(5.0, -3.0), 0.0, -1, -1, -1, None)
        .unwrap();
    assert_eq!((hit.line_id, hit.pos), (1, v3(5.0, 0.0)));
    // Without remapping there is no hit.
    assert_eq!(
        mp.check_floor(v(5.0, -0.5), v(5.0, -3.0), 0.0, -1, -1, -1, None),
        None
    );
    // mpGetSpeed reports the carry: remap (5, -0.5) up by one unit.
    assert_eq!(mp.line_speed(1, v3(5.0, -0.5)), Some(v3(0.0, 1.0)));
}

#[test]
fn wall_vertex_crossing_a_moving_ecb_edge() {
    // A right wall (1.5, 8) -> (1.5, 2) and an ECB edge that moved from
    // x = 0 to x = 2 (given top to bottom). Remapping the vertex (1.5, 8)
    // into the moved edge's frame gives (3.5, 8): t = 0.2, so
    // 1.5 + 0.8 * 2 + 0.2 * 2. Its path (3.5, 8) -> (1.5, 8) crosses the
    // edge at (2, 8): hs_b0 = 15, hs_b1 = -5, area = 20, t = 0.2. The
    // distance from the old position is 0.25, negated because the hit is
    // behind it.
    let mut mp = build(
        &[(1.5, 8.0), (1.5, 2.0)],
        &[(0, 1, -1, -1, RIGHT_WALL, 0)],
        [0, 0, 1, 0, 0],
    );
    let (a0, a1, b0, b1) = (v(0.0, 10.0), v(0.0, 0.0), v(2.0, 10.0), v(2.0, 0.0));
    assert_eq!(
        mp.check_right_wall_vertices(a0, a1, b0, b1, -1, -1),
        Some(0)
    );
    // An edge that does not reach the wall.
    let c0 = v(1.0, 10.0);
    let c1 = v(1.0, 0.0);
    assert_eq!(mp.check_right_wall_vertices(a0, a1, c0, c1, -1, -1), None);
    // The left-wall version only looks at left walls.
    assert_eq!(mp.check_left_wall_vertices(a0, a1, b0, b1, -1, -1), None);
}

#[test]
fn bounding_prepass_hides_far_joints_unless_already_checked() {
    // Joint 1's lines are near the origin but its bounding box is far
    // away, so the check's own pre-pass marks it TOO_FAR and skips it.
    let lines = [(0, 1, -1, -1, FLOOR, 0), (2, 3, -1, -1, FLOOR, 0)];
    let mut ranges0 = [MapLineRange::default(); 5];
    ranges0[line_group::FLOOR] = MapLineRange { start: 0, count: 1 };
    let mut ranges1 = [MapLineRange::default(); 5];
    ranges1[line_group::FLOOR] = MapLineRange { start: 1, count: 1 };
    let mut far = joint(ranges1, 2, 2);
    far.left_bound = 500.0;
    far.right_bound = 600.0;
    let coll = MapColl {
        verts: vec![v(-20.0, 0.0), v(-10.0, 0.0), v(0.0, 3.0), v(10.0, 3.0)],
        lines: lines.iter().map(map_line).collect(),
        ranges: ranges([2, 0, 0, 0, 0]),
        joints: vec![joint(ranges0, 0, 2), far],
        x2c: 0,
    };
    let mut mp = MpLib::load(coll, 1.0, false);
    let (a, b) = (v(5.0, 10.0), v(5.0, -10.0));
    assert_eq!(mp.check_floor(a, b, 0.0, -1, -1, -1, None), None);
    assert!(mp.joints.iter().all(|j| j.flags & coll_joint::TOO_FAR == 0));
    // A caller that already ran a pre-pass covering joint 1 sees it.
    mp.bounding_check(-1000.0, -1000.0, 1000.0, 1000.0);
    let hit = mp.check_floor(a, b, 0.0, -1, -1, -1, None);
    assert_eq!(hit.map(|h| (h.line_id, h.pos)), Some((1, v3(5.0, 3.0))));
    assert!(mp.did_check_bounding);
    mp.uncheck_bounding();
    // Hidden or disabled joints are always too far.
    mp.joint_hide(0);
    mp.bounding_check(-1000.0, -1000.0, 1000.0, 1000.0);
    assert_ne!(mp.joints[0].flags & coll_joint::TOO_FAR, 0);
    // B10 joints never are.
    mp.joints[1].flags |= coll_joint::B10;
    mp.bounding_check(0.0, 0.0, 1.0, 1.0);
    assert_eq!(mp.joints[1].flags & coll_joint::TOO_FAR, 0);
}

#[test]
fn floor_below_returns_the_first_floor_in_order() {
    let mut mp = island();
    assert_eq!(mp.floor_below(v3(5.0, 3.0), -1, -1), 1);
    // On the slope (20,0)-(30,5) at x = 25 the surface is
    // fmadds(5 / 10, 25 - 20, 0) = 2.5.
    assert_eq!(mp.floor_below(v3(25.0, 1.0), -1, -1), -1);
    assert_eq!(mp.floor_below(v3(25.0, 3.0), -1, -1), 2);
    assert_eq!(mp.floor_below(v3(5.0, -1.0), -1, -1), -1);
    assert_eq!(mp.floor_below(v3(5.0, 3.0), 0, -1), -1);
}

#[test]
fn find_ledge_picks_the_outermost_ledge_in_the_box() {
    let mut mp = island();
    // A box around the right end: only line 1 overlaps; dir < 0 keeps v1.
    assert_eq!(
        mp.find_ledge(-1, -1, -1, -1, 8.0, -2.0, 12.0, 2.0),
        (1, Some(v3(10.0, 0.0)))
    );
    // Around the left end, dir > 0 keeps v0.
    assert_eq!(
        mp.find_ledge(-1, -1, -1, 1, -12.0, -2.0, -8.0, 2.0),
        (0, Some(v3(-10.0, 0.0)))
    );
    // Both ledges overlap; dir < 0 keeps the larger v1 (10), clamped to
    // the box's right side.
    assert_eq!(
        mp.find_ledge(-1, -1, -1, -1, -5.0, -2.0, 5.0, 2.0),
        (1, Some(v3(5.0, 0.0)))
    );
    assert_eq!(
        mp.find_ledge(1, -1, -1, -1, 8.0, -2.0, 12.0, 2.0),
        (-1, None)
    );
}

#[test]
#[should_panic]
fn find_ledge_without_direction_asserts() {
    let _ = island().find_ledge(-1, -1, -1, 0, -1.0, -1.0, 1.0, 1.0);
}

#[test]
fn line_walkers() {
    let mp = island();
    assert_eq!(mp.line_next_non_floor(0), 4);
    assert_eq!(mp.line_prev_non_floor(1), 5);
    assert_eq!(mp.line_next_non_floor(2), -1);
    assert_eq!(mp.line_next_non_ceiling(3), 5);
    assert_eq!(mp.line_prev_non_ceiling(3), 4);
    assert_eq!(mp.line_next_non_right_wall(4), 3);
    assert_eq!(mp.line_prev_non_right_wall(4), 1);
    assert_eq!(mp.line_next_non_left_wall(5), 0);
    assert_eq!(mp.line_prev_non_left_wall(5), 3);
    assert_eq!(mp.line_next0_non_floor(0), 4);
    assert_eq!(mp.line_prev0_non_floor(1), 5);
    assert_eq!(mp.line_next0_non_ceiling(3), 5);
    assert_eq!(mp.line_prev0_non_ceiling(3), 4);
    assert_eq!(mp.floor_next0_end(0), v3(10.0, 0.0));
    assert_eq!(mp.floor_prev0_end(1), v3(-10.0, 0.0));

    assert_eq!(mp.floor_get_right(0), v3(10.0, 0.0));
    assert_eq!(mp.floor_get_left(1), v3(-10.0, 0.0));
    assert_eq!(mp.ceiling_get_right(3), v3(10.0, -10.0));
    assert_eq!(mp.ceiling_get_left(3), v3(-10.0, -10.0));
    assert_eq!(mp.left_wall_get_top(5), v3(-10.0, 0.0));
    assert_eq!(mp.left_wall_get_bottom(5), v3(-10.0, -10.0));
    assert_eq!(mp.right_wall_get_top(4), v3(10.0, 0.0));
    assert_eq!(mp.right_wall_get_bottom(4), v3(10.0, -10.0));

    // Walking a closed ring of floors loops back to the start: -1.
    let ring = build(
        &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)],
        &[
            (0, 1, 2, 1, FLOOR, 0),
            (1, 2, 0, 2, FLOOR, 0),
            (2, 0, 1, 0, FLOOR, 0),
        ],
        [3, 0, 0, 0, 0],
    );
    assert_eq!(ring.line_next_non_floor(0), -1);
    assert!(ring.lines_connected(0, 2));
}

#[test]
fn alternate_links_and_junction_walks() {
    // Floor A (0..10) has primary next B (a wall) and alternate next C, a
    // floor starting 0.5 away (within the 2-unit link radius).
    let mut coll_lines: Vec<MapLine> = [
        (0, 1, -1, 2, FLOOR, 0),
        (2, 3, -1, -1, FLOOR, 0),
        (1, 4, 0, -1, RIGHT_WALL, 0),
    ]
    .iter()
    .map(map_line)
    .collect();
    coll_lines[0].next_id1 = 1;
    coll_lines[1].prev_id1 = 0;
    let ranges = ranges([2, 0, 1, 0, 0]);
    let coll = MapColl {
        verts: vec![
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(10.5, 0.0),
            v(20.0, 0.0),
            v(10.0, -10.0),
        ],
        lines: coll_lines,
        ranges,
        joints: vec![joint(ranges, 0, 5)],
        x2c: 0,
    };
    let mut mp = MpLib::load(coll, 1.0, false);
    assert_eq!(mp.line_get_next(0), 1);
    // The raw walker ignores the alternate link.
    assert_eq!(mp.line_next0_non_floor(0), 2);
    // Arrived through the alternate link: returned at once.
    assert_eq!(mp.floor_next_via_alt(0), 1);
    // From C there is nothing further.
    assert_eq!(mp.floor_next_via_alt(1), -1);
    assert_eq!(mp.floor_get_right(0), v3(20.0, 0.0));
    assert!(mp.lines_connected(0, 1));
    assert!(mp.lines_connected(1, 0));
    // Hiding C breaks the alternate link.
    mp.lines[1].flags |= line_flag::HIDDEN;
    assert_eq!(mp.line_get_next(0), 2);
    assert_eq!(mp.floor_next_via_alt(0), -1);
}

#[test]
fn line_getters_and_setters() {
    let mut mp = island();
    assert_eq!(mp.line_v0_pos(2), v3(20.0, 0.0));
    assert_eq!(mp.line_v1_pos(2), v3(30.0, 5.0));
    assert_eq!(mp.line_kind(4), RIGHT_WALL);
    assert_eq!(mp.line_flags(2), u32::from(PLATFORM));
    // (old & ~0xFF) | flags, truncated to a halfword.
    mp.line_set_flags_low(2, 0x05);
    assert_eq!(mp.line_flags(2), 0x105);
    mp.line_set_flags_low(2, 0x1234);
    assert_eq!(mp.line_flags(2), 0x1334);
    assert!(close3(mp.line_normal(1), v3(0.0, 1.0)));
    assert!(!mp.line_active(-1));
    assert!(mp.line_active(1));
    assert!(mp.lines_connected(0, 1));
    assert!(!mp.lines_connected(0, 4));
    assert!(!mp.lines_connected(0, 2));
    assert!(mp.lines_connected(2, 2));
    assert_eq!(mp.joint_from_line(4), 0);
    assert_eq!(mp.joint_from_line(-1), -1);
    assert_eq!(mp.line_speed(2, v3(24.0, 2.0)), Some(v3(0.0, 0.0)));
    mp.line_set_pos(2, 21.0, 2.0, 31.0, 7.0);
    assert_eq!(mp.line_v1_pos(2), v3(31.0, 7.0));
    // A translation by (1, 2) carries a point by the same amount.
    assert_eq!(mp.line_speed(2, v3(24.0, 2.0)), Some(v3(1.0, 2.0)));
    // Data positions plus offsets (base_pos is unscaled).
    mp.line_set_pos_from_base(2, 0.5, 0.0, 0.5, 0.0);
    assert_eq!(mp.line_v0_pos(2), v3(20.5, 0.0));
    assert_eq!(mp.vtx_pos(6), v(30.5, 5.0));
}

#[test]
#[should_panic(expected = "not found lineID")]
fn lineid_check_halts_on_minus_one() {
    island().line_kind(-1);
}

#[test]
fn floor_walk_moves_along_the_surface() {
    let mp = island();
    // From x = 5 on line 1, 3 units right. The start is lifted by the
    // floor query's 0.0001 bias; the 5-unit run to v1 is longer than 3,
    // so t = 3 / 5 and x = fmadds(0.6, 10 - 5, 5) = 8.
    let walk = mp.floor_walk(1, v3(5.0, 0.0), 3.0, 0.0).unwrap();
    let bias = 0.0001f64 as f32;
    let t = 3.0f32 / 5.0;
    let y = (f64::from(t) * f64::from(-bias) + f64::from(bias)) as f32;
    assert!(walk.reached);
    assert_eq!((walk.line_id, walk.pos), (1, v3(8.0, y)));
    assert_eq!(walk.surface.unwrap().0, u32::from(LEDGE));

    // Across a line joint: 5 units to v1 of line 0, then 3 more on line 1
    // from (0, 0): t = 0.3, x = fmadds(0.3, 10, 0) = 3.
    let walk = mp.floor_walk(0, v3(-5.0, 0.0), 8.0, 0.0).unwrap();
    assert_eq!(
        (walk.reached, walk.line_id, walk.pos),
        (true, 1, v3(3.0, 0.0))
    );

    // Leftward: t = 2 / 5, x = fmadds(0.4, 0 - 5, 5) = 3.
    let walk = mp.floor_walk(1, v3(5.0, 0.0), -2.0, 0.0).unwrap();
    let t = 2.0f32 / 5.0;
    let y = (f64::from(t) * f64::from(-bias) + f64::from(bias)) as f32;
    assert_eq!((walk.line_id, walk.pos), (1, v3(3.0, y)));

    // Off the edge onto the right wall: the wall's 10 units exceed the
    // wall limit, so the walk fails at the wall's v1 and the line is not a
    // floor.
    let walk = mp.floor_walk(1, v3(5.0, 0.0), 10.0, 0.0).unwrap();
    assert!(!walk.reached);
    assert_eq!(
        (walk.line_id, walk.pos, walk.surface),
        (-1, v3(10.0, -10.0), None)
    );

    // No floor under the start.
    assert_eq!(mp.floor_walk(1, v3(50.0, 0.0), 1.0, 0.0), None);
}

#[test]
fn joint_hide_unhide_and_bounding() {
    let mut mp = island();
    mp.vtx[6].pos = v(31.0, 6.0);
    mp.joint_hide(0);
    assert!(mp.lines.iter().all(|l| l.flags & line_flag::HIDDEN != 0));
    assert!(!mp.line_active(1));
    mp.joint_unhide(0);
    assert!(mp.lines.iter().all(|l| l.flags & line_flag::HIDDEN == 0));
    // Unhiding resets previous positions.
    assert_eq!(mp.vtx[6].prev_pos, v(31.0, 6.0));

    mp.joints[0].bounding_min = v(0.0, 0.0);
    mp.joints[0].bounding_max = v(0.0, 0.0);
    mp.joint_update_bounding(0);
    assert_eq!(mp.joints[0].bounding_min, v(-40.0, -40.0));
    assert_eq!(mp.joints[0].bounding_max, v(61.0, 36.0));
}

#[test]
fn dynamic_lines_are_classified_by_slope() {
    // One vertex pair per dynamic line; the second vertex sets the slope.
    let tan60_up = f32::from_bits(1.732_050_8f32.to_bits() + 1);
    let cases: [((f32, f32), u32); 9] = [
        ((10.0, 5.0), FLOOR),
        ((1.0, 2.0), LEFT_WALL),
        ((1.0, -2.0), RIGHT_WALL),
        ((-10.0, 2.0), CEILING),
        ((-1.0, -1.0), RIGHT_WALL),
        ((-1.0, 1.0), LEFT_WALL),
        ((0.0, 1.0), LEFT_WALL),
        ((0.0, -1.0), RIGHT_WALL),
        // dy / dx = 1.7320509f32 exceeds the double TAN60
        // (1.7320508368950045) but not TAN60 rounded to single, which is
        // this same value: the double compare makes it a wall.
        ((1.0, tan60_up), LEFT_WALL),
    ];
    let mut verts = Vec::new();
    let mut lines = Vec::new();
    for (i, &((dx, dy), _)) in cases.iter().enumerate() {
        verts.push((0.0, 0.0));
        verts.push((dx, dy));
        let i = i as u16;
        lines.push((2 * i, 2 * i + 1, -1, -1, FLOOR, 0x400));
    }
    let mut mp = build(&verts, &lines, [0, 0, 0, 0, cases.len() as i16]);
    mp.joint_update_dynamics(0);
    for (i, &(_, kind)) in cases.iter().enumerate() {
        let flags = mp.lines[i].flags;
        assert_eq!(flags & line_flag::KIND, kind, "case {i}");
        // lo_flags 0x400: floors become enabled platforms, others disabled.
        if kind == FLOOR {
            assert_ne!(flags & line_flag::PLATFORM, 0);
            assert_ne!(flags & line_flag::ENABLED, 0);
            assert_eq!(
                mp.line_flags(i as i32) & u32::from(PLATFORM),
                u32::from(PLATFORM)
            );
        } else {
            assert_eq!(flags & line_flag::ENABLED, 0, "case {i}");
        }
    }
}

#[test]
fn joint_list_add_remove_and_islands() {
    let mut coll = island_coll();
    coll.joints = vec![coll.joints[0]; 3];
    let mut mp = MpLib::load(coll, 1.0, false);
    // Removing the middle joint relinks the list and disables its lines.
    let update = mp.joint_list_remove(1).unwrap();
    assert_eq!(
        (
            update.joint_id,
            update.vtx_start,
            update.vtx_count,
            update.flag
        ),
        (1, 0, 7, false)
    );
    assert_eq!(mp.joint_list().collect::<Vec<_>>(), [0, 2]);
    assert!(mp.lines.iter().all(|l| l.flags & line_flag::ENABLED == 0));
    assert_eq!(mp.joint_list_remove(1), None);
    // Re-adding appends it at the end.
    let update = mp.joint_list_add(1).unwrap();
    assert!(update.flag);
    assert_eq!(mp.joint_list().collect::<Vec<_>>(), [0, 2, 1]);
    assert_eq!(mp.joint_list_end, Some(1));
    assert!(mp.lines.iter().all(|l| l.flags & line_flag::ENABLED != 0));
    assert_eq!(mp.joint_list_add(1), None);
    // Removing the end moves the end back.
    let _ = mp.joint_list_remove(1);
    assert_eq!(mp.joint_list_end, Some(2));
    let update = mp.joint_set_b11(0);
    assert!(!update.flag);
    let update = mp.joint_clear_b11(0);
    assert!(update.flag);
    // Disabling one line goes through its joint (joint 0 holds vertex 0).
    let update = mp.line_disable(0).unwrap();
    assert_eq!(update.joint_id, 0);
    assert_eq!(mp.lines[0].flags & line_flag::ENABLED, 0);
}

fn island_coll() -> MapColl {
    island().coll_data
}

#[test]
fn joints_connect_where_their_vertices_meet() {
    // Joint 0: floor A (0,0)-(10,0). Joint 1: floor B (10.5,0)-(20,0).
    let lines = [(0, 1, -1, -1, FLOOR, 0), (2, 3, -1, -1, FLOOR, 0)];
    let mut r0 = [MapLineRange::default(); 5];
    r0[line_group::FLOOR] = MapLineRange { start: 0, count: 1 };
    let mut r1 = [MapLineRange::default(); 5];
    r1[line_group::FLOOR] = MapLineRange { start: 1, count: 1 };
    let coll = MapColl {
        verts: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.5, 0.0), v(20.0, 0.0)],
        lines: lines.iter().map(map_line).collect(),
        ranges: ranges([2, 0, 0, 0, 0]),
        joints: vec![joint(r0, 0, 2), joint(r1, 2, 2)],
        x2c: 0,
    };
    let mut mp = MpLib::load(coll, 1.0, false);
    assert_eq!(mp.line_get_next(0), -1);
    mp.joint_connect_all();
    assert_eq!(
        (
            mp.coll_data.lines[0].next_id1,
            mp.coll_data.lines[1].prev_id1
        ),
        (1, 0)
    );
    assert_eq!(mp.line_get_next(0), 1);
    assert_eq!(mp.line_get_prev(1), 0);
    assert_eq!(mp.joint_from_line(1), 1);
    assert_eq!(mp.floor_get_left(1), v3(0.0, 0.0));

    // The floor extent covers both joints' floors once any joint changed.
    mp.update_floor_bounds();
    let b = mp.floor_bounds;
    assert_eq!((b.left, b.right, b.bottom, b.top), (0.0, 20.0, 0.0, 0.0));
    assert!(mp.joints.iter().all(|j| !j.xe));
    // Nothing changed since: the extent is not recomputed.
    mp.vtx[3].pos.x = 30.0;
    mp.update_floor_bounds();
    assert_eq!(mp.floor_bounds.right, 20.0);
}

// ---------------------------------------------------------------------------
// Real stages from the vanilla disc.

/// Stage files come from the vanilla disc: 20XX replaces many of them with
/// alternates. Uses `MELEE_VANILLA_ISO` or `originals/melee-ntsc-1.02.iso`.
fn vanilla_archive(path: &str) -> Option<Archive> {
    let iso = std::env::var_os("MELEE_VANILLA_ISO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../originals/melee-ntsc-1.02.iso")
        });
    let Ok(mut file) = File::open(&iso) else {
        eprintln!("skipping: no vanilla disc image at {}", iso.display());
        return None;
    };
    let disc = Disc::read(&mut file).expect("readable disc");
    let bytes = disc.read_file(&mut file, path).expect("file on disc");
    Some(Archive::parse(&bytes).expect("valid archive"))
}

fn load_stage(file: &str) -> Option<MpLib> {
    use melee::gr::{ground_scale, load_ground_param};
    let archive = vanilla_archive(file)?;
    let param = load_ground_param(&archive).unwrap();
    let coll = melee::mp::load_map_coll(&archive).unwrap();
    Some(MpLib::load(coll, ground_scale(Some(&param)), false))
}

#[test]
fn final_destination_queries() {
    let Some(mut mp) = load_stage("GrNLa.dat") else {
        return;
    };
    // Straight down onto the main floor (line 1, flat): exact normal.
    let hit = mp
        .check_floor(v(0.0, 10.0), v(0.0, -10.0), 0.0, -1, -1, -1, None)
        .unwrap();
    assert_eq!(
        hit,
        CollHit {
            pos: v3(0.0, 0.0),
            line_id: 1,
            flags: 0,
            normal: v3(0.0, 1.0),
        }
    );
    // Height above the floor: 0 - 10, plus the double 0.0001.
    let floor = mp.floor_offset(1, v3(0.0, 10.0)).unwrap();
    assert_eq!(
        (floor.line_id, floor.offset),
        (1, (-10.0f64 + 0.0001) as f32)
    );
    assert!(floor.normal.x.is_sign_negative() && close(floor.normal.y, 1.0));
    // Line 1 spans x = +-75 with neighbors on both sides; the right end is
    // pushed out from the already-moved left end: 75 + (75 - -76) / 150.
    assert_eq!(
        mp.line_extended(1),
        (-76.0, 0.0, 75.0 + 151.0f32 / 150.0, 0.0)
    );

    let ledge_x = mp.vtx_pos(i32::from(mp.coll_data.lines[2].v1_idx)).x;
    assert!((ledge_x - 85.5657).abs() < 1e-4);
    // Near the right ledge: line 2, with the ledge flag.
    let hit = mp
        .check_floor(v(85.0, 5.0), v(85.0, -5.0), 0.0, -1, -1, -1, None)
        .unwrap();
    assert_eq!((hit.line_id, hit.flags), (2, 0x200));
    let ledge = mp.find_ledge(-1, -1, -1, -1, 80.0, -5.0, 90.0, 5.0);
    assert_eq!(ledge, (2, Some(v3(ledge_x, 0.0))));
    let left_x = mp.line_v0_pos(0).x;
    assert!((left_x + 85.5657).abs() < 1e-4);
    let ledge = mp.find_ledge(-1, -1, -1, 1, -90.0, -5.0, -80.0, 5.0);
    assert_eq!(ledge, (0, Some(v3(left_x, 0.0))));

    // The vertical side walls below the ledges.
    let hit = mp.check_right_wall(v(90.0, -5.0), v(80.0, -5.0), -1, -1);
    assert_eq!(
        hit,
        Some(CollHit {
            pos: v3(mp.line_v0_pos(9).x, -5.0),
            line_id: 9,
            flags: 0,
            normal: v3(1.0, 0.0),
        })
    );
    let hit = mp.check_left_wall(v(-90.0, -5.0), v(-80.0, -5.0), -1, -1);
    assert_eq!(
        hit,
        Some(CollHit {
            pos: v3(mp.line_v0_pos(11).x, -5.0),
            line_id: 11,
            flags: 0,
            normal: v3(-1.0, 0.0),
        })
    );
    // The flat underside (line 4).
    let underside = mp.line_v0_pos(4).y;
    let hit = mp.check_ceiling(v(0.0, -60.0), v(0.0, -50.0), -1, -1);
    assert_eq!(
        hit,
        Some(CollHit {
            pos: v3(0.0, underside),
            line_id: 4,
            flags: 0,
            normal: v3(0.0, -1.0),
        })
    );

    assert_eq!(mp.line_next_non_floor(1), 9);
    assert_eq!(mp.line_prev_non_floor(1), 11);
    assert_eq!(mp.floor_get_right(0), v3(ledge_x, 0.0));
    assert_eq!(mp.floor_get_left(2), v3(left_x, 0.0));
    assert!(mp.lines_connected(0, 2));
    assert!(!mp.lines_connected(0, 9));
    assert_eq!(mp.floor_below(v3(0.0, 10.0), -1, -1), 1);
    assert_eq!(mp.joint_from_line(9), 0);
}

#[test]
fn battlefield_queries() {
    let Some(mut mp) = load_stage("GrNBa.dat") else {
        return;
    };
    let platform_y = mp.line_v0_pos(4).y;
    assert!((platform_y - 27.2).abs() < 1e-4);
    // Onto the right side platform (line 4).
    let hit = mp
        .check_floor(v(40.0, 30.0), v(40.0, 20.0), 0.0, -1, -1, -1, None)
        .unwrap();
    assert_eq!(
        hit,
        CollHit {
            pos: v3(40.0, platform_y),
            line_id: 4,
            flags: 0x100,
            normal: v3(0.0, 1.0),
        }
    );
    // A fighter dropping through platforms rejects them in the callback.
    let mut solid_only = |mp: &MpLib, line: i32| mp.line_flags(line) & 0x100 == 0;
    let hit = mp
        .check_floor(
            v(40.0, 30.0),
            v(40.0, -5.0),
            0.0,
            -1,
            -1,
            -1,
            Some(&mut solid_only),
        )
        .unwrap();
    assert_eq!((hit.line_id, hit.pos), (1, v3(40.0, 0.0)));
    // mpLib_8005199C_Floor returns the first floor under the point in line
    // order (the main floor), not the nearer platform.
    assert_eq!(mp.floor_below(v3(40.0, 30.0), -1, -1), 1);

    // Walking from the main floor onto the ledge piece.
    let floor = mp.floor_offset(1, v3(65.0, 0.0)).unwrap();
    assert_eq!((floor.line_id, floor.offset), (5, 0.0001f64 as f32));
    assert_eq!(floor.flags, 0x200);

    assert_eq!(mp.line_next_non_floor(1), 16);
    assert_eq!(mp.line_prev_non_floor(1), 17);
    // Platforms are islands: no neighbors.
    assert_eq!(mp.line_next_non_floor(2), -1);
    assert_eq!(mp.floor_get_left(2), mp.line_v0_pos(2));
    assert_eq!(mp.floor_get_right(2), mp.line_v1_pos(2));
    assert!(!mp.lines_connected(1, 2));

    // The flat center of the underside (line 8).
    let underside = mp.line_v0_pos(8).y;
    let hit = mp
        .check_ceiling(v(0.0, -40.0), v(0.0, -30.0), -1, -1)
        .unwrap();
    assert_eq!(
        (hit.line_id, hit.pos, hit.normal),
        (8, v3(0.0, underside), v3(0.0, -1.0))
    );
}

#[test]
fn tournament_stage_floors_agree_with_floor_offset() {
    // Battlefield, Final Destination, Dream Land, Yoshi's Story, Fountain of
    // Dreams, Pokemon Stadium: a downward check at x = 0 and the floor
    // height query at the hit must agree (two separately ported paths).
    for file in [
        "GrNBa.dat",
        "GrNLa.dat",
        "GrOp.dat",
        "GrSt.dat",
        "GrIz.dat",
        "GrPs.dat",
    ] {
        let Some(mut mp) = load_stage(file) else {
            return;
        };
        let hit = mp
            .check_floor(v(0.0, 200.0), v(0.0, -200.0), 0.0, -1, -1, -1, None)
            .unwrap_or_else(|| panic!("{file}: no floor at x = 0"));
        assert_eq!(mp.line_kind(hit.line_id), FLOOR, "{file}");
        assert!(hit.normal.y > 0.0, "{file}");
        let floor = mp
            .floor_offset(hit.line_id, hit.pos)
            .unwrap_or_else(|| panic!("{file}: floor_offset"));
        assert_eq!(floor.line_id, hit.line_id, "{file}");
        assert!((floor.offset - 0.0001).abs() < 1e-3, "{file}: {floor:?}");
        // Every check leaves the bounding pre-pass undone.
        assert!(!mp.did_check_bounding);
    }
}
