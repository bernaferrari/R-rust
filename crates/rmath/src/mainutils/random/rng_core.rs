#![allow(unreachable_code)]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

pub(crate) fn error(msg: &str) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// RNG type enumerations (matching R_ext/Random.h)
// ---------------------------------------------------------------------------

/// RNG algorithm types.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RNGtype {
    WICHMANN_HILL = 0,
    MARSAGLIA_MULTICARRY = 1,
    SUPER_DUPER = 2,
    MERSENNE_TWISTER = 3,
    KNUTH_TAOCP = 4,
    USER_UNIF = 5,
    KNUTH_TAOCP2 = 6,
    LECUYER_CMRG = 7,
}

/// Normal variate generation method.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum N01type {
    BUGGY_KINDERMAN_RAMAGE = 0,
    AHRENS_DIETER = 1,
    BOX_MULLER = 2,
    USER_NORM = 3,
    INVERSION = 4,
    KINDERMAN_RAMAGE = 5,
}

/// Sampling method for discrete uniform.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampletype {
    ROUNDING = 0,
    REJECTION = 1,
}

/// Different kinds of "Bin(n,p)" generators (R_ext/Random.h's Binomtype).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binomtype {
    BUGGY_BTPE = 0,
    BTPE = 1,
}

type Int32 = u32; // unsigned 32-bit, matching R's typedef

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub(crate) const RNG_DEFAULT: RNGtype = RNGtype::MERSENNE_TWISTER;
pub(crate) const N01_DEFAULT: N01type = N01type::INVERSION;
pub(crate) const Sample_DEFAULT: Sampletype = Sampletype::REJECTION;
pub(crate) const Binom_DEFAULT: Binomtype = Binomtype::BTPE;

/// Threshold for using stack allocation vs heap allocation in Walker's method.
const SMALL: usize = 10000;

const I2_32M1: f64 = 2.328306437080797e-10; /* 1/(2^32 - 1) */
const KT: f64 = 9.31322574615479e-10; /* 2^-30 */

// Mersenne Twister constants
pub(crate) const MT_N: usize = 624;
const MT_M: usize = 397;
const MT_MATRIX_A: u32 = 0x9908b0df;
const MT_UPPER_MASK: u32 = 0x80000000;
const MT_LOWER_MASK: u32 = 0x7fffffff;
const MT_TEMPERING_MASK_B: u32 = 0x9d2c5680;
const MT_TEMPERING_MASK_C: u32 = 0xefc60000;

// Knuth TAOCP constants
const KT_KK: usize = 100; /* the long lag */
const KT_LL: usize = 37; /* the short lag */
const KT_MM: i64 = 1 << 30; /* the modulus */
const KT_TT: usize = 70; /* guaranteed separation between streams */
const KT_QUALITY: usize = 1009;

// L'Ecuyer-CMRG constants
const CMRG_M1: i64 = 4294967087;
const CMRG_M2: i64 = 4294944443;
const CMRG_NORMC: f64 = 2.328306549295727688e-10;
const CMRG_A12: i64 = 1403580;
const CMRG_A13N: i64 = 810728;
const CMRG_A21: i64 = 527612;
const CMRG_A23N: i64 = 1370589;

// ---------------------------------------------------------------------------
// RNG Table (matching R's RNG_Table)
// ---------------------------------------------------------------------------

pub(crate) struct RNGTab {
    pub(crate) kind: RNGtype,
    pub(crate) n_seed: usize,
    pub(crate) i_seed: Vec<Int32>,
}

/// Per-instance RNG state.
///
/// MT and Knuth-TAOCP scratch arrays mirror R's layout: the persisted seed
/// state lives in `rng_table[kind].i_seed` (so `.Random.seed` round-trips
/// through `PutRNGstate`/`GetRNGstate`); `kt_x`/`kt_ran_arr_*` hold the
/// Knuth working state that R also keeps outside `.Random.seed`.
pub(crate) struct RNGState {
    pub(crate) rng_kind: Cell<RNGtype>,
    pub(crate) n01_kind: Cell<N01type>,
    pub(crate) sample_kind: Cell<Sampletype>,
    pub(crate) binom_kind: Cell<Binomtype>,
    pub(crate) rng_table: [RNGTab; 8],
    // Knuth TAOCP state
    pub(crate) kt_x: [i64; KT_KK],
    pub(crate) kt_ran_arr_buf: [i64; KT_QUALITY],
    pub(crate) kt_ran_arr_ptr: Cell<usize>,
    pub(crate) kt_pos: Cell<usize>,
    // Box-Muller saved value
    pub(crate) bm_norm_keep: Cell<f64>,
    // Whether RNG_Init has run (stock randomizes lazily on first use)
    pub(crate) initialized: Cell<bool>,
}

