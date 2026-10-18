//! Print a fighter's `ftCo_DatAttrs` from a disc image:
//! `cargo run -p melee --example fighter_attrs -- DISC.iso PlFx.dat ftDataFox`

use gc_disc::Disc;
use hsd_dat::Archive;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [iso, file, symbol] = args.as_slice() else {
        return Err("usage: fighter_attrs DISC.iso Pl??.dat ftDataName".into());
    };
    let mut iso = File::open(iso)?;
    let disc = Disc::read(&mut iso)?;
    let archive = Archive::parse(&disc.read_file(&mut iso, file)?)?;
    println!("{:#?}", melee::ft::load_attributes(&archive, symbol)?);
    Ok(())
}
