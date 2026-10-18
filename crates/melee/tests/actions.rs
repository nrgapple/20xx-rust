//! Motion tables, subaction scripts, and animation slices from real discs.
//!
//! Runs on each disc that is present: `MELEE_ISO` (default
//! `originals/20xx-hp-5.0.2.iso`) and `MELEE_VANILLA_ISO` (default
//! `originals/melee-ntsc-1.02.iso`). Without either, the tests print a note
//! and pass. Run with `--nocapture` to see the coverage summary.

use gc_disc::{Disc, Dol};
use hsd_dat::Archive;
use melee::ft::action::{
    decode_script, Command, FIGHTER_COMMAND_WORDS, FIGHTER_COMMAND_WORDS_ADDRESS,
};
use melee::ft::kinds::FIGHTERS;
use melee::ft::motion::{
    anim_slice, load_demo_motions, load_fighter_data, load_motions, Motion, ANIM_FILES,
    DEMO_MOTION_COUNTS, DEMO_MOTION_COUNTS_ADDRESS, KIND_NANA, KIND_POPO, MAX_ANIM_SIZE,
    MOTION_COUNTS, MOTION_COUNTS_ADDRESS,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// The tournament cast: `FighterKind` 0 (Mario) through 0x1A (Roy),
/// including Nana.
const TOURNAMENT_KINDS: std::ops::Range<usize> = 0..27;

struct Image {
    label: &'static str,
    disc: Disc,
    file: File,
}

impl Image {
    fn read(&mut self, path: &str) -> Vec<u8> {
        self.disc
            .read_file(&mut self.file, path)
            .expect("file on disc")
    }

    fn archive(&mut self, path: &str) -> Archive {
        Archive::parse(&self.read(path)).expect("valid archive")
    }

    fn dol(&mut self) -> Dol {
        let mut bytes = vec![0; self.disc.dol_size as usize];
        self.file
            .seek(SeekFrom::Start(self.disc.dol_offset))
            .unwrap();
        self.file.read_exact(&mut bytes).unwrap();
        Dol::parse(bytes).expect("valid DOL")
    }
}

fn images() -> Vec<Image> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../originals");
    let candidates = [
        ("20XX", "MELEE_ISO", "20xx-hp-5.0.2.iso"),
        ("vanilla", "MELEE_VANILLA_ISO", "melee-ntsc-1.02.iso"),
    ];
    let mut found = Vec::new();
    for (label, var, default) in candidates {
        let path = std::env::var_os(var)
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(default));
        let Ok(mut file) = File::open(&path) else {
            eprintln!("skipping {label}: no disc image at {}", path.display());
            continue;
        };
        let disc = Disc::read(&mut file).expect("readable disc");
        found.push(Image { label, disc, file });
    }
    found
}

fn be32(bytes: &[u8], index: usize) -> u32 {
    u32::from_be_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
}

/// The DOL's own tables agree with the constants the port uses.
#[test]
fn dol_tables_match_constants() {
    for mut image in images() {
        let dol = image.dol();
        let counts = dol.at(MOTION_COUNTS_ADDRESS, 33 * 8).unwrap();
        let demo = dol.at(DEMO_MOTION_COUNTS_ADDRESS, 33 * 8).unwrap();
        for kind in 0..33 {
            // `{void* data, int count}`: data is NULL until loaded.
            assert_eq!(be32(counts, kind * 2), 0);
            assert_eq!(
                be32(counts, kind * 2 + 1),
                MOTION_COUNTS[kind],
                "kind {kind}"
            );
            assert_eq!(be32(demo, kind * 2), 0);
            assert_eq!(
                be32(demo, kind * 2 + 1),
                DEMO_MOTION_COUNTS[kind],
                "kind {kind}"
            );
        }
        let lengths = dol.at(FIGHTER_COMMAND_WORDS_ADDRESS, 49).unwrap();
        assert_eq!(lengths, FIGHTER_COMMAND_WORDS, "{}", image.label);
        // ftData_803C23E4: pointers to each kind's AJ file name.
        let names = dol.at(0x803C_23E4, 33 * 4).unwrap();
        for (kind, expected) in ANIM_FILES.iter().enumerate() {
            let address = be32(names, kind);
            let text = dol.at(address, expected.len() as u32 + 1).unwrap();
            assert_eq!(&text[..expected.len()], expected.as_bytes());
            assert_eq!(text[expected.len()], 0);
        }
    }
}

/// Offsets that start some object: public symbols and pointer targets.
fn boundaries(archive: &Archive) -> BTreeSet<u32> {
    let mut set: BTreeSet<u32> = archive.publics().iter().map(|s| s.offset).collect();
    for &reloc in archive.relocations() {
        set.insert(archive.u32(reloc).unwrap());
    }
    set.insert(archive.data().len() as u32);
    set
}