impl RNGState {
    pub(crate) fn new() -> Self {
        let rng_table: [RNGTab; 8] = [
            RNGTab {
                kind: RNGtype::WICHMANN_HILL,
                n_seed: 3,
                i_seed: vec![0; 3],
            },
            RNGTab {
                kind: RNGtype::MARSAGLIA_MULTICARRY,
                n_seed: 2,
                i_seed: vec![0; 2],
            },
            RNGTab {
                kind: RNGtype::SUPER_DUPER,
                n_seed: 2,
                i_seed: vec![0; 2],
            },
            RNGTab {
                kind: RNGtype::MERSENNE_TWISTER,
                n_seed: 1 + MT_N,
                i_seed: vec![0; 1 + MT_N],
            },
            RNGTab {
                kind: RNGtype::KNUTH_TAOCP,
                n_seed: 1 + KT_KK,
                i_seed: vec![0; 1 + KT_KK],
            },
            RNGTab {
                kind: RNGtype::USER_UNIF,
                n_seed: 0,
                i_seed: vec![],
            },
            RNGTab {
                kind: RNGtype::KNUTH_TAOCP2,
                n_seed: 1 + KT_KK,
                i_seed: vec![0; 1 + KT_KK],
            },
            RNGTab {
                kind: RNGtype::LECUYER_CMRG,
                n_seed: 6,
                i_seed: vec![0; 6],
            },
        ];

        let mut state = RNGState {
            rng_kind: Cell::new(RNG_DEFAULT),
            n01_kind: Cell::new(N01_DEFAULT),
            sample_kind: Cell::new(Sample_DEFAULT),
            binom_kind: Cell::new(Binom_DEFAULT),
            rng_table,
            kt_x: [0i64; KT_KK],
            kt_ran_arr_buf: [0i64; KT_QUALITY],
            kt_ran_arr_ptr: Cell::new(0),
            kt_pos: Cell::new(100),
            bm_norm_keep: Cell::new(0.0),
            initialized: Cell::new(false),
        };
        // Initialize MT index
        state.rng_table[3].i_seed[0] = MT_N as u32; // mti = N+1 means not initialized
        // Initialize KT_pos
        state.rng_table[4].i_seed[KT_KK] = 100; // KT_pos
        state.rng_table[6].i_seed[KT_KK] = 100; // KT_pos for TAOCP2
        state
    }
}

pub(crate) fn with_rng_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut RNGState) -> R,
{
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        f(&mut (*instance).random_state)
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Ensure 0 and 1 are never returned from unif_rand.
pub(crate) fn fixup(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.5 * I2_32M1;
    }
    if (1.0 - x) <= 0.0 {
        return 1.0 - 0.5 * I2_32M1;
    }
    x
}

/// Modular subtraction for Knuth TAOCP.
#[inline(always)]
fn kt_mod_diff(x: i64, y: i64) -> i64 {
    (x.wrapping_sub(y)) & (KT_MM - 1)
}

fn is_odd(x: i64) -> bool {
    (x & 1) != 0
}

// ---------------------------------------------------------------------------
// Mersenne Twister implementation
// ---------------------------------------------------------------------------

/// MT working words: `i_seed[1 ..= 624]`; `i_seed[0]` holds `mti`.
///
/// Keeping the state in `i_seed` (like R's shared `dummy` array) makes
/// `.Random.seed` round-trips exact: `mt_genrand` mutations are visible to
/// `PutRNGstate` without a separate sync.
fn mt_words(rng: &mut RNGState) -> &mut [u32] {
    let kind = rng.rng_kind.get() as usize;
    &mut rng.rng_table[kind].i_seed[1..1 + MT_N]
}

fn mt_sgenrand(mt: &mut [u32], seed: u32) {
    let mut s = seed;
    for word in mt.iter_mut() {
        *word = s & 0xffff0000;
        s = 69069u32.wrapping_mul(s).wrapping_add(1);
        *word |= (s & 0xffff0000) >> 16;
        s = 69069u32.wrapping_mul(s).wrapping_add(1);
    }
}

fn mt_genrand(rng: &mut RNGState) -> f64 {
    let rng_kind = rng.rng_kind.get();
    let mut mti = rng.rng_table[rng_kind as usize].i_seed[0] as usize;

    if mti >= MT_N {
        if mti == MT_N + 1 {
            // Not initialized, use default seed
            mt_sgenrand(mt_words(rng), 4357);
        }

        // Generate N words at one time
        let mt = mt_words(rng);
        let mut kk = 0usize;
        while kk < MT_N - MT_M {
            let y = (mt[kk] & MT_UPPER_MASK) | (mt[kk + 1] & MT_LOWER_MASK);
            mt[kk] = mt[kk + MT_M] ^ (y >> 1) ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
            kk += 1;
        }
        while kk < MT_N - 1 {
            let y = (mt[kk] & MT_UPPER_MASK) | (mt[kk + 1] & MT_LOWER_MASK);
            // M - N = 397 - 624 = -227, which wraps in unsigned arithmetic
            mt[kk] = mt[kk.wrapping_add(MT_M).wrapping_sub(MT_N)]
                ^ (y >> 1)
                ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
            kk += 1;
        }
        let y = (mt[MT_N - 1] & MT_UPPER_MASK) | (mt[0] & MT_LOWER_MASK);
        mt[MT_N - 1] = mt[MT_M - 1] ^ (y >> 1) ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };

        mti = 0;
        rng.rng_table[rng_kind as usize].i_seed[0] = 0; // mti = 0
    }

    let mt = mt_words(rng);
    let mut y = mt[mti];
    rng.rng_table[rng_kind as usize].i_seed[0] += 1;
    y ^= y >> 11;
    y ^= (y << 7) & MT_TEMPERING_MASK_B;
    y ^= (y << 15) & MT_TEMPERING_MASK_C;
    y ^= y >> 18;

    y as f64 * 2.3283064365386963e-10
}

