//! Deterministic pseudo-random number generation using SplitMix64
pub struct SplitMix64 {
    state: u64,
}

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

const MIX_1: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX_2: u64 = 0x94D0_49BB_1331_11EB;

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(MIX_1);
        z = (z ^ (z >> 27)).wrapping_mul(MIX_2);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_seed_gives_same_sequence() {
        let mut a = SplitMix64::new(12345);
        let mut b = SplitMix64::new(12345);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn test_different_seeds_diverge() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn test_no_short_cycle_or_constant_output() {
        let mut rng = SplitMix64::new(0);
        let values: Vec<u64> = (0..1024).map(|_| rng.next_u64()).collect();
        let mut sorted = values.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), values.len(), "repeated value in 1024 draws");
    }

    #[test]
    fn test_zero_seed_is_not_degenerate() {
        let mut rng = SplitMix64::new(0);
        assert_ne!(rng.next_u64(), 0);
        assert_ne!(rng.next_u64(), 0);
    }

    #[test]
    fn test_usable_in_const_context() {
        const KEYS: [u64; 4] = {
            let mut rng = SplitMix64::new(42);
            let mut out = [0; 4];
            let mut i = 0;
            while i < 4 {
                out[i] = rng.next_u64();
                i += 1;
            }
            out
        };
        let mut rng = SplitMix64::new(42);
        for expected in KEYS {
            assert_eq!(rng.next_u64(), expected);
        }
    }

    #[test]
    fn test_bit_distribution_is_balanced() {
        let mut rng = SplitMix64::new(0xDEAD_BEEF);
        let draws = 4096;
        let ones: u32 = (0..draws).map(|_| rng.next_u64().count_ones()).sum();
        let expected = draws * 32;
        let deviation = (ones as i64 - expected as i64).unsigned_abs();
        assert!(
            deviation < expected as u64 / 50,
            "bit population {ones} too far from {expected}",
        );
    }
}
