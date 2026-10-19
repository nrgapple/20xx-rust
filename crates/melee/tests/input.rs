//! Controller input → fighter input state, frame by frame.
//!
//! The synthetic tests use the 1.02 `ftCommonData` thresholds from
//! `PlCo.dat` (checked against the disc in `common_data_thresholds_match_disc`
//! when an image is available). Disc-backed tests read
//! `originals/melee-ntsc-1.02.iso` and `originals/20xx-hp-5.0.2.iso` (or
//! `MELEE_VANILLA_ISO`/`MELEE_ISO`) and skip without them.

use gc_disc::{Disc, Dol};
use hsd_dat::Archive;
use melee::ft::common::check_fall_fast;
use melee::ft::data::CommonData;
use melee::ft::fighter::{Fighter, GroundOrAir};
use melee::ft::input::{
    init_input, jump_get_input, proc_input, JumpInput, NoHooks, ProcInputEnv, ProcInputHooks,
};
use melee::hsd::controller::{
    HsdPadStatus, PadData, PadInput, PadLib, PadLibData, PadStatus, AD_CONVERT_CONSTANTS,
    ATANF_LOOKUP_BITS, HSD_PAD_A, HSD_PAD_B, HSD_PAD_L, HSD_PAD_LR, HSD_PAD_X, HSD_PAD_Z,
    PAD_STICK_DOWN, PAD_STICK_LEFT, PAD_STICK_RIGHT, PAD_STICK_UP,
};
use std::fs::File;
use std::path::PathBuf;

/// The input thresholds of 1.02's `ftCommonData`.
fn common() -> CommonData {
    CommonData {
        horizontal_stick_deadzone: 0.28,
        vertical_stick_deadzone: 0.28,
        horizontal_stick_smash_deadzone: 0.25,
        vertical_stick_smash_deadzone: 0.25,
        analog_shoulder_deadzone: 0.3,
        z_press_analog_value: 0.35,
        shield_press_threshold: 0.25,
        tap_jump_threshold: 0.6625,
        tap_jump_window: 4,
        x88: 0.6625,
        x8c: 4,
        x218: 0.6,
        x21c: 0.55,
        x6fc: 16,
        x7b8: 0.5,
        x7bc: 0.5,
        x7c0: 4.0,
        ..CommonData::default()
    }
}

/// One human fighter on port 1 and the HSD pad library, stepped together.
struct Rig {
    pads: PadLib,
    fp: Fighter,
    common: CommonData,
}

impl Rig {
    fn new() -> Self {
        let mut fp = Fighter {
            pad_port: 1,
            ..Fighter::default()
        };
        init_input(&mut fp);
        Self {
            pads: PadLib::melee(),
            fp,
            common: common(),
        }
    }

    fn step_with(&mut self, input: PadInput, hooks: &mut impl ProcInputHooks) {
        let mut poll: PadData = [PadStatus::DISCONNECTED; 4];
        poll[1] = input.into();
        self.pads.renew_for_frame(&poll);
        proc_input(
            &mut self.fp,
            &self.common,
            &ProcInputEnv::vs(&self.pads.game),
            hooks,
        );
    }

    fn step(&mut self, input: PadInput) {
        self.step_with(input, &mut NoHooks);
    }
}

fn stick(x: i8, y: i8) -> PadInput {
    PadInput {
        stick_x: x,
        stick_y: y,
        ..PadInput::default()
    }
}

fn buttons(button: u32) -> PadInput {
    PadInput {
        button: button as u16,
        ..PadInput::default()
    }
}