// ---------------------------------------------------------------------------
// Knuth TAOCP implementation
// ---------------------------------------------------------------------------

fn kt_ran_array(rng: &mut RNGState, aa: &mut [i64], n: usize) {
    let mut j = 0usize;
    while j < KT_KK && j < n {
        aa[j] = rng.kt_x[j];
        j += 1;
    }
    while j < n {
        aa[j] = kt_mod_diff(aa[j - KT_KK], aa[j - KT_LL]);
        j += 1;
    }
    let mut i = 0usize;
    let mut j = KT_KK;
    while i < KT_LL && j < n {
        rng.kt_x[i] = kt_mod_diff(aa[j - KT_KK], aa[j - KT_LL]);
        i += 1;
        j += 1;
    }
    while i < KT_KK && j < n {
        rng.kt_x[i] = kt_mod_diff(aa[j - KT_KK], rng.kt_x[i - KT_LL]);
        i += 1;
        j += 1;
    }
}

fn kt_ran_arr_cycle(rng: &mut RNGState) {
    let mut aa = [0i64; KT_QUALITY];
    kt_ran_array(rng, &mut aa, KT_QUALITY);
    aa[KT_KK] = -1;
    rng.kt_ran_arr_ptr.set(1);
    rng.kt_ran_arr_buf.copy_from_slice(&aa);
}

fn kt_next(rng: &mut RNGState) -> i64 {
    if rng.kt_pos.get() >= 100 {
        kt_ran_arr_cycle(rng);
        rng.kt_pos.set(0);
    }
    let pos = rng.kt_pos.get();
    rng.kt_pos.set(pos + 1);
    rng.kt_x[pos]
}

fn ran_start(rng: &mut RNGState, seed: i64) {
    let mut x = [0i64; KT_KK + KT_KK - 1];
    let mut ss = (seed + 2) & (KT_MM - 2);
    for j in 0..KT_KK {
        x[j] = ss;
        ss <<= 1;
        if ss >= KT_MM {
            ss -= KT_MM - 2;
        }
    }
    x[1] += 1; // make x[1] (and only x[1]) odd

    let mut ss = seed & (KT_MM - 1);
    let mut t = KT_TT;
    while t > 0 {
        for j in (1..KT_KK).rev() {
            x[j + j] = x[j];
            x[j + j - 1] = 0;
        }
        for j in (KT_KK..KT_KK + KT_KK - 1).rev() {
            x[j - (KT_KK - KT_LL)] = kt_mod_diff(x[j - (KT_KK - KT_LL)], x[j]);
            x[j - KT_KK] = kt_mod_diff(x[j - KT_KK], x[j]);
        }
        if is_odd(ss) {
            for j in (1..=KT_KK).rev() {
                x[j] = x[j - 1];
            }
            x[0] = x[KT_KK];
            x[KT_LL] = kt_mod_diff(x[KT_LL], x[KT_KK]);
        }
        if ss != 0 {
            ss >>= 1;
        } else {
            t -= 1;
        }
    }

    for j in 0..KT_LL {
        rng.kt_x[j + KT_KK - KT_LL] = x[j];
    }
    rng.kt_x[..(KT_KK - KT_LL)].copy_from_slice(&x[KT_LL..KT_KK]);
    // Warm things up
    for _ in 0..10 {
        kt_ran_array(rng, &mut x, KT_KK + KT_KK - 1);
    }
    rng.kt_ran_arr_ptr.set(KT_QUALITY); // sentinel
}

// ---------------------------------------------------------------------------
// FixupSeeds -- depending on RNG, set 0 values to non-0, etc.
// ---------------------------------------------------------------------------

