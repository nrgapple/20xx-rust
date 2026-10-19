//! Ports from `src/melee/ft` (fighters).

pub mod action;
pub mod common;
pub mod data;
pub mod ext_attrs;
mod ext_data;
pub mod fighter;
pub mod ft_081b;
pub mod input;
pub mod kinds;
pub mod motion;
mod motion_data;

use data::{CommonData, FighterAttributes};
use hsd_dat::{Archive, Result};

use crate::dat::DatField;

/// Load a fighter's `ftCo_DatAttrs` from its `Pl??.dat` archive (`data_symbol`
/// is the `ftData` root, e.g. `ftDataFox` in `PlFx.dat`), as
/// `Fighter_Create` copies `ftData::x0` into `Fighter::co_attrs`.
pub fn load_attributes(archive: &Archive, data_symbol: &str) -> Result<FighterAttributes> {
    let ft_data = archive.public(data_symbol)?;
    let attrs = archive.non_null_ptr(ft_data)?;
    FighterAttributes::read(archive, attrs)
}

/// Load `ftCommonData` from `PlCo.dat`, as `Fighter_LoadCommonData` does
/// (`ftLoadCommonData::common`, the first field).
pub fn load_common_data(archive: &Archive) -> Result<CommonData> {
    let root = archive.public("ftLoadCommonData")?;
    let common = archive.non_null_ptr(root)?;
    CommonData::read(archive, common)
}

#[cfg(test)]
mod tests {
    use super::data::*;
    use crate::dat::{check_layout, DatField};

    #[test]
    fn generated_layouts_are_consistent() {
        check_layout(FighterAttributes::FIELDS, FighterAttributes::SIZE);
        check_layout(FighterAttributesXbc::FIELDS, FighterAttributesXbc::SIZE);
        check_layout(CommonData::FIELDS, CommonData::SIZE);
        check_layout(FallCommon::FIELDS, FallCommon::SIZE);
        check_layout(HitDesc::FIELDS, HitDesc::SIZE);
        assert_eq!(FighterAttributes::SIZE, 0x184);
        assert_eq!(CommonData::SIZE, 0x818);
    }
}
