//! Fighter motion tables: `ftData::xC` (in-game motions) and `ftData::x14`
//! (demo motions), arrays of `Fighter_WaitAnimData`.
//!
//! Each entry names a motion's animation (a figatree, the public symbol of an
//! archive stored at `[aj_offset, aj_offset + aj_size)` in the fighter's
//! `Pl??AJ.dat` bank), its subaction command script (decoded by
//! [`super::action`]), and flags copied into `Fighter::x594` when the motion
//! starts. `Fighter_ChangeMotionState` indexes the table by the new action
//! state's `anim_id`.
//!
//! The archive holds no counts: the game takes them from tables in the DOL
//! ([`MOTION_COUNTS`], [`DEMO_MOTION_COUNTS`]).

use super::kinds::FIGHTERS;
pub use super::motion_data::{FighterData, MotionData};
use crate::dat::{DatField, DatPtr};
use hsd_dat::{Archive, Error, Result};

/// Fighter kinds (`enum FighterKind`, `ft/forward.h`). Equal to the index of
/// the fighter in [`FIGHTERS`].
pub const KIND_COUNT: usize = 33;
pub const KIND_POPO: usize = 0x0a;
pub const KIND_NANA: usize = 0x0b;

/// `ftData_Table_Unk0[kind].count` (`ftdata.c`, 0x803C0FC8, `{data, count}`
/// pairs): entries in `ftData::xC`. `ftData_80085A14` relocates exactly this
/// many entries, and `ftData_80085B10` stores it in `Fighter::x58C`, the bound
/// `ftData_80085CD8` checks motion ids against.
pub const MOTION_COUNTS: [u32; KIND_COUNT] = [
    303, 327, 318, 337, 479, 316, 314, 317, 326, 318, 321, 321, 320, 313, 314, 327, 314, 312, 327,
    311, 314, 303, 327, 320, 323, 318, 327, 345, 344, 295, 295, 316, 296,
];
/// DOL address of `ftData_Table_Unk0`.
pub const MOTION_COUNTS_ADDRESS: u32 = 0x803C_0FC8;

/// `ftData_UnkIntPairs[kind].count` (`ftdata.c`, 0x803C25F4): entries in
/// `ftData::x14`, the motions of the result, intro, ending, and vs. screens
/// (`ftData_80085B98`, `ftDemo_SetArchiveData`). Their animations come from
/// separate demo archives, not `Pl??AJ.dat`.
pub const DEMO_MOTION_COUNTS: [u32; KIND_COUNT] = [
    16, 14, 14, 14, 18, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 16, 14, 14, 14, 14, 14, 14,
    14, 14, 14, 14, 14, 14, 14, 15, 14,
];
/// DOL address of `ftData_UnkIntPairs`.
pub const DEMO_MOTION_COUNTS_ADDRESS: u32 = 0x803C_25F4;

/// `ftData_803C23E4`: each kind's animation bank (`ft??_Init_AnimDatFilename`).
pub const ANIM_FILES: [&str; KIND_COUNT] = [
    "PlMrAJ.dat",
    "PlFxAJ.dat",
    "PlCaAJ.dat",
    "PlDkAJ.dat",
    "PlKbAJ.dat",
    "PlKpAJ.dat",
    "PlLkAJ.dat",
    "PlSkAJ.dat",
    "PlNsAJ.dat",
    "PlPeAJ.dat",
    "PlPpAJ.dat",
    "PlNnAJ.dat",
    "PlPkAJ.dat",
    "PlSsAJ.dat",
    "PlYsAJ.dat",
    "PlPrAJ.dat",
    "PlMtAJ.dat",
    "PlLgAJ.dat",
    "PlMsAJ.dat",
    "PlZdAJ.dat",
    "PlClAJ.dat",
    "PlDrAJ.dat",
    "PlFcAJ.dat",
    "PlPcAJ.dat",
    "PlGwAJ.dat",
    "PlGnAJ.dat",
    "PlFeAJ.dat",
    "PlMhAJ.dat",
    "PlChAJ.dat",
    "PlBoAJ.dat",
    "PlGlAJ.dat",
    "PlGkAJ.dat",
    "PlSbAJ.dat",
];

/// `ftData_80085A14` reports "fighter figatree over!" for an in-game motion
/// whose animation archive is larger than this (`cmplwi r4, 0x8000`).
pub const MAX_ANIM_SIZE: u32 = 0x8000;
/// The same limit for demo motions (`ftData_80085B98`, `cmplwi r4, 0xB000`),
/// the size of the `fighter_x59C_alloc_data` buffers (`ftDemo_ObjAllocInit`).
pub const MAX_DEMO_ANIM_SIZE: u32 = 0xB000;

/// The fighter kind whose `ftData` root is `data_symbol` (its [`FIGHTERS`] index).
pub fn kind_of(data_symbol: &str) -> Option<usize> {
    FIGHTERS
        .iter()
        .position(|&(_, _, symbol)| symbol == data_symbol)
}

/// One `Fighter_WaitAnimData` entry, with its pointers resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    /// Index in the table: the motion id (`Fighter::anim_id`).
    pub index: u32,
    /// `x0`: the animation's public symbol in its archive slice, e.g.
    /// `PlyFox5K_Share_ACTION_Wait1_figatree`. `None` when NULL.
    pub anim_name: Option<String>,
    /// `x4`: byte offset of the animation archive in the AJ bank.
    pub aj_offset: i32,
    /// `x8`: its size in bytes; 0 means no animation (`ftData_80085A14`
    /// leaves `x14` unset, and `ftData_80085CD8` then clears the figatree).
    pub aj_size: i32,
    /// `xC`: data offset of the subaction script, `None` when NULL.
    pub script: Option<u32>,
    /// `x10_animCurrFlags`: copied to `Fighter::x594` by
    /// `Fighter_ChangeMotionState`.
    pub flags: i32,
    /// `x14`: the slice's address, filled in at load time (0 on disc).
    pub x14: u32,
    /// `ftData::x10[index]` (or `x18` for demo motions), `None` when that
    /// table is NULL. Byte 0 is the default animation blend in frames
    /// (`Fighter_ChangeMotionState`); `ftCo_8009E7B4` reads both bytes.
    pub x10_pair: Option<[u8; 2]>,
}

