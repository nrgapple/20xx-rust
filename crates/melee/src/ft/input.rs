//! Port of the fighter input update: `Fighter_procInput` (0x8006AD10,
//! `src/melee/ft/fighter.c`) and the input bookkeeping it runs each frame.
//!
//! Each frame the fighter copies `HSD_PadGameStatus[pad_port]` into its input
//! buffers, applies the stick and trigger deadzones, derives `HSD_PAD_LR` and
//! the Z macro, computes pressed/released buttons, and advances the input
//! timers that smash, dash, tap-jump, and fast-fall checks read.
//!
//! All functions were audited with `dol-inspect` against the 1.02 DOL. None
//! uses fused multiply-adds except `lb_8000D148` (noted there). Calls into
//! systems that are not ported (CPU AI, items, smash charge, statistics,
//! action-state callbacks) go through [`ProcInputHooks`].

// `!(a < b)` and `else if (x < 255) x++` forms mirror the C source.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::implicit_saturating_add)]

use super::data::CommonData;
use super::fighter::{Fighter, FtInputTimers};
use crate::hsd::controller::{
    sqrtf, HsdPadStatus, HSD_PAD_A, HSD_PAD_B, HSD_PAD_DPADDOWN, HSD_PAD_DPADUP, HSD_PAD_L,
    HSD_PAD_LR, HSD_PAD_R, HSD_PAD_XY, HSD_PAD_Z,
};
use crate::math::{abs_c, Vec2};
use crate::ppc;

/// `DbLKind_DebugRom`: from this debug level up, fighters ignore the C-stick.
const DB_LKIND_DEBUG_ROM: i32 = 3;

/// What a CPU player's controller reports (`ftCo_GetCpuLStickX` ..
/// `ftCo_GetCpuButtons`). CPU AI is not ported.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CpuInput {
    pub lstick: Vec2,
    pub cstick: Vec2,
    pub trigger_l: f32,
    pub trigger_r: f32,
    pub buttons: u32,
}

/// Game state outside the fighter that `Fighter_procInput` reads.
#[derive(Debug, Clone, Copy)]
pub struct ProcInputEnv<'a> {
    /// `HSD_PadGameStatus`
    pub pads: &'a [HsdPadStatus; 4],
    /// `Some` when `ftCo_IsCpuControlled(fp)`.
    pub cpu: Option<CpuInput>,
    /// `DbLevel`: 0 (`DbLKind_Master`) on a retail disc.
    pub db_level: i32,
    /// `gm_IsCurrently1PMode_inline()` (0x8016B41C): the major scene is a
    /// single-player mode (Classic through Event, including Training).
    /// Fighters then ignore the C-stick. 20XX forces this false.
    pub one_player_mode: bool,
    /// `gm_8016B0FC()`: the match's single-button rule. 20XX forces it off.
    pub single_button: bool,
    /// `gm_80479D58.unk_10.x0`, tested by `gm_GetDbPauseFlag(bit)`.
    pub db_pause_flags: u64,
}

impl<'a> ProcInputEnv<'a> {
    /// A human player in a retail VS match.
    pub fn vs(pads: &'a [HsdPadStatus; 4]) -> Self {
        Self {
            pads,
            cpu: None,
            db_level: 0,
            one_player_mode: false,
            single_button: false,
            db_pause_flags: 0,
        }
    }

    /// `gm_GetDbPauseFlag` (0x801A45E8)
    fn db_pause_flag(&self, bit: u32) -> bool {
        self.db_pause_flags & (1u64 << bit) != 0
    }
}

/// Calls `Fighter_procInput` makes into systems that are not ported. Every
/// method defaults to doing nothing.
pub trait ProcInputHooks {
    /// `ftCommon_8007FFD8(fp, (float) p_ftCommonData->x6FC)`, run when
    /// `fp->x1980` is set: mashing out of an item (items are not ported).
    fn mash_x1980(&mut self, _fp: &mut Fighter, _amount: f32) {}
    /// `ftCo_800DF0D0`: the smash-attack charge state machine, which starts,
    /// holds, or releases a charge from `held_buttons[0] & HSD_PAD_A`.
    fn smash_charge(&mut self, _fp: &mut Fighter) {}
    /// `Player_UpdateJoystickCountByIndex(player_idx, is_sub_fighter)`.
    fn update_joystick_count(&mut self, _player_idx: u8, _is_sub_fighter: bool) {}
    /// `fp->input_cb(gobj)`, the action state's input callback (interrupts).
    fn input_cb(&mut self, _fp: &mut Fighter) {}
}

