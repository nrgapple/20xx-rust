//! Subaction command scripts (`Fighter_WaitAnimData::xC`): decoding only.
//!
//! A script is a stream of 32-bit words. Each command starts with a word
//! whose top 6 bits are its opcode (`ftAction_80073240`:
//! `lbz r28, 0(r4); rlwinm r28, r28, 30, 26, 31`). Opcodes 0-9 are the
//! generic commands every script shares (`lbCommand_803B9840`, run by
//! `Command_Execute`); opcodes 10-58 are fighter commands
//! (`ftAction_803C06E8`), whose lengths in words are [`FIGHTER_COMMAND_WORDS`]
//! (`ftAction_803C0870`). The fighter tables have 49 entries and the
//! interpreter does not bounds-check, so opcodes 59-63 are invalid.
//!
//! Field layouts mirror the decomp's bit-field structs (`lb/types.h`): fields
//! are allocated from the most significant bit in declaration order (MWCC is
//! big-endian), unsigned C types zero-extend and signed ones sign-extend.
//! Every struct here packs whole 32-bit words (or two exactly 16-bit
//! halves), so no field straddles an allocation unit. The 6-bit opcode field
//! is named `opcode` throughout (the decomp also calls it `code`, `id`, or
//! `unk0`). Each command cites its handler. The extractions of several
//! handlers were checked against the 1.02 machine code, including every
//! signed field (listed in `research/notes/actions.md`); the rest follow
//! the matching decomp's structs.
//!
//! This module only decodes. Executing commands is the interpreter's job.

use hsd_dat::Archive;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

/// A field type extracted from a bit-field word.
trait BitField: Sized {
    /// The field `width` bits wide starting `pos` bits below the MSB.
    fn extract(word: u32, pos: u32, width: u32) -> Self;
}

impl BitField for u32 {
    fn extract(word: u32, pos: u32, width: u32) -> Self {
        (word << pos) >> (32 - width)
    }
}

impl BitField for i32 {
    fn extract(word: u32, pos: u32, width: u32) -> Self {
        ((word << pos) as i32) >> (32 - width)
    }
}

/// Declare one command word laid out like a decomp bit-field struct.
macro_rules! bitfields {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $($field:ident : $ty:ident = $width:literal,)*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl $name {
            /// Bits the fields occupy, from the MSB.
            pub const BITS: u32 = 0 $(+ $width)*;

            pub fn decode(word: u32) -> Self {
                let mut _pos = 0;
                $(
                    let $field = <$ty as BitField>::extract(word, _pos, $width);
                    _pos += $width;
                )*
                Self { $($field,)* }
            }

            /// `(name, value)` of every field, for display.
            pub fn fields(&self) -> Vec<(&'static str, i64)> {
                vec![$((stringify!($field), i64::from(self.$field)),)*]
            }
        }

        const _: () = assert!($name::BITS <= 32, "fields exceed one word");
    };
}

