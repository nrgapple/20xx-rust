//! Port of the movement helpers in `src/melee/ft/ftcommon.c`.
//!
//! Each function names its decomp symbol and 1.02 address. All were audited
//! with `dol-inspect`: none uses fused multiply-adds, so plain `f32`
//! arithmetic rounds exactly as the original. `p_ftCommonData` (a global in
//! the game) is passed explicitly as `common`.

// `!(a < b)` mirrors the C source; it differs from `a >= b` when a value is NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::data::CommonData;
use super::fighter::{Fighter, GroundOrAir};
use crate::math::abs_c;

/// `ftCommon_CalcGroundAccel_Deaccel` (0x8007C930)
pub fn calc_ground_accel_deaccel(fp: &mut Fighter, mut friction: f32) {
    if abs_c(friction) > abs_c(fp.gr_vel) {
        friction = -fp.gr_vel;
    } else if fp.gr_vel > 0.0 {
        friction = -friction;
    }
    fp.xe4_ground_accel_1 = friction;
}

/// `ftCommon_CalcGroundAccel_DashRun` (0x8007C98C)
pub fn calc_ground_accel_dash_run(
    fp: &mut Fighter,
    mut accel: f32,
    target_vel: f32,
    friction: f32,
) {
    if target_vel == 0.0 {
        calc_ground_accel_deaccel(fp, friction);
        return;
    }
    if !(fp.gr_vel * accel < 0.0) {
        let max = fp.co_attrs.ground_max_horizontal_velocity;
        if accel > 0.0 {
            if fp.gr_vel + accel > target_vel {
                accel = -friction;
                if fp.gr_vel + accel < target_vel {
                    accel = target_vel - fp.gr_vel;
                }
                if fp.gr_vel + accel > max {
                    accel = max - fp.gr_vel;
                }
            }
        } else if fp.gr_vel + accel < target_vel {
            accel = friction;
            if fp.gr_vel + accel > target_vel {
                accel = target_vel - fp.gr_vel;
            }
            if fp.gr_vel + accel < -max {
                accel = -max - fp.gr_vel;
            }
        }
    }
    fp.xe4_ground_accel_1 = accel;
}

/// `ftCommon_CalcGroundAccel_AccelToVel` (0x8007CA80). `friction` is unused.
pub fn calc_ground_accel_accel_to_vel(
    fp: &mut Fighter,
    mut accel: f32,
    target_vel: f32,
    _friction: f32,
) {
    if target_vel == 0.0 {
        accel = -fp.gr_vel;
    } else if !(fp.gr_vel * accel < 0.0) {
        if accel > 0.0 {
            if fp.gr_vel + accel > target_vel {
                accel = target_vel - fp.gr_vel;
            }
        } else if fp.gr_vel + accel < target_vel {
            accel = target_vel - fp.gr_vel;
        }
    }
    fp.xe4_ground_accel_1 = accel;
}

/// `ftCommon_CalcGroundAccel_AccelToLStickX` (0x8007CADC)
pub fn calc_ground_accel_accel_to_lstick_x(
    fp: &mut Fighter,
    threshold: f32,
    accel_max: f32,
    target_max: f32,
) {
    let lsx = fp.input.lstick[0].x;
    let (accel, target_vel) = if abs_c(lsx) >= threshold {
        (lsx * accel_max, lsx * target_max)
    } else {
        (0.0, 0.0)
    };
    calc_ground_accel_accel_to_vel(fp, accel, target_vel, lsx);
}

/// `ftCommon_SetSelfMovementFromGroundedMovement_NoFriction` (0x8007CC1C):
/// project ground speed and acceleration onto the floor's tangent.
pub fn set_self_movement_from_grounded_movement_no_friction(fp: &mut Fighter) {
    let normal = fp.coll_data.floor.normal;
    fp.x74_self_accel.x = normal.y * fp.xe4_ground_accel_1;
    fp.x74_self_accel.y = -normal.x * fp.xe4_ground_accel_1;
    fp.x74_self_accel.z = 0.0;
    fp.self_vel.x = normal.y * fp.gr_vel;
    fp.self_vel.y = -normal.x * fp.gr_vel;
    fp.self_vel.z = 0.0;
}

