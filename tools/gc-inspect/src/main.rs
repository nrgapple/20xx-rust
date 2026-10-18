use gc_disc::Disc;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};

const USAGE: &str = "usage: gc-inspect DISC.iso [--extract-dol OUTPUT.dol | --list | --extract-file DISC_PATH OUTPUT]";

/// Copy `size` bytes at `offset` into a new file. Never overwrites an original
/// or an earlier extraction.
fn extract(file: &mut File, offset: u64, size: u64, output: &OsString) -> io::Result<()> {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.seek(SeekFrom::Start(offset))?;
    let copied = io::copy(&mut file.take(size), &mut out)?;
    if copied != size {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "disc changed or was truncated during extraction",
        ));
    }
    out.flush()?;
    println!("Extracted {copied} bytes to {}", output.to_string_lossy());
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mode = args.get(1).and_then(|a| a.to_str());
    let valid = matches!(
        (args.len(), mode),
        (1, None) | (2, Some("--list")) | (3, Some("--extract-dol")) | (4, Some("--extract-file"))
    );
    if !valid {
        return Err(USAGE.into());
    }
    let mut file = File::open(&args[0])?;
    let disc = Disc::read(&mut file)?;
    match mode {
        Some("--list") => {
            for entry in disc.files(&mut file)? {
                println!("{:#010x} {:>10} {}", entry.offset, entry.size, entry.path);
            }
            return Ok(());
        }
        Some("--extract-file") => {
            let path = args[2].to_str().ok_or("disc path must be UTF-8")?;
            let entry = disc.find_file(&mut file, path)?;
            extract(&mut file, entry.offset, entry.size, &args[3])?;
            return Ok(());
        }
        _ => {}
    }
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
        extract(&mut file, disc.dol_offset, disc.dol_size, &args[2])?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("gc-inspect: {error}");
        std::process::exit(1);
    }
}
