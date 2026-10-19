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

#[test]
fn every_fighter_has_attributes() {
    for &(name, file, symbol) in melee::ft::kinds::FIGHTERS {
        let Some(archive) = disc_archive(file) else {
            return;
        };
        let attrs = load_attributes(&archive, symbol).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            attrs.gravity > 0.0 && attrs.terminal_velocity > 0.0,
            "{name}: implausible fall attributes {attrs:?}"
        );
    }
}

/// Scan a JSON rendering for misalignment signatures: integers holding float
/// bit patterns (huge) or floats holding small integers (denormal-sized).
/// Returns `field: value` for each suspicious scalar (array elements report
/// their field name).
fn misaligned_values(json: &str) -> Vec<String> {
    let mut field = "";
    let mut found = Vec::new();
    for part in json.split(['{', '}', '[', ']', ',']) {
        let value = match part.split_once("\":") {
            Some((key, value)) => {
                field = key.trim_start_matches('"');
                value
            }
            None => part,
        };
        let Ok(number) = value.parse::<f64>() else {
            continue;
        };
        let is_float = value.contains(['.', 'e', 'E']);
        if (is_float && number != 0.0 && !(1e-6..1e7).contains(&number.abs()))
            || (!is_float && number.abs() >= 1e7)
        {
            found.push(format!("{field}: {value}"));
        }
    }
    found
}

#[test]
fn every_fighter_has_plausible_special_attributes() {
    use melee::dat::DatJson;
    use melee::ft::ext_attrs::load_ext_attrs;
    let mut loaded = 0;
    for &(name, file, symbol) in melee::ft::kinds::FIGHTERS {
        let Some(archive) = disc_archive(file) else {
            return;
        };
        let Some(attrs) =
            load_ext_attrs(&archive, symbol).unwrap_or_else(|e| panic!("{name}: {e}"))
        else {
            continue;
        };
        loaded += 1;
        let bad = misaligned_values(&attrs.to_json());
        assert!(bad.is_empty(), "{name}: suspicious values {bad:?}");
    }
    assert_eq!(loaded, 27, "26 selectable characters plus Nana");
}

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
    if disc.find_file(&mut file, "AI_Engine.bin").is_ok() {
        eprintln!("skipping: {} is a 20XX disc, not vanilla", iso.display());
        return None;
    }
    let bytes = disc.read_file(&mut file, path).expect("file on disc");
    Some(Archive::parse(&bytes).expect("valid archive"))
}

fn load_stage(file: &str) -> Option<melee::mp::lib::MpLib> {
    use melee::gr::{ground_scale, load_ground_param};
    let archive = vanilla_archive(file)?;
    let param = load_ground_param(&archive).unwrap();
    let coll = melee::mp::load_map_coll(&archive).unwrap();
    Some(melee::mp::lib::MpLib::load(
        coll,
        ground_scale(Some(&param)),
        false,
    ))
}

/// Endpoints of floor line `id` after loading.
fn floor(mp: &melee::mp::lib::MpLib, id: usize) -> ((f32, f32), (f32, f32), u16) {
    let data = mp.coll_data.lines[mp.lines[id].inner.unwrap()];
    let a = mp.vtx[data.v0_idx as usize].pos;
    let b = mp.vtx[data.v1_idx as usize].pos;
    ((a.x, a.y), (b.x, b.y), data.lo_flags)
}

#[test]
fn final_destination_geometry() {
    let Some(mp) = load_stage("GrNLa.dat") else {
        return;
    };
    // The main floor spans the well-known ledge positions, x = +-85.5657.
    let (left, _, left_flags) = floor(&mp, 0);
    let (_, right, right_flags) = floor(&mp, 2);
    assert!((left.0 + 85.5657).abs() < 1e-4 && left.1 == 0.0);
    assert!((right.0 - 85.5657).abs() < 1e-4 && right.1 == 0.0);
    assert_eq!(left_flags & 0x200, 0x200, "ledge");
    assert_eq!(right_flags & 0x200, 0x200, "ledge");
    // Floors chain left to right, then into the right wall.
    assert_eq!(mp.line_get_next(0), 1);
    assert_eq!(mp.line_get_next(1), 2);
    assert_eq!(mp.line_get_prev(1), 0);
}

#[test]
fn battlefield_geometry() {
    let Some(mp) = load_stage("GrNBa.dat") else {
        return;
    };
    // grGroundParam scale 0.8 applied to every vertex.
    // Published figures are rounded; the game's own values are f32 products
    // (e.g. 0.8 * 72.0 = 57.600002 in single precision), so compare loosely.
    let platforms: Vec<_> = (0..6)
        .map(|id| floor(&mp, id))
        .filter(|&(_, _, flags)| flags & 0x100 != 0)
        .map(|(a, b, _)| [a.0, a.1, b.0, b.1])
        .collect();
    let expected = [
        [-57.6, 27.2, -20.0, 27.2],
        [-18.8, 54.4, 18.8, 54.4],
        [20.0, 27.2, 57.6, 27.2],
    ];
    assert_eq!(platforms.len(), 3);
    for (got, want) in platforms.iter().zip(expected) {
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-4, "{got:?} vs {want:?}");
        }
    }
    let (edge, _, _) = floor(&mp, 0);
    assert_eq!(edge, (-68.4, 0.0));
    // Pass-through platforms are not linked to neighbors.
    assert_eq!((mp.line_get_next(2), mp.line_get_prev(2)), (-1, -1));
}

#[test]
fn tournament_stages_load() {
    // Battlefield, Final Destination, Dream Land, Yoshi's Story, Fountain of
    // Dreams, Pokemon Stadium.
    for file in [
        "GrNBa.dat",
        "GrNLa.dat",
        "GrOp.dat",
        "GrSt.dat",
        "GrIz.dat",
        "GrPs.dat",
    ] {
        let Some(mp) = load_stage(file) else {
            return;
        };
        assert!(!mp.lines.is_empty() && !mp.vtx.is_empty(), "{file}");
        for (id, line) in mp.lines.iter().enumerate() {
            if let Some(inner) = line.inner {
                let data = mp.coll_data.lines[inner];
                assert!((data.v0_idx as usize) < mp.vtx.len(), "{file} line {id}");
                assert!((data.v1_idx as usize) < mp.vtx.len(), "{file} line {id}");
            }
        }
    }
}