/// A NUL-terminated string at `offset` in the data block.
pub fn read_c_string(archive: &Archive, offset: u32) -> Result<String> {
    let tail = archive
        .data()
        .get(offset as usize..)
        .ok_or(Error::OutOfBounds { offset, len: 1 })?;
    let len = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or(Error::OutOfBounds {
            offset,
            len: tail.len() as u32,
        })?;
    Ok(String::from_utf8_lossy(&tail[..len]).into_owned())
}

/// Read a fighter's `ftData` root (e.g. `ftDataFox` in `PlFx.dat`).
pub fn load_fighter_data(archive: &Archive, data_symbol: &str) -> Result<FighterData> {
    FighterData::read(archive, archive.public(data_symbol)?)
}

/// Read `count` entries of a motion table and the matching byte pairs.
pub fn read_motions(
    archive: &Archive,
    table: DatPtr,
    pairs: DatPtr,
    count: u32,
) -> Result<Vec<Motion>> {
    let Some(table) = table.0 else {
        return if count == 0 {
            Ok(Vec::new())
        } else {
            Err(Error::NullPointer(0))
        };
    };
    (0..count)
        .map(|index| {
            let raw = MotionData::read(archive, table + index * MotionData::SIZE)?;
            let anim_name = match raw.x0.0 {
                Some(offset) => Some(read_c_string(archive, offset)?),
                None => None,
            };
            let x10_pair = match pairs.0 {
                Some(offset) => Some(<[u8; 2]>::read(archive, offset + index * 2)?),
                None => None,
            };
            Ok(Motion {
                index,
                anim_name,
                aj_offset: raw.x4,
                aj_size: raw.x8,
                script: raw.xc.0,
                flags: raw.x10_animcurrflags,
                x14: raw.x14,
                x10_pair,
            })
        })
        .collect()
}

/// A fighter's in-game motions (`ftData::xC`, [`MOTION_COUNTS`] entries).
pub fn load_motions(archive: &Archive, data_symbol: &str) -> Result<Vec<Motion>> {
    let kind = kind_of(data_symbol).ok_or_else(|| Error::MissingSymbol(data_symbol.into()))?;
    let data = load_fighter_data(archive, data_symbol)?;
    read_motions(archive, data.xc, data.x10, MOTION_COUNTS[kind])
}

/// A fighter's demo motions (`ftData::x14`, [`DEMO_MOTION_COUNTS`] entries).
pub fn load_demo_motions(archive: &Archive, data_symbol: &str) -> Result<Vec<Motion>> {
    let kind = kind_of(data_symbol).ok_or_else(|| Error::MissingSymbol(data_symbol.into()))?;
    let data = load_fighter_data(archive, data_symbol)?;
    read_motions(archive, data.x14, data.x18, DEMO_MOTION_COUNTS[kind])
}

/// The bytes of a motion's animation archive in its AJ bank, if it has one.
pub fn anim_slice<'a>(bank: &'a [u8], motion: &Motion) -> Option<&'a [u8]> {
    if motion.aj_size == 0 {
        return None;
    }
    let start = usize::try_from(motion.aj_offset).ok()?;
    let end = start.checked_add(usize::try_from(motion.aj_size).ok()?)?;
    bank.get(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::check_layout;

    #[test]
    fn layouts_match_machine_code() {
        check_layout(FighterData::FIELDS, FighterData::SIZE);
        check_layout(MotionData::FIELDS, MotionData::SIZE);
        // ftData_80085A14: `lwz r3, 12(r27)` loads ftData::xC, the loop steps
        // `addi r30, r30, 24`, reads x8 (`addi r0, r30, 8; lwzx`) and x4
        // (`lwz r0, 4(r3)`), and stores x14 (`stw r0, 20(r3)`).
        // ftData_80085B98: `lwz r3, 20(r3)` loads ftData::x14, `mulli r27, r26, 24`.
        assert_eq!(MotionData::SIZE, 0x18);
        let offset = |fields: &[(&str, u32, u32)], name: &str| {
            fields.iter().find(|f| f.0 == name).map(|f| f.1)
        };
        assert_eq!(offset(FighterData::FIELDS, "xc"), Some(0xc));
        assert_eq!(offset(FighterData::FIELDS, "x14"), Some(0x14));
        assert_eq!(offset(MotionData::FIELDS, "x4"), Some(0x4));
        assert_eq!(offset(MotionData::FIELDS, "x8"), Some(0x8));
        assert_eq!(offset(MotionData::FIELDS, "x14"), Some(0x14));
    }

    #[test]
    fn tables_cover_every_kind() {
        assert_eq!(FIGHTERS.len(), KIND_COUNT);
        assert_eq!(kind_of("ftDataPopo"), Some(KIND_POPO));
        assert_eq!(kind_of("ftDataNana"), Some(KIND_NANA));
        for (kind, &(_, file, _)) in FIGHTERS.iter().enumerate() {
            assert_eq!(ANIM_FILES[kind], file.replace(".dat", "AJ.dat"));
        }
    }
}
