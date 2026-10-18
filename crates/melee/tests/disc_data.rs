//! Load real game data from a local disc image and check it against values
//! documented by the decomp and the competitive community.
//!
//! Uses `MELEE_ISO`, or `originals/20xx-hp-5.0.2.iso` in the workspace.
//! The disc is never committed; without it these tests print a note and pass.

use gc_disc::Disc;
use hsd_dat::Archive;
use melee::ft::{load_attributes, load_common_data};
use std::fs::File;
use std::path::PathBuf;

fn disc_archive(path: &str) -> Option<Archive> {
    let iso = std::env::var_os("MELEE_ISO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../originals/20xx-hp-5.0.2.iso")
        });
    let Ok(mut file) = File::open(&iso) else {
        eprintln!("skipping: no disc image at {}", iso.display());
        return None;
    };
    let disc = Disc::read(&mut file).expect("readable disc");
    let bytes = disc.read_file(&mut file, path).expect("file on disc");
    Some(Archive::parse(&bytes).expect("valid archive"))
}

#[test]
fn common_data_matches_decomp_annotations() {
    let Some(archive) = disc_archive("PlCo.dat") else {
        return;
    };
    let common = load_common_data(&archive).unwrap();
    // `@datvalue{GALE01, PlCo.dat, ...}` annotations in ft/types.h.
    assert_eq!(common.horizontal_stick_deadzone, 0.28);
    assert_eq!(common.vertical_stick_deadzone, 0.28);
    assert_eq!(common.horizontal_stick_smash_deadzone, 0.25);
    assert_eq!(common.vertical_stick_smash_deadzone, 0.25);
    assert_eq!(common.analog_shoulder_deadzone, 0.3);
    assert_eq!(common.z_press_analog_value, 0.35);
    assert_eq!(common.shield_press_threshold, 0.25);
    assert_eq!(common.walk_stick_threshold, 0.18);
    assert_eq!(common.run_dash_turn_friction_multiplier, 1.0);
    assert_eq!(common.friction_when_above_walk_speed, 2.0);
    assert_eq!(common.tap_jump_threshold, 0.6625);
    assert_eq!(common.tap_jump_window, 4);
    assert_eq!(common.relaxed_tap_jump_threshold, 0.5625);
    assert_eq!(common.teeter_walk_threshold, 0.75);
}

#[test]
fn fox_attributes_match_known_values() {
    let Some(archive) = disc_archive("PlFx.dat") else {
        return;
    };
    let fox = load_attributes(&archive, "ftDataFox").unwrap();
    // Widely published 1.02 values for Fox. The often-quoted 2.02 initial dash
    // speed is the first frame at full stick: 1.9 + 0.1 * 1.0 + 0.02.
    assert_eq!(fox.walk_max_vel, 1.6);
    assert_eq!(fox.dash_initial_velocity, 1.9);
    assert_eq!(fox.dash_accel_mul, 0.1);
    assert_eq!(fox.dash_accel_base, 0.02);
    assert_eq!(fox.dash_max_velocity, 2.2);
    assert_eq!(fox.ground_friction, 0.08);
    assert_eq!(fox.jump_startup_time, 3.0);
    assert_eq!(fox.jump_v_initial_velocity, 3.68);
    assert_eq!(fox.hop_v_initial_velocity, 2.1);
    assert_eq!(fox.max_jumps, 2);
    assert_eq!(fox.gravity, 0.23);
    assert_eq!(fox.terminal_velocity, 2.8);
    assert_eq!(fox.fast_fall_velocity, 3.4);
    assert_eq!(fox.air_drift_max, 0.83);
    assert_eq!(fox.weight, 75.0);
}
