//! Inspect HSD archives directly on a GameCube disc image.
//!
//! `dat-inspect DISC.iso` parses every `.dat`/`.usd` archive and reports failures.
//! `dat-inspect DISC.iso FILE.dat` prints one archive's public and extern symbols.

use gc_disc::Disc;
use hsd_dat::Archive;
use std::fs::File;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        return Err("usage: dat-inspect DISC.iso [DISC_PATH.dat]".into());
    }
    let mut iso = File::open(&args[0])?;
    let disc = Disc::read(&mut iso)?;
    if let Some(path) = args.get(1) {
        let archive = Archive::parse(&disc.read_file(&mut iso, path)?)?;
        println!(
            "data {:#x} bytes, {} relocations",
            archive.data().len(),
            archive.relocations().len()
        );
        for symbol in archive.publics() {
            println!("public {:#08x} {}", symbol.offset, symbol.name);
        }
        for symbol in archive.externs() {
            println!("extern {:#08x} {}", symbol.offset, symbol.name);
        }
        return Ok(());
    }
    let (mut parsed, mut failed) = (0, 0);
    for entry in disc.files(&mut iso)? {
        if !(entry.path.ends_with(".dat") || entry.path.ends_with(".usd")) {
            continue;
        }
        match Archive::parse(&disc.read_file(&mut iso, &entry.path)?) {
            Ok(_) => parsed += 1,
            Err(error) => {
                failed += 1;
                println!("FAIL {}: {error}", entry.path);
            }
        }
    }
    println!("{parsed} archives parsed, {failed} failed");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dat-inspect: {error}");
        std::process::exit(1);
    }
}
