//! Offline FigaTree packaging. Preserves original streams and node order;
//! lbAnim_8001E6D8 (0x8001E6D8) supplies per-node AObj flags/end frames.
use crate::{ensure, Result};
use gc_disc::Disc;
use hsd_dat::Archive;
use melee::ft::{
    kinds::FIGHTERS,
    motion::{anim_slice, load_motions, ANIM_FILES},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
pub fn load(
    disc: &Disc,
    file: &mut File,
    id: &str,
    skeleton: &[Value],
    binary: &mut Vec<u8>,
) -> Result<Value> {
    let kind = match id {
        "mario" => 0,
        "fox" => 1,
        "falcon" => 2,
        "marth" => 18,
        _ => return Ok(Value::Null),
    };
    let (_, data, symbol) = FIGHTERS[kind];
    let archive = Archive::parse(&disc.read_file(file, data)?)?;
    let bank = disc.read_file(file, ANIM_FILES[kind])?;
    let mut clips = Vec::new();
    for motion in load_motions(&archive, symbol)? {
        let Some(name) = &motion.anim_name else {
            continue;
        };
        let label = name
            .split("_ACTION_")
            .nth(1)
            .unwrap_or(name)
            .trim_end_matches("_figatree");
        // Basic shared locomotion/combat clips; dynamic fighter-part variants
        // require ftParts remapping before those clips can be attached safely.
        if ![
            "Wait1",
            "WalkSlow",
            "WalkMiddle",
            "WalkFast",
            "Dash",
            "Run",
            "JumpF",
            "JumpB",
            "Fall",
            "Attack11",
            "AttackAirN",
            "KneeBend",
            "Landing",
        ]
        .contains(&label)
        {
            continue;
        }
        let Some(bytes) = anim_slice(&bank, &motion) else {
            continue;
        };
        let a = Archive::parse(bytes)?;
        let root = a.public(name)?;
        let nodes = a.non_null_ptr(root + 12)?;
        let tracks = a.non_null_ptr(root + 16)?;
        let mut entries = Vec::new();
        let mut index = 0;
        let mut terminated = false;
        for node in 0..256 {
            let n = a.u8(nodes + node)?;
            if n == 255 {
                terminated = true;
                break;
            }
            ensure(n < 128, "invalid FigaTree track count")?;
            let mut ts = Vec::new();
            for _ in 0..n {
                let p = tracks + index * 12;
                index += 1;
                let size = u32::from(a.u16(p)?);
                let data = if size == 0 {
                    &[][..]
                } else {
                    a.bytes(a.non_null_ptr(p + 8)?, size)?
                };
                let offset = binary.len();
                binary.extend_from_slice(data);
                ts.push(json!({"offset":offset,"size":size,"start":a.u16(p+2)? as i16,"channel":a.u8(p+4)?,"value":a.u8(p+5)?,"slope":a.u8(p+6)?}));
            }
            entries.push(Value::Array(ts));
        }
        ensure(terminated, "unterminated FigaTree nodes")?;
        ensure(
            entries.len() == skeleton.len(),
            &format!(
                "{id}/{label}: {} nodes vs {} joints; requires part remapping",
                entries.len(),
                skeleton.len()
            ),
        )?;
        clips.push(json!({"name":label,"motion":motion.index,"symbol":name,"flags":a.u32(root+4)?,"type":a.u32(root)?,"end":a.f32(root+8)?,"nodes":entries,"source_sha256":format!("{:x}",Sha256::digest(bytes))}));
    }
    ensure(clips.iter().any(|c| c["name"] == "Wait1"), "missing Wait1")?;
    Ok(
        json!({"source":ANIM_FILES[kind],"source_sha256":format!("{:x}",Sha256::digest(&bank)),"clips":clips}),
    )
}