fn FixupSeeds(rng: &mut RNGState, kind: RNGtype, initial: bool) {
    match kind {
        RNGtype::WICHMANN_HILL => {
            let table = &mut rng.rng_table[kind as usize];
            table.i_seed[0] %= 30269;
            table.i_seed[1] %= 30307;
            table.i_seed[2] %= 30323;
            if table.i_seed[0] == 0 {
                table.i_seed[0] = 1;
            }
            if table.i_seed[1] == 0 {
                table.i_seed[1] = 1;
            }
            if table.i_seed[2] == 0 {
                table.i_seed[2] = 1;
            }
        }
        RNGtype::SUPER_DUPER => {
            let table = &mut rng.rng_table[kind as usize];
            if table.i_seed[0] == 0 {
                table.i_seed[0] = 1;
            }
            table.i_seed[1] |= 1; // must be odd
        }
        RNGtype::MARSAGLIA_MULTICARRY => {
            let table = &mut rng.rng_table[kind as usize];
            if table.i_seed[0] == 0 {
                table.i_seed[0] = 1;
            }
            if table.i_seed[1] == 0 {
                table.i_seed[1] = 1;
            }
        }
        RNGtype::MERSENNE_TWISTER => {
            let table = &mut rng.rng_table[kind as usize];
            if initial {
                table.i_seed[0] = MT_N as u32;
            }
            if table.i_seed[0] == 0 {
                table.i_seed[0] = MT_N as u32;
            }
            let mut notallzero = false;
            for j in 1..=624 {
                if table.i_seed[j] != 0 {
                    notallzero = true;
                    break;
                }
            }
            if !notallzero {
                RNG_Init(rng, kind, TimeToSeed() as i64);
            }
        }
        RNGtype::KNUTH_TAOCP | RNGtype::KNUTH_TAOCP2 => {
            if rng.kt_pos.get() == 0 {
                rng.kt_pos.set(100);
            }
            let table = &rng.rng_table[kind as usize];
            let mut notallzero = false;
            for j in 0..KT_KK {
                if table.i_seed[j] != 0 {
                    notallzero = true;
                    break;
                }
            }
            if !notallzero {
                RNG_Init(rng, kind, TimeToSeed() as i64);
            }
        }
        RNGtype::USER_UNIF => {}
        RNGtype::LECUYER_CMRG => {
            let table = &rng.rng_table[kind as usize];
            let mut notallzero = false;
            let mut all_ok = true;
            for j in 0..3 {
                let tmp = table.i_seed[j];
                if tmp != 0 {
                    notallzero = true;
                }
                if tmp as u64 >= CMRG_M1 as u64 {
                    all_ok = false;
                }
            }
            if !notallzero || !all_ok {
                RNG_Init(rng, kind, TimeToSeed() as i64);
                return;
            }
            notallzero = false;
            all_ok = true;
            for j in 3..6 {
                let tmp = table.i_seed[j];
                if tmp != 0 {
                    notallzero = true;
                }
                if tmp as u64 >= CMRG_M2 as u64 {
                    all_ok = false;
                }
            }
            if !notallzero || !all_ok {
                RNG_Init(rng, kind, TimeToSeed() as i64);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RNG_Init -- initialize an RNG with a seed
// ---------------------------------------------------------------------------

pub(crate) fn RNG_Init(rng: &mut RNGState, kind: RNGtype, seed: i64) {
    rng.initialized.set(true);
    rng.bm_norm_keep.set(0.0); // zap Box-Muller history

    // Initial scrambling (C uses int which wraps at 32 bits)
    let mut seed = seed as i32;
    for _ in 0..50 {
        seed = seed.wrapping_mul(69069).wrapping_add(1);
    }

    match kind {
        RNGtype::WICHMANN_HILL
        | RNGtype::MARSAGLIA_MULTICARRY
        | RNGtype::SUPER_DUPER
        | RNGtype::MERSENNE_TWISTER => {
            // i_seed[0] is mti for MT, but the chain still fills it for
            // historical consistency; FixupSeeds(initial) resets it below.
            let n_seed = rng.rng_table[kind as usize].n_seed;
            for j in 0..n_seed {
                seed = seed.wrapping_mul(69069).wrapping_add(1);
                rng.rng_table[kind as usize].i_seed[j] = seed as u32;
            }
            FixupSeeds(rng, kind, true);
        }
        RNGtype::KNUTH_TAOCP => {
            RNG_Init_KT(rng, seed as i64);
        }
        RNGtype::KNUTH_TAOCP2 => {
            RNG_Init_KT2(rng, seed as i64);
        }
        RNGtype::LECUYER_CMRG => {
            let n_seed = rng.rng_table[kind as usize].n_seed;
            let mut seed = seed as i32;
            for j in 0..n_seed {
                seed = seed.wrapping_mul(69069).wrapping_add(1);
                // Int32 is unsigned: compare the raw bit pattern against m2.
                while (seed as u32) as i64 >= CMRG_M2 {
                    seed = seed.wrapping_mul(69069).wrapping_add(1);
                }
                rng.rng_table[kind as usize].i_seed[j] = seed as u32;
            }
        }
        RNGtype::USER_UNIF => {
            // R errors when no user-supplied generator is registered.
            error("'user_unif_rand' not in load table");
        }
    }
}

fn RNG_Init_KT(rng: &mut RNGState, seed: i64) {
    let s = seed.rem_euclid(1073741821);
    ran_start(rng, s);
    rng.kt_pos.set(100);
    sync_kt_seeds(rng, RNGtype::KNUTH_TAOCP);
}

fn RNG_Init_KT2(rng: &mut RNGState, seed: i64) {
    let s = seed.rem_euclid(1073741821);
    ran_start(rng, s);
    rng.kt_pos.set(100);
    sync_kt_seeds(rng, RNGtype::KNUTH_TAOCP2);
}

/// Mirror the Knuth working state into `i_seed` so `.Random.seed` carries the
/// live x array and KT_pos, exactly as R's shared `dummy` backing store does.
fn sync_kt_seeds(rng: &mut RNGState, kind: RNGtype) {
    let table = &mut rng.rng_table[kind as usize];
    for j in 0..KT_KK {
        table.i_seed[j] = rng.kt_x[j] as u32;
    }
    table.i_seed[KT_KK] = rng.kt_pos.get() as u32;
}

// ---------------------------------------------------------------------------
// R-level unif_rand() -- dispatches by RNG kind
// ---------------------------------------------------------------------------

pub fn r_unif_rand() -> f64 {
    with_rng_state(|rng| {
        let kind = rng.rng_kind.get();
        match kind {
            RNGtype::WICHMANN_HILL => {
                let table = &mut rng.rng_table[kind as usize];
                let i1 = table.i_seed[0].wrapping_mul(171) % 30269;
                let i2 = table.i_seed[1].wrapping_mul(172) % 30307;
                let i3 = table.i_seed[2].wrapping_mul(170) % 30323;
                table.i_seed[0] = i1;
                table.i_seed[1] = i2;
                table.i_seed[2] = i3;
                let value = i1 as f64 / 30269.0 + i2 as f64 / 30307.0 + i3 as f64 / 30323.0;
                fixup(value - (value as i64 as f64))
            }
            RNGtype::MARSAGLIA_MULTICARRY => {
                let table = &mut rng.rng_table[kind as usize];
                let i1 = table.i_seed[0];
                let i2 = table.i_seed[1];
                let new_i1 = 36969u32.wrapping_mul(i1 & 0xFFFF).wrapping_add(i1 >> 16);
                let new_i2 = 18000u32.wrapping_mul(i2 & 0xFFFF).wrapping_add(i2 >> 16);
                table.i_seed[0] = new_i1;
                table.i_seed[1] = new_i2;
                fixup(((new_i1 << 16) ^ (new_i2 & 0xFFFF)) as f64 * I2_32M1)
            }
            RNGtype::SUPER_DUPER => {
                let table = &mut rng.rng_table[kind as usize];
                let i1 = table.i_seed[0];
                let i2 = table.i_seed[1];
                let new_i1 = (i1 ^ ((i1 >> 15) & 0xFFFF)) ^ (i1 << 17);
                let new_i2 = i2.wrapping_mul(69069);
                table.i_seed[0] = new_i1;
                table.i_seed[1] = new_i2;
                fixup((new_i1 ^ new_i2) as f64 * I2_32M1)
            }
            RNGtype::MERSENNE_TWISTER => fixup(mt_genrand(rng)),
            RNGtype::KNUTH_TAOCP | RNGtype::KNUTH_TAOCP2 => fixup(kt_next(rng) as f64 * KT),
            RNGtype::LECUYER_CMRG => {
                let table = &mut rng.rng_table[kind as usize];
                let i0 = table.i_seed[0];
                let i1 = table.i_seed[1];
                let i2 = table.i_seed[2];
                let i3 = table.i_seed[3];
                let i4 = table.i_seed[4];
                let i5 = table.i_seed[5];

                let p1 = CMRG_A12 * (i1 as i64) - CMRG_A13N * (i0 as i64);
                let k = (p1 / CMRG_M1) as i64;
                let mut p1 = p1 - k * CMRG_M1;
                if p1 < 0 {
                    p1 += CMRG_M1;
                }

                let p2 = CMRG_A21 * (i5 as i64) - CMRG_A23N * (i3 as i64);
                let k = (p2 / CMRG_M2) as i64;
                let mut p2 = p2 - k * CMRG_M2;
                if p2 < 0 {
                    p2 += CMRG_M2;
                }

                table.i_seed[0] = i1;
                table.i_seed[1] = i2;
                table.i_seed[2] = p1 as u32;
                table.i_seed[3] = i4;
                table.i_seed[4] = i5;
                table.i_seed[5] = p2 as u32;

                let result = if p1 > p2 { p1 - p2 } else { p1 - p2 + CMRG_M1 };
                result as f64 * CMRG_NORMC
            }
            RNGtype::USER_UNIF => 0.0,
        }
    })
}

// ---------------------------------------------------------------------------
// R-level norm_rand() -- dispatches by N01 kind
// ---------------------------------------------------------------------------

pub fn r_norm_rand() -> f64 {
    use crate::dist::normal::qnorm5_inner;
    const BIG: f64 = 134217728.0; /* 2^27 */

    let kind = with_rng_state(|rng| rng.n01_kind.get());
    match kind {
        N01type::BOX_MULLER => {
            let bm = with_rng_state(|rng| rng.bm_norm_keep.get());
            if bm != 0.0 {
                with_rng_state(|rng| rng.bm_norm_keep.set(0.0));
                bm
            } else {
                let u1 = r_unif_rand();
                let u2 = r_unif_rand();
                let radius = (-2.0 * u1.ln()).sqrt();
                let theta = 2.0 * std::f64::consts::PI * u2;
                with_rng_state(|rng| rng.bm_norm_keep.set(radius * theta.sin()));
                radius * theta.cos()
            }
        }
        _ => {
            // INVERSION, BUGGY_KINDERMAN_RAMAGE, AHRENS_DIETER,
            // KINDERMAN_RAMAGE, USER_NORM all fall back to inversion
            let mut u1 = r_unif_rand();
            u1 = (BIG * u1) as i64 as f64 + r_unif_rand();
            qnorm5_inner(u1 / BIG, 0.0, 1.0, true, false)
        }
    }
}

// ---------------------------------------------------------------------------
// R_unif_index -- generate random index in [0, dn)
// ---------------------------------------------------------------------------

/// Our PRNGs have at most 32 bit of precision. This provides a higher
/// precision uniform for use with R_unif_index.
fn ru() -> f64 {
    let u = 33554432.0;
    (u * r_unif_rand()).floor() + r_unif_rand()
}

fn r_unif_index_0(dn: f64) -> f64 {
    let cut = with_rng_state(|rng| {
        let kind = rng.rng_kind.get();
        match kind {
            RNGtype::KNUTH_TAOCP | RNGtype::USER_UNIF | RNGtype::KNUTH_TAOCP2 => 33554431.0_f64,
            _ => i32::MAX as f64,
        }
    });
    let u = if dn > cut { ru() } else { r_unif_rand() };
    (dn * u).floor()
}

/// Generate a random non-negative integer < 2^bits in 16-bit chunks.
fn rbits(bits: i32) -> f64 {
    let mut v: u64 = 0;
    let mut n = 0;
    while n <= bits {
        let v1 = (r_unif_rand() * 65536.0) as u64;
        v = 65536 * v + v1;
        n += 16;
    }
    let mask = (1u64 << (bits as u32)) - 1;
    (v & mask) as f64
}

pub fn r_R_unif_index(dn: f64) -> f64 {
    let sample_kind = with_rng_state(|rng| rng.sample_kind.get());
    if sample_kind == Sampletype::ROUNDING {
        return r_unif_index_0(dn);
    }
    if dn <= 0.0 {
        return 0.0;
    }
    let bits = (dn.log2().ceil()) as i32;
    loop {
        let dv = rbits(bits);
        if dn > dv {
            return dv;
        }
    }
}

pub fn r_sample_kind() -> Sampletype {
    with_rng_state(|rng| rng.sample_kind.get())
}

pub fn r_binom_kind() -> Binomtype {
    with_rng_state(|rng| rng.binom_kind.get())
}

// ---------------------------------------------------------------------------
// GetRNGstate / PutRNGstate
// ---------------------------------------------------------------------------

/// Issue a call-less warning, like R's `warning(...)`.
///
/// Records it in the session warning state (stock semantics) and renders it
/// immediately, mirroring the port's `warning()` builtin so script output
/// shows RNG warnings the way stock R does.
pub(crate) fn warn(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe { crate::mainutils::errors::Rf_warning1(c_msg.as_ptr()) };
    let message = format!("Warning message:\n{} \n", msg);
    unsafe {
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stderr(&message);
        } else {
            eprint!("{message}");
        }
    }
}

fn type_to_char(x: SEXP) -> &'static str {
    let t = unsafe { TYPEOF(x) };
    if t == SEXPTYPE::REALSXP.as_c_int() {
        "double"
    } else if t == SEXPTYPE::STRSXP.as_c_int() {
        "character"
    } else if t == SEXPTYPE::LGLSXP.as_c_int() {
        "logical"
    } else if t == SEXPTYPE::VECSXP.as_c_int() {
        "list"
    } else if t == SEXPTYPE::CPLXSXP.as_c_int() {
        "complex"
    } else if t == SEXPTYPE::RAWSXP.as_c_int() {
        "raw"
    } else {
        "unknown"
    }
}

/// Get the .Random.seed into proper variables.
///
/// Port of R's `GetRNGstate`: loads kind codes and the engine state from the
/// global `.Random.seed`; a missing or corrupt seed falls back to defaults
/// with a time-based seed (and a warning + write-back for corrupt values).
pub unsafe fn GetRNGstate() {
    unsafe {
        let seeds_sym = R_SeedsSymbol();
        let seeds = R_findVarInFrame(R_GlobalEnv(), seeds_sym);

        if seeds == R_UnboundValue() {
            // No .Random.seed -- randomize, then bind it. GNU PutRNGstate
            // runs before the first draw so find() sees .GlobalEnv.
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, TimeToSeed() as i64);
            });
            PutRNGstate();
            return;
        }

        if TYPEOF(seeds) == SEXPTYPE::PROMSXP {
            // R evaluates the promise; randomizing keeps the stream local
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, TimeToSeed() as i64);
            });
            return;
        }

        // Corrupt .Random.seed: warn, fall back to the defaults, randomize,
        // and write the restored state back out (R's GetRNGkind invalid path).
        macro_rules! invalid {
            ($msg:expr) => {{
                warn(&$msg);
                with_rng_state(|rng| {
                    rng.rng_kind.set(RNG_DEFAULT);
                    rng.n01_kind.set(N01_DEFAULT);
                    rng.sample_kind.set(Sample_DEFAULT);
                    rng.binom_kind.set(Binom_DEFAULT);
                    RNG_Init(rng, RNG_DEFAULT, TimeToSeed() as i64);
                });
                PutRNGstate();
                return;
            }};
        }

        if TYPEOF(seeds) != SEXPTYPE::INTSXP {
            invalid!(format!(
                "'.Random.seed' is not an integer vector but of type '{}', so ignored",
                type_to_char(seeds)
            ));
        }

        let is = INTEGER(seeds);
        let tmp = *is.add(0);
        // avoid overflow here: max current value is 110507
        if tmp == NA_INTEGER || tmp < 0 || tmp > 111000 {
            invalid!("'.Random.seed[1]' is not a valid integer, so ignored");
        }

        let new_rng = tmp % 100;
        let new_n01 = (tmp % 10000) / 100;
        let new_sample = (tmp % 100000) / 10000;
        let new_binom = tmp / 100000;

        if new_n01 > 5 || new_sample > 1 || new_binom > 1 {
            invalid!("'.Random.seed[1]' is not a valid Normal | Sample | Binom type, so ignored");
        }

        let rng_kind = match new_rng {
            0 => RNGtype::WICHMANN_HILL,
            1 => RNGtype::MARSAGLIA_MULTICARRY,
            2 => RNGtype::SUPER_DUPER,
            3 => RNGtype::MERSENNE_TWISTER,
            4 => RNGtype::KNUTH_TAOCP,
            5 => {
                invalid!("'.Random.seed[1] %% 100 = 5' but no user-supplied generator, so ignored");
            }
            6 => RNGtype::KNUTH_TAOCP2,
            7 => RNGtype::LECUYER_CMRG,
            _ => {
                invalid!("'.Random.seed[1] %% 100' is not a valid RNG kind so ignored");
            }
        };

        let n01_kind = match new_n01 {
            0 => N01type::BUGGY_KINDERMAN_RAMAGE,
            1 => N01type::AHRENS_DIETER,
            2 => N01type::BOX_MULLER,
            3 => N01type::USER_NORM,
            4 => N01type::INVERSION,
            _ => N01type::KINDERMAN_RAMAGE,
        };

        let sample_kind = if new_sample == 0 {
            Sampletype::ROUNDING
        } else {
            Sampletype::REJECTION
        };

        let binom_kind = if new_binom == 0 {
            Binomtype::BUGGY_BTPE
        } else {
            Binomtype::BTPE
        };

        let len_seed = rng_table_len_seed(rng_kind);
        let seeds_len = XLENGTH(seeds) as usize;
        if seeds_len > 1 && seeds_len < len_seed + 1 {
            error("'.Random.seed' has wrong length");
        }
        with_rng_state(|rng| {
            rng.rng_kind.set(rng_kind);
            rng.n01_kind.set(n01_kind);
            rng.sample_kind.set(sample_kind);
            rng.binom_kind.set(binom_kind);

            if seeds_len == 1 && rng_kind != RNGtype::USER_UNIF {
                RNG_Init(rng, rng_kind, TimeToSeed() as i64);
            } else {
                for j in 0..len_seed {
                    rng.rng_table[rng_kind as usize].i_seed[j] = *is.add(j + 1) as u32;
                }
                // Restore the Knuth working state from the persisted x array
                if matches!(rng_kind, RNGtype::KNUTH_TAOCP | RNGtype::KNUTH_TAOCP2) {
                    let table = &rng.rng_table[rng_kind as usize];
                    for j in 0..KT_KK {
                        rng.kt_x[j] = table.i_seed[j] as i64;
                    }
                    rng.kt_pos.set(table.i_seed[KT_KK] as usize);
                }
                FixupSeeds(rng, rng_kind, false);
            }
        });
    }
}