/// Hooks that do nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl ProcInputHooks for NoHooks {}

/// `ftCo_JumpInput`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpInput {
    None = 0,
    LStick = 1,
    CStick = 2,
    Xy = 3,
}

/// `Fighter_UnkInitLoad_80068914_Inner1` (inlined): clear the input
/// buffers' current and previous entries and reset every timer.
pub fn clear_input(fp: &mut Fighter) {
    let input = &mut fp.input;
    input.triggers[0] = 0.0;
    input.triggers[1] = 0.0;
    input.cstick[0] = Vec2::new(0.0, 0.0);
    input.cstick[1] = Vec2::new(0.0, 0.0);
    input.lstick[0] = Vec2::new(0.0, 0.0);
    input.lstick[1] = Vec2::new(0.0, 0.0);
    input.held_buttons[1] = 0;
    input.released_buttons = 0;
    input.pressed_buttons = 0;
    input.held_buttons[0] = 0;
    let stamp = FtInputTimers {
        lstick: [254, 254],
        trigger: 254,
    };
    fp.activity_timer = stamp;
    fp.active_duration = stamp;
    fp.active_sticky = stamp;
    fp.active_timer = stamp;
    set_button_counters(fp, 255);
}

fn set_button_counters(fp: &mut Fighter, value: u8) {
    fp.x67c = value;
    fp.x67d = value;
    fp.x67e = value;
    fp.x67f = value;
    fp.x680 = value;
    fp.x681 = value;
    fp.x682 = value;
    fp.x683 = value;
    fp.x684 = value;
    fp.x685 = value;
    fp.x686 = value;
    fp.x687 = value;
    fp.x688 = value;
    fp.x689 = value;
    fp.x68a = value;
    fp.x68b = value;
}

/// The input setup in `Fighter_UnkInitLoad_80068914` (0x80068914), run when
/// a fighter is created: zero the saved (`[2]`) entries, [`clear_input`],
/// and later clear `has_prev_input` and `input_disabled`.
pub fn init_input(fp: &mut Fighter) {
    fp.input.lstick[2] = Vec2::new(0.0, 0.0);
    fp.input.cstick[2] = Vec2::new(0.0, 0.0);
    fp.input.triggers[2] = 0.0;
    fp.input.held_buttons[2] = 0;
    clear_input(fp);
    fp.has_prev_input = false;
    fp.input_disabled = false;
}

/// `Fighter_ResetInputData_80068854` (0x80068854): the same resets as
/// [`clear_input`], used by `ftlib` when disabling input and by `ft_0C8C`.
pub fn reset_input_data(fp: &mut Fighter) {
    clear_input(fp);
}

/// `Fighter_ClampSpecificValue`: increment a `u8` counter, saturating at
/// 254 (255 wraps to 0 first, as the `stb`/`lbz` sequence does).
fn increment_timer(timer: &mut u8) {
    *timer = timer.wrapping_add(1);
    if *timer > 254 {
        *timer = 254;
    }
}

/// A button counter: reset on press, otherwise count up to 255.
fn count_button(pressed: bool, counter: &mut u8) {
    if pressed {
        *counter = 0;
    } else if *counter < 255 {
        *counter += 1;
    }
}

/// `Fighter_procInput_Inner1` (inlined): buttons pressed and released since
/// last frame. In hitlag (`x2219_b5`) they accumulate until it ends.
fn update_pressed_released(fp: &mut Fighter) {
    let held = fp.input.held_buttons;
    let pressed = held[0] & (held[1] ^ held[0]);
    let released = held[1] & (held[1] ^ held[0]);
    if fp.x2219_b5 {
        fp.input.pressed_buttons |= pressed;
        fp.input.released_buttons |= released;
    } else {
        fp.input.pressed_buttons = pressed;
        fp.input.released_buttons = released;
    }
}