bitfields! {
    /// `struct Command_00` (also read by `Command_01`).
    pub struct Command00 { opcode: u32 = 6, value: u32 = 26, }
}
bitfields! {
    /// `struct Command_02`.
    pub struct Command02 { opcode: u32 = 6, value: u32 = 26, }
}
bitfields! {
    /// `struct Command_03`.
    pub struct Command03 { opcode: u32 = 6, value: u32 = 26, }
}
bitfields! {
    /// `struct Command_09`.
    pub struct Command09 { opcode: u32 = 6, param_1: u32 = 8, param_2: u32 = 18, }
}
bitfields! {
    /// `struct spawn_gfx_0`.
    pub struct SpawnGfx0 {
        opcode: u32 = 6, bone_id: u32 = 8, use_common_bone_ids: u32 = 1,
        destroy_on_state_change: u32 = 1, use_unk_bone: u32 = 1, unk1: u32 = 15,
    }
}
bitfields! {
    /// `struct spawn_gfx_1`.
    pub struct SpawnGfx1 { gfx_id: u32 = 16, unk_float: u32 = 16, }
}
bitfields! {
    /// `struct spawn_gfx_2` (`s16` fields).
    pub struct SpawnGfx2 { offset_z: i32 = 16, offset_y: i32 = 16, }
}
bitfields! {
    /// `struct spawn_gfx_3` (`s16`, `u16`).
    pub struct SpawnGfx3 { offset_x: i32 = 16, range_z: u32 = 16, }
}
bitfields! {
    /// `struct spawn_gfx_4` (`u16` fields).
    pub struct SpawnGfx4 { range_y: u32 = 16, range_x: u32 = 16, }
}
bitfields! {
    /// `struct spawn_hitbox_0`.
    pub struct SpawnHitbox0 {
        opcode: u32 = 6, id: u32 = 3, hit_group: u32 = 3, only_hit_grabbed: u32 = 1,
        bone: u32 = 8, use_common_bone_ids: u32 = 1, damage: u32 = 10,
    }
}
bitfields! {
    /// `struct spawn_hitbox_1`.
    pub struct SpawnHitbox1 { size: u32 = 16, z_offset: i32 = 16, }
}
bitfields! {
    /// `struct spawn_hitbox_2`.
    pub struct SpawnHitbox2 { y_offset: i32 = 16, x_offset: i32 = 16, }
}
bitfields! {
    /// `struct spawn_hitbox_3`.
    pub struct SpawnHitbox3 {
        angle: u32 = 9, knockback_growth: u32 = 9, weight_set_knockback: u32 = 9,
        item_hit_interaction: u32 = 1, ignore_thrown_fighters: u32 = 1,
        ignore_fighter_scale: u32 = 1, clank: u32 = 1, rebound: u32 = 1,
    }
}
bitfields! {
    /// `struct spawn_hitbox_4`.
    pub struct SpawnHitbox4 {
        base_knockback: u32 = 9, element: u32 = 5, shield_damage: i32 = 8,
        hit_sfx_severity: u32 = 3, hit_sfx_kind: u32 = 5, hit_grounded: u32 = 1,
        hit_aerial: u32 = 1,
    }
}
bitfields! {
    /// `struct spawn_hitbox_5`. `ftAction_8007121C` reads `x1_b4` from the
    /// word *after* the hitbox command (see [`Command::Hitbox`]).
    pub struct SpawnHitbox5 {
        x0: u32 = 8, x1_b0: u32 = 1, x1_b1: u32 = 1, x1_b2: u32 = 1, x1_b3: u32 = 1,
        x1_b4: u32 = 1, x1_b5: u32 = 1, x1_b6: u32 = 1, x1_b7: u32 = 1,
    }
}
bitfields! {
    /// `struct set_hitbox_damage`.
    pub struct SetHitboxDamage { opcode: u32 = 6, idx: u32 = 3, value: u32 = 23, }
}
bitfields! {
    /// `struct set_hitbox_scale`.
    pub struct SetHitboxScale { opcode: u32 = 6, idx: u32 = 3, value: u32 = 23, }
}
bitfields! {
    /// `struct set_hitbox_x42_b57`.
    pub struct SetHitboxX42B57 { opcode: u32 = 6, idx: u32 = 24, type_: u32 = 1, value: u32 = 1, }
}
bitfields! {
    /// `struct set_throw_flags`.
    pub struct SetThrowFlags { opcode: u32 = 6, hit_idx: u32 = 26, }
}
bitfields! {
    /// `struct sound_effect_0`.
    pub struct SoundEffect0 { opcode: u32 = 6, behavior: u32 = 8, unknown: u32 = 18, }
}
bitfields! {
    /// `struct sound_effect_1`.
    pub struct SoundEffect1 { sfx_id: u32 = 32, }
}
bitfields! {
    /// `struct sound_effect_2`.
    pub struct SoundEffect2 { padding: u32 = 16, volume: u32 = 8, panning: u32 = 8, }
}
bitfields! {
    /// `struct set_cmd_var`.
    pub struct SetCmdVar { opcode: u32 = 6, idx: u32 = 2, value: u32 = 24, }
}
bitfields! {
    /// `struct set_airborne_state` (also read by `ftAction_80071A14` and
    /// `ftAction_80071A58`).
    pub struct SetAirborneState { opcode: u32 = 6, state: u32 = 26, }
}
bitfields! {
    /// `struct set_hurt_state`.
    pub struct SetHurtState { opcode: u32 = 6, bone_idx: u32 = 8, state: u32 = 18, }
}
bitfields! {
    /// `struct set_jab_combo`.
    pub struct SetJabCombo { opcode: u32 = 6, disabled: u32 = 26, }
}
bitfields! {
    /// `struct set_jab_rapid`.
    pub struct SetJabRapid { opcode: u32 = 6, state: u32 = 26, }
}
bitfields! {
    /// `struct set_dobj_flags`.
    pub struct SetDobjFlags { opcode: u32 = 6, idx: i32 = 7, value: i32 = 19, }
}
bitfields! {
    /// `struct set_throw_hitbox_0`.
    pub struct SetThrowHitbox0 { opcode: u32 = 6, idx: u32 = 3, damage: u32 = 23, }
}
bitfields! {
    /// `struct set_throw_hitbox_1`.
    pub struct SetThrowHitbox1 { unk0: u32 = 9, hit_x24: u32 = 9, hit_x28: u32 = 9, }
}
bitfields! {
    /// `struct set_throw_hitbox_2`.
    pub struct SetThrowHitbox2 {
        hit_x2c: u32 = 9, element: u32 = 4, sfx_severity: u32 = 3, sfx_kind: u32 = 4,
    }
}
bitfields! {
    /// `struct unk27`.
    pub struct Unk27 { opcode: u32 = 6, value: u32 = 26, }
}
bitfields! {
    /// `struct set_article_vis`.
    pub struct SetArticleVis { opcode: u32 = 6, value: u32 = 26, }
}
bitfields! {
    /// `struct set_fighter_vis`.
    pub struct SetFighterVis { opcode: u32 = 6, value: u32 = 26, }
}
bitfields! {
    /// `struct pseudo_random_sfx_0`.
    pub struct PseudoRandomSfx0 {
        opcode: u32 = 6, volume: u32 = 8, panning: u32 = 8, behavior: u32 = 4,
        random_range: u32 = 6,
    }
}
bitfields! {
    /// `struct stage_sfx_0`.
    pub struct StageSfx0 { opcode: u32 = 6, sfx_base: u32 = 10, x2_b0_7: u32 = 8, pitch_select: u32 = 8, }
}
bitfields! {
    /// `struct stage_sfx_1`.
    pub struct StageSfx1 { sfx_id: u32 = 32, }
}
bitfields! {
    /// `struct stage_sfx_2`.
    pub struct StageSfx2 { x0_b0_15: u32 = 16, x2_b0_15: u32 = 16, }
}
bitfields! {
    /// `struct stage_sfx_3`.
    pub struct StageSfx3 { x0_b0_15: u32 = 16, x2_b0_7: u32 = 8, x3_b0_7: u32 = 8, }
}
bitfields! {
    /// `struct set_tex_anim`.
    pub struct SetTexAnim { opcode: u32 = 6, b: u32 = 1, idx: i32 = 7, idx2: i32 = 7, frame: i32 = 11, }
}
bitfields! {
    /// `struct part_anim`.
    pub struct PartAnim { opcode: i32 = 6, unk1: i32 = 7, unk2: i32 = 7, unk3: u32 = 12, }
}
bitfields! {
    /// `struct unk9`.
    pub struct Unk9 { opcode: i32 = 6, unk1: u32 = 13, unk2: u32 = 13, }
}
bitfields! {
    /// `struct unk10`.
    pub struct Unk10 { opcode: i32 = 6, unk1: u32 = 1, unk2: u32 = 12, unk3: u32 = 13, }
}
bitfields! {
    /// `struct unk11`.
    pub struct Unk11 { opcode: i32 = 6, unk1: u32 = 26, }
}
bitfields! {
    /// `struct unk12`.
    pub struct Unk12 { opcode: u32 = 6, unk1: u32 = 2, unk2: u32 = 10, unk3: u32 = 14, }
}
bitfields! {
    /// `struct unk13`.
    pub struct Unk13 { opcode: u32 = 6, unk1: u32 = 8, unk2: u32 = 18, }
}
bitfields! {
    /// `struct unk14`.
    pub struct Unk14 { opcode: u32 = 6, unk1: u32 = 8, }
}
bitfields! {
    /// `struct unk15`.
    pub struct Unk15 { opcode: u32 = 6, unk1: u32 = 26, }
}
bitfields! {
    /// `struct unk16`.
    pub struct Unk16 { opcode: u32 = 6, unk3: i32 = 1, unk4: i32 = 25, }
}
bitfields! {
    /// `struct unk17`.
    pub struct Unk17 { opcode: u32 = 6, unk1: i32 = 26, }
}
bitfields! {
    /// `struct unk18`.
    pub struct Unk18 { opcode: u32 = 6, damage_amount: i32 = 26, }
}
bitfields! {
    /// `struct unk19`.
    pub struct Unk19 { opcode: u32 = 6, unk1: u32 = 26, }
}
bitfields! {
    /// `struct unk20`.
    pub struct Unk20 { opcode: u32 = 6, unk1: u32 = 26, }
}
bitfields! {
    /// `struct unk21`.
    pub struct Unk21 { opcode: u32 = 6, unk1: u32 = 1, unk2: u32 = 8, }
}
bitfields! {
    /// `struct footstep_fx_0`.
    pub struct FootstepFx0 {
        opcode: u32 = 6, bone_id: u32 = 8, use_alt_bone: u32 = 1, x1_b7: u32 = 1,
        x2_b0_7: u32 = 8, x3_b0_7: u32 = 8,
    }
}
bitfields! {
    /// `struct unk_fx_0`.
    pub struct UnkFx0 { opcode: u32 = 6, x0_b6_7: u32 = 2, x1_b0_7: u32 = 8, gfx_id: u32 = 16, }
}
bitfields! {
    /// `struct smash_charge_0`.
    pub struct SmashCharge0 { opcode: u32 = 6, charge_frames: u32 = 10, charge_rate: u32 = 16, }
}
bitfields! {
    /// `struct smash_charge_1`.
    pub struct SmashCharge1 { color_anim: u32 = 8, x1_b0_23: u32 = 24, }
}
bitfields! {
    /// `struct wind_fx_0`.
    pub struct WindFx0 { opcode: u32 = 6, x0_b6_17: u32 = 18, bone: u32 = 8, }
}
bitfields! {
    /// `struct wind_fx_1` (`s16` fields).
    pub struct WindFx1 { timer: i32 = 16, x: i32 = 16, }
}
bitfields! {
    /// `struct wind_fx_2` (`s16` fields).
    pub struct WindFx2 { y: i32 = 16, mag: i32 = 16, }
}
bitfields! {
    /// `struct wind_fx_3` (`s16` fields).
    pub struct WindFx3 { angle: i32 = 16, decay: i32 = 16, }
}