/// `ftCommon_ClampGroundVel` (0x8007CC78)
pub fn clamp_ground_vel(fp: &mut Fighter, max: f32) {
    if fp.gr_vel < -max {
        fp.gr_vel = -max;
    } else if fp.gr_vel > max {
        fp.gr_vel = max;
    }
}

/// `ftCommon_ApplyGroundedKnockbackFriction` (0x8007CCA0)
pub fn apply_grounded_knockback_friction(fp: &mut Fighter, friction: f32) {
    if fp.xf0_ground_kb_vel < 0.0 {
        fp.xf0_ground_kb_vel += friction;
        if fp.xf0_ground_kb_vel > 0.0 {
            fp.xf0_ground_kb_vel = 0.0;
        }
    } else {
        fp.xf0_ground_kb_vel -= friction;
        if fp.xf0_ground_kb_vel < 0.0 {
            fp.xf0_ground_kb_vel = 0.0;
        }
    }
}

/// `ftCommon_SetGroundedKnockbackIfLanded` (0x8007CCE8)
pub fn set_grounded_knockback_if_landed(fp: &mut Fighter, common: &CommonData) {
    if fp.ground_or_air == GroundOrAir::Ground && fp.xf0_ground_kb_vel == 0.0 {
        let normal = fp.coll_data.floor.normal;
        let max = common.max_grounded_kb_on_landing;
        fp.xf0_ground_kb_vel = fp.x8c_kb_vel.x;
        if fp.xf0_ground_kb_vel > max {
            fp.xf0_ground_kb_vel = max;
        }
        if fp.xf0_ground_kb_vel < -max {
            fp.xf0_ground_kb_vel = -max;
        }
        fp.x8c_kb_vel.x = normal.y * fp.xf0_ground_kb_vel;
        fp.x8c_kb_vel.y = -normal.x * fp.xf0_ground_kb_vel;
    }
}

/// `ftCommon_SandbagKnockbackDeaccel` (0x8007CD6C)
pub fn sandbag_knockback_deaccel(kb: f32, decrement: f32) -> f32 {
    let mut result = kb;
    if kb > 0.0 {
        result -= decrement;
        if result < 0.0 {
            return 0.0;
        }
    } else if kb < 0.0 {
        result += decrement;
        if result > 0.0 {
            return 0.0;
        }
    }
    result
}

/// `ftCommon_ApplyShieldKnockbackFriction` (0x8007CE4C)
pub fn apply_shield_knockback_friction(fp: &mut Fighter, val: f32) {
    if fp.xf4_ground_attacker_shield_kb_vel < 0.0 {
        fp.xf4_ground_attacker_shield_kb_vel += val;
        if fp.xf4_ground_attacker_shield_kb_vel > 0.0 {
            fp.xf4_ground_attacker_shield_kb_vel = 0.0;
        }
    } else {
        fp.xf4_ground_attacker_shield_kb_vel -= val;
        if fp.xf4_ground_attacker_shield_kb_vel < 0.0 {
            fp.xf4_ground_attacker_shield_kb_vel = 0.0;
        }
    }
}

/// `ftCommon_CalcSelfAccel_Deaccel` (0x8007CE94). Unlike the ground version,
/// friction that exactly equals the speed also stops the fighter (`>=`).
pub fn calc_self_accel_deaccel(fp: &mut Fighter, mut friction: f32) {
    if abs_c(friction) >= abs_c(fp.self_vel.x) {
        friction = -fp.self_vel.x;
    } else if fp.self_vel.x > 0.0 {
        friction = -friction;
    }
    fp.x74_self_accel.x = friction;
}

/// `ftCommon_CalcSelfAccel_DeaccelAir` (0x8007CEF4)
pub fn calc_self_accel_deaccel_air(fp: &mut Fighter) {
    let friction = fp.co_attrs.aerial_friction;
    calc_self_accel_deaccel(fp, friction);
}

