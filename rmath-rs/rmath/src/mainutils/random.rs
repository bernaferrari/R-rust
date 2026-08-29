#![allow(unreachable_code)]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/RNG.c and src/main/random.c
//!
//! This module implements R's random number generation infrastructure:
//!   - Multiple RNG algorithms (Wichmann-Hill, Marsaglia-MultiCarry, Super-Duper,
//!     Mersenne-Twister, Knuth-TAOCP, Knuth-TAOCP-2002, L'Ecuyer-CMRG)
//!   - Normal variate generation (Box-Muller, Inversion, etc.)
//!   - RNG state management (GetRNGstate, PutRNGstate)
//!   - R-level functions: RNGkind(), set.seed(), rnorm(), runif(), rbinom(), etc.
//!   - Probability sampling: sample(), ProbSampleReplace, ProbSampleNoReplace

use crate::mainutils::coerce::coerceVector;
use crate::mainutils::main::R_SeedsSymbol;
use crate::mainutils::times::TimeToSeed;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::*;
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use std::cell::Cell;
use std::os::raw::{c_double, c_int};

fn error(msg: &str) {
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

const RNG_DEFAULT: RNGtype = RNGtype::MERSENNE_TWISTER;
const N01_DEFAULT: N01type = N01type::INVERSION;
const Sample_DEFAULT: Sampletype = Sampletype::REJECTION;
const Binom_DEFAULT: Binomtype = Binomtype::BTPE;

/// Threshold for using stack allocation vs heap allocation in Walker's method.
const SMALL: usize = 10000;

const I2_32M1: f64 = 2.328306437080797e-10; /* 1/(2^32 - 1) */
const KT: f64 = 9.31322574615479e-10; /* 2^-30 */

// Mersenne Twister constants
const MT_N: usize = 624;
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

struct RNGTab {
    kind: RNGtype,
    n_seed: usize,
    i_seed: Vec<Int32>,
}

/// Per-instance RNG state.
///
/// MT and Knuth-TAOCP scratch arrays mirror R's layout: the persisted seed
/// state lives in `rng_table[kind].i_seed` (so `.Random.seed` round-trips
/// through `PutRNGstate`/`GetRNGstate`); `kt_x`/`kt_ran_arr_*` hold the
/// Knuth working state that R also keeps outside `.Random.seed`.
pub(crate) struct RNGState {
    rng_kind: Cell<RNGtype>,
    n01_kind: Cell<N01type>,
    sample_kind: Cell<Sampletype>,
    binom_kind: Cell<Binomtype>,
    rng_table: [RNGTab; 8],
    // Knuth TAOCP state
    kt_x: [i64; KT_KK],
    kt_ran_arr_buf: [i64; KT_QUALITY],
    kt_ran_arr_ptr: Cell<usize>,
    kt_pos: Cell<usize>,
    // Box-Muller saved value
    bm_norm_keep: Cell<f64>,
    // Whether RNG_Init has run (stock randomizes lazily on first use)
    initialized: Cell<bool>,
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

fn with_rng_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut RNGState) -> R,
{
    crate::sexp::instance::with_required_current_instance(|instance| f(&mut instance.random_state))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Ensure 0 and 1 are never returned from unif_rand.
fn fixup(x: f64) -> f64 {
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

fn RNG_Init(rng: &mut RNGState, kind: RNGtype, seed: i64) {
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
fn warn(msg: &str) {
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
            // No .Random.seed -- randomize with the current kind
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, TimeToSeed() as i64);
            });
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

fn r_RNGkind(newkind: RNGtype) {
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

fn r_Norm_kind(kind: N01type) {
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

fn r_Samp_kind(kind: Sampletype) {
    if kind as i32 > 1 {
        error("invalid sample type in 'RNGkind'");
    }
    unsafe { GetRNGstate() }; /* might not be initialized */
    with_rng_state(|rng| rng.sample_kind.set(kind));
    unsafe {
        PutRNGstate();
    }
}

fn r_Bin_kind(kind: Binomtype) {
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

pub fn R_runif(a: c_double, b: c_double) -> c_double {
    if !a.is_finite() || !b.is_finite() {
        return f64::NAN;
    }
    if a > b {
        return f64::NAN;
    }
    if a == b {
        return a;
    }
    a + (b - a) * r_unif_rand()
}

pub fn R_rnorm(mu: c_double, sigma: c_double) -> c_double {
    if !sigma.is_finite() {
        return f64::NAN;
    }
    if sigma == 0.0 || !mu.is_finite() {
        return mu;
    }
    mu + sigma * r_norm_rand()
}

pub fn R_rbinom(n: c_double, p: c_double) -> c_double {
    crate::dist::binomial::rbinom(n, p)
}

pub fn R_rexp(rate: c_double) -> c_double {
    crate::dist::exponential::rexp(rate)
}

pub fn R_rpois(mu: c_double) -> c_double {
    crate::dist::poisson::rpois(mu)
}

pub fn R_rchisq(df: c_double) -> c_double {
    crate::dist::chisq::rchisq(df)
}

pub fn R_rgamma(shape: c_double, scale: c_double) -> c_double {
    crate::dist::gamma::rgamma(shape, scale)
}

pub fn R_rbeta(a: c_double, b: c_double) -> c_double {
    crate::dist::beta::rbeta(a, b)
}

pub fn R_rt(df: c_double) -> c_double {
    crate::dist::t_dist::rt(df)
}

pub fn R_rf(n1: c_double, n2: c_double) -> c_double {
    crate::dist::f_dist::rf(n1, n2)
}

pub fn R_rcauchy(location: c_double, scale: c_double) -> c_double {
    crate::dist::cauchy::rcauchy(location, scale)
}

pub fn R_rlnorm(meanlog: c_double, sdlog: c_double) -> c_double {
    crate::dist::lnorm::rlnorm(meanlog, sdlog)
}

pub fn R_rlogis(location: c_double, scale: c_double) -> c_double {
    crate::dist::logistic::rlogis(location, scale)
}

pub fn R_rweibull(shape: c_double, scale: c_double) -> c_double {
    crate::dist::weibull::rweibull(shape, scale)
}

pub fn R_rwilcox(m: c_double, n: c_double) -> c_double {
    crate::dist::wilcox::rwilcox(m, n)
}

pub fn R_rsignrank(n: c_double) -> c_double {
    crate::dist::signrank::rsignrank(n)
}

pub fn R_rnbinom(size: c_double, prob: c_double) -> c_double {
    crate::dist::nbinom::rnbinom(size, prob)
}

pub fn R_rnbinom_mu(size: c_double, mu: c_double) -> c_double {
    crate::dist::nbinom::rnbinom_mu(size, mu)
}

pub fn R_rnchisq(df: c_double, ncp: c_double) -> c_double {
    crate::dist::nchisq::rnchisq(df, ncp)
}

pub fn R_rhyper(nn1: c_double, nn2: c_double, kk: c_double) -> c_double {
    crate::dist::hypergeometric::rhyper(nn1, nn2, kk)
}

pub fn R_rgeom(p: c_double) -> c_double {
    crate::dist::geometric::rgeom(p)
}

pub fn R_unif_index(dn: c_double) -> c_double {
    r_R_unif_index(dn)
}

pub fn R_sample_kind() -> c_int {
    r_sample_kind() as c_int
}

pub fn R_binom_kind() -> c_int {
    r_binom_kind() as c_int
}

// ---------------------------------------------------------------------------
// Internal random generation helpers (used by do_random1/2/3)
// ---------------------------------------------------------------------------

/// One-parameter random generation using a function pointer wrapper.
fn random1_call(f: fn(f64) -> f64, a: *const f64, na: R_xlen_t, x: *mut f64, n: R_xlen_t) -> bool {
    let mut naflag = false;
    let mut ia: R_xlen_t = 0;
    for i in 0..n {
        let ai = unsafe { *a.add(ia as usize) };
        let val = f(ai);
        unsafe {
            *x.add(i as usize) = val;
        };
        if val.is_nan() {
            naflag = true;
        }
        ia += 1;
        if ia >= na {
            ia = 0;
        }
    }
    naflag
}

/// Two-parameter random generation using a function pointer wrapper.
fn random2_call(
    f: fn(f64, f64) -> f64,
    a: *const f64,
    na: R_xlen_t,
    b: *const f64,
    nb: R_xlen_t,
    x: *mut f64,
    n: R_xlen_t,
) -> bool {
    let mut naflag = false;
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    for i in 0..n {
        let ai = unsafe { *a.add(ia as usize) };
        let bi = unsafe { *b.add(ib as usize) };
        let val = f(ai, bi);
        unsafe {
            *x.add(i as usize) = val;
        };
        if val.is_nan() {
            naflag = true;
        }
        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
    }
    naflag
}

/// Three-parameter random generation using a function pointer wrapper.
fn random3_call(
    f: fn(f64, f64, f64) -> f64,
    a: *const f64,
    na: R_xlen_t,
    b: *const f64,
    nb: R_xlen_t,
    c: *const f64,
    nc: R_xlen_t,
    x: *mut f64,
    n: R_xlen_t,
) -> bool {
    let mut naflag = false;
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    let mut ic: R_xlen_t = 0;
    for i in 0..n {
        let ai = unsafe { *a.add(ia as usize) };
        let bi = unsafe { *b.add(ib as usize) };
        let ci = unsafe { *c.add(ic as usize) };
        let val = f(ai, bi, ci);
        unsafe {
            *x.add(i as usize) = val;
        };
        if val.is_nan() {
            naflag = true;
        }
        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
        ic += 1;
        if ic >= nc {
            ic = 0;
        }
    }
    naflag
}

// ---------------------------------------------------------------------------
// Helper functions for SEXP argument handling
// ---------------------------------------------------------------------------

unsafe fn isVector(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::RAWSXP
    }
}

unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP
    }
}

unsafe fn asInteger_local(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(x)
        } else if t == SEXPTYPE::REALSXP {
            let v = *REAL(x);
            if !v.is_finite() || v > i32::MAX as f64 || v < i32::MIN as f64 {
                NA_INTEGER
            } else {
                v as c_int
            }
        } else {
            NA_INTEGER
        }
    }
}