fn rng_table_len_seed(kind: RNGtype) -> usize {
    match kind {
        RNGtype::WICHMANN_HILL => 3,
        RNGtype::MARSAGLIA_MULTICARRY => 2,
        RNGtype::SUPER_DUPER => 2,
        RNGtype::MERSENNE_TWISTER => 1 + MT_N,
        RNGtype::KNUTH_TAOCP | RNGtype::KNUTH_TAOCP2 => 1 + KT_KK,
        RNGtype::USER_UNIF => 0,
        RNGtype::LECUYER_CMRG => 6,
    }
}

fn rng_kind_out_of_range() -> bool {
    with_rng_state(|rng| {
        rng.rng_kind.get() as i32 > 7
            || rng.n01_kind.get() as i32 > 5
            || rng.sample_kind.get() as i32 > 1
            || rng.binom_kind.get() as i32 > 1
    })
}

/// Copy seeds out to .Random.seed.
pub unsafe fn PutRNGstate() {
    unsafe {
        if rng_kind_out_of_range() {
            warn("Internal .Random.seed is corrupt: not saving");
            return;
        }

        let (len_seed, seeds_vec) = with_rng_state(|rng| {
            let kind = rng.rng_kind.get();
            let n01 = rng.n01_kind.get();
            let samp = rng.sample_kind.get();
            let binom = rng.binom_kind.get();
            let ls = rng.rng_table[kind as usize].n_seed;

            // Keep the Knuth working state in i_seed so it round-trips
            if matches!(kind, RNGtype::KNUTH_TAOCP | RNGtype::KNUTH_TAOCP2) {
                sync_kt_seeds(rng, kind);
            }

            // Build the full seed vector: [kinds, i_seed[0], i_seed[1], ...]
            let mut sv = vec![0i32; ls + 1];
            sv[0] = kind as i32 + 100 * n01 as i32 + 10000 * samp as i32 + 100000 * binom as i32;
            for j in 0..ls {
                sv[j + 1] = rng.rng_table[kind as usize].i_seed[j] as i32;
            }
            (ls, sv)
        });

        let seeds_sym = R_SeedsSymbol();
        let existing = R_findVarInFrame(R_GlobalEnv(), seeds_sym);

        // Check if we can reuse the existing vector
        let can_reuse = !existing.is_null()
            && TYPEOF(existing) == SEXPTYPE::INTSXP
            && XLENGTH(existing) as usize == len_seed + 1
            && ATTRIB(existing) == R_NilValue();

        if can_reuse {
            let p = INTEGER(existing);
            for j in 0..=len_seed {
                *p.add(j) = seeds_vec[j];
            }
        } else {
            let seeds = Rf_allocVector(SEXPTYPE::INTSXP, (len_seed + 1) as c_int);
            let _seeds_guard = protect(seeds);
            let p = INTEGER(seeds);
            for j in 0..=len_seed {
                *p.add(j) = seeds_vec[j];
            }
            defineVar(seeds_sym, seeds, R_GlobalEnv());
        }
    }
}

