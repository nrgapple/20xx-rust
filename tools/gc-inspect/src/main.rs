use gc_disc::Disc;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 && !(args.len() == 3 && args[1] == "--extract-dol") {
        return Err("usage: gc-inspect DISC.iso [--extract-dol OUTPUT.dol]".into());
    }
    let mut file = File::open(&args[0])?;
    let disc = Disc::read(&mut file)?;
    println!("Game ID: {}", disc.game_id);
    println!("Revision byte: {}", disc.revision);
    println!("Title: {}", disc.title);
    println!(
        "DOL offset: {:#x}; size: {:#x}",
        disc.dol_offset, disc.dol_size
    );
    println!("Entry point: {:#010x}", disc.entry_point);
    for section in &disc.sections {
        println!(
            "{}: file {:#x}, memory {:#010x}, size {:#x}",
            section.kind, section.file_offset, section.address, section.size
        );
    }
    if args.len() == 3 {
        // Never overwrite an original or an earlier extraction.
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        file.seek(SeekFrom::Start(disc.dol_offset))?;
        let copied = io::copy(&mut file.take(disc.dol_size), &mut output)?;
        if copied != disc.dol_size {
            return Err("disc changed or was truncated during extraction".into());
        }
        output.flush()?;
        println!("Extracted {copied} bytes to {}", args[2].to_string_lossy());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("gc-inspect: {error}");
        std::process::exit(1);
    }
}
