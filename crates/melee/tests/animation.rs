//! Real-data smoke and restore/replay checks for the HSD track interpreter.
//! These are parser/finite-output checks, NOT a frame-fidelity oracle.
use gc_disc::Disc;
use hsd_dat::Archive;
use melee::ft::{
    kinds::FIGHTERS,
    motion::{anim_slice, load_motions, ANIM_FILES},
};
use melee::hsd::fobj::Track;
use std::{fs::File, path::PathBuf};

#[test]
fn tournament_animation_tracks_decode_and_restore() {
    let path = std::env::var_os("MELEE_VANILLA_ISO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../originals/melee-ntsc-1.02.iso")
        });
    let Ok(mut file) = File::open(path) else {
        eprintln!("skipping real animation data: no developer disc");
        return;
    };
    let disc = Disc::read(&mut file).unwrap();
    let mut count = 0;
    for kind in 0..27 {
        let (_, archive, symbol) = FIGHTERS[kind];
        let a = Archive::parse(&disc.read_file(&mut file, archive).unwrap()).unwrap();
        let bank = disc.read_file(&mut file, ANIM_FILES[kind]).unwrap();
        for motion in load_motions(&a, symbol).unwrap() {
            let Some(bytes) = anim_slice(&bank, &motion) else {
                continue;
            };
            let anim = Archive::parse(bytes).unwrap();
            let Some(name) = &motion.anim_name else {
                continue;
            };
            let root = anim.public(name).unwrap();
            // FigaTree / FigaTrack layouts in melee/lb/lbanim.h.
            let nodes = anim.non_null_ptr(root + 12).unwrap();
            let tracks = anim.non_null_ptr(root + 16).unwrap();
            let end = anim.f32(root + 8).unwrap().min(120.);
            let mut index = 0;
            for node in 0..256 {
                let n = anim.u8(nodes + node).unwrap();
                if n == 255 {
                    break;
                }
                assert!(n < 128, "invalid node count in {name}");
                for _ in 0..n {
                    let p = tracks + index * 12;
                    index += 1;
                    let size = u32::from(anim.u16(p).unwrap());
                    let data = if size == 0 {
                        &[][..]
                    } else {
                        anim.bytes(anim.non_null_ptr(p + 8).unwrap(), size).unwrap()
                    };
                    let mut track = Track::new(
                        data,
                        anim.u8(p + 4).unwrap(),
                        anim.u8(p + 5).unwrap(),
                        anim.u8(p + 6).unwrap(),
                        anim.u16(p + 2).unwrap() as i16,
                    )
                    .unwrap();
                    track.request(0.).unwrap();
                    let first = track
                        .advance(0.)
                        .unwrap_or_else(|e| panic!("{name} track {index} at frame0: {e}"));
                    assert!(first.iter().all(|v| v.is_finite()), "{name} {index}");
                    let saved = track.clone();
                    let advanced = track
                        .advance(end)
                        .unwrap_or_else(|e| panic!("{name} track {index} at {end}: {e}"));
                    assert!(advanced.iter().all(|v| v.is_finite()), "{name} {index}");
                    let repeated = saved.clone().advance(end).unwrap();
                    assert_eq!(
                        advanced.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        repeated.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                    );
                    count += 1;
                }
            }
        }
    }
    assert!(count > 100_000, "unexpectedly small corpus: {count}");
    eprintln!(
        "validated {count} animation tracks (start, advance up to 120 frames, clone/restore)"
    );
}