unsafe fn asReal_local(x: SEXP) -> c_double {
    unsafe {
        if x.is_null() {
            return f64::NAN;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER { f64::NAN } else { v as f64 }
        } else {
            f64::NAN
        }
    }
}

#[allow(clippy::if_same_then_else)]
unsafe fn asLogical_local(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP {
            *INTEGER(x)
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(x)
        } else {
            NA_INTEGER
        }
    }
}

unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

unsafe fn checkArity(op: SEXP, args: SEXP) {
    // stub
}

unsafe fn PRIMVAL_local(op: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::PRIMVAL(op) }
}

// ---------------------------------------------------------------------------
// do_random1 -- random sampling from 1 parameter families
// ---------------------------------------------------------------------------

/// R's .Internal interface for 1-parameter distributions:
/// rchisq, rexp, rgeom, rpois, rt, rsignrank
pub unsafe fn do_random1(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        if !isVector(CAR(args)) || !isNumeric(CADR(args)) {
            error("invalid arguments");
        }

        let n: R_xlen_t;
        if XLENGTH(CAR(args)) == 1 {
            let dn = asReal_local(CAR(args));
            if dn.is_nan() || dn < 0.0 {
                error("invalid arguments");
            }
            n = dn as R_xlen_t;
        } else {
            n = XLENGTH(CAR(args));
        }

        let x = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
        let _x_guard = protect(x);
        if n == 0 {
            return x;
        }

        let na = XLENGTH(CADR(args));
        if na < 1 {
            for i in 0..n {
                *REAL(x).add(i as usize) = f64::NAN;
            }
            return x;
        }

        let a = coerceVector(CADR(args), SEXPTYPE::REALSXP.as_c_int());
        let _a_guard = protect(a);
        GetRNGstate();

        let primval = PRIMVAL_local(op);
        let _naflag: bool;

        match primval {
            0 => {
                let _ = random1_call(crate::dist::chisq::rchisq, REAL(a), na, REAL(x), n);
            }
            1 => {
                let _ = random1_call(crate::dist::exponential::rexp, REAL(a), na, REAL(x), n);
            }
            2 => {
                let _ = random1_call(crate::dist::geometric::rgeom, REAL(a), na, REAL(x), n);
            }
            3 => {
                let _ = random1_call(crate::dist::poisson::rpois, REAL(a), na, REAL(x), n);
            }
            4 => {
                let _ = random1_call(crate::dist::t_dist::rt, REAL(a), na, REAL(x), n);
            }
            5 => {
                let _ = random1_call(crate::dist::signrank::rsignrank, REAL(a), na, REAL(x), n);
            }
            _ => {} // intentionally unhandled: unknown distribution type
        }

        PutRNGstate();
        x
    }
}

