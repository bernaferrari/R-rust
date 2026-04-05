//! Mersenne Twister MT19937 RNG.
//! Faithful port of R's src/main/RNG.c implementation.
//!
//! In R's C code, `mt = dummy + 1` and `i_seed = dummy`, so `mt[i] = i_seed[i+1]`.
//! `i_seed[0]` stores `mti`. The generic seed-filling loop writes to `i_seed[0..624]`,
//! then FixupSeeds overwrites `i_seed[0]` with N (=624). This means `mt[0..623]`
//! gets the values from scramblings 52..675 (after the 50 initial scramblings).

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908b0df;
const UPPER_MASK: u32 = 0x80000000;
const LOWER_MASK: u32 = 0x7fffffff;
const TEMPERING_B: u32 = 0x9d2c5680;
const TEMPERING_C: u32 = 0xefc60000;

pub struct MersenneTwister {
    mt: [u32; N],
    mti: usize,
}

impl MersenneTwister {
    pub fn new() -> Self {
        Self {
            mt: [0u32; N],
            mti: N + 1, // not initialized
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        // R's RNG_Init: 50 initial scramblings using signed 32-bit wrapping
        let mut seed = seed as i32;
        for _ in 0..50 {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        // In C, the generic loop fills i_seed[0..624] (625 values).
        // i_seed[0] = mti gets overwritten to N by FixupSeeds.
        // mt[0..623] = i_seed[1..624], which are scramblings 52..675.
        // So we skip one scramble (i_seed[0]) and fill mt directly.
        seed = seed.wrapping_mul(69069).wrapping_add(1); // skip i_seed[0]
        for i in 0..N {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
            self.mt[i] = seed as u32;
        }
        self.mti = N; // FixupSeeds sets mti = N
    }

    pub fn get_rand(&mut self) -> f64 {
        if self.mti >= N {
            if self.mti == N + 1 {
                // Not initialized, use default seed
                self.set_seed(4357);
            }
            self.generate();
        }

        let mut y = self.mt[self.mti];
        self.mti += 1;

        // Tempering
        y ^= y >> 11;
        y ^= (y << 7) & TEMPERING_B;
        y ^= (y << 15) & TEMPERING_C;
        y ^= y >> 18;

        y as f64 * 2.3283064365386963e-10
    }

    fn generate(&mut self) {
        let mag01 = [0u32, MATRIX_A];

        let mut kk = 0usize;
        while kk < N - M {
            let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
            self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            kk += 1;
        }
        while kk < N - 1 {
            let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
            self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            kk += 1;
        }
        let y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
        self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ mag01[(y & 1) as usize];

        self.mti = 0;
    }
}