/// Shared body of the two `DeaccelQuick` variants: stronger friction above `max_vel`.
fn deaccel_quick(fp: &mut Fighter, common: &CommonData, max_vel: f32) -> bool {
    let vel = fp.self_vel.x;
    let over = abs_c(vel) > max_vel;
    let friction = if over {
        common.aerial_friction_oob
    } else {
        fp.co_attrs.aerial_friction
    };
    let accel = if abs_c(friction) >= abs_c(vel) {
        -vel
    } else if vel > 0.0 {
        -friction
    } else {
        friction
    };
    fp.x74_self_accel.x = accel;
    over
}

/// `ftCommon_CalcSelfAccel_DeaccelQuickAir` (0x8007CF58)
pub fn calc_self_accel_deaccel_quick_air(fp: &mut Fighter, common: &CommonData) -> bool {
    let max = fp.co_attrs.air_drift_max;
    deaccel_quick(fp, common, max)
}

/// `ftCommon_CalcSelfAccel_DeaccelQuick` (0x8007D050)
pub fn calc_self_accel_deaccel_quick(fp: &mut Fighter, common: &CommonData, max_vel: f32) -> bool {
    deaccel_quick(fp, common, max_vel)
}

/// `ftCommon_CalcSelfAccel_AccelToVelClamped` (0x8007D140)
pub fn calc_self_accel_accel_to_vel_clamped(
    fp: &mut Fighter,
    accel: f32,
    target_vel: f32,
    friction: f32,
) {
    let vel = fp.self_vel.x;
    calc_self_accel_accel_to_vel_clamped_from(fp, vel, accel, target_vel, friction);
}

/// `ftCommon_CalcSelfAccel_AccelToVelClampedFrom` (0x8007D174)
pub fn calc_self_accel_accel_to_vel_clamped_from(
    fp: &mut Fighter,
    vel: f32,
    mut accel: f32,
    target_vel: f32,
    friction: f32,
) {
    if target_vel == 0.0 {
        calc_self_accel_deaccel(fp, friction);
        return;
    }
    if !(vel * accel < 0.0) {
        let max = fp.co_attrs.air_max_horizontal_velocity;
        if accel > 0.0 {
            if vel + accel > target_vel {
                accel = -friction;
                if vel + accel < target_vel {
                    accel = target_vel - vel;
                }
                if vel + accel > max {
                    accel = max - vel;
                }
            }
        } else if vel + accel < target_vel {
            accel = friction;
            if vel + accel > target_vel {
                accel = target_vel - vel;
            }
            if vel + accel < -max {
                accel = -max - vel;
            }
        }
    }
    fp.x74_self_accel.x = accel;
}

/// `ftCommon_CalcSelfAccel_Drift` (0x8007D268)
pub fn calc_self_accel_drift(fp: &mut Fighter) {
    let vel = fp.self_vel.x;
    calc_self_accel_drift_from(fp, vel);
}

/// `ftCommon_CalcSelfAccel_DriftFrom` (0x8007D28C): aerial drift from the stick.
/// The scaled and flat accelerations are rounded separately (`fmuls`, then
/// `fadds`), not fused.
pub fn calc_self_accel_drift_from(fp: &mut Fighter, vel: f32) {
    let lsx = fp.input.lstick[0].x;
    let attrs = fp.co_attrs;
    let accel_scaling = lsx * attrs.air_drift_stick_mul;
    let accel_flat = if lsx > 0.0 {
        attrs.aerial_drift_base
    } else {
        -attrs.aerial_drift_base
    };
    calc_self_accel_accel_to_vel_clamped_from(
        fp,
        vel,
        accel_scaling + accel_flat,
        lsx * attrs.air_drift_max,
        attrs.aerial_friction,
    );
}