#[test]
fn stick_deadzone_boundaries() {
    // 22/80 = 0.275 <= 0.28 is zeroed; 23/80 = 0.2875 passes.
    for (raw, want) in [
        (22, 0.0),
        (23, 0.2875),
        (-22, 0.0),
        (-23, -0.2875),
        (80, 1.0),
    ] {
        let mut rig = Rig::new();
        rig.step(stick(raw, 0));
        assert_eq!(rig.fp.input.lstick[0].x, want, "x {raw}");
        let mut rig = Rig::new();
        rig.step(stick(0, raw));
        assert_eq!(rig.fp.input.lstick[0].y, want, "y {raw}");
        let mut rig = Rig::new();
        rig.step(PadInput {
            substick_x: raw,
            substick_y: raw,
            ..PadInput::default()
        });
        let c = rig.fp.input.cstick[0];
        // Diagonal C-stick values are clamped first for |raw| > 56.
        if raw.unsigned_abs() <= 56 {
            assert_eq!((c.x, c.y), (want, want), "c {raw}");
        }
    }
}

#[test]
fn stick_is_clamped_to_the_circle_before_scaling() {
    let mut rig = Rig::new();
    rig.step(stick(127, 0));
    assert_eq!(rig.fp.input.lstick[0].x, 1.0);
    rig.step(stick(100, 100));
    // r = 141.4 > 80: 100 * 80 / 141.4 = 56.57 truncates to 56.
    assert_eq!(rig.fp.input.lstick[0].x, 0.7);
    assert_eq!(rig.fp.input.lstick[0].y, 0.7);
    rig.step(stick(-60, -80));
    assert_eq!(rig.pads.game[1].stick_x, -48);
    assert_eq!(rig.fp.input.lstick[0].x, -0.6);
    assert_eq!(rig.fp.input.lstick[0].y, -0.8);
}

#[test]
fn analog_trigger_deadzone_lr_and_z_macro() {
    let trigger = |l: u8, r: u8| PadInput {
        trigger_left: l,
        trigger_right: r,
        ..PadInput::default()
    };
    let mut rig = Rig::new();
    // 42 / 140 = 0.3 (the same float as the deadzone) is zeroed.
    rig.step(trigger(42, 0));
    assert_eq!(rig.fp.input.triggers[0], 0.0);
    assert_eq!(rig.fp.input.held_buttons[0] & HSD_PAD_LR, 0);
    // 43 / 140 passes and counts as an L/R press.
    rig.step(trigger(0, 43));
    assert_eq!(rig.fp.input.triggers[0], 43.0 / 140.0);
    assert_eq!(rig.fp.input.held_buttons[0] & HSD_PAD_LR, HSD_PAD_LR);
    assert_eq!(rig.fp.input.pressed_buttons, HSD_PAD_LR);
    // Analog values clamp at 140; the larger trigger wins.
    rig.step(trigger(255, 70));
    assert_eq!(rig.fp.input.triggers[0], 1.0);
    // A digital click forces 1.0.
    rig.step(PadInput {
        trigger_left: 50,
        ..buttons(HSD_PAD_L)
    });
    assert_eq!(rig.fp.input.triggers[0], 1.0);
    // Z is A + LR with a light-shield value.
    rig.step(buttons(HSD_PAD_Z));
    assert_eq!(
        rig.fp.input.held_buttons[0],
        HSD_PAD_Z | HSD_PAD_A | HSD_PAD_LR
    );
    assert_eq!(rig.fp.input.triggers[0], 0.35);
}

#[test]
fn button_edges_and_counters() {
    let mut rig = Rig::new();
    rig.step(buttons(0));
    assert_eq!(rig.fp.x67c, 255);
    rig.step(buttons(HSD_PAD_A));
    assert_eq!(rig.fp.input.pressed_buttons, HSD_PAD_A);
    assert_eq!(rig.fp.x67c, 0);
    assert_eq!(rig.fp.x683, 255);
    rig.step(buttons(HSD_PAD_A | HSD_PAD_B));
    assert_eq!(rig.fp.input.pressed_buttons, HSD_PAD_B);
    assert_eq!(rig.fp.input.released_buttons, 0);
    assert_eq!((rig.fp.x67c, rig.fp.x67d), (1, 0));
    rig.step(buttons(HSD_PAD_B));
    assert_eq!(rig.fp.input.pressed_buttons, 0);
    assert_eq!(rig.fp.input.released_buttons, HSD_PAD_A);
    rig.step(buttons(HSD_PAD_A));
    // x67c counted 0, 1, 2 since the last A press.
    assert_eq!(rig.fp.x683, 2);
    assert_eq!(rig.fp.input.held_buttons[1], HSD_PAD_B);
}

