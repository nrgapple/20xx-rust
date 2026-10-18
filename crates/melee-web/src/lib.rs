//! WebAssembly entry points for the browser inspector (`web/`).
//!
//! The page reads byte ranges of the player's disc image with `File.slice`
//! (nothing is uploaded) and passes them here. Every call returns a status
//! (0 = ok) and leaves JSON, or an error message, in an output buffer that
//! JavaScript reads with `output_ptr`/`output_len`.

use gc_disc::{parse_fst, Header};
use hsd_dat::Archive;
use melee::dat::DatJson;
use melee::ft::data::CommonData;
use melee::ft::fighter::{Fighter, GroundOrAir};
use melee::ft::{ft_081b, kinds, load_attributes, load_common_data};
use std::cell::RefCell;

#[derive(Default)]
struct State {
    output: String,
    common: Option<CommonData>,
    fighter: Option<Fighter>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Run `f`, store its JSON (or error text) as output, and return a status.
fn respond(f: impl FnOnce(&mut State) -> Result<String, String>) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let (status, output) = match f(&mut state) {
            Ok(json) => (0, json),
            Err(error) => (1, error),
        };
        state.output = output;
        status
    })
}

/// # Safety
/// `ptr..ptr + len` must be memory returned by [`alloc`] and initialized by the caller.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    std::slice::from_raw_parts(ptr, len)
}

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from one [`alloc`] call, freed once.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}

#[no_mangle]
pub extern "C" fn output_ptr() -> *const u8 {
    STATE.with(|state| state.borrow().output.as_ptr())
}

#[no_mangle]
pub extern "C" fn output_len() -> usize {
    STATE.with(|state| state.borrow().output.len())
}

/// Parse the 0x440-byte disc header. `image_size` is the file's size in bytes.
///
/// # Safety
/// See [`bytes`].
#[no_mangle]
pub unsafe extern "C" fn disc_header(ptr: *const u8, len: usize, image_size: f64) -> i32 {
    let header = bytes(ptr, len);
    respond(|_| {
        let h = Header::parse(header, image_size as u64).map_err(|e| e.to_string())?;
        Ok(format!(
            "{{\"game_id\":{},\"revision\":{},\"title\":{},\"fst_offset\":{},\"fst_size\":{}}}",
            json_string(&h.game_id),
            h.revision,
            json_string(&h.title),
            h.fst_offset,
            h.fst_size
        ))
    })
}

/// List the files in a raw FST as `[[path, offset, size], ...]`.
///
/// # Safety
/// See [`bytes`].
#[no_mangle]
pub unsafe extern "C" fn disc_files(ptr: *const u8, len: usize, image_size: f64) -> i32 {
    let fst = bytes(ptr, len);
    respond(|_| {
        let files = parse_fst(fst, image_size as u64).map_err(|e| e.to_string())?;
        let rows: Vec<String> = files
            .iter()
            .map(|f| format!("[{},{},{}]", json_string(&f.path), f.offset, f.size))
            .collect();
        Ok(format!("[{}]", rows.join(",")))
    })
}

/// The fighter table as `[[name, archive, symbol], ...]`.
#[no_mangle]
pub extern "C" fn fighters() -> i32 {
    respond(|_| {
        let rows: Vec<String> = kinds::FIGHTERS
            .iter()
            .map(|(name, file, symbol)| {
                format!(
                    "[{},{},{}]",
                    json_string(name),
                    json_string(file),
                    json_string(symbol)
                )
            })
            .collect();
        Ok(format!("[{}]", rows.join(",")))
    })
}

/// Load `ftCommonData` from the bytes of `PlCo.dat`.
///
/// # Safety
/// See [`bytes`].
#[no_mangle]
pub unsafe extern "C" fn load_common(ptr: *const u8, len: usize) -> i32 {
    let file = bytes(ptr, len);
    respond(|state| {
        let archive = Archive::parse(file).map_err(|e| e.to_string())?;
        let common = load_common_data(&archive).map_err(|e| e.to_string())?;
        let json = common.to_json();
        state.common = Some(common);
        Ok(json)
    })
}

