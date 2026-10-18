//! Port of `src/sysdolphin/baselib/random.c`.
//!
//! The original keeps the seed behind a global pointer (`HSD_RandSeedPtr`);
//! `gmmain.c` seeds it from `OSGetTick()`. The port holds the seed as explicit
//! state so it can be saved, restored, and agreed on between peers.

/// The game's linear congruential generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Random {
    pub seed: u32,
}

impl Default for Random {
    /// `static u32 seed = 1;`
    fn default() -> Self {
        Self { seed: 1 }
    }
}

impl Random {
    pub const fn new(seed: u32) -> Self {
        Self { seed }
    }

    fn step(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(214013).wrapping_add(2531011);
        self.seed
    }

    /// `HSD_Rand` (0x803804F8): the high 16 bits of the next seed.
    pub fn rand(&mut self) -> i32 {
        (self.step() >> 16) as i32
    }

    /// `HSD_Randf` (0x80380528): in `[0, 1)`, with 16 bits of resolution.
    pub fn randf(&mut self) -> f32 {
        (self.step() >> 16) as f32 / 65536.0
    }

    /// `HSD_Randi` (0x80380580): `max_val * HSD_Rand() / 65536`.
    /// The multiply wraps (`mullw`) and the divide truncates toward zero
    /// (`srawi` + `addze`), as in the original machine code.
    pub fn randi(&mut self, max_val: i32) -> i32 {
        max_val.wrapping_mul(self.rand()) / (1 << 16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_sequence() {
        // seed 1 -> 1 * 214013 + 2531011 = 2745024 (0x29E2C0) -> high half 41,
        // the same first value as MSVC's rand(), which shares these constants.
        let mut rng = Random::default();
        assert_eq!(rng.rand(), 41);
        assert_eq!(rng.seed, 2_745_024);
        // Next: 2745024 * 214013 + 2531011 mod 2^32.
        let expected = 2_745_024u32.wrapping_mul(214_013).wrapping_add(2_531_011);
        assert_eq!(rng.rand(), (expected >> 16) as i32);
    }

    #[test]
    fn randi_and_randf_ranges() {
        let mut rng = Random::new(0xdead_beef);
        for _ in 0..10_000 {
            let i = rng.randi(7);
            assert!((0..7).contains(&i));
            let f = rng.randf();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn randi_wraps_like_mullw() {
        // Choose the seed whose successor is 0xFFFF0000, so HSD_Rand() returns
        // 0xFFFF and 0x7FFFFFFF * 0xFFFF overflows s32.
        let seed = 0xffff_0000u32
            .wrapping_sub(2_531_011)
            .wrapping_mul(inverse(214_013));
        let mut rng = Random::new(seed);
        let max_val = 0x7fff_ffff;
        assert_eq!(rng.randi(max_val), max_val.wrapping_mul(0xffff) / 65536);
        assert_eq!(rng.seed, 0xffff_0000);
    }

    /// Multiplicative inverse of an odd number modulo 2^32 (Newton iteration).
    fn inverse(a: u32) -> u32 {
        let mut x = a;
        for _ in 0..5 {
            x = x.wrapping_mul(2u32.wrapping_sub(a.wrapping_mul(x)));
        }
        assert_eq!(a.wrapping_mul(x), 1);
        x
    }
}