/// The opcode of a command's first word.
pub fn opcode(word: u32) -> u32 {
    word >> 26
}

/// First fighter opcode; lower opcodes are generic (`Command_Execute`).
pub const FIRST_FIGHTER_OPCODE: u32 = 10;

/// `ftAction_803C0870` (0x803C0870, 0x31 bytes): words per fighter command,
/// indexed by `opcode - 10`. Used by `ftAction_8007349C` to skip commands;
/// the handlers in `ftAction_803C06E8` and the skip handlers in
/// `ftAction_803C07AC` advance by the same amounts (exceptions in
/// [`Command::handler_words`]).
pub const FIGHTER_COMMAND_WORDS: [u8; 49] = [
    5, 5, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 7, 4, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 2, 1, 4,
];
/// DOL address of `ftAction_803C0870`.
pub const FIGHTER_COMMAND_WORDS_ADDRESS: u32 = 0x803C_0870;

/// Words per generic command (opcodes 0-9), from how far each
/// `lbcommand.c` handler moves the script pointer. `Command_05` and
/// `Command_07` read a target pointer from their second word.
pub const GENERIC_COMMAND_WORDS: [u8; 10] = [1, 1, 1, 1, 1, 2, 1, 2, 1, 1];

/// Words in the command with `opcode`, `None` if the opcode is invalid.
pub fn command_words(opcode: u32) -> Option<u32> {
    match opcode {
        0..=9 => Some(u32::from(GENERIC_COMMAND_WORDS[opcode as usize])),
        10..=58 => Some(u32::from(FIGHTER_COMMAND_WORDS[opcode as usize - 10])),
        _ => None,
    }
}