/// One stick axis of `Fighter_procInput`'s timer update. `smash_sign` is the
/// bit stored for the direction of a fresh smash input (`[positive,
/// negative]`).
fn update_axis_timers(fp: &mut Fighter, axis: usize, threshold: f32, smash_sign: [bool; 2]) {
    let (current, previous) = if axis == 0 {
        (fp.input.lstick[0].x, fp.input.lstick[1].x)
    } else {
        (fp.input.lstick[0].y, fp.input.lstick[1].y)
    };
    increment_timer(&mut fp.active_duration.lstick[axis]);
    let fresh_direction = if current >= threshold {
        if previous >= threshold {
            None
        } else {
            Some(smash_sign[0])
        }
    } else if current <= -threshold {
        if previous <= -threshold {
            None
        } else {
            Some(smash_sign[1])
        }
    } else {
        fp.activity_timer.lstick[axis] = 254;
        fp.active_sticky.lstick[axis] = 254;
        fp.active_timer.lstick[axis] = 254;
        return;
    };
    match fresh_direction {
        None => {
            increment_timer(&mut fp.active_timer.lstick[axis]);
            increment_timer(&mut fp.active_sticky.lstick[axis]);
            increment_timer(&mut fp.activity_timer.lstick[axis]);
        }
        Some(bit) => {
            fp.active_duration.lstick[axis] = 0;
            fp.active_sticky.lstick[axis] = 0;
            fp.active_timer.lstick[axis] = 0;
            if axis == 0 {
                fp.x2228_b7 = bit;
            } else {
                fp.x2229_b0 = bit;
            }
        }
    }
}

