//! Pure 2D segment geometry from `mplib.c`: remapping and the
//! direction-dependent line intersections the collision checks use.
//!
//! These compute in `double` where the C declares `double` locals or
//! compares against `double` literals (the 1.02 code loads those constants
//! with `lfd`), and use the fused ops the machine code uses.

use crate::math::{abs_c, Vec2};
use crate::ppc;

/// The C `ABS` macro on a `double`.
fn abs_c64(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// `mpRemap2d` (0x8004DC90): map point `p` from segment `a0`–`a1` to segment
/// `b0`–`b1`. `p` moves by the displacement of its projection onto `a`
/// (clamped to the segment), interpolated between the two endpoint
/// displacements.
///
/// Audited: 6 fused ops, all double-precision `fmadd` — `dist2` and the
/// projection numerator (`a*b + c*d` fuses the left product), and both
/// interpolated outputs (`px + (1-t)*d0 + t*d1` as two nested `fmadd`s).
/// The degenerate branch (`|dist2| <= 0.0001`) is unfused single precision
/// and adds `b1 - a0` (not `b1 - a1`), as the C does.
pub fn remap_2d(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2, p: Vec2) -> Vec2 {
    let dx = f64::from(a1.x - a0.x);
    let dy = f64::from(a1.y - a0.y);
    let apx = p.x - a0.x;
    let apy = p.y - a0.y;
    let dist2 = ppc::fmadd(dy, dy, dx * dx);
    if abs_c64(dist2) > 0.0001 {
        let mut t = ppc::fmadd(dy, f64::from(apy), dx * f64::from(apx)) / dist2;
        // The C's own clamp (NaN and -0.0 pass through unchanged).
        #[allow(clippy::manual_clamp)]
        if t > 1.0 {
            t = 1.0;
        } else if t < 0.0 {
            t = 0.0;
        }
        let s = 1.0 - t;
        let x = ppc::fmadd(
            t,
            f64::from(b1.x - a1.x),
            ppc::fmadd(s, f64::from(b0.x - a0.x), f64::from(p.x)),
        );
        let y = ppc::fmadd(
            t,
            f64::from(b1.y - a1.y),
            ppc::fmadd(s, f64::from(b0.y - a0.y), f64::from(p.y)),
        );
        Vec2::new(x as f32, y as f32)
    } else {
        Vec2::new(
            p.x + (b0.x - a0.x) + (b1.x - a0.x),
            p.y + (b0.y - a0.y) + (b1.y - a0.y),
        )
    }
}

/// `mpLineIntersection` (0x8004E97C): where segment `b0`–`b1` crosses
/// segment `a0`–`a1`, or `None`.
///
/// Direction dependent: `b` must start on the left of `a` (looking from
/// `a0` to `a1`; up to 0.1 of signed area onto the right side is allowed)
/// and end on the right (up to 0.1 left). The result snaps to `a0` or `a1`
/// when the crossing parameter leaves (0, 1).
///
/// Audited: 7 fused double ops — five `fmsub` cross products (`a*b - c*d`
/// fuses the left product; their products of single differences are exact
/// in double, so fusing cannot change them) and two `fmadd` for the
/// result (`aw*t + a0x`, `ah*t + a0y`), which do round once.
pub fn line_intersection(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> Option<Vec2> {
    let (a0x, a0y, a1x, a1y) = (a0.x, a0.y, a1.x, a1.y);
    let (b0x, b0y, b1x, b1y) = (b0.x, b0.y, b1.x, b1.y);
    let mut b1_below_a = false;
    let mut b2_above_a = false;

    // b entirely left/right of a
    if a0x <= a1x {
        if (b0x < a0x && b1x < a0x) || (a1x < b0x && a1x < b1x) {
            return None;
        }
    } else if (b0x < a1x && b1x < a1x) || (a0x < b0x && a0x < b1x) {
        return None;
    }

    // b entirely above/below a
    if a0y <= a1y {
        if (b0y < a0y && b1y < a0y) || (a1y < b0y && a1y < b1y) {
            return None;
        }
    } else if (b0y < a1y && b1y < a1y) || (a0y < b0y && a0y < b1y) {
        return None;
    }

    let ah = f64::from(a1y - a0y);
    let d0x = f64::from(b0x - a0x);
    let aw = f64::from(a1x - a0x);
    let d0y = f64::from(b0y - a0y);
    let hs_b0_a = ppc::fmsub(aw, d0y, ah * d0x);
    if hs_b0_a < 0.0 {
        if hs_b0_a < -0.1 {
            return None;
        }
        b1_below_a = true;
    }

    let d1x = f64::from(b1x - a1x);
    let d1y = f64::from(b1y - a1y);
    let hs_b1_a = ppc::fmsub(aw, d1y, ah * d1x);
    if hs_b1_a > 0.0 {
        if hs_b1_a > 0.1 {
            return None;
        }
        b2_above_a = true;
    }

    // a and b are colinear
    if hs_b0_a == 0.0 && hs_b1_a == 0.0 {
        return None;
    }

    let det = ppc::fmsub(d0x, d1y, d0y * d1x);
    if det < hs_b0_a {
        if det < hs_b1_a {
            return None;
        }
    } else if det > hs_b0_a && det > hs_b1_a {
        return None;
    }

    let bw = f64::from(b1x - b0x);
    let bh = f64::from(b1y - b0y);
    if !((bw == 0.0 && bh == 0.0) || (b1_below_a && b2_above_a) || (hs_b0_a >= 0.0 && b2_above_a)) {
        let area = ppc::fmsub(bw, ah, bh * aw);
        // `0.0001F`: a float literal compared in double.
        if abs_c64(area) > f64::from(0.0001f32) {
            // barycentric weight
            let t = ppc::fmsub(bw, d0y, bh * d0x) / area;
            return Some(if t > 0.0 {
                if t < 1.0 {
                    Vec2::new(
                        ppc::fmadd(aw, t, f64::from(a0x)) as f32,
                        ppc::fmadd(ah, t, f64::from(a0y)) as f32,
                    )
                } else {
                    a1
                }
            } else {
                a0
            });
        }
    }
    None
}

/// `mpLineIntersectionH` (0x8004EBF8): crossing of segment `b0`–`b1` with
/// the horizontal segment from `(a0x, a0y)` to `(a1x, a0y)`.
///
/// `b` must start above (or within 0.0001 below) and end below (or within
/// 0.0001 above) when `a` runs left to right, and the reverse when it runs
/// right to left. A crossing up to 0.1 past either end snaps to that end.
///
/// Audited: 1 fused double op, `fmadd` for
/// `dbx / dby * (a0y - b0y) + b0x` (the quotient is double, the difference
/// single).
pub fn line_intersection_h(a0x: f32, a0y: f32, a1x: f32, b0: Vec2, b1: Vec2) -> Option<Vec2> {
    let (b0x, b0y, b1x, b1y) = (b0.x, b0.y, b1.x, b1.y);
    let (min_ax, max_ax);
    if a0x < a1x {
        if (b0x < a0x && b1x < a0x) || (a1x < b0x && a1x < b1x) {
            return None;
        }
        if f64::from(b0y - a0y) < -0.0001 || f64::from(b1y - a0y) > 0.0001 {
            return None;
        }
        min_ax = a0x;
        max_ax = a1x;
    } else {
        if (b0x < a1x && b1x < a1x) || (a0x < b0x && a0x < b1x) {
            return None;
        }
        if f64::from(b1y - a0y) < -0.0001 || f64::from(b0y - a0y) > 0.0001 {
            return None;
        }
        min_ax = a1x;
        max_ax = a0x;
    }
    let dby = f64::from(b1y - b0y);
    let dbx = f64::from(b1x - b0x);
    if abs_c64(dby) < 0.0001 {
        return None;
    }
    let mut new_x = ppc::fmadd(dbx / dby, f64::from(a0y - b0y), f64::from(b0x));
    let dx = new_x - f64::from(min_ax);
    if dx < 0.0 {
        if dx < -0.1 {
            return None;
        }
        new_x = f64::from(min_ax);
    }
    if new_x - f64::from(max_ax) > 0.0 {
        if new_x - f64::from(max_ax) > 0.1 {
            return None;
        }
        new_x = f64::from(max_ax);
    }
    Some(Vec2::new(new_x as f32, a0y))
}

/// `mpLineIntersectionV` (0x80050068): crossing of segment `b0`–`b1` with
/// the vertical segment from `(a0x, a0y)` to `(a0x, a1y)`; the mirror of
/// [`line_intersection_h`]. When `a` runs upward, `b` must move left to
/// right (0.0001 tolerance); when it runs downward, right to left.
///
/// Audited: 1 fused double op, `fmadd` for `dby / dbx * (a0x - b0x) + b0y`.
pub fn line_intersection_v(a0x: f32, a0y: f32, a1y: f32, b0: Vec2, b1: Vec2) -> Option<Vec2> {
    let (b0x, b0y, b1x, b1y) = (b0.x, b0.y, b1.x, b1.y);
    let (min_ay, max_ay);
    if a0y < a1y {
        if (b0y < a0y && b1y < a0y) || (a1y < b0y && a1y < b1y) {
            return None;
        }
        if f64::from(b1x - a0x) < -0.0001 || f64::from(b0x - a0x) > 0.0001 {
            return None;
        }
        min_ay = a0y;
        max_ay = a1y;
    } else {
        if (b0y < a1y && b1y < a1y) || (a0y < b0y && a0y < b1y) {
            return None;
        }
        if f64::from(b0x - a0x) < -0.0001 || f64::from(b1x - a0x) > 0.0001 {
            return None;
        }
        min_ay = a1y;
        max_ay = a0y;
    }
    let dby = f64::from(b1y - b0y);
    let dbx = f64::from(b1x - b0x);
    if abs_c64(dbx) < 0.0001 {
        return None;
    }
    let mut new_y = ppc::fmadd(dby / dbx, f64::from(a0x - b0x), f64::from(b0y));
    let dy = new_y - f64::from(min_ay);
    if dy < 0.0 {
        if dy < -0.1 {
            return None;
        }
        new_y = f64::from(min_ay);
    }
    let dy = new_y - f64::from(max_ay);
    if dy > 0.0 {
        if dy > 0.1 {
            return None;
        }
        new_y = f64::from(max_ay);
    }
    Some(Vec2::new(a0x, new_y as f32))
}

/// `SQ(a) + SQ(b)` where the 1.02 code fuses it (`fmadds a, a, b*b`).
pub(super) fn sq_sum_fused(a: f32, b: f32) -> f32 {
    ppc::fmadds(a, a, b * b)
}

/// `ABS(x) > 0.0001` with the `double` literal.
pub(super) fn exceeds_epsilon(x: f32) -> bool {
    f64::from(abs_c(x)) > 0.0001
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn intersection_of_downward_segment_with_floor() {
        // Hand-evaluated from the C: aw = 10, ah = 0, d0 = (5, 5),
        // hs_b0 = 50, d1 = (-5, -5), hs_b1 = -50, det = 0, bw = 0, bh = -10,
        // area = 100, t = 50 / 100 = 0.5, result (aw * t + 0, 0).
        let hit = line_intersection(v(0.0, 0.0), v(10.0, 0.0), v(5.0, 5.0), v(5.0, -5.0));
        assert_eq!(hit, Some(v(5.0, 0.0)));
    }

    #[test]
    fn intersection_is_direction_dependent() {
        // Moving up through a left-to-right segment starts on its right
        // side: hs_b0 = 10 * -5 = -50 < -0.1.
        let hit = line_intersection(v(0.0, 0.0), v(10.0, 0.0), v(5.0, -5.0), v(5.0, 5.0));
        assert_eq!(hit, None);
        // Outside the x range.
        let miss = line_intersection(v(0.0, 0.0), v(10.0, 0.0), v(11.0, 5.0), v(12.0, -5.0));
        assert_eq!(miss, None);
    }

    #[test]
    fn intersection_snaps_to_endpoints() {
        // b starts on a0: hs_b0 = 0, hs_b1 = -50, det = 0, area = 50, and
        // t = (0 * 0 - (-5) * 0) / 50 = 0, which is not > 0, so a0 itself.
        let hit = line_intersection(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 0.0), v(0.0, -5.0));
        assert_eq!(hit, Some(v(0.0, 0.0)));
    }

    #[test]
    fn intersection_with_a_slope() {
        // a: (0,0)-(10,5); b straight down through x = 4. hs_b0 = 30,
        // hs_b1 = -70, det = -10, area = 100, t = 40 / 100 = 0.4, so
        // (fmadd(10, 0.4, 0), fmadd(5, 0.4, 0)) = (4, 2) after frsp.
        let hit = line_intersection(v(0.0, 0.0), v(10.0, 5.0), v(4.0, 5.0), v(4.0, -5.0));
        assert_eq!(hit, Some(v(4.0, 2.0)));
    }

    #[test]
    fn horizontal_intersection_and_clamping() {
        // new_x = 2 / -4 * (0 - 2) + 3 = 4.
        let hit = line_intersection_h(0.0, 0.0, 10.0, v(3.0, 2.0), v(5.0, -2.0));
        assert_eq!(hit, Some(v(4.0, 0.0)));
        // Upward crossings are rejected: b0y - a0y = -2 < -0.0001.
        assert_eq!(
            line_intersection_h(0.0, 0.0, 10.0, v(3.0, -2.0), v(5.0, 2.0)),
            None
        );
        // new_x = -0.05: within 0.1 of the left end, so it snaps to 0.
        // dbx / dby = 0.1 / -2 = -0.05, times (0 - 1), plus -0.1.
        let hit = line_intersection_h(0.0, 0.0, 10.0, v(-0.1, 1.0), v(0.0, -1.0));
        assert_eq!(hit, Some(v(0.0, 0.0)));
        // A right-to-left segment flips the vertical test.
        let hit = line_intersection_h(10.0, 0.0, 0.0, v(3.0, -2.0), v(5.0, 2.0));
        assert_eq!(hit, Some(v(4.0, 0.0)));
        // Nearly horizontal b: |dby| < 0.0001.
        assert_eq!(
            line_intersection_h(0.0, 0.0, 10.0, v(3.0, 0.0), v(5.0, 0.0)),
            None
        );
    }

    #[test]
    fn vertical_intersection() {
        // a runs upward at x = 5; b moves left to right:
        // new_y = dby / dbx * (5 - 3) + 1 = (2 / 4) * 2 + 1 = 2.
        let hit = line_intersection_v(5.0, 0.0, 10.0, v(3.0, 1.0), v(7.0, 3.0));
        assert_eq!(hit, Some(v(5.0, 2.0)));
        // Right to left is rejected for an upward a.
        assert_eq!(
            line_intersection_v(5.0, 0.0, 10.0, v(7.0, 3.0), v(3.0, 1.0)),
            None
        );
        // ...and accepted for a downward a.
        let hit = line_intersection_v(5.0, 10.0, 0.0, v(7.0, 3.0), v(3.0, 1.0));
        assert_eq!(hit, Some(v(5.0, 2.0)));
    }

    #[test]
    fn remap_follows_a_translated_segment() {
        // a: (0,0)-(10,0) moves up 5. t = 30 / 100 = 0.3, so
        // y = 1 + 0.7 * 5 + 0.3 * 5 = 6 and x is unchanged.
        let p = remap_2d(
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(0.0, 5.0),
            v(10.0, 5.0),
            v(3.0, 1.0),
        );
        assert_eq!(p, v(3.0, 6.0));
        // Projections past the end clamp t to 1: only b1 - a1 applies.
        let p = remap_2d(
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(0.0, 0.0),
            v(10.0, 4.0),
            v(20.0, 0.0),
        );
        assert_eq!(p, v(20.0, 4.0));
    }

    #[test]
    fn remap_of_a_degenerate_segment_uses_b1_minus_a0() {
        // |dist2| = 0: p + (b0 - a0) + (b1 - a0) = 1 + 2 + 3.
        let p = remap_2d(
            v(1.0, 1.0),
            v(1.0, 1.0),
            v(3.0, 1.0),
            v(4.0, 1.0),
            v(1.0, 0.0),
        );
        assert_eq!(p, v(6.0, 0.0));
    }
}