#[test]
fn hitlag_accumulates_presses() {
    let mut rig = Rig::new();
    rig.fp.x2219_b5 = true;
    rig.step(buttons(HSD_PAD_A));
    rig.step(buttons(HSD_PAD_A | HSD_PAD_X));
    rig.step(buttons(0));
    assert_eq!(rig.fp.input.pressed_buttons, HSD_PAD_A | HSD_PAD_X);
    assert_eq!(rig.fp.input.released_buttons, HSD_PAD_A | HSD_PAD_X);
    rig.fp.x2219_b5 = false;
    rig.step(buttons(0));
    assert_eq!(rig.fp.input.pressed_buttons, 0);
}

#[test]
fn active_timer_counts_from_the_smash_threshold_and_saturates() {
    let mut rig = Rig::new();
    rig.step(stick(80, 0));
    assert_eq!(rig.fp.active_timer.lstick[0], 0);
    assert!(rig.fp.x2228_b7);
    for frame in 1..300u32 {
        rig.step(stick(80, 0));
        assert_eq!(
            u32::from(rig.fp.active_timer.lstick[0]),
            frame.min(254),
            "frame {frame}"
        );
    }
    assert_eq!(rig.fp.active_duration.lstick[0], 254);
    // Back to neutral: stamped 254, but active_duration keeps counting.
    rig.step(stick(0, 0));
    assert_eq!(rig.fp.active_timer.lstick[0], 254);
    // Left is a new smash input.
    rig.step(stick(-80, 0));
    assert_eq!(rig.fp.active_timer.lstick[0], 0);
    assert!(!rig.fp.x2228_b7);
    // Down sets the vertical direction bit.
    rig.step(stick(0, -80));
    assert_eq!(rig.fp.active_timer.lstick[1], 0);
    assert!(rig.fp.x2229_b0);
}

#[test]
fn tap_jump_needs_a_fast_upward_flick() {
    let jump_frames = |ys: &[i8]| {
        let mut rig = Rig::new();
        ys.iter()
            .map(|&y| {
                rig.step(stick(0, y));
                jump_get_input(&rig.fp, &rig.common) == JumpInput::LStick
            })
            .collect::<Vec<_>>()
    };
    // 0.7 >= 0.6625 on the frame the stick leaves the deadzone, and for the
    // next three frames (timer 1..3 < tap_jump_window 4).
    assert_eq!(
        jump_frames(&[0, 56, 56, 56, 56, 56]),
        [false, true, true, true, true, false]
    );
    // Sliding up: past the smash deadzone (0.3) four frames before 0.7.
    assert_eq!(
        jump_frames(&[24, 24, 24, 24, 56]),
        [false, false, false, false, false]
    );
    // Three frames is still in time.
    assert_eq!(jump_frames(&[24, 24, 24, 56]), [false, false, false, true]);
}

/// Frame sequence: a falling fighter fast-falls on the frame of a down
/// flick, as `ftCommon_CheckFallFast` reads `active_timer.lstick.y`.
#[test]
fn down_flick_triggers_fast_fall_on_its_frame() {
    let fall = |ys: &[i8]| {
        let mut rig = Rig::new();
        rig.fp.ground_or_air = GroundOrAir::Air;
        rig.fp.self_vel.y = -1.0;
        ys.iter()
            .position(|&y| {
                rig.step(stick(0, y));
                check_fall_fast(&mut rig.fp, &rig.common)
            })
            .inspect(|_| {
                assert!(rig.fp.fall_fast);
                assert_eq!(rig.fp.active_timer.lstick[1], 0xfe);
            })
    };
    assert_eq!(fall(&[0, 0, 0, -80, -80]), Some(3));
    // Holding down from the start: the flick is on frame 0.
    assert_eq!(fall(&[-80]), Some(0));
    // A slow press reaches -0.6625 on the fourth frame past -0.25: too late.
    assert_eq!(fall(&[0, -24, -24, -24, -24, -80, -80]), None);
    assert_eq!(fall(&[0, -24, -24, -24, -80]), Some(4));
}

