//! Randomness from the game's seed. SplitMix64 gives the same numbers on every
//! platform, so a game started from a seed plays the same natively and in the
//! browser. Nothing in the rules reads the clock or an unseeded generator.

/// A SplitMix64 generator.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// A number in `0..n`; `n` must not be zero.
    pub fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n)) as u32
    }

    /// A number in `lo..=hi`.
    pub fn between(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// Fisher–Yates.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// SplitMix64's finaliser: spreads the bits of `z` over the whole word.
pub fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A hash of a tile under a seed, for values fixed per tile such as its trees.
pub fn tile_hash(seed: u64, x: i32, y: i32) -> u64 {
    mix(seed ^ mix(((x as u32 as u64) << 32) | y as u32 as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_for_a_seed() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(8).next_u64(), Rng::new(7).next_u64());
    }

    #[test]
    fn stays_in_range() {
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            assert!((-3..=4).contains(&rng.between(-3, 4)));
        }
    }
}
