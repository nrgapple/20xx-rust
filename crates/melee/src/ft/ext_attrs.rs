//! Fighter-specific attributes (`ftData::ext_attr`), copied into
//! `Fighter::dat_attrs` by each fighter's `*_Init_LoadSpecialAttrs`.

pub use super::ext_data::*;
use crate::dat::{DatField, DatJson};
use hsd_dat::{Archive, Result};

/// `union ftData_ExtAttr` for the tournament cast. Clones share their
/// original's layout (their `LoadSpecialAttrs` calls the original's).
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::large_enum_variant)] // Kirby's copy-ability table; copied rarely.
pub enum ExtAttrs {
    Mario(MarioAttributes),
    Fox(FoxAttributes),
    Captain(CaptainAttributes),
    Donkey(DonkeyAttributes),
    Kirby(KirbyAttributes),
    Koopa(KoopaAttributes),
    Link(LinkAttributes),
    Seak(SeakAttributes),
    Ness(NessAttributes),
    Peach(PeachAttributes),
    IceClimber(IceClimberAttributes),
    Pikachu(PikachuAttributes),
    Samus(SamusAttributes),
    Yoshi(YoshiAttributes),
    Purin(PurinAttributes),
    Mewtwo(MewtwoAttributes),
    Luigi(LuigiAttributes),
    Mars(MarsAttributes),
    Zelda(ZeldaAttributes),
    GameWatch(GameWatchAttributes),
}

macro_rules! dispatch {
    ($value:expr, $bound:ident => $body:expr) => {
        match $value {
            ExtAttrs::Mario($bound) => $body,
            ExtAttrs::Fox($bound) => $body,
            ExtAttrs::Captain($bound) => $body,
            ExtAttrs::Donkey($bound) => $body,
            ExtAttrs::Kirby($bound) => $body,
            ExtAttrs::Koopa($bound) => $body,
            ExtAttrs::Link($bound) => $body,
            ExtAttrs::Seak($bound) => $body,
            ExtAttrs::Ness($bound) => $body,
            ExtAttrs::Peach($bound) => $body,
            ExtAttrs::IceClimber($bound) => $body,
            ExtAttrs::Pikachu($bound) => $body,
            ExtAttrs::Samus($bound) => $body,
            ExtAttrs::Yoshi($bound) => $body,
            ExtAttrs::Purin($bound) => $body,
            ExtAttrs::Mewtwo($bound) => $body,
            ExtAttrs::Luigi($bound) => $body,
            ExtAttrs::Mars($bound) => $body,
            ExtAttrs::Zelda($bound) => $body,
            ExtAttrs::GameWatch($bound) => $body,
        }
    };
}

impl DatJson for ExtAttrs {
    fn write_json(&self, out: &mut String) {
        dispatch!(self, attrs => attrs.write_json(out))
    }
}