#[test]
fn disabled_input_saves_and_clears_buffers() {
    let mut rig = Rig::new();
    rig.step(stick(80, 0));
    rig.fp.input_disabled = true;
    rig.step(PadInput {
        stick_x: -80,
        ..buttons(HSD_PAD_B)
    });
    assert_eq!(rig.fp.input.lstick[2].x, -1.0);
    // Held buttons include the HSD stick-direction bits.
    assert_eq!(rig.fp.input.held_buttons[2], HSD_PAD_B | PAD_STICK_LEFT);
    assert_eq!(rig.fp.input.lstick[0].x, 0.0);
    assert_eq!(rig.fp.active_timer.lstick[0], 254);
    assert!(!rig.fp.has_prev_input);
    // Re-enabled: the saved values seed the previous frame.
    rig.fp.input_disabled = false;
    rig.step(PadInput {
        stick_x: -80,
        ..buttons(HSD_PAD_B)
    });
    assert_eq!(rig.fp.input.lstick[1].x, -1.0);
    assert_eq!(rig.fp.input.pressed_buttons, 0);
    // A direction held through the disabled frames is not a fresh smash:
    // the stamped 254 just saturates.
    assert_eq!(rig.fp.active_timer.lstick[0], 254);
}

#[derive(Default)]
struct Recorder {
    joystick_counts: u32,
    smash_charges: u32,
    input_cbs: u32,
}

impl ProcInputHooks for Recorder {
    fn smash_charge(&mut self, _fp: &mut Fighter) {
        self.smash_charges += 1;
    }
    fn update_joystick_count(&mut self, _player_idx: u8, _is_sub_fighter: bool) {
        self.joystick_counts += 1;
    }
    fn input_cb(&mut self, _fp: &mut Fighter) {
        self.input_cbs += 1;
    }
}

#[test]
fn hooks_and_special_move_counters() {
    let mut rig = Rig::new();
    let mut hooks = Recorder::default();
    rig.step_with(stick(0, 0), &mut hooks);
    // A fresh stick movement past 0.5 is counted once.
    rig.step_with(stick(80, 0), &mut hooks);
    rig.step_with(stick(80, 0), &mut hooks);
    assert_eq!(hooks.joystick_counts, 1);
    assert_eq!(rig.fp.activity_timer.lstick, [0xfe, 0xfe]);
    // Up-B and side-B inputs reset their counters.
    rig.step_with(
        PadInput {
            stick_y: 80,
            ..buttons(HSD_PAD_B)
        },
        &mut hooks,
    );
    assert_eq!(rig.fp.x686, 0);
    rig.step_with(
        PadInput {
            stick_x: 80,
            ..buttons(0)
        },
        &mut hooks,
    );
    rig.step_with(
        PadInput {
            stick_x: 80,
            ..buttons(HSD_PAD_B)
        },
        &mut hooks,
    );
    assert_eq!((rig.fp.x686, rig.fp.x688), (2, 0));
    assert_eq!(hooks.smash_charges, 6);
    assert_eq!(hooks.input_cbs, 6);
    // In hitlag none of the post-processing runs.
    rig.fp.x2219_b5 = true;
    rig.step_with(stick(0, 0), &mut hooks);
    assert_eq!(hooks.input_cbs, 6);
}

