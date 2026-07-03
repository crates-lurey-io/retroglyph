//! Minimal linear congruential generator for example use.
#![allow(dead_code)] // not every example uses every item in this module
//!
//! Not suitable for cryptography or statistical sampling. Use it when an
//! example needs a seeded, deterministic sequence without pulling in a
//! heavy RNG dependency.

/// A 64-bit LCG using Knuth's multiplicative constants.
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// Create a new generator with the given seed. A zero seed is remapped to 1.
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Seed from the current wall-clock time, falling back to 42 on error.
    ///
    /// Not available on `wasm32` targets (no `SystemTime`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_time() -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(42, |d| d.as_nanos() as u64);
        Self::new(seed)
    }

    /// Advance the state and return the next value.
    pub const fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state >> 33
    }
}