/// `ftCommon_CalcSelfAccel_AccelToVel` (0x8007D2E8). The last parameter is unused.
pub fn calc_self_accel_accel_to_vel(
    fp: &mut Fighter,
    mut accel: f32,
    target_vel: f32,
    _unused: f32,
) {
    let vel = fp.self_vel.x;
    if target_vel == 0.0 {
        accel = -vel;
    } else if !(vel * accel < 0.0) {
        if accel > 0.0 {
            if vel + accel > target_vel {
                accel = target_vel - vel;
            }
        } else if vel + accel < target_vel {
            accel = target_vel - vel;
        }
    }
    fp.x74_self_accel.x = accel;
}

fn stick_accel(fp: &Fighter, threshold: f32, accel_max: f32, target_max: f32) -> (f32, f32) {
    let lsx = fp.input.lstick[0].x;
    if abs_c(lsx) >= threshold {
        (lsx * accel_max, lsx * target_max)
    } else {
        (0.0, 0.0)
    }
}

/// `ftCommon_CalcSelfAccel_DriftSimple` (0x8007D344)
pub fn calc_self_accel_drift_simple(
    fp: &mut Fighter,
    threshold: f32,
    accel_max: f32,
    target_max: f32,
) {
    let (accel, target) = stick_accel(fp, threshold, accel_max, target_max);
    let friction = fp.co_attrs.aerial_friction;
    calc_self_accel_accel_to_vel_clamped(fp, accel, target, friction);
}

/// `ftCommon_CalcSelfAccel_DriftSimple_NoFriction` (0x8007D3A8)
pub fn calc_self_accel_drift_simple_no_friction(
    fp: &mut Fighter,
    threshold: f32,
    accel_max: f32,
    target_max: f32,
) {
    let (accel, target) = stick_accel(fp, threshold, accel_max, target_max);
    calc_self_accel_accel_to_vel(fp, accel, target, 0.0);
}

/// `ftCommon_ClampSelfVelX` (0x8007D440)
pub fn clamp_self_vel_x(fp: &mut Fighter, max: f32) {
    let vel = fp.self_vel.x;
    if vel < -max {
        fp.self_vel.x = -max;
    } else if vel > max {
        fp.self_vel.x = max;
    }
}

/// `ftCommon_ClampAirDrift` (0x8007D468)
pub fn clamp_air_drift(fp: &mut Fighter) {
    let max = fp.co_attrs.air_drift_max;
    clamp_self_vel_x(fp, max);
}

/// `ftCommon_Fall` (0x8007D494)
pub fn fall(fp: &mut Fighter, gravity: f32, terminal_vel: f32) {
    fp.self_vel.y -= gravity;
    if fp.self_vel.y < -terminal_vel {
        fp.self_vel.y = -terminal_vel;
    }
}

/// `ftCommon_FallBasic` (0x8007D4B8)
pub fn fall_basic(fp: &mut Fighter) {
    let (gravity, terminal) = (fp.co_attrs.gravity, fp.co_attrs.terminal_velocity);
    fall(fp, gravity, terminal);
}

/// `ftCommon_FallFast` (0x8007D4E4)
pub fn fall_fast(fp: &mut Fighter) {
    fp.self_vel.y = -fp.co_attrs.fast_fall_velocity;
}

/// `ftCommon_ClampFallSpeed` (0x8007D4F4)
pub fn clamp_fall_speed(fp: &mut Fighter, val: f32) {
    if fp.self_vel.y > val {
        fp.self_vel.y = val;
    }
}