/// The stick-direction bits for every in-circle stick position: the sector
/// of the true angle, with the diagonals resolved by `atan2f`'s rounding
/// against the double bounds (up-right → up, up-left → left, down-left →
/// left, down-right → down).
#[test]
fn stick_direction_bits() {
    let mut pads = PadLib::melee();
    for y in -80i8..=80 {
        for x in -80i8..=80 {
            let (xi, yi) = (i32::from(x), i32::from(y));
            if xi * xi + yi * yi > 6400 {
                continue;
            }
            let mut poll: PadData = [PadStatus::DISCONNECTED; 4];
            poll[0] = stick(x, y).into();
            pads.renew_for_frame(&poll);
            let bits = pads.game[0].button & 0xf0000;
            let want = if xi * xi + yi * yi < 900 {
                0
            } else if xi == yi {
                if x > 0 {
                    PAD_STICK_UP
                } else {
                    PAD_STICK_LEFT
                }
            } else if xi == -yi {
                if x > 0 {
                    PAD_STICK_DOWN
                } else {
                    PAD_STICK_LEFT
                }
            } else if xi > yi.abs() {
                PAD_STICK_RIGHT
            } else if -xi > yi.abs() {
                PAD_STICK_LEFT
            } else if yi > 0 {
                PAD_STICK_UP
            } else {
                PAD_STICK_DOWN
            };
            assert_eq!(bits, want, "({x}, {y})");
        }
    }
}

#[test]
fn default_pad_status_and_library() {
    let status = HsdPadStatus::default();
    assert_eq!(status.cross_dir, 1);
    let pads = PadLib::melee();
    assert_eq!(pads.lib.qnum, 5);
    assert_eq!(pads.queue.len(), 5);
    assert_eq!(pads.lib.repeat_start, 45);
}

// ---- Disc-backed checks -------------------------------------------------

fn disc_path(var: &str, default: &str) -> Option<PathBuf> {
    let path = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(default));
    path.exists().then_some(path)
}

