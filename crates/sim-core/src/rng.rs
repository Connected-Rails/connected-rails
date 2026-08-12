//! Deterministischer Zufallsgenerator (Plan 16.1) — gleicher Seed, gleicher Lauf.

use serde::{Deserialize, Serialize};

/// xorshift64*, ausreichend für Wetter, Störungen und Streuung von KI-Reaktionszeiten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Default for Rng {
    fn default() -> Self {
        Self::new(0x2545_F491_4F6C_DD1D)
    }
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Gleichverteilt in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Gleichverteilt in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = Rng::new(43);
        assert_ne!(a.next_u64(), c.next_u64());
    }

    #[test]
    fn values_stay_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.next_f64();
            assert!((0.0..1.0).contains(&v));
        }
    }
}