#[derive(Default)]
struct Coverage {
    fighters: usize,
    motions: usize,
    demo_motions: usize,
    scripts: BTreeSet<(usize, u32)>,
    commands: usize,
    tournament_scripts: usize,
    tournament_commands: usize,
    distinct_commands: usize,
    tournament_distinct_commands: usize,
    null_jumps: usize,
    opcodes: BTreeMap<u32, usize>,
    max_stack: u32,
    with_anim: usize,
    slices_parsed: usize,
    tournament_slices: usize,
    largest_anim: (u32, String),
    nana_borrowed: usize,
    handler_mismatches: Vec<String>,
    extent_notes: Vec<String>,
}

/// Fields the handlers use as array indices stay inside the arrays
/// (`Fighter::x914[4]` hitboxes, `xDF4[2]` throw hitboxes), loops have a
/// nonzero count (`Command_04` would run 2^32 times otherwise), and the
/// first-byte encodings 20XX's custom subaction events test for (`SDR.txt`:
/// 0x33 for `SetHitboxDamage`, 0x5D-0x5F for `AllowInterrupt`, 0x75 for
/// `SetJabCombo`) do not appear. `word` is the command's first word.
fn in_range(command: &Command, word: u32) -> bool {
    match command {
        Command::AllowInterrupt => word == 23 << 26,
        Command::Hitbox(h, ..) => h.id < 4,
        Command::SetHitboxDamage(c) => c.idx < 4,
        Command::SetHitboxScale(c) => c.idx < 4,
        Command::SetHitboxFlag(c) => c.idx < 4,
        Command::RemoveHitbox(c) => c.hit_idx < 4,
        Command::ThrowHitbox(c, ..) => c.idx < 2,
        Command::SetLoop(c) => c.value != 0,
        Command::SetJabCombo(c) => c.disabled <= 1,
        _ => true,
    }
}

fn check_motions(
    label: &str,
    name: &str,
    kind: usize,
    archive: &Archive,
    motions: &[Motion],
    words: &mut BTreeMap<u32, u32>,
    coverage: &mut Coverage,
) {
    for motion in motions {
        let Some(entry) = motion.script else {
            continue;
        };
        let script = decode_script(archive, entry)
            .unwrap_or_else(|e| panic!("{label} {name} motion {}: {e}", motion.index));
        coverage.max_stack = coverage.max_stack.max(script.max_stack_words);
        if !coverage.scripts.insert((kind, entry)) {
            continue;
        }
        if TOURNAMENT_KINDS.contains(&kind) {
            coverage.tournament_scripts += 1;
            coverage.tournament_commands += script.commands.len();
        }
        for (offset, command) in &script.commands {
            // Every script of the archive decodes each word the same way.
            for i in 0..command.words() {
                let start = *words.entry(offset + 4 * i).or_insert(*offset);
                assert_eq!(
                    start, *offset,
                    "{label} {name}: scripts disagree at {offset:#x}"
                );
            }
            if matches!(
                command,
                Command::Goto { target: None } | Command::Subroutine { target: None }
            ) {
                coverage.null_jumps += 1;
            }
            coverage.commands += 1;
            *coverage.opcodes.entry(command.opcode()).or_default() += 1;
            let (lo, hi) = command.handler_words();
            if (lo, hi) != (command.words(), command.words()) {
                coverage.handler_mismatches.push(format!(
                    "{label} {name} motion {} at {offset:#x}: {command} ({lo}-{hi} words)",
                    motion.index
                ));
            }
            let word = archive.u32(*offset).unwrap();
            assert!(
                in_range(command, word),
                "{label} {name}: {command} at {offset:#x}"
            );
        }
    }
}