// ---------------------------------------------------------------------------
// RNGkind -- choose a new RNG kind
// ---------------------------------------------------------------------------

pub(crate) fn r_RNGkind(newkind: RNGtype) {
    // Choose a new kind of RNG; initialize its seed from the old RNG's
    // unif_rand() (R's RNGkind()).
    if newkind == RNGtype::MARSAGLIA_MULTICARRY {
        warn("RNGkind: Marsaglia-Multicarry has poor statistical properties");
    }
    if !matches!(
        newkind,
        RNGtype::WICHMANN_HILL
            | RNGtype::MARSAGLIA_MULTICARRY
            | RNGtype::SUPER_DUPER
            | RNGtype::MERSENNE_TWISTER
            | RNGtype::KNUTH_TAOCP
            | RNGtype::USER_UNIF
            | RNGtype::KNUTH_TAOCP2
            | RNGtype::LECUYER_CMRG
    ) {
        error(&format!(
            "RNGkind: unimplemented RNG kind {}",
            newkind as i32
        ));
    }

    unsafe { GetRNGstate() };
    // Precaution against corruption as per package randtoolbox
    let u = r_unif_rand();
    let seed = if !(0.0..=1.0).contains(&u) {
        warn("someone corrupted the random-number generator: re-initializing");
        TimeToSeed() as i64
    } else {
        (u * u32::MAX as f64) as i64
    };

    with_rng_state(|rng| {
        RNG_Init(rng, newkind, seed);
        rng.rng_kind.set(newkind);
    });

    unsafe {
        PutRNGstate();
    }
}