/// One decoded command. Each variant cites the handler that runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// 0, `Command_00` (decomp "Reset"): stop the script (`x8.u = NULL`).
    End(Command00),
    /// 1, `Command_01`: `timer += value`.
    SynchronousTimer(Command00),
    /// 2, `Command_02`: `timer = value - frame_count`.
    AsynchronousTimer(Command02),
    /// 3, `Command_03`: push the next command and `value` (the loop count).
    SetLoop(Command03),
    /// 4, `Command_04`: decrement the count; jump back while nonzero.
    ExecuteLoop,
    /// 5, `Command_05`: push the return address (two words on) and jump to
    /// `target`, a data offset. A NULL target stops the script, since the
    /// interpreter loop ends when `x8.u` is NULL.
    Subroutine { target: Option<u32> },
    /// 6, `Command_06`: pop the return address.
    Return,
    /// 7, `Command_07`: jump to `target`, a data offset. A NULL target stops
    /// the script, like [`Command::End`].
    Goto { target: Option<u32> },
    /// 8, `Command_08` (SetTimerAnimation): `timer = F32_MAX`, which waits
    /// for the animation to loop (`ftAction_80073240`).
    SetTimerAnimation,
    /// 9, `Command_09`: `lbBgFlash_80021C48(param_1, param_2)`.
    BgFlash(Command09),
    /// 10, `ftAction_80071028`: spawn a graphic effect (skipped while
    /// invisible). Offsets and ranges scale by 0.003906f; the handler
    /// stores `offset_z` in `offset.x` and `offset_x` in `offset.z`.
    SpawnGfx(SpawnGfx0, SpawnGfx1, SpawnGfx2, SpawnGfx3, SpawnGfx4),
    /// 11, `ftAction_8007121C`: create hitbox `id`. Size and offsets scale by
    /// 0.003906f. After its five words the handler sets
    /// `x134.hit_grabbed_victim_only` from bit `x1_b4` of the *next*
    /// command's first word (`spawn_hitbox_5`, `lbz r3, 1(r3)` after five
    /// pointer increments); see [`SpawnHitbox5`].
    Hitbox(
        SpawnHitbox0,
        SpawnHitbox1,
        SpawnHitbox2,
        SpawnHitbox3,
        SpawnHitbox4,
    ),
    /// 12, `ftAction_8007162C`: set hitbox `idx` damage.
    SetHitboxDamage(SetHitboxDamage),
    /// 13, `ftAction_8007169C`: set hitbox `idx` scale (`value * 0.003906f`).
    SetHitboxScale(SetHitboxScale),
    /// 14, `ftAction_80071708`: set hitbox `idx` flag `x42_b5` (`type_` 0)
    /// or `x42_b7` (`type_` 1).
    SetHitboxFlag(SetHitboxX42B57),
    /// 15, `ftAction_80071784`: `ftColl_8007AFC8(gobj, hit_idx)`, which calls
    /// `lbColl_80008428` on hitbox `hit_idx`.
    RemoveHitbox(SetThrowFlags),
    /// 16, `ftAction_800717D8`: `ftColl_8007AFF8`, which calls
    /// `lbColl_80008428` on all four hitboxes.
    ClearHitboxes,
    /// 17, `ftAction_80071B50`: play a sound. Behaviors 0-6 play `sfx_id`;
    /// 10-15 call per-fighter sound functions.
    SoundEffect(SoundEffect0, SoundEffect1, SoundEffect2),
    /// 18, `ftAction_80071CCC`: play the smash sound (`ftData::x4C_sfx->smash`).
    SmashSfx,
    /// 19, `ftAction_80071820`: `cmd_vars[idx] = value`.
    SetCmdVar(SetCmdVar),
    /// 20, `ftAction_800718A4`: `hit_idx` 0 sets `throw_flags_b3` and
    /// `cmd_timer`, 1 sets `throw_flags_b4`.
    SetThrowFlagB3B4(SetThrowFlags),
    /// 21, `ftAction_80071908`: set `throw_flags_b1`.
    SetThrowFlagB1,
    /// 22, `ftAction_8007192C`: set `throw_flags_b2`.
    SetThrowFlagB2,
    /// 23, `ftAction_80071950`: set `allow_interrupt`.
    AllowInterrupt,
    /// 24, `ftAction_80071974`: set `throw_flags_b0`.
    SetThrowFlagB0,
    /// 25, `ftAction_80071998`: `state` 0, 1, 2 call `ftCommon_8007D7FC`,
    /// `ftCommon_8007D5D4`, `ftCommon_8007D60C`.
    SetAirborneState(SetAirborneState),
    /// 26, `ftAction_80071A14`: `ftColl_8007B62C(gobj, state)` (`Fighter::x1988`).
    SetBodyState(SetAirborneState),
    /// 27, `ftAction_80071A58`: `ftColl_8007B0C0(gobj, state)` (every hurt capsule).
    SetAllHurtState(SetAirborneState),
    /// 28, `ftAction_80071A9C`: `ftColl_8007B128(gobj, bone_idx, state)`.
    SetHurtState(SetHurtState),
    /// 29, `ftAction_80071AE8`: set `x2218_b1` unless `disabled` (and no `x197C`).
    SetJabCombo(SetJabCombo),
    /// 30, `ftAction_80071B28`: `x2218_b2 = state`.
    SetJabRapid(SetJabRapid),
    /// 31, `ftAction_80071D40`: `ftParts_80074B0C(gobj, idx, value)`.
    SetDobjFlags(SetDobjFlags),
    /// 32, `ftAction_80071D94`: `ftParts_80074A8C(gobj)`.
    Parts80074A8C,
    /// 33, `ftAction_80071DCC`: `ftParts_80074ACC(gobj)`.
    Parts80074ACC,
    /// 34, `ftAction_80071E04`: set throw hitbox `idx` (`Fighter::xDF4`).
    ThrowHitbox(SetThrowHitbox0, SetThrowHitbox1, SetThrowHitbox2),
    /// 35, `ftAction_80071F34`: `ftCommon_8007F5CC(gobj, value)`.
    Common8007F5CC(Unk27),
    /// 36, `ftAction_80071F78`: `x221E_b4 = value`.
    SetArticleVis(SetArticleVis),
    /// 37, `ftAction_80071FA0`: `x221E_b5 = value`.
    SetFighterVis(SetFighterVis),
    /// 38, `ftAction_80071FC8`: play one of six sounds picked by
    /// `HSD_Randi(random_range)`.
    RandomSfx(PseudoRandomSfx0, [u32; 6]),
    /// 39, `ftAction_80072320`: `lbAudioAx_800263E8` sound (`sfx_id` remapped
    /// by `ft_80087D0C`).
    StageSfx(StageSfx0, StageSfx1, StageSfx2, StageSfx3),
    /// 40, `ftAction_800726F4`: `ftAnim_800704F0(gobj, idx, frame)`, and for
    /// `idx2` too when `b`.
    SetTexAnim(SetTexAnim),
    /// 41, `ftAction_800727C8`: `ftAnim_ApplyPartAnim(gobj, unk1, unk2, unk3)`.
    PartAnim(PartAnim),
    /// 42, `ftAction_80072894`: `ftCommon_8007E83C(gobj, unk1, unk2)`.
    Common8007E83C(Unk9),
    /// 43, `ftAction_800728F8`: rumble (`ftCommon_8007EC30` when `unk1`, else
    /// `ftCommon_8007EBAC`, which calls `lb_80014574(pad_port, ...)`).
    Rumble(Unk10),
    /// 44, `ftAction_8007297C`: `ftCommon_8007ECD4(fp, unk1)`.
    Common8007ECD4(Unk11),
    /// 45, `ftAction_800729D4`: `unk1` 0 calls `ftCommon_8007EEC8(fp, unk2,
    /// unk3)`, 1 calls `ftCommon_8007EF5C(fp, unk2)`.
    Common8007EEC8(Unk12),
    /// 46, `ftAction_80072A5C`: `ftCo_800BFFD0(fp, unk1, unk2)`.
    Co800BFFD0(Unk13),
    /// 47, `ftAction_80072ABC`: `ftCo_800C0200(fp, unk1)`.
    Co800C0200(Unk14),
    /// 48, `ftAction_80072B14`: `x221E_b1 = unk1`.
    SetX221EB1(Unk15),
    /// 49, `ftAction_80072B3C`: `x2100 = unk4`, `x2101_bits_8 = unk3`.
    SetX2100(Unk16),
    /// 50, `ftAction_80072B94`: `ftCo_8009E318(gobj, unk1, cur_anim_frame)`.
    Co8009E318(Unk17),
    /// 51, `ftAction_80072BF4`: `Fighter_TakeDamage_8006CC7C(fp, damage_amount)`.
    TakeDamage(Unk18),
    /// 52, `ftAction_80072C6C`: `ft_8008A1B8(gobj, unk1)`.
    Ft8008A1B8(Unk19),
    /// 53, `ftAction_80072CB0`: `x2225_b2 = unk1`.
    SetX2225B2(Unk20),
    /// 54, `ftAction_80072CD8`: footstep effect and sound. The sound path
    /// passes the whole command to `ftAction_80071B50`, which reads the first
    /// word as [`SoundEffect0`] (its `behavior` overlaps `bone_id`).
    FootstepFx(FootstepFx0, SoundEffect0, SoundEffect1, SoundEffect2),
    /// 55, `ftAction_80072E4C`: effect `gfx_id` and sound; like
    /// [`Command::FootstepFx`], the sound path reads the first word as
    /// [`SoundEffect0`].
    UnkFx(UnkFx0, SoundEffect0, SoundEffect1, SoundEffect2),
    /// 56, `ftAction_80073008`: start a smash charge
    /// (`ftCo_800DEE84(gobj, color_anim, charge_frames, charge_rate * 0.003906f)`).
    SmashCharge(SmashCharge0, SmashCharge1),
    /// 57, `ftAction_800730B8`: `ftCo_800C8B60(fp, unk1, unk2)`.
    Co800C8B60(Unk21),
    /// 58, `ftAction_80073118`: wind effect (`ftCo_8009E714`). The handler's
    /// argument order differs from the decomp field names.
    WindFx(WindFx0, WindFx1, WindFx2, WindFx3),
}