/// `Fighter_procInput` (0x8006AD10). `env.pads` is `HSD_PadGameStatus`
/// after this frame's [`crate::hsd::controller::PadLib::renew_for_frame`].
pub fn proc_input(
    fp: &mut Fighter,
    common: &CommonData,
    env: &ProcInputEnv,
    hooks: &mut impl ProcInputHooks,
) {
    if fp.is_sleeping {
        return;
    }
    if !fp.stamina_dead {
        let input = &mut fp.input;
        if !fp.has_prev_input {
            input.lstick[1] = input.lstick[2];
            input.cstick[1] = input.cstick[2];
            input.triggers[1] = input.triggers[2];
            input.held_buttons[1] = input.held_buttons[2];
            fp.has_prev_input = true;
        } else {
            input.lstick[1] = input.lstick[0];
            input.cstick[1] = input.cstick[0];
            input.triggers[1] = input.triggers[0];
            input.held_buttons[1] = input.held_buttons[0];
        }

        let cstick_enabled = env.db_level < DB_LKIND_DEBUG_ROM && !env.one_player_mode;
        if let Some(cpu) = env.cpu {
            input.lstick[0] = cpu.lstick;
            input.cstick[0] = if cstick_enabled {
                cpu.cstick
            } else {
                Vec2::new(0.0, 0.0)
            };
            let (l, r) = (cpu.trigger_l, cpu.trigger_r);
            input.triggers[0] = if l > r { l } else { r };
        } else {
            let pad = &env.pads[usize::from(fp.pad_port)];
            input.lstick[0] = Vec2::new(pad.nml_stick_x, pad.nml_stick_y);
            input.cstick[0] = if cstick_enabled {
                Vec2::new(pad.nml_sub_stick_x, pad.nml_sub_stick_y)
            } else {
                Vec2::new(0.0, 0.0)
            };
            let (r, l) = (pad.nml_analog_r, pad.nml_analog_l);
            input.triggers[0] = if l > r { l } else { r };
        }

        if abs_c(input.lstick[0].x) <= common.horizontal_stick_deadzone {
            input.lstick[0].x = 0.0;
        }
        if abs_c(input.lstick[0].y) <= common.vertical_stick_deadzone {
            input.lstick[0].y = 0.0;
        }
        if abs_c(input.cstick[0].x) <= common.horizontal_stick_deadzone {
            input.cstick[0].x = 0.0;
        }
        if abs_c(input.cstick[0].y) <= common.vertical_stick_deadzone {
            input.cstick[0].y = 0.0;
        }
        if input.triggers[0] <= common.analog_shoulder_deadzone {
            input.triggers[0] = 0.0;
        }

        input.held_buttons[0] = match env.cpu {
            Some(cpu) => cpu.buttons,
            None => env.pads[usize::from(fp.pad_port)].button,
        };

        if env.single_button {
            input.triggers[0] = 0.0;
            if env.cpu.is_some() {
                input.held_buttons[0] &= HSD_PAD_A | HSD_PAD_XY;
            } else {
                input.held_buttons[0] &= HSD_PAD_A;
            }
        } else {
            if input.held_buttons[0] & (HSD_PAD_L | HSD_PAD_R) != 0 {
                input.held_buttons[0] |= HSD_PAD_LR;
                input.triggers[0] = 1.0;
            } else if input.triggers[0] != 0.0 {
                input.held_buttons[0] |= HSD_PAD_LR;
            }
            if !env.db_pause_flag(0) && input.held_buttons[0] & HSD_PAD_Z != 0 {
                input.held_buttons[0] |= HSD_PAD_LR | HSD_PAD_A;
                input.triggers[0] = common.z_press_analog_value;
            }
        }

        update_pressed_released(fp);

        update_axis_timers(fp, 0, common.horizontal_stick_smash_deadzone, [true, false]);
        update_axis_timers(fp, 1, common.vertical_stick_smash_deadzone, [false, true]);

        let (prev, cur) = (fp.input.lstick[1], fp.input.lstick[0]);
        if lb_8000d148(
            prev.x,
            prev.y,
            cur.x,
            cur.y,
            0.0,
            0.0,
            common.horizontal_stick_smash_deadzone,
        ) {
            fp.activity_timer.lstick[1] = 0;
            fp.activity_timer.lstick[0] = 0;
        }

        increment_timer(&mut fp.active_duration.trigger);
        let threshold = common.shield_press_threshold;
        if fp.input.triggers[0] >= threshold {
            if fp.input.triggers[1] >= threshold {
                increment_timer(&mut fp.active_timer.trigger);
                increment_timer(&mut fp.active_sticky.trigger);
                increment_timer(&mut fp.activity_timer.trigger);
            } else {
                fp.activity_timer.trigger = 0;
                fp.active_duration.trigger = 0;
                fp.active_sticky.trigger = 0;
                fp.active_timer.trigger = 0;
            }
        } else {
            fp.activity_timer.trigger = 254;
            fp.active_sticky.trigger = 254;
            fp.active_timer.trigger = 254;
        }

        let pressed = fp.input.pressed_buttons;
        if pressed & HSD_PAD_A != 0 {
            fp.x683 = fp.x67c;
            fp.x67c = 0;
        } else if fp.x67c < 255 {
            fp.x67c += 1;
        }
        count_button(pressed & HSD_PAD_B != 0, &mut fp.x67d);
        count_button(pressed & HSD_PAD_XY != 0, &mut fp.x67e);
        count_button(pressed & HSD_PAD_DPADUP != 0, &mut fp.x681);
        count_button(pressed & HSD_PAD_DPADDOWN != 0, &mut fp.x682);
        count_button(pressed & HSD_PAD_LR != 0, &mut fp.x67f);
        if pressed & (HSD_PAD_L | HSD_PAD_R) != 0 {
            fp.x684 = fp.x680;
            fp.x680 = 0;
        } else if fp.x680 < 255 {
            fp.x680 += 1;
        }
    }

    if fp.input_disabled || fp.stamina_dead || env.db_pause_flag(2) {
        let input = &mut fp.input;
        input.lstick[2] = input.lstick[0];
        input.cstick[2] = input.cstick[0];
        input.triggers[2] = input.triggers[0];
        input.held_buttons[2] = input.held_buttons[0];
        fp.has_prev_input = false;
        clear_input(fp);
    }

    if !fp.x2219_b5 {
        if fp.x1980 {
            // `(float)` of the int `x6FC` (the 0x43300000 conversion, then
            // `fsubs`): exact for its value, 16.
            hooks.mash_x1980(fp, common.x6fc as f32);
        }
        hooks.smash_charge(fp);
        ft_common_8008031c(fp, common, hooks);
        unk_increment_counters_8006abec(fp, common);
        hooks.input_cb(fp);
    }
}

/// `ftCommon_8008031C` (0x8008031C): count a joystick or trigger "activity"
/// for match statistics, at most once per movement past `x7B8`/`x7BC`.
pub fn ft_common_8008031c(fp: &mut Fighter, common: &CommonData, hooks: &mut impl ProcInputHooks) {
    // The u8 timers convert exactly to float (0x43300000 trick, `fsubs`).
    if (abs_c(fp.input.lstick[0].x) >= common.x7b8
        && f32::from(fp.activity_timer.lstick[0]) < common.x7c0)
        || (abs_c(fp.input.lstick[0].y) >= common.x7b8
            && f32::from(fp.activity_timer.lstick[1]) < common.x7c0)
    {
        hooks.update_joystick_count(fp.player_idx, fp.is_sub_fighter);
        fp.activity_timer.lstick[1] = 0xfe;
        fp.activity_timer.lstick[0] = 0xfe;
    }
    if abs_c(fp.input.triggers[0]) >= common.x7bc
        && f32::from(fp.activity_timer.trigger) < common.x7c0
    {
        hooks.update_joystick_count(fp.player_idx, fp.is_sub_fighter);
        fp.activity_timer.trigger = 0xfe;
    }
}