/// `ftCommon_Ascend` (0x8007D508)
pub fn ascend(fp: &mut Fighter, accel: f32, max: f32) {
    fp.self_vel.y += accel;
    if fp.self_vel.y > max {
        fp.self_vel.y = max;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fighter() -> Fighter {
        let mut fp = Fighter::default();
        // Fox's 1.02 values (verified against PlFx.dat in tests/disc_data.rs).
        fp.co_attrs.gravity = 0.23;
        fp.co_attrs.terminal_velocity = 2.8;
        fp.co_attrs.fast_fall_velocity = 3.4;
        fp.co_attrs.air_drift_max = 0.83;
        fp.co_attrs.air_drift_stick_mul = 0.06;
        fp.co_attrs.aerial_drift_base = 0.02;
        fp.co_attrs.aerial_friction = 0.02;
        fp.co_attrs.air_max_horizontal_velocity = 3.0;
        fp
    }

    #[test]
    fn fall_reaches_terminal_velocity() {
        let mut fp = fighter();
        let mut frames = 0;
        while fp.self_vel.y > -2.8 {
            fall_basic(&mut fp);
            frames += 1;
        }
        // 2.8 / 0.23 = 12.17: the 13th frame clamps to exactly -2.8.
        assert_eq!(frames, 13);
        assert_eq!(fp.self_vel.y, -2.8);
        fall_fast(&mut fp);
        assert_eq!(fp.self_vel.y, -3.4);
    }

    #[test]
    fn deaccel_stops_at_zero_without_overshoot() {
        let mut fp = fighter();
        fp.self_vel.x = 0.01;
        calc_self_accel_deaccel(&mut fp, 0.02);
        assert_eq!(fp.x74_self_accel.x, -0.01);
        fp.self_vel.x = -0.5;
        calc_self_accel_deaccel(&mut fp, 0.02);
        assert_eq!(fp.x74_self_accel.x, 0.02);
        fp.gr_vel = 0.02;
        calc_ground_accel_deaccel(&mut fp, 0.02);
        // Ground version uses `>`: equal friction is negated, not -gr_vel.
        assert_eq!(fp.xe4_ground_accel_1, -0.02);
    }

    #[test]
    fn drift_accelerates_toward_stick_target() {
        let mut fp = fighter();
        fp.input.lstick[0].x = 1.0;
        calc_self_accel_drift(&mut fp);
        // 1.0 * 0.06 + 0.02, rounded in f32 at each step.
        assert_eq!(fp.x74_self_accel.x, 0.06f32 + 0.02f32);
        // Near the target the acceleration is cut to land exactly on it.
        fp.self_vel.x = 0.8;
        calc_self_accel_drift(&mut fp);
        assert_eq!(fp.self_vel.x + fp.x74_self_accel.x, 0.83);
        // With no stick input drift decelerates by aerial friction.
        fp.input.lstick[0].x = 0.0;
        calc_self_accel_drift(&mut fp);
        assert_eq!(fp.x74_self_accel.x, -0.02);
    }

    #[test]
    fn deaccel_quick_uses_out_of_bounds_friction() {
        let mut fp = fighter();
        let common = CommonData {
            aerial_friction_oob: 0.05,
            ..CommonData::default()
        };
        fp.self_vel.x = 2.0;
        assert!(calc_self_accel_deaccel_quick_air(&mut fp, &common));
        assert_eq!(fp.x74_self_accel.x, -0.05);
        fp.self_vel.x = -0.5;
        assert!(!calc_self_accel_deaccel_quick_air(&mut fp, &common));
        assert_eq!(fp.x74_self_accel.x, 0.02);
    }

    #[test]
    fn grounded_movement_follows_slope() {
        let mut fp = fighter();
        fp.coll_data.floor.normal = crate::math::Vec3::new(-0.6, 0.8, 0.0);
        fp.gr_vel = 1.0;
        fp.xe4_ground_accel_1 = 0.5;
        set_self_movement_from_grounded_movement_no_friction(&mut fp);
        assert_eq!(fp.self_vel.x, 0.8);
        assert_eq!(fp.self_vel.y, 0.6);
        assert_eq!(fp.x74_self_accel.x, 0.4);
        assert_eq!(fp.x74_self_accel.y, 0.3);
    }

    #[test]
    fn sandbag_knockback_decays_to_zero() {
        assert_eq!(sandbag_knockback_deaccel(1.0, 0.25), 0.75);
        assert_eq!(sandbag_knockback_deaccel(0.1, 0.25), 0.0);
        assert_eq!(sandbag_knockback_deaccel(-0.1, 0.25), 0.0);
        assert_eq!(sandbag_knockback_deaccel(0.0, 0.25), 0.0);
    }
}