impl Command {
    /// Decode the command whose words are `words` (at least its length).
    /// Subroutine and goto targets come from `target`, the resolved second
    /// word (`None` for NULL). Returns `None` for an invalid opcode or too
    /// few words.
    pub fn decode(words: &[u32], target: Option<u32>) -> Option<Self> {
        let op = opcode(*words.first()?);
        if words.len() < command_words(op)? as usize {
            return None;
        }
        let w = |i: usize| words[i];
        Some(match op {
            0 => Self::End(Command00::decode(w(0))),
            1 => Self::SynchronousTimer(Command00::decode(w(0))),
            2 => Self::AsynchronousTimer(Command02::decode(w(0))),
            3 => Self::SetLoop(Command03::decode(w(0))),
            4 => Self::ExecuteLoop,
            5 => Self::Subroutine { target },
            6 => Self::Return,
            7 => Self::Goto { target },
            8 => Self::SetTimerAnimation,
            9 => Self::BgFlash(Command09::decode(w(0))),
            10 => Self::SpawnGfx(
                SpawnGfx0::decode(w(0)),
                SpawnGfx1::decode(w(1)),
                SpawnGfx2::decode(w(2)),
                SpawnGfx3::decode(w(3)),
                SpawnGfx4::decode(w(4)),
            ),
            11 => Self::Hitbox(
                SpawnHitbox0::decode(w(0)),
                SpawnHitbox1::decode(w(1)),
                SpawnHitbox2::decode(w(2)),
                SpawnHitbox3::decode(w(3)),
                SpawnHitbox4::decode(w(4)),
            ),
            12 => Self::SetHitboxDamage(SetHitboxDamage::decode(w(0))),
            13 => Self::SetHitboxScale(SetHitboxScale::decode(w(0))),
            14 => Self::SetHitboxFlag(SetHitboxX42B57::decode(w(0))),
            15 => Self::RemoveHitbox(SetThrowFlags::decode(w(0))),
            16 => Self::ClearHitboxes,
            17 => Self::SoundEffect(
                SoundEffect0::decode(w(0)),
                SoundEffect1::decode(w(1)),
                SoundEffect2::decode(w(2)),
            ),
            18 => Self::SmashSfx,
            19 => Self::SetCmdVar(SetCmdVar::decode(w(0))),
            20 => Self::SetThrowFlagB3B4(SetThrowFlags::decode(w(0))),
            21 => Self::SetThrowFlagB1,
            22 => Self::SetThrowFlagB2,
            23 => Self::AllowInterrupt,
            24 => Self::SetThrowFlagB0,
            25 => Self::SetAirborneState(SetAirborneState::decode(w(0))),
            26 => Self::SetBodyState(SetAirborneState::decode(w(0))),
            27 => Self::SetAllHurtState(SetAirborneState::decode(w(0))),
            28 => Self::SetHurtState(SetHurtState::decode(w(0))),
            29 => Self::SetJabCombo(SetJabCombo::decode(w(0))),
            30 => Self::SetJabRapid(SetJabRapid::decode(w(0))),
            31 => Self::SetDobjFlags(SetDobjFlags::decode(w(0))),
            32 => Self::Parts80074A8C,
            33 => Self::Parts80074ACC,
            34 => Self::ThrowHitbox(
                SetThrowHitbox0::decode(w(0)),
                SetThrowHitbox1::decode(w(1)),
                SetThrowHitbox2::decode(w(2)),
            ),
            35 => Self::Common8007F5CC(Unk27::decode(w(0))),
            36 => Self::SetArticleVis(SetArticleVis::decode(w(0))),
            37 => Self::SetFighterVis(SetFighterVis::decode(w(0))),
            38 => Self::RandomSfx(
                PseudoRandomSfx0::decode(w(0)),
                [w(1), w(2), w(3), w(4), w(5), w(6)],
            ),
            39 => Self::StageSfx(
                StageSfx0::decode(w(0)),
                StageSfx1::decode(w(1)),
                StageSfx2::decode(w(2)),
                StageSfx3::decode(w(3)),
            ),
            40 => Self::SetTexAnim(SetTexAnim::decode(w(0))),
            41 => Self::PartAnim(PartAnim::decode(w(0))),
            42 => Self::Common8007E83C(Unk9::decode(w(0))),
            43 => Self::Rumble(Unk10::decode(w(0))),
            44 => Self::Common8007ECD4(Unk11::decode(w(0))),
            45 => Self::Common8007EEC8(Unk12::decode(w(0))),
            46 => Self::Co800BFFD0(Unk13::decode(w(0))),
            47 => Self::Co800C0200(Unk14::decode(w(0))),
            48 => Self::SetX221EB1(Unk15::decode(w(0))),
            49 => Self::SetX2100(Unk16::decode(w(0))),
            50 => Self::Co8009E318(Unk17::decode(w(0))),
            51 => Self::TakeDamage(Unk18::decode(w(0))),
            52 => Self::Ft8008A1B8(Unk19::decode(w(0))),
            53 => Self::SetX2225B2(Unk20::decode(w(0))),
            54 => Self::FootstepFx(
                FootstepFx0::decode(w(0)),
                SoundEffect0::decode(w(0)),
                SoundEffect1::decode(w(1)),
                SoundEffect2::decode(w(2)),
            ),
            55 => Self::UnkFx(
                UnkFx0::decode(w(0)),
                SoundEffect0::decode(w(0)),
                SoundEffect1::decode(w(1)),
                SoundEffect2::decode(w(2)),
            ),
            56 => Self::SmashCharge(SmashCharge0::decode(w(0)), SmashCharge1::decode(w(1))),
            57 => Self::Co800C8B60(Unk21::decode(w(0))),
            58 => Self::WindFx(
                WindFx0::decode(w(0)),
                WindFx1::decode(w(1)),
                WindFx2::decode(w(2)),
                WindFx3::decode(w(3)),
            ),
            _ => return None,
        })
    }

