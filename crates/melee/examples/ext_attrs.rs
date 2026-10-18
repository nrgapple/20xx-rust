//! Print a fighter's special attributes (`ftData::ext_attr`) as JSON:
//! `cargo run -p melee --example ext_attrs -- DISC.iso PlCa.dat ftDataCaptain`

use gc_disc::Disc;
use hsd_dat::Archive;
use melee::dat::DatJson;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [iso, file, symbol] = args.as_slice() else {
        return Err("usage: ext_attrs DISC.iso Pl??.dat ftDataName".into());
    };
    let mut iso = File::open(iso)?;
    let disc = Disc::read(&mut iso)?;
    let archive = Archive::parse(&disc.read_file(&mut iso, file)?)?;
    match melee::ft::ext_attrs::load_ext_attrs(&archive, symbol)? {
        Some(attrs) => println!("{}", attrs.to_json()),
        None => println!("null"),
    }
    Ok(())
}