/// Load a fighter's `ftCo_DatAttrs` from its archive bytes, given the index
/// into [`kinds::FIGHTERS`], and return the attributes as JSON.
///
/// # Safety
/// See [`bytes`].
#[no_mangle]
pub unsafe extern "C" fn load_fighter(ptr: *const u8, len: usize, kind: usize) -> i32 {
    let file = bytes(ptr, len);
    respond(|state| {
        let (_, _, symbol) = kinds::FIGHTERS.get(kind).ok_or("no such fighter")?;
        let archive = Archive::parse(file).map_err(|e| e.to_string())?;
        let attrs = load_attributes(&archive, symbol).map_err(|e| e.to_string())?;
        state.fighter = Some(Fighter {
            co_attrs: attrs,
            ..Fighter::default()
        });
        Ok(attrs.to_json())
    })
}

/// Simulate `frames` frames of the Fall action state from rest, with the
/// control stick held at `stick_x` and flicked fully down on `flick_frame`
/// (negative: never). Returns `{"x":[..],"y":[..],"vx":[..],"vy":[..],"fast":[..]}`.
///
/// Per frame: the Fall physics callback (`ft_80084DB0`, ported), then the
/// two `Fighter_procUpdate` steps that apply with no knockback, nudge, or
/// wind: `self_vel += self_accel` and `cur_pos += self_vel`. Input timers
/// are simulated here because `Fighter_procInput` is not yet ported.
#[no_mangle]
pub extern "C" fn simulate_fall(stick_x: f32, flick_frame: i32, frames: i32) -> i32 {
    respond(|state| {
        let common = state.common.ok_or("load PlCo.dat first")?;
        let mut fp = state.fighter.clone().ok_or("load a fighter first")?;
        fp.ground_or_air = GroundOrAir::Air;
        fp.active_timer.lstick = [0xfe, 0xfe];
        let mut series: [Vec<String>; 5] = Default::default();
        for frame in 0..frames.clamp(0, 600) {
            let down = flick_frame >= 0 && frame >= flick_frame;
            fp.input.lstick[0].x = stick_x;
            fp.input.lstick[0].y = if down { -1.0 } else { 0.0 };
            fp.active_timer.lstick[1] = if frame == flick_frame {
                0
            } else {
                fp.active_timer.lstick[1].saturating_add(1).min(0xfe)
            };
            ft_081b::ft_80084db0(&mut fp, &common);
            fp.self_vel.x += fp.x74_self_accel.x;
            fp.self_vel.y += fp.x74_self_accel.y;
            fp.self_vel.z += fp.x74_self_accel.z;
            fp.x74_self_accel = Default::default();
            fp.cur_pos.x += fp.self_vel.x;
            fp.cur_pos.y += fp.self_vel.y;
            fp.cur_pos.z += fp.self_vel.z;
            for (column, value) in series.iter_mut().zip([
                fp.cur_pos.x,
                fp.cur_pos.y,
                fp.self_vel.x,
                fp.self_vel.y,
                f32::from(u8::from(fp.fall_fast)),
            ]) {
                column.push(value.to_json());
            }
        }
        let names = ["x", "y", "vx", "vy", "fast"];
        let fields: Vec<String> = names
            .iter()
            .zip(&series)
            .map(|(name, values)| format!("\"{name}\":[{}]", values.join(",")))
            .collect();
        Ok(format!("{{{}}}", fields.join(",")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> String {
        STATE.with(|state| state.borrow().output.clone())
    }

    #[test]
    fn simulate_requires_loaded_data() {
        assert_eq!(simulate_fall(0.0, -1, 10), 1);
        assert!(output().contains("PlCo.dat"));
    }

    #[test]
    fn fighter_table_is_json() {
        assert_eq!(fighters(), 0);
        assert!(output().starts_with("[[\"Mario\",\"PlMr.dat\",\"ftDataMario\"]"));
    }

    #[test]
    fn fall_reaches_terminal_and_fast_falls() {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let mut fp = Fighter::default();
            fp.co_attrs.gravity = 0.23;
            fp.co_attrs.terminal_velocity = 2.8;
            fp.co_attrs.fast_fall_velocity = 3.4;
            state.fighter = Some(fp);
            state.common = Some(CommonData {
                x88: 0.6625,
                x8c: 4,
                ..CommonData::default()
            });
        });
        assert_eq!(simulate_fall(0.0, 20, 25), 0);
        let out = output();
        let vy = out
            .split("\"vy\":[")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap();
        let vy: Vec<f32> = vy.split(',').map(|v| v.parse().unwrap()).collect();
        assert_eq!(vy[0], -0.23);
        assert_eq!(vy[19], -2.8);
        assert_eq!(vy[20], -3.4);
    }
}