    pub fn opcode(&self) -> u32 {
        match self {
            Self::End(_) => 0,
            Self::SynchronousTimer(_) => 1,
            Self::AsynchronousTimer(_) => 2,
            Self::SetLoop(_) => 3,
            Self::ExecuteLoop => 4,
            Self::Subroutine { .. } => 5,
            Self::Return => 6,
            Self::Goto { .. } => 7,
            Self::SetTimerAnimation => 8,
            Self::BgFlash(_) => 9,
            Self::SpawnGfx(..) => 10,
            Self::Hitbox(..) => 11,
            Self::SetHitboxDamage(_) => 12,
            Self::SetHitboxScale(_) => 13,
            Self::SetHitboxFlag(_) => 14,
            Self::RemoveHitbox(_) => 15,
            Self::ClearHitboxes => 16,
            Self::SoundEffect(..) => 17,
            Self::SmashSfx => 18,
            Self::SetCmdVar(_) => 19,
            Self::SetThrowFlagB3B4(_) => 20,
            Self::SetThrowFlagB1 => 21,
            Self::SetThrowFlagB2 => 22,
            Self::AllowInterrupt => 23,
            Self::SetThrowFlagB0 => 24,
            Self::SetAirborneState(_) => 25,
            Self::SetBodyState(_) => 26,
            Self::SetAllHurtState(_) => 27,
            Self::SetHurtState(_) => 28,
            Self::SetJabCombo(_) => 29,
            Self::SetJabRapid(_) => 30,
            Self::SetDobjFlags(_) => 31,
            Self::Parts80074A8C => 32,
            Self::Parts80074ACC => 33,
            Self::ThrowHitbox(..) => 34,
            Self::Common8007F5CC(_) => 35,
            Self::SetArticleVis(_) => 36,
            Self::SetFighterVis(_) => 37,
            Self::RandomSfx(..) => 38,
            Self::StageSfx(..) => 39,
            Self::SetTexAnim(_) => 40,
            Self::PartAnim(_) => 41,
            Self::Common8007E83C(_) => 42,
            Self::Rumble(_) => 43,
            Self::Common8007ECD4(_) => 44,
            Self::Common8007EEC8(_) => 45,
            Self::Co800BFFD0(_) => 46,
            Self::Co800C0200(_) => 47,
            Self::SetX221EB1(_) => 48,
            Self::SetX2100(_) => 49,
            Self::Co8009E318(_) => 50,
            Self::TakeDamage(_) => 51,
            Self::Ft8008A1B8(_) => 52,
            Self::SetX2225B2(_) => 53,
            Self::FootstepFx(..) => 54,
            Self::UnkFx(..) => 55,
            Self::SmashCharge(..) => 56,
            Self::Co800C8B60(_) => 57,
            Self::WindFx(..) => 58,
        }
    }

    /// Words this command occupies ([`command_words`]).
    pub fn words(&self) -> u32 {
        command_words(self.opcode()).expect("decoded commands have valid opcodes")
    }

    /// The fewest and most words the executing handler
    /// (`ftAction_803C06E8`, via `ftAction_80073240`) advances past this
    /// command, over every runtime branch. Usually both equal
    /// [`Self::words`]; they differ only where a handler's advance depends on
    /// the data:
    /// - `ftAction_80071B50` advances 3 words for behaviors 0-6 and 10-15
    ///   and 2 otherwise. `ftAction_80072CD8` and `ftAction_80072E4C` either
    ///   call it with the whole command or skip 3 words.
    /// - `ftAction_80071FC8` advances 7 words when `HSD_Randi(random_range)`
    ///   is 0-5 and 2 otherwise, so ranges above 6 can advance 2.
    ///
    /// Control-flow commands (5, 6, 7) and End report their length.
    pub fn handler_words(&self) -> (u32, u32) {
        let sound = |s: &SoundEffect0| match s.behavior {
            0..=6 | 10..=15 => 3,
            _ => 2,
        };
        match self {
            Self::SoundEffect(s, ..) => (sound(s), sound(s)),
            Self::FootstepFx(_, s, ..) | Self::UnkFx(_, s, ..) => (sound(s).min(3), 3),
            Self::RandomSfx(r, _) if r.random_range > 6 => (2, 7),
            _ => (self.words(), self.words()),
        }
    }

    /// A short name for display.
    pub fn name(&self) -> String {
        let debug = format!("{self:?}");
        let end = debug.find(['(', ' ']).unwrap_or(debug.len());
        debug[..end].to_owned()
    }

    /// `(name, value)` of every field except opcodes, in word order.
    pub fn fields(&self) -> Vec<(&'static str, i64)> {
        let mut out = match self {
            Self::End(a) | Self::SynchronousTimer(a) => a.fields(),
            Self::AsynchronousTimer(a) => a.fields(),
            Self::SetLoop(a) => a.fields(),
            Self::Subroutine { target } | Self::Goto { target } => {
                vec![("target", target.map_or(-1, i64::from))]
            }
            Self::BgFlash(a) => a.fields(),
            Self::SpawnGfx(a, b, c, d, e) => {
                [a.fields(), b.fields(), c.fields(), d.fields(), e.fields()].concat()
            }
            Self::Hitbox(a, b, c, d, e) => {
                [a.fields(), b.fields(), c.fields(), d.fields(), e.fields()].concat()
            }
            Self::SetHitboxDamage(a) => a.fields(),
            Self::SetHitboxScale(a) => a.fields(),
            Self::SetHitboxFlag(a) => a.fields(),
            Self::RemoveHitbox(a) | Self::SetThrowFlagB3B4(a) => a.fields(),
            Self::SoundEffect(a, b, c) => [a.fields(), b.fields(), c.fields()].concat(),
            Self::SetCmdVar(a) => a.fields(),
            Self::SetAirborneState(a) | Self::SetBodyState(a) | Self::SetAllHurtState(a) => {
                a.fields()
            }
            Self::SetHurtState(a) => a.fields(),
            Self::SetJabCombo(a) => a.fields(),
            Self::SetJabRapid(a) => a.fields(),
            Self::SetDobjFlags(a) => a.fields(),
            Self::ThrowHitbox(a, b, c) => [a.fields(), b.fields(), c.fields()].concat(),
            Self::Common8007F5CC(a) => a.fields(),
            Self::SetArticleVis(a) => a.fields(),
            Self::SetFighterVis(a) => a.fields(),
            Self::RandomSfx(a, ids) => {
                let names = [
                    "sfx_id0", "sfx_id1", "sfx_id2", "sfx_id3", "sfx_id4", "sfx_id5",
                ];
                let ids = names.iter().zip(ids).map(|(&n, &id)| (n, i64::from(id)));
                a.fields().into_iter().chain(ids).collect()
            }
            Self::StageSfx(a, b, c, d) => [a.fields(), b.fields(), c.fields(), d.fields()].concat(),
            Self::SetTexAnim(a) => a.fields(),
            Self::PartAnim(a) => a.fields(),
            Self::Common8007E83C(a) => a.fields(),
            Self::Rumble(a) => a.fields(),
            Self::Common8007ECD4(a) => a.fields(),
            Self::Common8007EEC8(a) => a.fields(),
            Self::Co800BFFD0(a) => a.fields(),
            Self::Co800C0200(a) => a.fields(),
            Self::SetX221EB1(a) => a.fields(),
            Self::SetX2100(a) => a.fields(),
            Self::Co8009E318(a) => a.fields(),
            Self::TakeDamage(a) => a.fields(),
            Self::Ft8008A1B8(a) => a.fields(),
            Self::SetX2225B2(a) => a.fields(),
            Self::FootstepFx(a, _, c, d) => [a.fields(), c.fields(), d.fields()].concat(),
            Self::UnkFx(a, _, c, d) => [a.fields(), c.fields(), d.fields()].concat(),
            Self::SmashCharge(a, b) => [a.fields(), b.fields()].concat(),
            Self::Co800C8B60(a) => a.fields(),
            Self::WindFx(a, b, c, d) => [a.fields(), b.fields(), c.fields(), d.fields()].concat(),
            Self::ExecuteLoop
            | Self::Return
            | Self::SetTimerAnimation
            | Self::ClearHitboxes
            | Self::SmashSfx
            | Self::SetThrowFlagB1
            | Self::SetThrowFlagB2
            | Self::AllowInterrupt
            | Self::SetThrowFlagB0
            | Self::Parts80074A8C
            | Self::Parts80074ACC => Vec::new(),
        };
        out.retain(|&(name, _)| name != "opcode");
        out
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())?;
        for (name, value) in self.fields() {
            if name == "target" && value < 0 {
                write!(f, " {name}=NULL")?;
            } else if name == "target" {
                write!(f, " {name}={value:#x}")?;
            } else {
                write!(f, " {name}={value}")?;
            }
        }
        Ok(())
    }
}