/// Load the fighter-specific attributes for the fighter whose `ftData`
/// root is `data_symbol`. `None` for fighters outside the tournament cast.
pub fn load_ext_attrs(archive: &Archive, data_symbol: &str) -> Result<Option<ExtAttrs>> {
    let ft_data = archive.public(data_symbol)?;
    let ext = archive.non_null_ptr(ft_data + 4)?;
    fn read<T: DatField>(
        archive: &Archive,
        offset: u32,
        wrap: fn(T) -> ExtAttrs,
    ) -> Result<Option<ExtAttrs>> {
        T::read(archive, offset).map(|attrs| Some(wrap(attrs)))
    }
    match data_symbol {
        "ftDataMario" | "ftDataDrmario" => read(archive, ext, ExtAttrs::Mario),
        "ftDataFox" | "ftDataFalco" => read(archive, ext, ExtAttrs::Fox),
        "ftDataCaptain" | "ftDataGanon" => read(archive, ext, ExtAttrs::Captain),
        "ftDataDonkey" => read(archive, ext, ExtAttrs::Donkey),
        "ftDataKirby" => read(archive, ext, ExtAttrs::Kirby),
        "ftDataKoopa" => read(archive, ext, ExtAttrs::Koopa),
        "ftDataLink" | "ftDataClink" => read(archive, ext, ExtAttrs::Link),
        "ftDataSeak" => read(archive, ext, ExtAttrs::Seak),
        "ftDataNess" => read(archive, ext, ExtAttrs::Ness),
        "ftDataPeach" => read(archive, ext, ExtAttrs::Peach),
        "ftDataPopo" | "ftDataNana" => read(archive, ext, ExtAttrs::IceClimber),
        "ftDataPikachu" | "ftDataPichu" => read(archive, ext, ExtAttrs::Pikachu),
        "ftDataSamus" => read(archive, ext, ExtAttrs::Samus),
        "ftDataYoshi" => read(archive, ext, ExtAttrs::Yoshi),
        "ftDataPurin" => read(archive, ext, ExtAttrs::Purin),
        "ftDataMewtwo" => read(archive, ext, ExtAttrs::Mewtwo),
        "ftDataLuigi" => read(archive, ext, ExtAttrs::Luigi),
        "ftDataMars" | "ftDataEmblem" => read(archive, ext, ExtAttrs::Mars),
        "ftDataZelda" => read(archive, ext, ExtAttrs::Zelda),
        "ftDataGamewatch" => read(archive, ext, ExtAttrs::GameWatch),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::check_layout;

    /// `(fields, generated size, size from machine code)`.
    type Case = (&'static [(&'static str, u32, u32)], u32, u32);

    /// Copy sizes read from each `*_Init_LoadSpecialAttrs` copy loop in the
    /// 1.02 DOL (iterations x 8 bytes + trailing words).
    #[test]
    fn sizes_match_machine_code() {
        let cases: &[Case] = &[
            (MarioAttributes::FIELDS, MarioAttributes::SIZE, 0x84),
            (FoxAttributes::FIELDS, FoxAttributes::SIZE, 0xd4),
            (CaptainAttributes::FIELDS, CaptainAttributes::SIZE, 0x8c),
            (DonkeyAttributes::FIELDS, DonkeyAttributes::SIZE, 0x74),
            (KirbyAttributes::FIELDS, KirbyAttributes::SIZE, 0x424),
            (KoopaAttributes::FIELDS, KoopaAttributes::SIZE, 0xa0),
            (LinkAttributes::FIELDS, LinkAttributes::SIZE, 0xdc),
            (SeakAttributes::FIELDS, SeakAttributes::SIZE, 0x74),
            (NessAttributes::FIELDS, NessAttributes::SIZE, 0xdc),
            (PeachAttributes::FIELDS, PeachAttributes::SIZE, 0xc0),
            (
                IceClimberAttributes::FIELDS,
                IceClimberAttributes::SIZE,
                0x15c,
            ),
            (PikachuAttributes::FIELDS, PikachuAttributes::SIZE, 0xf8),
            (SamusAttributes::FIELDS, SamusAttributes::SIZE, 0xd4),
            (YoshiAttributes::FIELDS, YoshiAttributes::SIZE, 0x138),
            (PurinAttributes::FIELDS, PurinAttributes::SIZE, 0x100),
            (MewtwoAttributes::FIELDS, MewtwoAttributes::SIZE, 0x88),
            (LuigiAttributes::FIELDS, LuigiAttributes::SIZE, 0x98),
            (MarsAttributes::FIELDS, MarsAttributes::SIZE, 0x98),
            (ZeldaAttributes::FIELDS, ZeldaAttributes::SIZE, 0xa8),
            (GameWatchAttributes::FIELDS, GameWatchAttributes::SIZE, 0x94),
        ];
        for &(fields, size, machine) in cases {
            assert_eq!(size, machine);
            check_layout(fields, size);
        }
    }
}
