//! Versioned runtime-content adapter. No disc/archive parsing or emulator here.
//! Animation clocks, tracks and joint transforms execute in the Rust core.
use melee::{
    hsd::{
        aobj::Animation,
        fobj::Track,
        joint::{Joint, Skeleton},
    },
    math::Mtx,
};
use serde_json::Value;
use std::cell::RefCell;
struct Scene {
    base: Vec<Joint>,
    clips: Vec<Value>,
    bytes: Vec<u8>,
    pose: Skeleton,
}
thread_local! { static SCENES: RefCell<Vec<Option<Scene>>> = RefCell::new((0..5).map(|_|None).collect()); }
fn num(v: &Value) -> Result<f32, String> {
    let n = v.as_f64().ok_or("missing number")? as f32;
    if n.is_finite() {
        Ok(n)
    } else {
        Err("nonfinite number".into())
    }
}
fn integer(v: &Value) -> Result<usize, String> {
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("invalid integer".into())
}
fn vector(v: &Value) -> Result<[f32; 3], String> {
    Ok([num(&v[0])?, num(&v[1])?, num(&v[2])?])
}
fn matrix(v: &Value) -> Result<Mtx, String> {
    let mut m = [[0.; 4]; 3];
    for i in 0..3 {
        for j in 0..4 {
            m[i][j] = num(&v[i][j])?;
        }
    }
    Ok(m)
}
fn parse(meta: &[u8], bytes: &[u8]) -> Result<Scene, String> {
    if meta.len() > 4 * 1024 * 1024 || bytes.len() > 25 * 1024 * 1024 {
        return Err("scene too large".into());
    }
    let value: Value = serde_json::from_slice(meta).map_err(|e| e.to_string())?;
    if value["version"] != 2 {
        return Err("unsupported scene version".into());
    }
    let joints = value["skeleton"]
        .as_array()
        .ok_or("missing skeleton")?
        .iter()
        .map(|v| {
            Ok(Joint {
                parent: if v["parent"].is_null() {
                    None
                } else {
                    Some(integer(&v["parent"])?)
                },
                flags: u32::try_from(integer(&v["flags"])?).map_err(|_| "flags overflow")?,
                rotation: vector(&v["rotation"])?,
                translation: vector(&v["translation"])?,
                scale: vector(&v["scale"])?,
                inverse: matrix(&v["inverse"])?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let clips = value["animation"]["clips"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let pose = Skeleton::new(joints.clone())?;
    Ok(Scene {
        base: joints,
        clips,
        bytes: bytes.to_vec(),
        pose,
    })
}
impl Scene {
    fn play(&mut self, index: usize) -> Result<(), String> {
        let clip = self.clips.get(index).ok_or("missing clip")?;
        let nodes = clip["nodes"].as_array().ok_or("missing nodes")?;
        if nodes.len() != self.base.len() {
            return Err("joint mapping mismatch".into());
        }
        let mut pose = Skeleton::new(self.base.clone())?;
        for (i, node) in nodes.iter().enumerate() {
            let mut tracks = Vec::new();
            for desc in node.as_array().ok_or("invalid tracks")? {
                let offset = integer(&desc["offset"])?;
                let len = integer(&desc["size"])?;
                let end = offset.checked_add(len).ok_or("track range overflow")?;
                let bytes = self.bytes.get(offset..end).ok_or("track out of bounds")?;
                let byte = |k: &str| -> Result<u8, String> {
                    u8::try_from(integer(&desc[k])?).map_err(|_| "track byte overflow".into())
                };
                let start = desc["start"]
                    .as_i64()
                    .and_then(|n| i16::try_from(n).ok())
                    .ok_or("invalid start")?;
                let channel = byte("channel")?;
                if !matches!(channel,1..=3|5..=12) {
                    return Err(format!("unsupported joint channel {channel}"));
                }
                tracks.push(
                    Track::new(bytes, channel, byte("value")?, byte("slope")?, start)
                        .map_err(|e| e.to_string())?,
                );
            }
            if !tracks.is_empty() {
                if integer(&clip["type"])? & 1 != 0 {
                    pose.joints[i].flags |= 8;
                } else {
                    pose.joints[i].flags &= !8;
                }
                let mut animation =
                    Animation::new(tracks, num(&clip["end"])?, integer(&clip["flags"])? as u32)
                        .map_err(|e| e.to_string())?;
                animation.request(0.).map_err(|e| e.to_string())?;
                pose.animations[i] = Some(animation);
            }
        }
        pose.advance()?;
        self.pose = pose;
        Ok(())
    }
}
fn with_scene(slot: usize, f: impl FnOnce(&mut Scene) -> Result<String, String>) -> i32 {
    super::respond(|_| {
        SCENES.with(|s| {
            let mut s = s.borrow_mut();
            f(s.get_mut(slot)
                .and_then(Option::as_mut)
                .ok_or("scene not loaded")?)
        })
    })
}
/// # Safety
/// Both byte ranges must be initialized allocations from the exported alloc.
#[no_mangle]
pub unsafe extern "C" fn scene_load(
    slot: usize,
    meta: *const u8,
    meta_len: usize,
    data: *const u8,
    data_len: usize,
) -> i32 {
    super::respond(|_| {
        if slot >= 5 {
            return Err("invalid scene slot".into());
        }
        let scene = parse(super::bytes(meta, meta_len), super::bytes(data, data_len))?;
        let out=serde_json::json!({"clips":scene.clips.iter().map(|c|&c["name"]).collect::<Vec<_>>(),"joints":scene.base.len()}).to_string();
        SCENES.with(|s| s.borrow_mut()[slot] = Some(scene));
        Ok(out)
    })
}
#[no_mangle]
pub extern "C" fn scene_play(slot: usize, clip: usize) -> i32 {
    with_scene(slot, |s| {
        s.play(clip)?;
        Ok("{}".into())
    })
}
#[no_mangle]
pub extern "C" fn scene_advance(slot: usize) -> i32 {
    with_scene(slot, |s| {
        s.pose.advance()?;
        Ok("{}".into())
    })
}
#[no_mangle]
pub extern "C" fn scene_palette_ptr(slot: usize) -> *const f32 {
    SCENES.with(|s| {
        s.borrow()
            .get(slot)
            .and_then(Option::as_ref)
            .map_or(std::ptr::null(), |s| s.pose.palette.as_ptr())
    })
}
#[no_mangle]
pub extern "C" fn scene_joint_hidden(slot: usize, joint: usize) -> i32 {
    SCENES.with(|s| {
        s.borrow()
            .get(slot)
            .and_then(Option::as_ref)
            .and_then(|s| s.pose.joints.get(joint))
            .map_or(1, |j| i32::from(j.flags & 16 != 0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn original_clips_restore_and_match_native_goldens() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = root.join("generated/content-v2");
        if !dir.join("fox.json").exists() {
            eprintln!("skipping developer content test: no converted pack");
            return;
        }
        let mut golden = serde_json::Map::new();
        for id in ["fox", "mario", "marth", "falcon"] {
            let mut scene = parse(
                &std::fs::read(dir.join(format!("{id}.json"))).unwrap(),
                &std::fs::read(dir.join(format!("{id}.bin"))).unwrap(),
            )
            .unwrap();
            let mut samples = Vec::new();
            for clip in 0..scene.clips.len() {
                scene.play(clip).unwrap();
                let end = num(&scene.clips[clip]["end"]).unwrap() as usize;
                for frame in 0..=end {
                    if frame != 0 {
                        scene.pose.advance().unwrap();
                    }
                    assert!(
                        scene.pose.palette.iter().all(|x| x.is_finite()),
                        "{id} {clip} {frame}"
                    );
                    let mut restored = scene.pose.clone();
                    let mut advanced = scene.pose.clone();
                    restored.advance().unwrap();
                    advanced.advance().unwrap();
                    assert_eq!(
                        restored
                            .palette
                            .iter()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>(),
                        advanced
                            .palette
                            .iter()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>()
                    );
                    if frame == 0 || frame == 1 || frame == end || frame == 30 {
                        let bytes: Vec<u8> = scene
                            .pose
                            .palette
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect();
                        samples.push(serde_json::json!({"clip":clip,"frame":frame,"sha256":format!("{:x}",Sha256::digest(bytes))}));
                    }
                }
            }
            golden.insert(id.into(), Value::Array(samples));
        }
        let actual = Value::Object(golden);
        let file = root.join("research/animation-goldens.json");
        if std::env::var_os("UPDATE_ANIMATION_GOLDENS").is_some() {
            std::fs::write(&file, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
        }
        let expected: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(actual, expected);
    }
    #[test]
    fn rejects_missing_or_unsupported_content() {
        assert!(parse(b"{}", &[]).is_err());
        assert!(parse(br#"{"version":2,"skeleton":[]}"#, &[]).is_err());
    }
}