#[test]
fn every_fighter_motion_decodes_and_animates() {
    for mut image in images() {
        let label = image.label;
        let mut coverage = Coverage::default();
        for kind in 0..FIGHTERS.len() {
            let (name, file, symbol) = FIGHTERS[kind];
            let archive = image.archive(file);
            let motions = load_motions(&archive, symbol).unwrap_or_else(|e| panic!("{name}: {e}"));
            let demo =
                load_demo_motions(&archive, symbol).unwrap_or_else(|e| panic!("{name}: {e}"));
            coverage.fighters += 1;
            coverage.motions += motions.len();
            coverage.demo_motions += demo.len();

            // The table's extent in the data: up to the next object.
            let data = load_fighter_data(&archive, symbol).unwrap();
            let bounds = boundaries(&archive);
            for (table, count, what) in [
                (data.xc.0.unwrap(), MOTION_COUNTS[kind], "xC"),
                (data.x14.0.unwrap(), DEMO_MOTION_COUNTS[kind], "x14"),
            ] {
                let next = *bounds.range(table + 1..).next().unwrap();
                let extent = next - table;
                if extent != count * 0x18 {
                    coverage.extent_notes.push(format!(
                        "{name} {what}: {count} entries, extent {extent:#x}"
                    ));
                }
            }

            // Data offset of each decoded word -> its command's start.
            let mut words = BTreeMap::new();
            check_motions(
                label,
                name,
                kind,
                &archive,
                &motions,
                &mut words,
                &mut coverage,
            );
            check_motions(
                label,
                name,
                kind,
                &archive,
                &demo,
                &mut words,
                &mut coverage,
            );
            let distinct = words.values().collect::<BTreeSet<_>>().len();
            coverage.distinct_commands += distinct;
            if TOURNAMENT_KINDS.contains(&kind) {
                coverage.tournament_distinct_commands += distinct;
            }

            // Every motion with animation data is an archive in the AJ bank
            // that exports the motion's figatree by name.
            let bank = image.read(ANIM_FILES[kind]);
            for motion in &motions {
                assert_eq!(motion.x14, 0, "{name}: x14 is filled at load time");
                if motion.aj_size == 0 {
                    assert_eq!(motion.anim_name, None, "{name} {}", motion.index);
                    if kind == KIND_NANA {
                        coverage.nana_borrowed += 1;
                    }
                    continue;
                }
                coverage.with_anim += 1;
                let anim_name = motion
                    .anim_name
                    .as_deref()
                    .unwrap_or_else(|| panic!("{name} {}: sized but unnamed", motion.index));
                let slice = anim_slice(&bank, motion)
                    .unwrap_or_else(|| panic!("{name} {}: slice outside bank", motion.index));
                let anim = Archive::parse(slice)
                    .unwrap_or_else(|e| panic!("{name} {} {anim_name}: {e}", motion.index));
                assert!(
                    anim.publics().iter().any(|s| s.name == anim_name),
                    "{name} {}: {anim_name} not exported",
                    motion.index
                );
                assert!(anim_name.ends_with("_figatree"), "{anim_name}");
                coverage.slices_parsed += 1;
                if TOURNAMENT_KINDS.contains(&kind) {
                    coverage.tournament_slices += 1;
                }
                let size = motion.aj_size as u32;
                if size > coverage.largest_anim.0 {
                    coverage.largest_anim = (size, format!("{name} {anim_name}"));
                }
            }
        }
        assert_eq!(coverage.slices_parsed, coverage.with_anim);
        assert!(
            coverage.largest_anim.0 <= MAX_ANIM_SIZE,
            "{:?} exceeds the figatree limit",
            coverage.largest_anim
        );
        let tournament: u32 = TOURNAMENT_KINDS.map(|k| MOTION_COUNTS[k]).sum();
        eprintln!(
            "{label}: {} fighters, {} motions ({tournament} tournament) + {} demo motions; \
             {} distinct scripts, {} commands decoded, {} at distinct offsets \
             (tournament: {} scripts, {} commands, {} distinct); \
             {} NULL gotos/subroutines; max stack {} words; \
             {} animated motions, {} AJ slices parsed (tournament: {}); largest {:#x} ({}); \
             Nana motions without animation: {}",
            coverage.fighters,
            coverage.motions,
            coverage.demo_motions,
            coverage.scripts.len(),
            coverage.commands,
            coverage.distinct_commands,
            coverage.tournament_scripts,
            coverage.tournament_commands,
            coverage.tournament_distinct_commands,
            coverage.null_jumps,
            coverage.max_stack,
            coverage.with_anim,
            coverage.slices_parsed,
            coverage.tournament_slices,
            coverage.largest_anim.0,
            coverage.largest_anim.1,
            coverage.nana_borrowed,
        );
        eprintln!("{label}: opcode counts {:?}", coverage.opcodes);
        let unused: Vec<u32> = (0..59)
            .filter(|op| !coverage.opcodes.contains_key(op))
            .collect();
        eprintln!("{label}: opcodes never used {unused:?}");
        for note in &coverage.extent_notes {
            eprintln!("{label}: extent {note}");
        }
        for note in &coverage.handler_mismatches {
            eprintln!("{label}: handler length differs: {note}");
        }
        assert!(coverage.handler_mismatches.is_empty());
        // Each table runs exactly up to the next object in the archive, so
        // the DOL's counts match the data.
        assert!(coverage.extent_notes.is_empty());
        // The CommandInfo stack (Fighter+0x3F4..0x408) holds five words.
        assert!(coverage.max_stack <= 5);
    }
}