/// Why a script could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    Archive(hsd_dat::Error),
    /// A command offset is not word-aligned.
    Misaligned {
        offset: u32,
    },
    /// Opcode 59-63, outside the fighter tables.
    UnknownOpcode {
        offset: u32,
        opcode: u32,
    },
    /// A word that is not a goto/subroutine target is relocated, so the
    /// decode is out of step with the data.
    UnexpectedPointer {
        offset: u32,
    },
    /// A command starts inside another decoded command.
    Overlap {
        offset: u32,
    },
    /// `Return` with no return address on top of the stack.
    BadReturn {
        offset: u32,
    },
    /// `ExecuteLoop` with no loop on top of the stack.
    BadLoop {
        offset: u32,
    },
    /// The walk did not finish within its step budget.
    TooLong {
        offset: u32,
    },
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(e) => write!(f, "{e}"),
            Self::Misaligned { offset } => write!(f, "misaligned command at {offset:#x}"),
            Self::UnknownOpcode { offset, opcode } => {
                write!(f, "unknown opcode {opcode} at {offset:#x}")
            }
            Self::UnexpectedPointer { offset } => {
                write!(f, "relocated word inside a command at {offset:#x}")
            }
            Self::Overlap { offset } => write!(f, "command at {offset:#x} overlaps another"),
            Self::BadReturn { offset } => write!(f, "return without a caller at {offset:#x}"),
            Self::BadLoop { offset } => write!(f, "loop end without a loop at {offset:#x}"),
            Self::TooLong { offset } => write!(f, "walk exceeded its budget at {offset:#x}"),
        }
    }
}

impl std::error::Error for ScriptError {}

impl From<hsd_dat::Error> for ScriptError {
    fn from(e: hsd_dat::Error) -> Self {
        Self::Archive(e)
    }
}

/// Decode the command at data offset `offset`.
///
/// Checks the relocation table: the second word of a subroutine or goto
/// must be a pointer or NULL, and no other word of a command may be one.
pub fn decode_at(archive: &Archive, offset: u32) -> Result<Command, ScriptError> {
    if !offset.is_multiple_of(4) {
        return Err(ScriptError::Misaligned { offset });
    }
    let op = opcode(archive.u32(offset)?);
    let len = command_words(op).ok_or(ScriptError::UnknownOpcode { offset, opcode: op })?;
    let words = (0..len)
        .map(|i| archive.u32(offset + 4 * i))
        .collect::<hsd_dat::Result<Vec<_>>>()?;
    let jumps = op == 5 || op == 7;
    for i in 0..len {
        if !(jumps && i == 1) && archive.is_relocated(offset + 4 * i) {
            return Err(ScriptError::UnexpectedPointer {
                offset: offset + 4 * i,
            });
        }
    }
    // A nonzero word without a relocation is an error (`UnrelocatedPointer`).
    let target = if jumps {
        archive.ptr(offset + 4)?
    } else {
        None
    };
    Ok(Command::decode(&words, target).expect("length and target checked above"))
}

/// A script with every command reachable from its entry decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub entry: u32,
    /// Reachable commands by data offset.
    pub commands: BTreeMap<u32, Command>,
    /// Command offsets in the order execution first reaches them.
    pub order: Vec<u32>,
    /// The deepest the command stack gets, in words (`CommandInfo::loop_count`).
    /// A loop takes two words and a subroutine call one.
    pub max_stack_words: u32,
}

/// A word on the command stack (`CommandInfo::event_return`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StackWord {
    /// A return address or loop start.
    Address(u32),
    /// A loop's remaining count (`SetLoop` pushes the start, then this).
    LoopCount,
}

