//! Ports from `src/MSL` (Metrowerks Standard Library math the game executes).
//!
//! Which implementations run, and how often they are called, is recorded in
//! `research/notes/math.md`. Each function was checked against the 1.02
//! machine code: by reading the disassembly, and by running the original
//! code in `ppc_asm::interp` against the port over many inputs
//! (`crates/melee/tests/math.rs`, which needs a local DOL or disc).

pub mod math_ppc;
pub mod trigf;

pub use math_ppc::{sqrtf, sqrtf_accurate};
pub use trigf::{cosf, sinf, tanf};