/// `Fighter_UnkIncrementCounters_8006ABEC` (0x8006ABEC): frames since each
/// jump and special-move input, read by those moves' interrupt checks.
pub fn unk_increment_counters_8006abec(fp: &mut Fighter, common: &CommonData) {
    if jump_get_input(fp, common) != JumpInput::None {
        fp.x68a = fp.x685;
        fp.x685 = 0;
    } else if fp.x685 < 255 {
        fp.x685 += 1;
    }
    if ft_co_800d6928(fp, common) {
        fp.x68b = fp.x686;
        fp.x686 = 0;
    } else if fp.x686 < 255 {
        fp.x686 += 1;
    }
    let down_special = ft_co_800d688c(fp, common);
    count_button(down_special, &mut fp.x687);
    let side_special = special_s_has_input(fp, common);
    count_button(side_special, &mut fp.x688);
    let neutral_special = ft_co_800d67c4(fp, common);
    count_button(neutral_special, &mut fp.x689);
}

/// `ftCo_Jump_GetInput` (0x800CAE80): a tap jump (stick up past
/// `tap_jump_threshold` within `tap_jump_window` frames of leaving the
/// deadzone) or an X/Y press.
pub fn jump_get_input(fp: &Fighter, common: &CommonData) -> JumpInput {
    if fp.input.lstick[0].y >= common.tap_jump_threshold
        && i32::from(fp.active_timer.lstick[1]) < common.tap_jump_window
    {
        return JumpInput::LStick;
    }
    if fp.input.pressed_buttons & HSD_PAD_XY != 0 {
        return JumpInput::Xy;
    }
    JumpInput::None
}

/// `ftCo_800D6928` (0x800D6928): B pressed with the stick up (up-special).
pub fn ft_co_800d6928(fp: &Fighter, common: &CommonData) -> bool {
    fp.input.pressed_buttons & HSD_PAD_B != 0 && fp.input.lstick[0].y >= common.x21c
}

/// `ftCo_800D688C` (0x800D688C): B pressed with the stick down (down-special).
pub fn ft_co_800d688c(fp: &Fighter, common: &CommonData) -> bool {
    fp.input.pressed_buttons & HSD_PAD_B != 0 && fp.input.lstick[0].y < -common.x21c
}

/// `ftCo_SpecialS_HasInput` (0x800964FC): B pressed with the stick sideways.
pub fn special_s_has_input(fp: &Fighter, common: &CommonData) -> bool {
    fp.input.pressed_buttons & HSD_PAD_B != 0 && abs_c(fp.input.lstick[0].x) >= common.x218
}

/// `ftCo_800D67C4` (0x800D67C4): B pressed with the stick near neutral.
pub fn ft_co_800d67c4(fp: &Fighter, common: &CommonData) -> bool {
    fp.input.pressed_buttons & HSD_PAD_B != 0
        && abs_c(fp.input.lstick[0].x) < common.x218
        && abs_c(fp.input.lstick[0].y) < common.x21c
}