/// Decode every command reachable from `entry`, following control flow the
/// way the interpreter does, minus timers. Scripts have no conditional
/// jumps, so this is one path: gotos jump, subroutines push a return
/// address, returns pop it, loops run their body once (enough to reach every
/// command) and fall through, and `End` or a NULL jump stops. The walk also
/// stops when it returns to a state (offset and stack) it has visited, as an
/// endless goto loop does.
pub fn decode_script(archive: &Archive, entry: u32) -> Result<Script, ScriptError> {
    const STEP_BUDGET: usize = 1_000_000;
    let mut commands: BTreeMap<u32, Command> = BTreeMap::new();
    // Data offset of each word to the start of the command covering it.
    let mut owner: BTreeMap<u32, u32> = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    let mut max_stack_words = 0;
    let (mut pc, mut stack) = (entry, Vec::<StackWord>::new());
    loop {
        max_stack_words = max_stack_words.max(stack.len() as u32);
        if !seen.insert((pc, stack.clone())) {
            break;
        }
        if seen.len() > STEP_BUDGET {
            return Err(ScriptError::TooLong { offset: pc });
        }
        let command = match commands.get(&pc) {
            Some(command) => command.clone(),
            None => {
                let command = decode_at(archive, pc)?;
                for i in 0..command.words() {
                    if owner.insert(pc + 4 * i, pc).is_some() {
                        return Err(ScriptError::Overlap { offset: pc + 4 * i });
                    }
                }
                commands.insert(pc, command.clone());
                order.push(pc);
                command
            }
        };
        let next = pc + 4 * command.words();
        pc = match command {
            Command::End(_) | Command::Goto { target: None } => break,
            Command::Subroutine { target: None } => {
                // Pushes the return address, then stops with `x8.u` NULL.
                max_stack_words = max_stack_words.max(stack.len() as u32 + 1);
                break;
            }
            Command::Goto {
                target: Some(target),
            } => target,
            Command::Subroutine {
                target: Some(target),
            } => {
                stack.push(StackWord::Address(next));
                target
            }
            Command::Return => match stack.pop() {
                Some(StackWord::Address(address)) => address,
                _ => return Err(ScriptError::BadReturn { offset: pc }),
            },
            Command::SetLoop(_) => {
                stack.push(StackWord::Address(next));
                stack.push(StackWord::LoopCount);
                next
            }
            Command::ExecuteLoop => match stack.as_slice() {
                [.., StackWord::Address(_), StackWord::LoopCount] => {
                    stack.truncate(stack.len() - 2);
                    next
                }
                _ => return Err(ScriptError::BadLoop { offset: pc }),
            },
            _ => next,
        };
    }
    Ok(Script {
        entry,
        commands,
        order,
        max_stack_words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitfields_allocate_from_the_msb() {
        // Fox's up-smash style hitbox word: opcode 11, id 1, group 0, bone 2,
        // damage 17 (constructed, not disc data).
        let word = (11 << 26) | (1 << 23) | (2 << 11) | 17;
        let hit = SpawnHitbox0::decode(word);
        assert_eq!(hit.opcode, 11);
        assert_eq!(hit.id, 1);
        assert_eq!(hit.hit_group, 0);
        assert_eq!(hit.bone, 2);
        assert_eq!(hit.damage, 17);
        // Signed halves sign-extend; unsigned ones do not.
        let size = SpawnHitbox1::decode(0xffff_ff00);
        assert_eq!(size.size, 0xffff);
        assert_eq!(size.z_offset, -256);
        // set_dobj_flags: s32 idx:7 at bit 6, s32 value:19 at bit 13
        // (ftAction_80071D40: rlwinm 22,0,7 + srawi 25; rlwinm 13,0,19 + srawi 13).
        let flags = SetDobjFlags::decode((31 << 26) | (0x7f << 19) | 0x7_ffff);
        assert_eq!((flags.idx, flags.value), (-1, -1));
        // set_hitbox_x42_b57: idx is 24 bits (`rlwinm r0, r0, 30, 8, 31`).
        let flag = SetHitboxX42B57::decode((14 << 26) | (3 << 2) | 0b10);
        assert_eq!((flag.idx, flag.type_, flag.value), (3, 1, 0));
    }

    #[test]
    fn command_lengths_follow_the_tables() {
        assert_eq!(command_words(0), Some(1));
        assert_eq!(command_words(5), Some(2));
        assert_eq!(command_words(7), Some(2));
        assert_eq!(command_words(10), Some(5));
        assert_eq!(command_words(11), Some(5));
        assert_eq!(command_words(17), Some(3));
        assert_eq!(command_words(38), Some(7));
        assert_eq!(command_words(58), Some(4));
        assert_eq!(command_words(59), None);
        assert_eq!(command_words(63), None);
    }

    #[test]
    fn every_valid_opcode_decodes() {
        let words = [0u32; 7];
        for op in 0..64 {
            let mut words = words;
            words[0] = op << 26;
            let command = Command::decode(&words, Some(0));
            if op < 59 {
                let command = command.unwrap();
                assert_eq!(command.opcode(), op);
                assert_eq!(command.handler_words().1, command.words());
            } else {
                assert!(command.is_none());
            }
        }
    }

    #[test]
    fn handler_lengths_follow_data_dependent_branches() {
        let sfx = |behavior: u32| {
            let words = [(17 << 26) | (behavior << 18), 0, 0];
            Command::decode(&words, None).unwrap().handler_words()
        };
        assert_eq!(sfx(0), (3, 3));
        assert_eq!(sfx(12), (3, 3));
        assert_eq!(sfx(7), (2, 2));
        let random = |range: u32| {
            let words = [(38 << 26) | range, 0, 0, 0, 0, 0, 0];
            Command::decode(&words, None).unwrap().handler_words()
        };
        assert_eq!(random(6), (7, 7));
        assert_eq!(random(7), (2, 7));
    }

    /// An archive whose data block is `words`, with pointer fields at the
    /// word indices in `pointers`.
    fn archive(words: &[u32], pointers: &[u32]) -> Archive {
        let data_size = words.len() as u32 * 4;
        let file_size = 0x20 + data_size + pointers.len() as u32 * 4;
        let mut file = Vec::new();
        for word in [file_size, data_size, pointers.len() as u32, 0, 0, 0, 0, 0] {
            file.extend(word.to_be_bytes());
        }
        for word in words {
            file.extend(word.to_be_bytes());
        }
        for index in pointers {
            file.extend((index * 4).to_be_bytes());
        }
        Archive::parse(&file).unwrap()
    }

    const END: u32 = 0;
    const TIMER: u32 = 2 << 26 | 5;
    const GOTO: u32 = 7 << 26;
    const CALL: u32 = 5 << 26;
    const RET: u32 = 6 << 26;
    const LOOP: u32 = 3 << 26 | 2;
    const LOOP_END: u32 = 4 << 26;

    #[test]
    fn walk_follows_calls_loops_and_gotos() {
        // 0: call 7; 2: loop x2 { timer }; 5: goto 0 (back to the start)
        // 7: timer; 8: return
        let words = [CALL, 28, LOOP, TIMER, LOOP_END, GOTO, 0, TIMER, RET];
        let script = decode_script(&archive(&words, &[1, 6]), 0).unwrap();
        let offsets: Vec<_> = script.commands.keys().copied().collect();
        assert_eq!(offsets, [0, 8, 12, 16, 20, 28, 32]);
        assert_eq!(script.order, [0, 28, 32, 8, 12, 16, 20]);
        assert_eq!(
            script.commands[&0],
            Command::Subroutine { target: Some(28) }
        );
        assert_eq!(script.commands[&20], Command::Goto { target: Some(0) });
        assert_eq!(script.max_stack_words, 2);
    }

    #[test]
    fn null_jumps_stop_the_script() {
        let script = decode_script(&archive(&[TIMER, GOTO, 0, 0xffff_ffff], &[]), 0).unwrap();
        assert_eq!(script.commands[&4], Command::Goto { target: None });
        assert_eq!(script.commands.len(), 2);
    }

    #[test]
    fn walk_rejects_inconsistent_scripts() {
        let decode = |words: &[u32], pointers: &[u32]| decode_script(&archive(words, pointers), 0);
        assert_eq!(
            decode(&[59 << 26], &[]),
            Err(ScriptError::UnknownOpcode {
                offset: 0,
                opcode: 59
            })
        );
        assert_eq!(
            decode(&[RET], &[]),
            Err(ScriptError::BadReturn { offset: 0 })
        );
        assert_eq!(
            decode(&[LOOP_END], &[]),
            Err(ScriptError::BadLoop { offset: 0 })
        );
        // A goto into the middle of a three-word sound command.
        assert_eq!(
            decode(&[17 << 26, 0, 0x7f40, GOTO, 4], &[4]),
            Err(ScriptError::Overlap { offset: 4 })
        );
        // A pointer inside a non-jump command, and a jump without one.
        assert_eq!(
            decode(&[17 << 26, 8, 0x7f40, END], &[1]),
            Err(ScriptError::UnexpectedPointer { offset: 4 })
        );
        assert!(matches!(
            decode(&[GOTO, 4, END], &[]),
            Err(ScriptError::Archive(
                hsd_dat::Error::UnrelocatedPointer { .. }
            ))
        ));
        assert_eq!(
            decode_script(&archive(&[END], &[]), 2),
            Err(ScriptError::Misaligned { offset: 2 })
        );
    }
}