fn discs() -> Vec<PathBuf> {
    let found: Vec<_> = [
        disc_path("MELEE_VANILLA_ISO", "../../originals/melee-ntsc-1.02.iso"),
        disc_path("MELEE_ISO", "../../originals/20xx-hp-5.0.2.iso"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if found.is_empty() {
        eprintln!("skipping: no disc image");
    }
    found
}

fn read_dol(path: &PathBuf) -> Dol {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path).unwrap();
    let disc = Disc::read(&mut file).unwrap();
    let mut bytes = vec![0u8; disc.dol_size as usize];
    file.seek(SeekFrom::Start(disc.dol_offset)).unwrap();
    file.read_exact(&mut bytes).unwrap();
    Dol::parse(bytes).unwrap()
}

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// `PadLibData` in memory order, up to `rumble_info` (+0x2C).
fn pad_lib_bytes(p: &PadLibData) -> Vec<u8> {
    let mut b = vec![p.qnum, p.qread, p.qwrite, p.qcount, p.qtype, 0, 0, 0];
    b.extend([0; 4]); // queue pointer (NULL)
    b.extend(p.repeat_start.to_be_bytes());
    b.extend(p.repeat_interval.to_be_bytes());
    b.extend([p.adc_type as u8, p.adc_th as u8, 0, 0]);
    b.extend(p.adc_angle.to_be_bytes());
    b.extend([
        p.clamp_stick_type,
        p.clamp_stick_shift,
        p.clamp_stick_max as u8,
        p.clamp_stick_min as u8,
        p.clamp_analog_lr_shift,
        p.clamp_analog_lr_max,
        p.clamp_analog_lr_min,
        p.clamp_analog_ab_shift,
        p.clamp_analog_ab_max,
        p.clamp_analog_ab_min,
        p.scale_stick as u8,
        p.scale_analog_lr,
        p.scale_analog_ab,
        p.cross_dir,
        p.reset_switch_status,
        p.reset_switch,
    ]);
    b
}

/// Embedded constants match the 1.02 (and 20XX) executables: the pad
/// defaults, `gmMain_8015FD24`'s stores, `atanf_lookup`, and the direction
/// bounds.
#[test]
fn embedded_constants_match_the_dol() {
    for path in discs() {
        let dol = read_dol(&path);
        // default_status_data: zero except cross_dir (+0x40) = 1.
        let status = dol.at(0x8040_6d68, 0x44).unwrap();
        let mut want = [0u8; 0x44];
        want[0x40] = 1;
        assert_eq!(status, want, "{}", path.display());
        // default_libinfo_data
        assert_eq!(
            dol.at(0x8040_6dac, 0x2c).unwrap(),
            pad_lib_bytes(&PadLibData::default()),
            "{}",
            path.display()
        );
        // gmMain_8015FD24: `lis r3, 0x804c; addi r5, r3, 0x1f78` makes r5
        // &HSD_PadLibData; then `li` immediates stored with `stb rS, d(r5)`,
        // applied over the defaults.
        let setup = dol.at(0x8015_fd54, 8).unwrap();
        assert_eq!((be32(setup, 0), be32(setup, 4)), (0x3c60_804c, 0x38a3_1f78));
        let code = dol.at(0x8015_fd5c, 0x34).unwrap();
        let mut regs = [0u32; 32];
        // `HSD_PadInit(5, ...)` copies the defaults and sets qnum.
        let mut lib = pad_lib_bytes(&PadLibData {
            qnum: 5,
            ..PadLibData::default()
        });
        for i in 0..13 {
            let word = be32(code, i * 4);
            let (op, rd, ra, imm) = (
                word >> 26,
                (word >> 21) & 31,
                (word >> 16) & 31,
                word & 0xffff,
            );
            match (op, ra) {
                (14, 0) => regs[rd as usize] = imm, // li rD, imm
                (38, 5) => lib[imm as usize] = regs[rd as usize] as u8, // stb rS, d(r5)
                _ => panic!("unexpected instruction {word:08x}"),
            }
        }
        assert_eq!(
            lib,
            pad_lib_bytes(&PadLib::melee().lib),
            "{}",
            path.display()
        );
        // atanf_lookup
        let table = dol.at(0x803b_7300, 0xb8).unwrap();
        for (i, bits) in ATANF_LOOKUP_BITS.iter().enumerate() {
            assert_eq!(be32(table, i * 4), *bits, "atanf_lookup[{i}]");
        }
        // HSD_PadADConvertCheck1's doubles: M_PI_2 at 0x804DE5E0, then the
        // four bounds from 0x804DE5F8.
        let read_f64 = |address| {
            f64::from_bits(u64::from_be_bytes(
                dol.at(address, 8).unwrap().try_into().unwrap(),
            ))
        };
        assert_eq!(read_f64(0x804d_e5e0), AD_CONVERT_CONSTANTS[0]);
        for (i, want) in AD_CONVERT_CONSTANTS[1..].iter().enumerate() {
            assert_eq!(read_f64(0x804d_e5f8 + 8 * i as u32), *want);
        }
    }
}

#[test]
fn common_data_thresholds_match_disc() {
    for path in discs() {
        let mut file = File::open(&path).unwrap();
        let disc = Disc::read(&mut file).unwrap();
        let bytes = disc.read_file(&mut file, "PlCo.dat").unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        let disc_common = melee::ft::load_common_data(&archive).unwrap();
        let ours = common();
        macro_rules! same {
            ($($field:ident),*) => {$(
                assert_eq!(disc_common.$field, ours.$field, stringify!($field));
            )*};
        }
        same!(
            horizontal_stick_deadzone,
            vertical_stick_deadzone,
            horizontal_stick_smash_deadzone,
            vertical_stick_smash_deadzone,
            analog_shoulder_deadzone,
            z_press_analog_value,
            shield_press_threshold,
            tap_jump_threshold,
            tap_jump_window,
            x88,
            x8c,
            x218,
            x21c,
            x6fc,
            x7b8,
            x7bc,
            x7c0
        );
    }
}
