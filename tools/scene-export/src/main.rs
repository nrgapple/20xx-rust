//! Developer-only asset preparation. Reads a verified local disc; writes a new
//! versioned directory of converted meshes/textures, never changes the source.
mod animation;
mod scene;
mod texture;
use gc_disc::Disc;
use hsd_dat::Archive;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn ensure(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure(
        args.len() == 4,
        "usage: scene-export DISC ARCHIVE.dat OUTPUT_DIR ASSET_ID",
    )?;
    let mut file = fs::File::open(&args[0])?;
    let disc = Disc::read(&mut file)?;
    ensure(
        disc.game_id == "GALE01" && disc.revision == 2,
        "requires NTSC-U 1.02",
    )?;
    let bytes = disc.read_file(&mut file, &args[1])?;
    let a = Archive::parse(&bytes)?;
    let roots = if let Ok(head) = a.public("map_head") {
        let table = a.non_null_ptr(head + 8)?;
        let count = a.u32(head + 12)?;
        ensure(count < 128, "stage model count")?;
        (0..count)
            .map(|i| Ok(a.non_null_ptr(table + i * 0x34)?))
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![
            a.publics()
                .iter()
                .find(|s| s.name.ends_with("_joint") && !s.name.ends_with("matanim_joint"))
                .ok_or("no model joint symbol")?
                .offset,
        ]
    };
    let scene = scene::load(&a, &roots)?;
    let mut binary = Vec::new();
    let mut meshes = Vec::new();
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for mesh in scene.meshes {
        let offset = binary.len();
        for vertex in &mesh.vertices {
            for (i, &value) in vertex.display[..3].iter().enumerate() {
                bounds[0][i] = bounds[0][i].min(value);
                bounds[1][i] = bounds[1][i].max(value);
            }
            for v in &vertex.display {
                ensure(v.is_finite(), "nonfinite output")?;
                binary.extend(v.to_le_bytes());
            }
        }
        let skin_offset = binary.len();
        for v in &mesh.vertices {
            for x in v.animated {
                binary.extend(x.to_le_bytes());
            }
        }
        meshes.push(json!({"offset":offset,"skin_offset":skin_offset,"count":mesh.vertices.len(),"color":mesh.color,"texture":mesh.texture,"joint":mesh.joint,"root":mesh.root}));
    }
    let mut textures = Vec::new();
    for t in scene.textures {
        let offset = binary.len();
        binary.extend(&t.rgba);
        textures.push(json!({"offset":offset,"width":t.width,"height":t.height}));
    }
    ensure(!meshes.is_empty(), "no visible meshes")?;
    let animation = animation::load(&disc, &mut file, &args[3], &scene.skeleton, &mut binary)?;
    let metadata = json!({"version":2,"id":args[3],"source":args[1],"source_sha256":format!("{:x}",Sha256::digest(&bytes)),"binary_sha256":format!("{:x}",Sha256::digest(&binary)),"joints":scene.joints,"skeleton":scene.skeleton,"animation":animation,"roots":roots,"bounds":bounds,"meshes":meshes,"textures":textures,"warnings":scene.warnings,"pose":"Rust-evaluated original animation tracks", "rendering":"first UV texture and diffuse material; TEV, animation and effects pending"});
    let dir = Path::new(&args[2]);
    fs::create_dir_all(dir)?;
    save(&dir.join(format!("{}.bin", args[3])), &binary)?;
    save(
        &dir.join(format!("{}.json", args[3])),
        &serde_json::to_vec(&metadata)?,
    )?;
    println!(
        "{}: {} joints, {} meshes, {} textures, {} bytes; bounds {:?}",
        args[3],
        scene.joints,
        meshes.len(),
        textures.len(),
        binary.len(),
        bounds
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("scene-export: {e}");
        std::process::exit(1)
    }
}
