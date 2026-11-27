//! Marsaglia-MultiCarry uniform generator state, ported from
//! standalone/sunif.c. Hosts thread explicit state through the sampler
//! APIs — the uniform source is a caller-supplied closure, so nothing in
//! this module is global or thread-local.

/// Marsaglia-MultiCarry generator state: two 32-bit seeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngState(pub u32, pub u32);

impl Default for RngState {
    fn default() -> Self {
        RngState(1234, 5678)
    }
}

impl RngState {
    /// Advance the stream once, yielding a uniform draw in [0, 1) exactly
    /// as standalone/sunif.c's multicarry step computes it.
    pub fn unif(&mut self) -> f64 {
        self.0 = 36969u32
            .wrapping_mul(self.0 & 0xFFFF)
            .wrapping_add(self.0 >> 16);
        self.1 = 18000u32
            .wrapping_mul(self.1 & 0xFFFF)
            .wrapping_add(self.1 >> 16);
        ((self.0 << 16) ^ (self.1 & 0xFFFF)) as f64 * 2.328306437080797e-10
    }
}
