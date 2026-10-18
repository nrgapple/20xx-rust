//! A hand port of Super Smash Bros. Melee NTSC 1.02 game logic to Rust.
//!
//! Modules mirror the decomp's layout (`src/melee/ft` → [`ft`],
//! `src/sysdolphin/baselib` → [`hsd`]). Porting rules:
//!
//! - Port from the matching decomp, keeping its control flow, operand order,
//!   and float types. Name each function's decomp symbol and 1.02 address.
//! - Audit each function's machine code with `dol-inspect` before porting.
//!   MWCC built game code with `-fp_contract on`, so `a * b + c` may compile
//!   to a fused `fmadds`; port fused operations as fused, unfused as separate
//!   `f32` operations. (`f32` `+ - * /` match Gekko single ops exactly.)
//! - Use [`math::abs_c`] where the C uses `ABS`.
//! - No globals: state the game keeps in globals (RNG seed, `p_ftCommonData`)
//!   is passed explicitly, so a whole match can be saved, restored, and
//!   compared for rollback.
//! - Game data is read from the player's own disc image, never bundled.

pub mod dat;
pub mod ft;
pub mod gr;
pub mod hsd;
pub mod math;
pub mod mp;
