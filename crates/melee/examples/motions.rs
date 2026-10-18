//! Print a fighter's motion table, or decode one motion's subaction script:
//!
//! ```text
//! cargo run -p melee --example motions -- DISC.iso PlFx.dat ftDataFox
//! cargo run -p melee --example motions -- DISC.iso PlFx.dat ftDataFox AttackHi4
//! ```
//!
//! The optional last argument is a motion index or a substring of its
//! animation name. Add `--demo` to use the demo motion table (`ftData::x14`).

use gc_disc::Disc;
use hsd_dat::Archive;
use melee::ft::action::decode_script;
use melee::ft::motion::{anim_slice, kind_of, load_demo_motions, load_motions, ANIM_FILES};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    args.retain(|a| a != "--demo");
    let (iso, file, symbol, pick) = match args.as_slice() {
        [iso, file, symbol] => (iso, file, symbol, None),
        [iso, file, symbol, pick] => (iso, file, symbol, Some(pick)),
        _ => return Err("usage: motions DISC.iso Pl??.dat ftDataName [MOTION] [--demo]".into()),
    };
    let mut iso = File::open(iso)?;
    let disc = Disc::read(&mut iso)?;
    let archive = Archive::parse(&disc.read_file(&mut iso, file)?)?;
    let motions = if demo {
        load_demo_motions(&archive, symbol)?
    } else {
        load_motions(&archive, symbol)?
    };

    let Some(pick) = pick else {
        println!("idx  aj_offset  aj_size  script    flags     pair     animation");
        for m in &motions {
            println!(
                "{:3}  {:#9x}  {:#7x}  {:8}  {:08x}  {:7}  {}",
                m.index,
                m.aj_offset,
                m.aj_size,
                m.script.map_or("NULL".into(), |s| format!("{s:#x}")),
                m.flags,
                m.x10_pair
                    .map_or("-".into(), |p| format!("{},{}", p[0], p[1])),
                m.anim_name.as_deref().unwrap_or("(none)"),
            );
        }
        return Ok(());
    };

    let motion = match pick.parse::<usize>() {
        Ok(index) => motions.get(index),
        Err(_) => motions.iter().find(|m| {
            m.anim_name
                .as_deref()
                .is_some_and(|name| name.contains(&format!("_{pick}_")))
        }),
    }
    .ok_or("no such motion")?;
    println!(
        "motion {} {} (AJ {:#x}+{:#x}, flags {:08x})",
        motion.index,
        motion.anim_name.as_deref().unwrap_or("(none)"),
        motion.aj_offset,
        motion.aj_size,
        motion.flags
    );
    if !demo {
        let kind = kind_of(symbol).ok_or("unknown fighter")?;
        let bank = disc.read_file(&mut iso, ANIM_FILES[kind])?;
        if let Some(slice) = anim_slice(&bank, motion) {
            let anim = Archive::parse(slice)?;
            let names: Vec<_> = anim.publics().iter().map(|s| s.name.as_str()).collect();
            println!("animation archive publics: {names:?}");
        }
    }
    let Some(entry) = motion.script else {
        println!("no script");
        return Ok(());
    };
    let script = decode_script(&archive, entry)?;
    println!(
        "script at {entry:#x}: {} commands, max stack {} words",
        script.commands.len(),
        script.max_stack_words
    );
    // In the order execution first reaches each command.
    for offset in &script.order {
        let command = &script.commands[offset];
        let words: Vec<_> = (0..command.words())
            .map(|i| format!("{:08x}", archive.u32(offset + 4 * i).unwrap_or(0)))
            .collect();
        println!("{offset:#8x}  {:<44}  {command}", words.join(" "));
    }
    Ok(())
}
