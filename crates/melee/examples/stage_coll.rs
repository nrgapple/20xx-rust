//! Print a stage's collision lines after `mpLibLoad`:
//! `cargo run -p melee --example stage_coll -- DISC.iso GrNLa.dat`

use gc_disc::Disc;
use hsd_dat::Archive;
use melee::gr::{ground_scale, load_ground_param};
use melee::mp::lib::MpLib;
use melee::mp::{line_flag, load_map_coll};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [iso, file] = args.as_slice() else {
        return Err("usage: stage_coll DISC.iso Gr??.dat".into());
    };
    let mut iso = File::open(iso)?;
    let disc = Disc::read(&mut iso)?;
    let archive = Archive::parse(&disc.read_file(&mut iso, file)?)?;
    let param = load_ground_param(&archive)?;
    let mp = MpLib::load(load_map_coll(&archive)?, ground_scale(Some(&param)), false);
    println!(
        "scale {}, {} verts, {} lines, {} joints, ranges {:?}",
        param.y,
        mp.vtx.len(),
        mp.lines.len(),
        mp.joints.len(),
        mp.coll_data.ranges.map(|r| (r.start, r.count))
    );
    for (id, line) in mp.lines.iter().enumerate() {
        let Some(inner) = line.inner else { continue };
        let data = mp.coll_data.lines[inner];
        let (a, b) = (
            mp.vtx[data.v0_idx as usize].pos,
            mp.vtx[data.v1_idx as usize].pos,
        );
        let kind = match line.flags & line_flag::KIND {
            line_flag::FLOOR => "floor",
            line_flag::CEILING => "ceil",
            line_flag::RIGHT_WALL => "rwall",
            line_flag::LEFT_WALL => "lwall",
            _ => "?",
        };
        // Platform/ledge bits live in the data's lo_flags, which the
        // collision queries return; the runtime flags hold hi_flags.
        let lo = u32::from(data.lo_flags);
        let mut tags = Vec::new();
        if lo & line_flag::PLATFORM != 0 {
            tags.push("platform");
        }
        if lo & line_flag::LEDGE != 0 {
            tags.push("ledge");
        }
        println!(
            "{id:3} {kind:5} ({:9.4}, {:9.4}) -> ({:9.4}, {:9.4}) next {:3} prev {:3} flags {:#06x}/{:#06x} {}",
            a.x, a.y, b.x, b.y, mp.line_get_next(id), mp.line_get_prev(id), data.hi_flags, data.lo_flags, tags.join(",")
        );
    }
    Ok(())
}
