//! Port of `struct Fighter` (`src/melee/ft/types.h`), grown field by field as
//! functions that use them are ported. Field names are the decomp's,
//! lowercased; each documents its original `fp+` offset.

use super::data::FighterAttributes;
use crate::math::{Vec2, Vec3};

/// `GroundOrAir` (`ft/forward.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroundOrAir {
    #[default]
    Ground = 0,
    Air = 1,
}

/// `struct Fighter_input` at `fp+620`. Each buffer holds this frame (`[0]`),
/// the previous frame (`[1]`), and the value saved while input was disabled
/// (`[2]`), which seeds `[1]` on the first frame afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FighterInput {
    /// `fp+620`: control stick, after the deadzone.
    pub lstick: [Vec2; 3],
    /// `fp+638`: C-stick, after the deadzone.
    pub cstick: [Vec2; 3],
    /// `fp+650`: the larger analog trigger, after the deadzone.
    pub triggers: [f32; 3],
    /// `fp+65C`: `HSD_PAD_*` buttons, with `HSD_PAD_LR` and the Z macro.
    pub held_buttons: [u32; 3],
    /// `fp+668`: buttons pressed this frame (accumulated during hitlag).
    pub pressed_buttons: u32,
    /// `fp+66C`: buttons released this frame (accumulated during hitlag).
    pub released_buttons: u32,
}

/// `FtInputTimers`: one counter per analog input (stick x, stick y,
/// trigger), saturating at 254.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FtInputTimers {
    /// `+0` (x), `+1` (y)
    pub lstick: [u8; 2],
    /// `+2`
    pub trigger: u8,
}

/// A sound the simulation asked to play (`ft_PlaySFX` arguments).
///
/// Sound is not presentation-only in Melee: `ft_PlaySFX` calls `HSD_Randi`
/// for some IDs (332..=370 after `ft_80087D0C`'s per-fighter remapping), so
/// sound dispatch advances the gameplay RNG. Until that remapping is ported,
/// requests are recorded here and do not touch the RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxRequest {
    pub id: i32,
    pub volume: u8,
    pub pan: u8,
}

/// `SurfaceData` (`lb/types.h`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SurfaceData {
    pub index: i32,
    pub flags: u32,
    pub normal: Vec3,
}

/// `CollData` at `fp+6F0` (`lb/types.h`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollData {
    /// `fp+83C`
    pub floor: SurfaceData,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fighter {
    /// `fp+C`: player slot (`Player_*` index).
    pub player_idx: u8,
    /// `fp+74`
    pub x74_self_accel: Vec3,
    /// `fp+80`
    pub self_vel: Vec3,
    /// `fp+8C`
    pub x8c_kb_vel: Vec3,
    /// `fp+B0`
    pub cur_pos: Vec3,
    /// `fp+E0`
    pub ground_or_air: GroundOrAir,
    /// `fp+E4`
    pub xe4_ground_accel_1: f32,
    /// `fp+EC`
    pub gr_vel: f32,
    /// `fp+F0`
    pub xf0_ground_kb_vel: f32,
    /// `fp+F4`
    pub xf4_ground_attacker_shield_kb_vel: f32,
    /// `fp+110`: this fighter's copy of its `ftCo_DatAttrs`.
    pub co_attrs: FighterAttributes,
    /// `fp+618`: controller port read by `Fighter_procInput`.
    pub pad_port: u8,
    /// `fp+620`
    pub input: FighterInput,
    /// `fp+670`: frames since each input crossed its threshold (the stick
    /// smash deadzones, `shield_press_threshold`). Consumers stamp 254 so an
    /// input is used only once.
    pub active_timer: FtInputTimers,
    /// `fp+673`: as `active_timer`, but no consumer resets it.
    pub active_sticky: FtInputTimers,
    /// `fp+676`: as `active_timer`, but keeps counting after the input ends.
    pub active_duration: FtInputTimers,
    /// `fp+679`: for the joystick-activity statistic (`ftCommon_8008031C`).
    pub activity_timer: FtInputTimers,
    /// `fp+67C`: frames since A was pressed (saturates at 255).
    pub x67c: u8,
    /// `fp+67D`: frames since B was pressed.
    pub x67d: u8,
    /// `fp+67E`: frames since X or Y was pressed.
    pub x67e: u8,
    /// `fp+67F`: frames since `HSD_PAD_LR` was pressed.
    pub x67f: u8,
    /// `fp+680`: frames since digital L or R was pressed.
    pub x680: u8,
    /// `fp+681`: frames since D-pad up was pressed.
    pub x681: u8,
    /// `fp+682`: frames since D-pad down was pressed.
    pub x682: u8,
    /// `fp+683`: `x67c` before the latest A press.
    pub x683: u8,
    /// `fp+684`: `x680` before the latest L/R press.
    pub x684: u8,
    /// `fp+685`: frames since a jump input (`ftCo_Jump_GetInput`).
    pub x685: u8,
    /// `fp+686`: frames since an up-special input (`ftCo_800D6928`).
    pub x686: u8,
    /// `fp+687`: frames since a down-special input (`ftCo_800D688C`).
    pub x687: u8,
    /// `fp+688`: frames since a side-special input (`ftCo_SpecialS_HasInput`).
    pub x688: u8,
    /// `fp+689`: frames since a neutral-special input (`ftCo_800D67C4`).
    pub x689: u8,
    /// `fp+68A`: `x685` before the latest jump input.
    pub x68a: u8,
    /// `fp+68B`: `x686` before the latest up-special input.
    pub x68b: u8,
    /// `fp+6F0`
    pub coll_data: CollData,
    /// `fp+1980` is non-null: an item GObj that `Fighter_procInput` lets
    /// the player mash on (`ftCommon_8007FFD8`). Items are not ported; only
    /// whether the pointer is set is modelled.
    pub x1980: bool,
    /// `fp+2219:5` (mask `0x04`): in hitlag (set after `pre_hitlag_cb` in
    /// `Fighter_UnkRecursiveFunc_8006D044`). Presses accumulate.
    pub x2219_b5: bool,
    /// `fp+221A:4` (mask `0x08`; MWCC allocates bitfields from the high bit)
    pub fall_fast: bool,
    /// `fp+221D:3` (mask `0x10`)
    pub has_prev_input: bool,
    /// `fp+221D:4` (mask `0x08`)
    pub input_disabled: bool,
    /// `fp+221F:3` (mask `0x10`)
    pub is_sleeping: bool,
    /// `fp+221F:4` (mask `0x08`): the player's second fighter (Nana).
    pub is_sub_fighter: bool,
    /// `fp+2224:2` (mask `0x20`)
    pub stamina_dead: bool,
    /// `fp+2228:7` (mask `0x01`): the last horizontal smash input was right.
    pub x2228_b7: bool,
    /// `fp+2229:0` (mask `0x80`): the last vertical smash input was down.
    pub x2229_b0: bool,
    /// Sounds requested this frame; see [`SfxRequest`].
    pub sfx: Vec<SfxRequest>,
}

impl Fighter {
    /// `ft_PlaySFX` (0x80088148), minus the pending remap and RNG call.
    pub fn play_sfx(&mut self, id: i32, volume: u8, pan: u8) {
        self.sfx.push(SfxRequest { id, volume, pan });
    }
}
