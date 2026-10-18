//! Ports from `src/melee/ft/ft_081B.c`.

use super::common::{calc_self_accel_drift, check_fall_fast, fall, fall_fast};
use super::data::CommonData;
use super::fighter::Fighter;

/// `ft_80084DB0` (0x80084DB0): the standard airborne physics callback used by
/// `ftCo_Fall_Phys` and others: fast-fall check, gravity or fast fall, then
/// stick drift. Audited: no fused multiply-adds.
pub fn ft_80084db0(fp: &mut Fighter, common: &CommonData) {
    check_fall_fast(fp, common);
    if fp.fall_fast {
        fall_fast(fp);
    } else {
        let (gravity, terminal) = (fp.co_attrs.gravity, fp.co_attrs.terminal_velocity);
        fall(fp, gravity, terminal);
    }
    calc_self_accel_drift(fp);
}