// ---------------------------------------------------------------------------
// do_random2 -- random sampling from 2 parameter families
// ---------------------------------------------------------------------------

/// R's .Internal interface for 2-parameter distributions:
/// rbeta, rbinom, rcauchy, rf, rgamma, rlnorm, rlogis, rnbinom, rnorm, runif,
/// rweibull, rwilcox, rnchisq, rnbinom_mu
pub unsafe fn do_random2(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        if !isVector(CAR(args)) || !isNumeric(CADR(args)) || !isNumeric(CADDR(args)) {
            error("invalid arguments");
        }

        let n: R_xlen_t;
        if XLENGTH(CAR(args)) == 1 {
            let dn = asReal_local(CAR(args));
            if dn.is_nan() || dn < 0.0 {
                error("invalid arguments");
            }
            n = dn as R_xlen_t;
        } else {
            n = XLENGTH(CAR(args));
        }

        let x = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
        let _x_guard = protect(x);
        if n == 0 {
            return x;
        }

        let na = XLENGTH(CADR(args));
        let nb = XLENGTH(CADDR(args));
        if na < 1 || nb < 1 {
            for i in 0..n {
                *REAL(x).add(i as usize) = f64::NAN;
            }
            return x;
        }

        let a = coerceVector(CADR(args), SEXPTYPE::REALSXP.as_c_int());
        let _a_guard = protect(a);
        let b = coerceVector(CADDR(args), SEXPTYPE::REALSXP.as_c_int());
        let _b_guard = protect(b);
        GetRNGstate();

        let primval = PRIMVAL_local(op);

        match primval {
            0 => {
                let _ = random2_call(
                    crate::dist::beta::rbeta,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            1 => {
                let _ = random2_call(
                    crate::dist::binomial::rbinom,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            2 => {
                let _ = random2_call(
                    crate::dist::cauchy::rcauchy,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            3 => {
                let _ = random2_call(
                    crate::dist::f_dist::rf,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            4 => {
                let _ = random2_call(
                    crate::dist::gamma::rgamma,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            5 => {
                let _ = random2_call(
                    crate::dist::lnorm::rlnorm,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            6 => {
                let _ = random2_call(
                    crate::dist::logistic::rlogis,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            7 => {
                let _ = random2_call(
                    crate::dist::nbinom::rnbinom,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            8 => {
                let _ = random2_call(
                    crate::dist::normal::rnorm,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            9 => {
                let _ = random2_call(
                    crate::dist::uniform::runif,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            10 => {
                let _ = random2_call(
                    crate::dist::weibull::rweibull,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            11 => {
                let _ = random2_call(
                    crate::dist::wilcox::rwilcox,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            12 => {
                let _ = random2_call(
                    crate::dist::nchisq::rnchisq,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            13 => {
                let _ = random2_call(
                    crate::dist::nbinom::rnbinom_mu,
                    REAL(a),
                    na,
                    REAL(b),
                    nb,
                    REAL(x),
                    n,
                );
            }
            _ => {} // intentionally unhandled: unknown distribution type
        }

        PutRNGstate();
        x
    }
}

// ---------------------------------------------------------------------------
// do_random3 -- random sampling from 3 parameter families
// ---------------------------------------------------------------------------

/// R's .Internal interface for 3-parameter distributions:
/// rhyper
pub unsafe fn do_random3(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        if !isVector(CAR(args)) {
            error("invalid arguments");
        }

        let n: R_xlen_t;
        if XLENGTH(CAR(args)) == 1 {
            let dn = asReal_local(CAR(args));
            if dn.is_nan() || dn < 0.0 {
                error("invalid arguments");
            }
            n = dn as R_xlen_t;
        } else {
            n = XLENGTH(CAR(args));
        }

        let x = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
        let _x_guard = protect(x);
        if n == 0 {
            return x;
        }

        let mut args_rest = CDR(args);
        let a = CAR(args_rest);
        args_rest = CDR(args_rest);
        let b = CAR(args_rest);
        args_rest = CDR(args_rest);
        let c = CAR(args_rest);
        if !isNumeric(a) || !isNumeric(b) || !isNumeric(c) {
            return x;
        }

        let na = XLENGTH(a);
        let nb = XLENGTH(b);
        let nc = XLENGTH(c);
        if na < 1 || nb < 1 || nc < 1 {
            for i in 0..n {
                *REAL(x).add(i as usize) = f64::NAN;
            }
            return x;
        }

        let a = coerceVector(a, SEXPTYPE::REALSXP.as_c_int());
        let _a_guard = protect(a);
        let b = coerceVector(b, SEXPTYPE::REALSXP.as_c_int());
        let _b_guard = protect(b);
        let c = coerceVector(c, SEXPTYPE::REALSXP.as_c_int());
        let _c_guard = protect(c);
        GetRNGstate();

        let primval = PRIMVAL_local(op);

        if primval == 0 {
            let _ = random3_call(
                crate::dist::hypergeometric::rhyper,
                REAL(a),
                na,
                REAL(b),
                nb,
                REAL(c),
                nc,
                REAL(x),
                n,
            );
        }

        PutRNGstate();
        x
    }
}

// ---------------------------------------------------------------------------
// do_RNGkind / do_setseed -- R's RNGkind() and set.seed()
//
// Stock R wraps the .Internal layer in base-package closures that validate
// string arguments (pmatch against the kind tables) and convert the integer
// result back to names. This port has no R-level closures, so the same
// validation and name conversion lives here.
// ---------------------------------------------------------------------------

const RNG_KIND_NAMES: [&str; 9] = [
    "Wichmann-Hill",
    "Marsaglia-Multicarry",
    "Super-Duper",
    "Mersenne-Twister",
    "Knuth-TAOCP",
    "user-supplied",
    "Knuth-TAOCP-2002",
    "L'Ecuyer-CMRG",
    "default",
];

const N01_KIND_NAMES: [&str; 7] = [
    "Buggy Kinderman-Ramage",
    "Ahrens-Dieter",
    "Box-Muller",
    "user-supplied",
    "Inversion",
    "Kinderman-Ramage",
    "default",
];

const SAMPLE_KIND_NAMES: [&str; 3] = ["Rounding", "Rejection", "default"];

const BINOM_KIND_NAMES: [&str; 3] = ["Buggy BTPE", "BTPE", "default"];

/// pmatch(x, table) for one string: exact match wins, then a unique prefix
/// match; ambiguity or no match is None (stock pmatch).
fn pmatch_one(x: &str, table: &[&str]) -> Option<usize> {
    if let Some(i) = table.iter().position(|t| *t == x) {
        return Some(i);
    }
    let mut matches = table.iter().enumerate().filter(|(_, t)| t.starts_with(x));
    match (matches.next(), matches.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

fn arg_is_absent(x: SEXP) -> bool {
    x.is_null() || unsafe { x == R_NilValue() || x == R_MissingArg() }
}

/// True when the slot carries the given parameter tag. The evaluator keeps
/// call order, so `RNGkind(normal.kind = "Inversion")` delivers the value in
/// the first slot; tag inspection restores the stock parameter mapping.
unsafe fn kind_tag_is(x: SEXP, name: &str) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let tag = TAG(x);
        if tag.is_null() || tag == R_NilValue() {
            return false;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(CHAR(pname)).to_bytes() == name.as_bytes()
    }
}

/// Map positional slots to kind parameters by tag when any tag is present.
///
/// `cells` are the pairlist cells holding the kind/normal.kind/sample.kind/
/// binom.kind arguments in call order; TAG is read from the cell, not the value.
unsafe fn resolve_kind_args(cells: [SEXP; 4]) -> (SEXP, SEXP, SEXP, SEXP) {
    unsafe {
        let nil = R_NilValue();
        let mut kind = nil;
        let mut norm = nil;
        let mut sample = nil;
        let mut binom = nil;
        for cell in cells {
            if cell.is_null() || cell == nil {
                continue;
            }
            let x = CAR(cell);
            if arg_is_absent(x) {
                continue;
            }
            if kind_tag_is(cell, "normal.kind") {
                norm = x;
            } else if kind_tag_is(cell, "sample.kind") {
                sample = x;
            } else if kind_tag_is(cell, "binom.kind") {
                binom = x;
            } else {
                // Untagged slots fill kind, then normal.kind, then
                // sample.kind, then binom.kind (stock positional order).
                if kind == nil {
                    kind = x;
                } else if norm == nil {
                    norm = x;
                } else if sample == nil {
                    sample = x;
                } else {
                    binom = x;
                }
            }
        }
        (kind, norm, sample, binom)
    }
}

unsafe fn kind_string(arg: SEXP) -> String {
    unsafe {
        let char_sexp = STRING_ELT(arg, 0);
        std::ffi::CStr::from_ptr(CHAR(char_sexp))
            .to_string_lossy()
            .into_owned()
    }
}

fn kind_error(msg: &str) -> ! {
    unsafe {
        let call = crate::mainutils::errors::R_getCurrentCall();
        let nil = R_NilValue();
        if !call.is_null() && call != nil {
            crate::mainutils::errors::errorcall_str(call, msg);
        }
        crate::mainutils::errors::errorcall_str(nil, msg);
    }
}

/// `kind = NULL` absent | code 0..=7 | -1 for "default".
unsafe fn parse_rng_kind_arg(arg: SEXP) -> Option<i32> {
    unsafe {
        if arg_is_absent(arg) || (TYPEOF(arg) == SEXPTYPE::STRSXP && XLENGTH(arg) == 0) {
            return None;
        }
        if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) > 1 {
            kind_error("'kind' must be a character string (RNG to be used).");
        }
        let s = kind_string(arg);
        match pmatch_one(&s, &RNG_KIND_NAMES) {
            Some(i) if i == RNG_KIND_NAMES.len() - 1 => Some(-1),
            Some(i) => Some(i as i32),
            None => kind_error(&format!("'{s}' is not a valid abbreviation of an RNG")),
        }
    }
}

/// `normal.kind = NULL` absent | code 0..=5 | -1 for "default".
unsafe fn parse_normal_kind_arg(arg: SEXP, from_setseed: bool) -> Option<i32> {
    unsafe {
        if arg_is_absent(arg) {
            return None;
        }
        if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) != 1 {
            kind_error("'normal.kind' must be a character string");
        }
        let s = kind_string(arg);
        match pmatch_one(&s, &N01_KIND_NAMES) {
            None => kind_error(&format!("'{s}' is not valid for 'normal.kind'")),
            Some(0) => {
                // Buggy Kinderman-Ramage
                if from_setseed {
                    kind_error("buggy version of Kinderman-Ramage generator is not allowed");
                }
                warn("buggy version of Kinderman-Ramage generator used");
                Some(0)
            }
            Some(i) if i == N01_KIND_NAMES.len() - 1 => Some(-1),
            Some(i) => Some(i as i32),
        }
    }
}

/// `sample.kind = NULL` absent | code 0..=1 | -1 for "default".
unsafe fn parse_sample_kind_arg(arg: SEXP) -> Option<i32> {
    unsafe {
        if arg_is_absent(arg) {
            return None;
        }
        if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) != 1 {
            kind_error("'sample.kind' must be a character string");
        }
        let s = kind_string(arg);
        match pmatch_one(&s, &SAMPLE_KIND_NAMES) {
            None => kind_error(&format!("'{s}' is not valid for 'sample.kind'")),
            Some(0) => {
                warn("non-uniform 'Rounding' sampler used");
                Some(0)
            }
            Some(i) if i == SAMPLE_KIND_NAMES.len() - 1 => Some(-1),
            Some(i) => Some(i as i32),
        }
    }
}

/// `binom.kind = NULL` absent | code 0..=1 | -1 for "default".
unsafe fn parse_binom_kind_arg(arg: SEXP) -> Option<i32> {
    unsafe {
        if arg_is_absent(arg) {
            return None;
        }
        if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) != 1 {
            kind_error("'binom.kind' must be a character string");
        }
        let s = kind_string(arg);
        match pmatch_one(&s, &BINOM_KIND_NAMES) {
            None => kind_error(&format!("'{s}' is not valid for 'binom.kind'")),
            Some(0) => {
                warn("Buggy BTPE algorithm used for rbinom()");
                Some(0)
            }
            Some(i) if i == BINOM_KIND_NAMES.len() - 1 => Some(-1),
            Some(i) => Some(i as i32),
        }
    }
}

fn rng_kind_from_code(code: i32) -> RNGtype {
    match code {
        0 => RNGtype::WICHMANN_HILL,
        1 => RNGtype::MARSAGLIA_MULTICARRY,
        2 => RNGtype::SUPER_DUPER,
        3 => RNGtype::MERSENNE_TWISTER,
        4 => RNGtype::KNUTH_TAOCP,
        5 => RNGtype::USER_UNIF,
        6 => RNGtype::KNUTH_TAOCP2,
        7 => RNGtype::LECUYER_CMRG,
        -1 => RNG_DEFAULT,
        _ => RNG_DEFAULT,
    }
}

fn n01_kind_from_code(code: i32) -> N01type {
    match code {
        0 => N01type::BUGGY_KINDERMAN_RAMAGE,
        1 => N01type::AHRENS_DIETER,
        2 => N01type::BOX_MULLER,
        3 => N01type::USER_NORM,
        4 => N01type::INVERSION,
        5 => N01type::KINDERMAN_RAMAGE,
        -1 => N01_DEFAULT,
        _ => N01_DEFAULT,
    }
}

fn sample_kind_from_code(code: i32) -> Sampletype {
    match code {
        0 => Sampletype::ROUNDING,
        -1 => Sample_DEFAULT,
        _ => Sampletype::REJECTION,
    }
}

fn binom_kind_from_code(code: i32) -> Binomtype {
    match code {
        0 => Binomtype::BUGGY_BTPE,
        -1 => Binom_DEFAULT,
        _ => Binomtype::BTPE,
    }
}

/// R's `RNGkind(kind = NULL, normal.kind = NULL, sample.kind = NULL,
/// binom.kind = NULL)`.
///
/// Returns the *old* kind names as a character vector; the value is visible
/// when nothing was set and invisible otherwise (stock closure behavior).
pub unsafe fn do_RNGkind(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        GetRNGstate(); /* might not be initialized */

        let (rng_code, norm_code, sample_code, binom_code) =
            resolve_kind_args([args, CDR(args), CDR(CDR(args)), CDR(CDR(CDR(args)))]);
        let rng_parsed = parse_rng_kind_arg(rng_code);
        let norm_parsed = parse_normal_kind_arg(norm_code, false);
        let sample_parsed = parse_sample_kind_arg(sample_code);
        let binom_parsed = parse_binom_kind_arg(binom_code);

        // Snapshot the old kinds before applying anything.
        let (old_rng, old_n01, old_sample, old_binom) = with_rng_state(|rng| {
            (
                rng.rng_kind.get() as c_int,
                rng.n01_kind.get() as c_int,
                rng.sample_kind.get() as c_int,
                rng.binom_kind.get() as c_int,
            )
        });

        // Stock pulls the kind codes from .Random.seed (if present) before
        // switching; reloading the full state from the same source matches.
        GetRNGstate();

        if let Some(code) = rng_parsed {
            r_RNGkind(rng_kind_from_code(code));
        }
        if let Some(code) = norm_parsed {
            r_Norm_kind(n01_kind_from_code(code));
        }
        if let Some(code) = sample_parsed {
            r_Samp_kind(sample_kind_from_code(code));
        }
        if let Some(code) = binom_parsed {
            r_Bin_kind(binom_kind_from_code(code));
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, 4);
        let _ans_guard = protect(ans);
        for (i, name) in [
            RNG_KIND_NAMES[old_rng as usize],
            N01_KIND_NAMES[old_n01 as usize],
            SAMPLE_KIND_NAMES[old_sample as usize],
            BINOM_KIND_NAMES[old_binom as usize],
        ]
        .iter()
        .enumerate()
        {
            let cstr = std::ffi::CString::new(*name).unwrap_or_default();
            SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        crate::sexp::globals::set_R_Visible(i32::from(
            rng_parsed.is_none()
                && norm_parsed.is_none()
                && sample_parsed.is_none()
                && binom_parsed.is_none(),
        ));
        ans
    }
}

/// R's `set.seed(seed, kind = NULL, normal.kind = NULL, sample.kind = NULL,
/// binom.kind = NULL)`.
///
/// Returns invisible NULL.
pub unsafe fn do_setseed(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        // The stock closure validates kind arguments before any state change.
        let tail = CDR(args);
        let (skind, nkind, sampkind, binomkind) =
            resolve_kind_args([tail, CDR(tail), CDR(CDR(tail)), CDR(CDR(CDR(tail)))]);
        let kind_parsed = parse_rng_kind_arg(skind);
        let norm_parsed = parse_normal_kind_arg(nkind, true);
        let sample_parsed = parse_sample_kind_arg(sampkind);
        let binom_parsed = parse_binom_kind_arg(binomkind);

        let seed: i64;
        if !isNull(CAR(args)) && CAR(args) != R_MissingArg() {
            let arg = CAR(args);
            if TYPEOF(arg) == SEXPTYPE::STRSXP {
                // asInteger() coerces strings via NAs
                warn("NAs introduced by coercion");
                kind_error("supplied seed is not a valid integer");
            }
            let v = asInteger_local(arg);
            if v == NA_INTEGER {
                kind_error("supplied seed is not a valid integer");
            }
            seed = v as i64;
        } else {
            seed = TimeToSeed() as i64;
        }

        // Pull RNG_kind/N01_kind from .Random.seed if present (stock).
        GetRNGstate();

        if let Some(code) = kind_parsed {
            r_RNGkind(rng_kind_from_code(code));
        }
        if let Some(code) = norm_parsed {
            r_Norm_kind(n01_kind_from_code(code));
        }
        if let Some(code) = sample_parsed {
            r_Samp_kind(sample_kind_from_code(code));
        }
        if let Some(code) = binom_parsed {
            r_Bin_kind(binom_kind_from_code(code));
        }

        // Initialize the RNG with the seed (zaps Box-Muller history)
        with_rng_state(|rng| {
            let kind = rng.rng_kind.get();
            RNG_Init(rng, kind, seed);
        });
        PutRNGstate();

        crate::sexp::globals::set_R_Visible(0);
        R_NilValue()
    }
}

/// Seed the session's R-level RNG from a 64-bit host seed and write
/// `.Random.seed` (host-embedding analogue of `set.seed`).
pub fn set_session_seed64(seed: i64) {
    with_rng_state(|rng| {
        let kind = rng.rng_kind.get();
        RNG_Init(rng, kind, seed);
    });
    unsafe {
        PutRNGstate();
    }
}

/// Hook installed into the nmath crate so its samplers draw from the
/// session's R-level RNG dispatch (all RNG kinds, `.Random.seed` state)
/// instead of the standalone MultiCarry stream.
pub fn nmath_unif_hook() -> f64 {
    if crate::sexp::instance::current_instance_ptr().is_some() {
        // Stock R randomizes lazily when the RNG is first used without a
        // .Random.seed; a virgin per-instance state gets the same treatment
        // instead of drawing from an all-zero Mersenne Twister state.
        let virgin = with_rng_state(|rng| !rng.initialized.get());
        if virgin {
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, crate::mainutils::times::TimeToSeed() as i64);
            });
        }
        r_unif_rand()
    } else {
        crate::nmath::rng::multicarry_unif_rand()
    }
}

/// Hook installed into the nmath crate so `rbinom()`'s BTPE algorithm
/// selection follows the session's `binom.kind` — the port-side equivalent
/// of stock RNG.c's `Bin_kind` writing nmath's shared `Binom_kind` global.
pub fn nmath_binom_kind_hook() -> crate::nmath::rng::Binomtype {
    if crate::sexp::instance::current_instance_ptr().is_some() {
        match r_binom_kind() {
            Binomtype::BUGGY_BTPE => crate::nmath::rng::Binomtype::BUGGY_BTPE,
            Binomtype::BTPE => crate::nmath::rng::Binomtype::BTPE,
        }
    } else {
        // No active session: the standalone default (ML_Binom_kind role).
        crate::nmath::rng::Binomtype::BTPE
    }
}

// ---------------------------------------------------------------------------
// do_sample -- R's sample() function
// ---------------------------------------------------------------------------

pub unsafe fn do_sample(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let sn = CAR(args);
        let args2 = CDR(args);
        let sk = CAR(args2);
        let args3 = CDR(args2);
        let sreplace = CAR(args3);
        let args4 = CDR(args3);
        let prob = CAR(args4);

        if LENGTH(sk) != 1 {
            error("invalid 'size' argument");
        }
        if LENGTH(sreplace) != 1 {
            error("invalid 'replace' argument");
        }
        let replace = asLogical_local(sreplace);
        if replace == NA_INTEGER {
            error("invalid 'replace' argument");
        }

        GetRNGstate();

        if !isNull(prob) {
            let n = asInteger_local(sn);
            let k = asInteger_local(sk);
            if n == NA_INTEGER || n < 0 || (k > 0 && n == 0) {
                PutRNGstate();
                error("invalid first argument");
            }
            if k == NA_INTEGER || k < 0 {
                PutRNGstate();
                error("invalid 'size' argument");
            }
            if replace == 0 && k > n {
                PutRNGstate();
                error("cannot take a sample larger than the population when 'replace = FALSE'");
            }

            let y = Rf_allocVector(SEXPTYPE::INTSXP, k);
            let _y_guard = protect(y);
            for i in 0..k as usize {
                *INTEGER(y).add(i) = 0;
            }
            PutRNGstate();
            return y;
        }

        // Uniform sampling
        let dn = asReal_local(sn);
        let k = if asReal_local(sk).is_nan() {
            0
        } else {
            asReal_local(sk) as R_xlen_t
        };
        if !dn.is_finite() || dn < 0.0 || dn > 4.5e15 {
            PutRNGstate();
            error("invalid first argument");
        }
        if k < 0 {
            PutRNGstate();
            error("invalid 'size' argument");
        }
        if replace == 0 && k > dn as R_xlen_t {
            PutRNGstate();
            error("cannot take a sample larger than the population when 'replace = FALSE'");
        }

        if dn > i32::MAX as f64 || k > i32::MAX as R_xlen_t {
            // Long vector support
            let y = Rf_allocVector(SEXPTYPE::REALSXP, k as c_int);
            let _y_guard = protect(y);
            let ry = REAL(y);
            if replace != 0 {
                for i in 0..k as usize {
                    *ry.add(i) = r_R_unif_index(dn) + 1.0;
                }
            }
            PutRNGstate();
            return y;
        }

        let n = dn as i32;
        let kk = k as i32;
        let y = Rf_allocVector(SEXPTYPE::INTSXP, kk);
        let _y_guard = protect(y);
        let iy = INTEGER(y);

        if replace != 0 || kk < 2 {
            for i in 0..kk as usize {
                *iy.add(i) = r_R_unif_index(n as f64) as i32 + 1;
            }
        } else {
            // Without replacement: Fisher-Yates shuffle
            let mut x: Vec<i32> = (0..n).collect();
            for i in 0..kk as usize {
                let remaining = (n - i as i32) as usize;
                let j = r_R_unif_index(remaining as f64) as usize;
                *iy.add(i) = x[j] + 1;
                x[j] = x[remaining - 1];
            }
        }

        PutRNGstate();
        y
    }
}

// ---------------------------------------------------------------------------
// Probability vector normalization (pure Rust, no SEXP)
// ---------------------------------------------------------------------------

/// Normalize a probability vector and validate it.
///
/// Ensures all probabilities are non-negative, finite, and sum to 1.
///
/// # Errors
/// Returns an error string if validation fails, or `None` on success.
pub fn FixupProb(p: &mut [f64], require_k: usize, replace: bool) -> Option<&'static str> {
    let n = p.len();
    let mut sum = 0.0;
    let mut npos = 0usize;

    for i in 0..n {
        if !p[i].is_finite() {
            return Some("NA in probability vector");
        }
        if p[i] < 0.0 {
            return Some("negative probability");
        }
        if p[i] > 0.0 {
            npos += 1;
            sum += p[i];
        }
    }

    if npos == 0 || (!replace && require_k > npos) {
        return Some("too few positive probabilities");
    }

    for i in 0..n {
        p[i] /= sum;
    }

    None
}

// ---------------------------------------------------------------------------
// Sampling with replacement (pure Rust)
// ---------------------------------------------------------------------------

/// Unequal probability sampling with replacement using the R RNG.
pub fn ProbSampleReplace_r(n: usize, p: &mut [f64], nans: usize) -> Vec<c_int> {
    let mut perm: Vec<c_int> = (1..=n as c_int).collect();
    revsort_with_perm(p, &mut perm, n);
    for i in 1..n {
        p[i] += p[i - 1];
    }
    let mut ans = vec![0 as c_int; nans];
    let nm1 = n - 1;
    for i in 0..nans {
        let r_u = r_unif_rand();
        let mut j = 0;
        for jj in 0..nm1 {
            if r_u <= p[jj] {
                j = jj;
                break;
            }
            j = jj;
        }
        ans[i] = perm[j];
    }
    ans
}

/// Simple reverse sort (descending) with permutation tracking.
fn revsort_with_perm(p: &mut [f64], perm: &mut [c_int], n: usize) {
    for i in 1..n {
        let pv = p[i];
        let iv = perm[i];
        let mut j = i;
        while j > 0 && p[j - 1] < pv {
            p[j] = p[j - 1];
            perm[j] = perm[j - 1];
            j -= 1;
        }
        p[j] = pv;
        perm[j] = iv;
    }
}

/// Unequal probability sampling without replacement using the R RNG.
pub fn ProbSampleNoReplace_r(n: usize, p: &mut [f64], nans: usize) -> Vec<c_int> {
    let mut perm: Vec<c_int> = (1..=n as c_int).collect();
    revsort_with_perm(p, &mut perm, n);
    let mut ans = vec![0 as c_int; nans];
    let mut totalmass = 1.0_f64;
    for i in 0..nans {
        let r_t = totalmass * r_unif_rand();
        let mut mass = 0.0;
        let mut j = n - 1 - i;
        for jj in 0..j {
            mass += p[jj];
            if r_t <= mass {
                j = jj;
                break;
            }
        }
        ans[i] = perm[j];
        totalmass -= p[j];
        for k in j..(n - 1 - i) {
            p[k] = p[k + 1];
            perm[k] = perm[k + 1];
        }
    }
    ans
}

/// Equal probability sampling with replacement using the R RNG.
pub fn SampleReplace_r(n: usize, nans: usize) -> Vec<c_int> {
    let mut ans = vec![0 as c_int; nans];
    for i in 0..nans {
        ans[i] = r_R_unif_index(n as f64) as c_int + 1;
    }
    ans
}

/// Equal probability sampling without replacement using the R RNG.
pub fn SampleNoReplace_r(n: usize, nans: usize) -> Vec<c_int> {
    let mut perm: Vec<c_int> = (1..=n as c_int).collect();
    for i in 0..nans {
        let remaining = (n - i) as f64;
        let j = r_R_unif_index(remaining) as usize;
        if j >= n - i {
            continue;
        }
        perm.swap(i, j);
    }
    perm[..nans].to_vec()
}

// ---------------------------------------------------------------------------
// S compatibility stubs
// ---------------------------------------------------------------------------

pub unsafe fn seed_in(ignored: *mut std::os::raw::c_long) {
    unsafe {
        let _ = ignored;
        GetRNGstate();
    }
}

pub unsafe fn seed_out(ignored: *mut std::os::raw::c_long) {
    unsafe {
        let _ = ignored;
        PutRNGstate();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::RSession;

    #[test]
    fn test_FixupProb_basic() {
        let mut p = vec![0.5, 0.3, 0.2];
        assert!(FixupProb(&mut p, 3, true).is_none());
        assert!((p[0] - 0.5).abs() < 1e-10);
        assert!((p[1] - 0.3).abs() < 1e-10);
        assert!((p[2] - 0.2).abs() < 1e-10);
    }

    #[test]
    fn test_FixupProb_normalize() {
        let mut p = vec![1.0, 2.0, 3.0];
        assert!(FixupProb(&mut p, 3, true).is_none());
        assert!((p[0] - 1.0 / 6.0).abs() < 1e-10);
        assert!((p[1] - 2.0 / 6.0).abs() < 1e-10);
        assert!((p[2] - 3.0 / 6.0).abs() < 1e-10);
    }

    #[test]
    fn test_FixupProb_negative() {
        let mut p = vec![0.5, -0.1, 0.6];
        assert!(FixupProb(&mut p, 3, true).is_some());
    }

    #[test]
    fn test_FixupProb_nan() {
        let mut p = vec![0.5, f64::NAN, 0.5];
        assert!(FixupProb(&mut p, 3, true).is_some());
    }

    #[test]
    fn test_FixupProb_zero() {
        let mut p = vec![0.0, 0.0, 0.0];
        assert!(FixupProb(&mut p, 1, true).is_some());
    }

    #[test]
    fn test_revsort_with_perm() {
        let mut p = vec![0.3, 0.1, 0.4, 0.2];
        let mut perm = vec![1, 2, 3, 4];
        revsort_with_perm(&mut p, &mut perm, 4);
        assert_eq!(p, vec![0.4, 0.3, 0.2, 0.1]);
        assert_eq!(perm[0], 3);
    }

    #[test]
    fn test_rng_state_creation() {
        let state = RNGState::new();
        assert_eq!(state.rng_kind.get(), RNG_DEFAULT);
        assert_eq!(state.n01_kind.get(), N01_DEFAULT);
        assert_eq!(state.sample_kind.get(), Sample_DEFAULT);
    }

    #[test]
    fn test_rng_init_mt() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
            assert_eq!(rng.rng_table[3].i_seed[0], MT_N as u32);
        });
    }

    #[test]
    fn test_unif_rand_range() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 12345);
        });
        for _ in 0..100 {
            let u = r_unif_rand();
            assert!(u > 0.0 && u < 1.0, "unif_rand returned {}", u);
        }
    }

    #[test]
    fn test_norm_rand_basic() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            rng.n01_kind.set(N01type::INVERSION);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 12345);
        });
        let mut sum = 0.0;
        let n = 1000;
        for _ in 0..n {
            let z = r_norm_rand();
            assert!(z.is_finite(), "norm_rand returned non-finite");
            sum += z;
        }
        let mean = sum / n as f64;
        assert!(mean.abs() < 0.2, "mean of norm_rand is {}", mean);
    }

    #[test]
    fn test_unif_rand_reproducibility() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        let v1: Vec<f64> = (0..10).map(|_| r_unif_rand()).collect();

        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        let v2: Vec<f64> = (0..10).map(|_| r_unif_rand()).collect();

        for (a, b) in v1.iter().zip(v2.iter()) {
            assert!(
                (a - b).abs() < 1e-15,
                "RNG not reproducible: {} vs {}",
                a,
                b
            );
        }
    }

    #[test]
    fn test_marsaglia_multicarry() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MARSAGLIA_MULTICARRY);
            RNG_Init(rng, RNGtype::MARSAGLIA_MULTICARRY, 42);
        });
        for _ in 0..100 {
            let u = r_unif_rand();
            assert!(u > 0.0 && u < 1.0, "Marsaglia unif_rand returned {}", u);
        }
    }

    #[test]
    fn test_wichmann_hill() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::WICHMANN_HILL);
            RNG_Init(rng, RNGtype::WICHMANN_HILL, 42);
        });
        for _ in 0..100 {
            let u = r_unif_rand();
            assert!(u > 0.0 && u < 1.0, "WH unif_rand returned {}", u);
        }
    }

    #[test]
    fn test_super_duper() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::SUPER_DUPER);
            RNG_Init(rng, RNGtype::SUPER_DUPER, 42);
        });
        for _ in 0..100 {
            let u = r_unif_rand();
            assert!(u > 0.0 && u < 1.0, "SD unif_rand returned {}", u);
        }
    }

    #[test]
    fn test_lecuyer_cmrg() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::LECUYER_CMRG);
            RNG_Init(rng, RNGtype::LECUYER_CMRG, 42);
        });
        for _ in 0..100 {
            let u = r_unif_rand();
            // CMRG does not use fixup — can return exactly 0.0 when p1 == p2
            assert!(u >= 0.0 && u < 1.0, "CMRG unif_rand returned {}", u);
        }
    }

    #[test]
    fn test_box_muller() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            rng.n01_kind.set(N01type::BOX_MULLER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 12345);
        });
        let mut sum = 0.0;
        let n = 1000;
        for _ in 0..n {
            let z = r_norm_rand();
            assert!(z.is_finite(), "Box-Muller norm_rand returned non-finite");
            sum += z;
        }
        let mean = sum / n as f64;
        assert!(mean.abs() < 0.2, "Box-Muller mean is {}", mean);
    }

    #[test]
    fn test_fixup() {
        assert!(fixup(0.0) > 0.0);
        assert!(fixup(1.0) < 1.0);
        assert_eq!(fixup(0.5), 0.5);
    }

    #[test]
    fn test_R_unif_index() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            rng.sample_kind.set(Sampletype::REJECTION);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        for _ in 0..100 {
            let idx = r_R_unif_index(10.0);
            assert!(idx >= 0.0 && idx < 10.0, "R_unif_index returned {}", idx);
        }
    }

    #[test]
    fn test_SampleReplace_r_range() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        let ans = SampleReplace_r(5, 100);
        for &a in &ans {
            assert!(a >= 1 && a <= 5);
        }
    }

    #[test]
    fn test_ProbSampleReplace_r_range() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        let mut p = vec![0.1, 0.2, 0.3, 0.4];
        let ans = ProbSampleReplace_r(4, &mut p, 10);
        for &a in &ans {
            assert!(a >= 1 && a <= 4);
        }
    }

    #[test]
    fn test_ProbSampleNoReplace_r_range() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        let mut p = vec![0.1, 0.2, 0.3, 0.4];
        let ans = ProbSampleNoReplace_r(4, &mut p, 2);
        assert_eq!(ans.len(), 2);
        for &a in &ans {
            assert!(a >= 1 && a <= 4);
        }
    }

    #[test]
    fn test_knuth_taocp2() {
        let _session = RSession::new();
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::KNUTH_TAOCP2);
            RNG_Init(rng, RNGtype::KNUTH_TAOCP2, 42);
        });
        for _ in 0..100 {
            let u = r_unif_rand();
            assert!(u > 0.0 && u < 1.0, "Knuth TAOCP2 unif_rand returned {}", u);
        }
    }

    #[test]
    fn test_rng_state_is_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let left_first = left.with_protected(|| {
            with_rng_state(|rng| {
                rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
                rng.n01_kind.set(N01type::INVERSION);
                rng.sample_kind.set(Sampletype::REJECTION);
                RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
            });
            r_unif_rand()
        });

        let right_first = right.with_protected(|| {
            with_rng_state(|rng| {
                assert_eq!(rng.rng_kind.get(), RNG_DEFAULT);
                rng.rng_kind.set(RNGtype::WICHMANN_HILL);
                RNG_Init(rng, RNGtype::WICHMANN_HILL, 99);
            });
            r_unif_rand()
        });

        let left_second = left.with_protected(|| {
            with_rng_state(|rng| {
                assert_eq!(rng.rng_kind.get(), RNGtype::MERSENNE_TWISTER);
                assert_eq!(rng.n01_kind.get(), N01type::INVERSION);
                assert_eq!(rng.sample_kind.get(), Sampletype::REJECTION);
            });
            r_unif_rand()
        });

        let left_replayed = left.with_protected(|| {
            with_rng_state(|rng| {
                rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
                RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
            });
            r_unif_rand()
        });

        assert_eq!(left_first, left_replayed);
        assert_ne!(left_first, right_first);
        assert_ne!(left_first, left_second);
    }
}
