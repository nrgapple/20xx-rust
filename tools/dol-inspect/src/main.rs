//! Disassemble functions from a DOL by symbol name, for porting audits.
//!
//! `dol-inspect MAIN.dol SYMBOLS.txt NAME...` prints each function and its
//! fused multiply-add count. SYMBOLS.txt uses the decomp's
//! `config/GALE01/symbols.txt` format:
//! `name = .text:0xADDRESS; // type:function size:0xSIZE ...`.
//! A NAME may also be a raw `0xADDRESS:0xSIZE` range.
//!
//! `dol-inspect --diff BASE.dol PATCHED.dol SYMBOLS.txt` lists every symbol
//! whose bytes differ between two DOLs with the same section layout, plus
//! changed ranges no symbol covers.

use gc_disc::Dol;
use ppc_asm::{decode, is_fused};

fn parse_hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

/// Find `(address, size)` for a function symbol.
fn lookup(symbols: &str, name: &str) -> Option<(u32, u32)> {
    if let Some((address, size)) = name.split_once(':') {
        return Some((parse_hex(address)?, parse_hex(size)?));
    }
    symbols.lines().find_map(|line| {
        let (symbol, rest) = line.split_once(" = ")?;
        if symbol != name || !rest.contains("type:function") {
            return None;
        }
        let address = parse_hex(rest.split_once(':')?.1.split(';').next()?)?;
        let size = parse_hex(rest.split("size:").nth(1)?.split_whitespace().next()?)?;
        Some((address, size))
    })
}

/// Every `(name, address, size, kind)` symbol with a size.
fn all_symbols(symbols: &str) -> Vec<(&str, u32, u32, &str)> {
    let mut list: Vec<_> = symbols
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(" = ")?;
            let address = parse_hex(rest.split_once(':')?.1.split(';').next()?)?;
            let size = parse_hex(rest.split("size:").nth(1)?.split_whitespace().next()?)?;
            let kind = rest.split("type:").nth(1)?.split_whitespace().next()?;
            Some((name, address, size, kind))
        })
        .collect();
    list.sort_by_key(|&(_, address, _, _)| address);
    list
}

fn diff(base: &Dol, patched: &Dol, symbols: &str) -> Result<(), Box<dyn std::error::Error>> {
    let symbols = all_symbols(symbols);
    let mut uncovered = Vec::<(u32, u32)>::new();
    let mut changed = Vec::<(&str, &str, u32, u32, u32)>::new();
    for section in &base.sections {
        let new = patched
            .at(section.address, section.size)
            .ok_or("section layouts differ")?;
        let old = base.at(section.address, section.size).unwrap();
        for offset in (0..section.size).filter(|&i| old[i as usize] != new[i as usize]) {
            let address = section.address + offset;
            let owner = symbols
                .partition_point(|&(_, start, _, _)| start <= address)
                .checked_sub(1)
                .map(|i| symbols[i])
                .filter(|&(_, start, size, _)| address < start + size);
            match owner {
                Some((name, start, size, kind)) => match changed.last_mut() {
                    Some(last) if last.0 == name => last.4 += 1,
                    _ => changed.push((name, kind, start, size, 1)),
                },
                None => match uncovered.last_mut() {
                    Some(last) if last.0 + last.1 == address => last.1 += 1,
                    _ => uncovered.push((address, 1)),
                },
            }
        }
    }
    for (name, kind, address, size, bytes) in &changed {
        println!("{address:#010x} {size:#8x} {bytes:>6} changed  {kind:<8} {name}");
    }
    for (address, len) in &uncovered {
        println!("{address:#010x} {len:#8x} {len:>6} changed  (no symbol)");
    }
    let total: u32 =
        changed.iter().map(|c| c.4).sum::<u32>() + uncovered.iter().map(|u| u.1).sum::<u32>();
    println!(
        "{} symbols changed, {} uncovered ranges, {total} bytes",
        changed.len(),
        uncovered.len()
    );
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 4 && args[0] == "--diff" {
        let base = Dol::parse(std::fs::read(&args[1])?)?;
        let patched = Dol::parse(std::fs::read(&args[2])?)?;
        return diff(&base, &patched, &std::fs::read_to_string(&args[3])?);
    }
    if args.len() < 3 {
        return Err("usage: dol-inspect MAIN.dol SYMBOLS.txt NAME... | --diff BASE.dol PATCHED.dol SYMBOLS.txt".into());
    }
    let dol = Dol::parse(std::fs::read(&args[0])?)?;
    let symbols = std::fs::read_to_string(&args[1])?;
    for name in &args[2..] {
        let (address, size) =
            lookup(&symbols, name).ok_or_else(|| format!("{name}: no such function symbol"))?;
        let code = dol
            .at(address, size)
            .ok_or_else(|| format!("{name}: {address:#x}+{size:#x} not loaded by DOL"))?;
        println!("{name} @ {address:#010x}, {size:#x} bytes");
        let mut fused = 0;
        for (index, word) in code.as_chunks::<4>().0.iter().enumerate() {
            let pc = address + index as u32 * 4;
            let word = u32::from_be_bytes(*word);
            let insn = decode(word, pc);
            fused += usize::from(is_fused(&insn));
            println!("  {pc:08x}: {word:08x}  {insn}");
        }
        println!("  fused multiply-adds: {fused}\n");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dol-inspect: {error}");
        std::process::exit(1);
    }
}