/// Nana's motions without animation use Popo's (`ftData_80085FD4`).
#[test]
fn nana_borrows_popo_animations() {
    for mut image in images() {
        let popo =
            load_motions(&image.archive(FIGHTERS[KIND_POPO].1), FIGHTERS[KIND_POPO].2).unwrap();
        let nana =
            load_motions(&image.archive(FIGHTERS[KIND_NANA].1), FIGHTERS[KIND_NANA].2).unwrap();
        assert_eq!(popo.len(), nana.len());
        let borrowed = nana
            .iter()
            .filter(|m| m.aj_size == 0 && popo[m.index as usize].aj_size != 0);
        eprintln!(
            "{}: Nana borrows {} of Popo's animations",
            image.label,
            borrowed.count()
        );
    }
}

/// Fox's up-smash, against widely published frame data: 18% clean hits from
/// frame 7, 13% late hits from frame 10, hitboxes gone after frame 17.
#[test]
fn fox_up_smash_script() {
    for mut image in images() {
        let archive = image.archive("PlFx.dat");
        let motions = load_motions(&archive, "ftDataFox").unwrap();
        let usmash = motions
            .iter()
            .find(|m| m.anim_name.as_deref() == Some("PlyFox5K_Share_ACTION_AttackHi4_figatree"))
            .unwrap();
        let script = decode_script(&archive, usmash.script.unwrap()).unwrap();
        let mut frame = 0;
        let mut hits = Vec::new();
        for command in script.commands.values() {
            match command {
                Command::AsynchronousTimer(t) => frame = t.value,
                Command::SynchronousTimer(t) => frame += t.value,
                Command::Hitbox(h0, _, _, h3, h4) => {
                    hits.push((
                        frame,
                        h0.id,
                        h0.damage,
                        h3.angle,
                        h3.knockback_growth,
                        h4.base_knockback,
                    ));
                }
                Command::ClearHitboxes => hits.push((frame, 9, 0, 0, 0, 0)),
                _ => {}
            }
        }
        assert_eq!(
            hits,
            [
                (7, 0, 18, 80, 112, 30),
                (7, 1, 18, 80, 112, 30),
                (10, 0, 13, 361, 100, 10),
                (10, 1, 13, 361, 100, 10),
                (18, 9, 0, 0, 0, 0),
            ]
        );
        assert!(matches!(
            script.commands.values().last(),
            Some(Command::End(_))
        ));
    }
}

/// Both discs give the same motion tables and scripts for every fighter.
#[test]
fn discs_agree() {
    let mut images = images();
    if images.len() < 2 {
        eprintln!("skipping: needs both discs");
        return;
    }
    for &(name, file, symbol) in FIGHTERS {
        let decoded: Vec<_> = images
            .iter_mut()
            .map(|image| {
                let archive = image.archive(file);
                let motions = load_motions(&archive, symbol).unwrap();
                let scripts: Vec<_> = motions
                    .iter()
                    .filter_map(|m| m.script)
                    .map(|entry| decode_script(&archive, entry).unwrap())
                    .collect();
                (motions, scripts)
            })
            .collect();
        assert!(decoded[0] == decoded[1], "{name} differs between discs");
    }
}

/// 20XX appends a 37-word script to `PlCa.dat`, after the archive's string
/// table, for its debug-menu "Shinesplosion" option (`Characters.txt`: the
/// code at 0x800180F4 copies replacement entries for actions 311 and 312,
/// whose script offset 0x277A0 points past the data block). It decodes as
/// ordinary commands and ends exactly at the end of the file.
#[test]
fn twentyxx_falcon_appendix_decodes() {
    for mut image in images() {
        let bytes = image.read("PlCa.dat");
        if image.label == "vanilla" {
            assert_eq!(bytes.len(), 0x277be);
            continue;
        }
        let archive = Archive::parse(&bytes).unwrap();
        assert_eq!(bytes.len(), 0x27850);
        assert!(archive.data().len() < 0x277a0);
        let words: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_be_bytes(*c))
            .collect();
        // Data offset 0x277A0 is file offset 0x277C0 (after the 0x20 header).
        let mut index = 0x277c0 / 4;
        let mut opcodes = Vec::new();
        loop {
            let command = Command::decode(&words[index..], None).expect("valid command");
            opcodes.push(command.opcode());
            index += command.words() as usize;
            if matches!(command, Command::End(_)) {
                break;
            }
        }
        assert_eq!(index * 4, bytes.len());
        assert_eq!(
            opcodes,
            [17, 2, 21, 2, 10, 43, 17, 11, 11, 11, 1, 21, 16, 46, 1, 0]
        );
    }
}