pub(crate) fn r_Norm_kind(kind: N01type) {
    let mm = with_rng_state(|rng| rng.rng_kind.get() == RNGtype::MARSAGLIA_MULTICARRY);
    if kind == N01type::KINDERMAN_RAMAGE && mm {
        warn(
            "RNGkind: severe deviations from normality for Kinderman-Ramage + Marsaglia-Multicarry",
        );
    }
    if kind == N01type::AHRENS_DIETER && mm {
        warn("RNGkind: deviations from normality for Ahrens-Dieter + Marsaglia-Multicarry");
    }
    if kind as i32 > 5 {
        error("invalid Normal type in 'RNGkind'");
    }
    if kind == N01type::USER_NORM {
        error("'user_norm_rand' not in load table");
    }
    unsafe { GetRNGstate() }; /* might not be initialized */
    if kind == N01type::BOX_MULLER {
        with_rng_state(|rng| rng.bm_norm_keep.set(0.0)); /* zap Box-Muller history */
    }
    with_rng_state(|rng| rng.n01_kind.set(kind));
    unsafe {
        PutRNGstate();
    }
}

pub(crate) fn r_Samp_kind(kind: Sampletype) {
    if kind as i32 > 1 {
        error("invalid sample type in 'RNGkind'");
    }
    unsafe { GetRNGstate() }; /* might not be initialized */
    with_rng_state(|rng| rng.sample_kind.set(kind));
    unsafe {
        PutRNGstate();
    }
}

pub(crate) fn r_Bin_kind(kind: Binomtype) {
    if kind as i32 > 1 {
        error("invalid binom type in 'RNGkind'");
    }
    unsafe { GetRNGstate() }; /* might not be initialized */
    with_rng_state(|rng| rng.binom_kind.set(kind));
    unsafe {
        PutRNGstate();
    }
}

// ---------------------------------------------------------------------------
// R scalar random generators (matching Rmath.h signatures)
// ---------------------------------------------------------------------------
