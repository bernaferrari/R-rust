//! The pure world's session-owned random stream — GNU's Mersenne
//! Twister with the exact seeding scramble, tempering, and inversion
//! normal generation, ported operation-for-operation from the old
//! world's rng_core so streams reproduce GNU byte-exactly. The state
//! lives on the heap: no thread-locals, no raw pointers, no install
//! dance — ownership is plain &mut exactly as the architecture doc's
//! RngState discipline.

/// GNU's fixup: uniforms never land exactly on 0 or 1.
fn fixup(x: f64) -> f64 {
    const I2_32M1: f64 = 2.328306437080797e-10; // 1/(2^32 - 1)
    if x <= 0.0 {
        return 0.5 * I2_32M1;
    }
    if (1.0 - x) <= 0.0 {
        return 1.0 - 0.5 * I2_32M1;
    }
    x
}

const MT_N: usize = 624;
const MT_M: usize = 397;
const MT_MATRIX_A: u32 = 0x9908b0df;
const MT_UPPER_MASK: u32 = 0x80000000;
const MT_LOWER_MASK: u32 = 0x7fffffff;
const MT_TEMPERING_MASK_B: u32 = 0x9d2c5680;
const MT_TEMPERING_MASK_C: u32 = 0xefc60000;
const BIG: f64 = 134217728.0; // 2^27

/// The Mersenne-Twister stream exactly as GNU carries it: 624 working
/// words plus the twist index. `mti == MT_N + 1` is GNU's uninitialized
/// sentinel — the first draw seeds 4357 exactly as mt_genrand does.
#[derive(Debug, Clone)]
pub struct PureRng {
    mt: [u32; MT_N],
    mti: usize,
}

impl Default for PureRng {
    fn default() -> Self {
        PureRng {
            mt: [0; MT_N],
            mti: MT_N + 1,
        }
    }
}

impl PureRng {
    /// GNU's mt_sgenrand: the 69069 interleave filling the words.
    fn sgenrand(&mut self, seed: u32) {
        let mut s = seed;
        for word in self.mt.iter_mut() {
            *word = s & 0xffff0000;
            s = 69069u32.wrapping_mul(s).wrapping_add(1);
            *word |= (s & 0xffff0000) >> 16;
            s = 69069u32.wrapping_mul(s).wrapping_add(1);
        }
    }

    /// set.seed exactly as RNG_Init for the Mersenne-Twister kind:
    /// fifty scramble iterations, the fill continuing the scramble,
    /// and FixupSeeds' mti reset — the all-zero guard included.
    pub fn set_seed(&mut self, seed: i64) {
        let mut s = seed as i32;
        for _ in 0..50 {
            s = s.wrapping_mul(69069).wrapping_add(1);
        }
        // The fill covers mti's slot first exactly as GNU fills
        /// i_seed[0..=624] before FixupSeeds resets the index.
        let mut fill = [0u32; MT_N + 1];
        for word in fill.iter_mut() {
            s = s.wrapping_mul(69069).wrapping_add(1);
            *word = s as u32;
        }
        self.mt.copy_from_slice(&fill[1..]);
        self.mti = MT_N;
        // FixupSeeds' all-zero guard reinitializes; the scramble never
        /// produces it, but mirror the check exactly.
        if self.mt.iter().all(|w| *w == 0) {
            self.mti = MT_N + 1;
        }
    }

    /// One uniform in [0,1) exactly as mt_genrand + fixup: the twist
    /// runs when the index exhausts, tempering is the four-step xor
    /// cascade, and the scale constant is 1/2^32 exactly as GNU's
    /// (note: not the fixup constant — the asymmetry is GNU's own).
    pub fn unif(&mut self) -> f64 {
        if self.mti >= MT_N {
            if self.mti == MT_N + 1 {
                self.sgenrand(4357);
            }
            let mt = &mut self.mt;
            let mut kk = 0usize;
            while kk < MT_N - MT_M {
                let y = (mt[kk] & MT_UPPER_MASK) | (mt[kk + 1] & MT_LOWER_MASK);
                mt[kk] = mt[kk + MT_M] ^ (y >> 1) ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
                kk += 1;
            }
            while kk < MT_N - 1 {
                let y = (mt[kk] & MT_UPPER_MASK) | (mt[kk + 1] & MT_LOWER_MASK);
                mt[kk] = mt[kk.wrapping_add(MT_M).wrapping_sub(MT_N)]
                    ^ (y >> 1)
                    ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
                kk += 1;
            }
            let y = (mt[MT_N - 1] & MT_UPPER_MASK) | (mt[0] & MT_LOWER_MASK);
            mt[MT_N - 1] = mt[MT_M - 1] ^ (y >> 1) ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
            self.mti = 0;
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & MT_TEMPERING_MASK_B;
        y ^= (y << 15) & MT_TEMPERING_MASK_C;
        y ^= y >> 18;
        fixup(y as f64 * 2.3283064365386963e-10)
    }

    /// One standard normal exactly as GNU's inversion default: two
    /// uniforms compose a 2^27-scaled fraction, refined through
    /// qnorm's inverse exactly as r_norm_rand's fallback branch.
    pub fn norm(&mut self) -> f64 {
        let mut u1 = self.unif();
        u1 = (BIG * u1) as i64 as f64 + self.unif();
        rmath_nmath::dist::normal::qnorm5_inner(u1 / BIG, 0.0, 1.0, true, false)
    }

    /// A uniform index in [0, dn) exactly as R_unif_index's rounding
    /// branch — rejection sampling belongs with sample's own slice.
    pub fn unif_index(&mut self, dn: f64) -> f64 {
        (dn * self.unif()).floor()
    }

    /// R_unif_index's default rejection branch exactly as GNU's rbits:
    /// 16-bit chunks assemble one uniform each until the bit width covers
    /// ceil(log2(dn)), values at or above dn rejected and redrawn. The
    /// stream consumption is one uniform per chunk exactly as random.c.
    pub fn unif_index_rejection(&mut self, dn: f64) -> f64 {
        if dn <= 0.0 {
            return 0.0;
        }
        let bits = dn.log2().ceil() as i32;
        loop {
            let mut v: u64 = 0;
            let mut n = 0;
            while n <= bits {
                let v1 = (self.unif() * 65536.0) as u64;
                v = 65536 * v + v1;
                n += 16;
            }
            let mask = (1u64 << (bits as u32)).wrapping_sub(1);
            let dv = (v & mask) as f64;
            if dn > dv {
                return dv;
            }
        }
    }
}
