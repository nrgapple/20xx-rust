//! Ports from `src/melee/lb` (the game's shared "library" layer). Only the
//! math parts so far: trigonometry (`lbtrigf.c`), vectors (`lbvector.c`),
//! and the numeric helpers in `lb_00CE.c`.

pub mod lb_00ce;
pub mod trigf;
pub mod vector;
