//! Ports from the Dolphin SDK (`libs/dolphin`) that game logic calls.
//!
//! The `PSVEC*`/`PSMTX*` routines are hand-written paired-single assembly;
//! each port replays the 1.02 machine code's dataflow with [`crate::ppc::Ps`]
//! (per-lane single rounding, fused `ps_madd`), including lanes that only
//! hold intermediate values. The SDK's C was built with `-fp_contract off`,
//! so its C functions (`C_MTXLookAt`) have no fused ops.

pub mod mtx;
pub mod vec;