/// `lb_8000D148` (0x8000D148, `src/melee/lb/lb_00CE.c`): whether the segment
/// from point 0 to point 1 passes within `threshold` of point 2 (in or out
/// of that circle, or across it). `Fighter_procInput` uses it to detect the
/// stick crossing the center between frames.
///
/// Fused as in the 1.02 code: `dist_01 = fmadds(dx, dx, dy * dy)`,
/// `cross = fmsubs(p0x, p1y, p0y * p1x)`, and
/// `fmadds(dx, p2x, dy * p2y)`; the inlined `sqrtf` uses `fnmsub`.
#[allow(clippy::too_many_arguments)]
pub fn lb_8000d148(
    point0_x: f32,
    point0_y: f32,
    point1_x: f32,
    point1_y: f32,
    point2_x: f32,
    point2_y: f32,
    threshold: f32,
) -> bool {
    let diff_01_x = point1_x - point0_x;
    let diff_01_y = point0_y - point1_y;
    let mut cross = point0_y * point1_x;
    let mut dist_01 = ppc::fmadds(diff_01_x, diff_01_x, diff_01_y * diff_01_y);
    cross = ppc::fmsubs(point0_x, point1_y, cross);
    if dist_01 < 0.00001 {
        return false;
    }
    dist_01 = sqrtf(dist_01);

    let mut var_f0 = cross + ppc::fmadds(diff_01_x, point2_x, diff_01_y * point2_y);
    if var_f0 < 0.0 {
        var_f0 = -var_f0;
    }

    if var_f0 / dist_01 <= threshold {
        let dist_squared_02 = (point0_x - point2_x) * (point0_x - point2_x)
            + (point0_y - point2_y) * (point0_y - point2_y);
        let threshold_squared = threshold * threshold;
        let dist_squared_12 = (point1_x - point2_x) * (point1_x - point2_x)
            + (point1_y - point2_y) * (point1_y - point2_y);
        if dist_squared_02 < threshold_squared {
            if dist_squared_12 > threshold_squared {
                return true;
            }
            if dist_squared_12 < threshold_squared {
                return false;
            }
            return true;
        }
        if dist_squared_02 > threshold_squared {
            if dist_squared_12 > threshold_squared {
                return (point0_x > point2_x && point1_x < point2_x)
                    || (point0_x < point2_x && point1_x > point2_x)
                    || (point0_y > point2_y && point1_y < point2_y)
                    || (point0_y < point2_y && point1_y > point2_y);
            }
            return true;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timers_saturate_at_254_and_wrap_from_255() {
        let mut t = 253;
        increment_timer(&mut t);
        assert_eq!(t, 254);
        increment_timer(&mut t);
        assert_eq!(t, 254);
        let mut t = 255;
        increment_timer(&mut t);
        assert_eq!(t, 0);
    }

    /// Every value a human player's `lstick` component can hold after the
    /// 0.28 deadzone: 0 and ±n/80 for n in 23..=80.
    fn stick_values() -> Vec<f32> {
        let mut values = vec![0.0];
        for n in 23..=80i8 {
            values.push(f32::from(n) / 80.0);
            values.push(f32::from(-n) / 80.0);
        }
        values
    }

    /// `lb_8000D148`'s `sqrtf(dist_01)` gives the correctly rounded result
    /// for every frame-to-frame stick movement a human can make, whatever
    /// `frsqrte` estimate (within its documented 1/4096 error) seeds it.
    /// (595 distinct deltas, 354,024 distances.)
    #[test]
    fn center_crossing_sqrt_does_not_depend_on_the_estimate() {
        use crate::hsd::controller::sqrtf_with;
        let values = stick_values();
        let mut deltas: Vec<u32> = values
            .iter()
            .flat_map(|a| values.iter().map(move |b| (a - b).to_bits()))
            .collect();
        deltas.sort_unstable();
        deltas.dedup();
        let deltas: Vec<f32> = deltas.into_iter().map(f32::from_bits).collect();
        let exact = |v: f64| 1.0 / v.sqrt();
        let low = |v: f64| exact(v) * (1.0 - 1.0 / 4096.0);
        let high = |v: f64| exact(v) * (1.0 + 1.0 / 4096.0);
        assert_eq!(deltas.len(), 595);
        for &dx in &deltas {
            for &dy in &deltas {
                let dist = ppc::fmadds(dx, dx, dy * dy);
                if dist < 0.00001 {
                    continue;
                }
                let want = dist.sqrt();
                assert_eq!(sqrtf(dist), want, "dist {dist}");
                assert_eq!(sqrtf_with(dist, exact), want, "dist {dist}");
                assert_eq!(sqrtf_with(dist, low), want, "dist {dist}");
                assert_eq!(sqrtf_with(dist, high), want, "dist {dist}");
            }
        }
    }

    #[test]
    fn center_crossing_detection() {
        let t = 0.25;
        // Full right to full left passes through the center.
        assert!(lb_8000d148(1.0, 0.0, -1.0, 0.0, 0.0, 0.0, t));
        // Right to up: the chord passes 0.707 from the center.
        assert!(!lb_8000d148(1.0, 0.0, 0.0, 1.0, 0.0, 0.0, t));
        // Into the circle, and out of it.
        assert!(lb_8000d148(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, t));
        assert!(lb_8000d148(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, t));
        // No movement.
        assert!(!lb_8000d148(0.5, 0.5, 0.5, 0.5, 0.0, 0.0, t));
        // Moving within the circle.
        assert!(!lb_8000d148(0.0, 0.0, 0.1, 0.0, 0.0, 0.0, t));
        // Along a line near the center but stopping short of it.
        assert!(!lb_8000d148(1.0, 0.1, 0.5, 0.1, 0.0, 0.0, t));
    }
}
