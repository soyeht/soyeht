//! A small deterministic generator (SplitMix64) so the acting layer is reproducible: the same
//! seed gives the same blinks and glances in every implementation, which is what lets a vector
//! file test it.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let z = xor_shift_multiply(self.0, 30, 0xBF58_476D_1CE4_E5B9);
        let z = xor_shift_multiply(z, 27, 0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)` with 53 random bits.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }

    pub(crate) fn range_of(&mut self, bounds: [f64; 2]) -> f64 {
        self.range(bounds[0], bounds[1])
    }

    pub(crate) fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let i = (self.unit() * items.len() as f64) as usize;
        items[i.min(items.len() - 1)]
    }

    /// One of `(value, weight)` with probability proportional to its weight.
    pub(crate) fn pick_weighted<T: Copy>(&mut self, items: &[(T, f64)]) -> T {
        let mut r = self.unit() * items.iter().map(|(_, w)| w).sum::<f64>();
        for (value, weight) in items {
            r -= weight;
            if r <= 0.0 {
                return *value;
            }
        }
        items[0].0
    }
}

fn xor_shift_multiply(z: u64, shift: u32, factor: u64) -> u64 {
    (z ^ (z >> shift)).wrapping_mul(factor)
}
