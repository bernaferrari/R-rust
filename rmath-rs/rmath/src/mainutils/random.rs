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

unsafe fn error(msg: &str) {
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

type Int32 = u32; // unsigned 32-bit, matching R's typedef

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RNG_DEFAULT: RNGtype = RNGtype::MERSENNE_TWISTER;
const N01_DEFAULT: N01type = N01type::INVERSION;
const Sample_DEFAULT: Sampletype = Sampletype::REJECTION;

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
pub(crate) struct RNGState {
    rng_kind: Cell<RNGtype>,
    n01_kind: Cell<N01type>,
    sample_kind: Cell<Sampletype>,
    rng_table: [RNGTab; 8],
    // Mersenne Twister state (stored separately for direct access)
    mt: [u32; MT_N],
    // Knuth TAOCP state
    kt_x: [i64; KT_KK],
    kt_ran_arr_buf: [i64; KT_QUALITY],
    kt_ran_arr_ptr: Cell<usize>,
    kt_pos: Cell<usize>,
    // Box-Muller saved value
    bm_norm_keep: Cell<f64>,
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
            rng_table,
            mt: [0u32; MT_N],
            kt_x: [0i64; KT_KK],
            kt_ran_arr_buf: [0i64; KT_QUALITY],
            kt_ran_arr_ptr: Cell::new(0),
            kt_pos: Cell::new(100),
            bm_norm_keep: Cell::new(0.0),
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

fn mt_sgenrand(rng: &mut RNGState, seed: u32) {
    let mut s = seed;
    for i in 0..MT_N {
        rng.mt[i] = s & 0xffff0000;
        s = 69069u32.wrapping_mul(s).wrapping_add(1);
        rng.mt[i] |= (s & 0xffff0000) >> 16;
        s = 69069u32.wrapping_mul(s).wrapping_add(1);
    }
}

fn mt_genrand(rng: &mut RNGState) -> f64 {
    let rng_kind = rng.rng_kind.get();
    let mti = rng.rng_table[rng_kind as usize].i_seed[0] as usize;

    if mti >= MT_N {
        if mti == MT_N + 1 {
            // Not initialized, use default seed
            mt_sgenrand(rng, 4357);
        }

        // Generate N words at one time
        let mut kk = 0usize;
        while kk < MT_N - MT_M {
            let y = (rng.mt[kk] & MT_UPPER_MASK) | (rng.mt[kk + 1] & MT_LOWER_MASK);
            rng.mt[kk] = rng.mt[kk + MT_M] ^ (y >> 1) ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
            kk += 1;
        }
        while kk < MT_N - 1 {
            let y = (rng.mt[kk] & MT_UPPER_MASK) | (rng.mt[kk + 1] & MT_LOWER_MASK);
            // M - N = 397 - 624 = -227, which wraps in unsigned arithmetic
            rng.mt[kk] = rng.mt[kk.wrapping_add(MT_M).wrapping_sub(MT_N)]
                ^ (y >> 1)
                ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };
            kk += 1;
        }
        let y = (rng.mt[MT_N - 1] & MT_UPPER_MASK) | (rng.mt[0] & MT_LOWER_MASK);
        rng.mt[MT_N - 1] = rng.mt[MT_M - 1] ^ (y >> 1) ^ if (y & 1) != 0 { MT_MATRIX_A } else { 0 };

        rng.rng_table[rng_kind as usize].i_seed[0] = 0; // mti = 0
    }

    let mut y = rng.mt[rng.rng_table[rng_kind as usize].i_seed[0] as usize];
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
            let n_seed = rng.rng_table[kind as usize].n_seed;
            for j in 0..n_seed {
                seed = seed.wrapping_mul(69069).wrapping_add(1);
                rng.rng_table[kind as usize].i_seed[j] = seed as u32;
            }
            // For MT, also fill the mt array
            if kind == RNGtype::MERSENNE_TWISTER {
                rng.rng_table[kind as usize].i_seed[0] = MT_N as u32;
                for j in 0..MT_N {
                    seed = seed.wrapping_mul(69069).wrapping_add(1);
                    rng.mt[j] = seed as u32;
                    rng.rng_table[kind as usize].i_seed[j + 1] = seed as u32;
                }
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
                while (seed as i64) >= CMRG_M2 {
                    seed = seed.wrapping_mul(69069).wrapping_add(1);
                }
                rng.rng_table[kind as usize].i_seed[j] = seed as u32;
            }
        }
        RNGtype::USER_UNIF => {
            // Not implemented -- requires user-supplied function
        }
    }
}

fn RNG_Init_KT(rng: &mut RNGState, seed: i64) {
    let s = seed.rem_euclid(1073741821);
    ran_start(rng, s);
    rng.kt_pos.set(100);
    rng.rng_table[RNGtype::KNUTH_TAOCP as usize].i_seed[KT_KK] = 100;
}

fn RNG_Init_KT2(rng: &mut RNGState, seed: i64) {
    let s = seed.rem_euclid(1073741821);
    ran_start(rng, s);
    rng.kt_pos.set(100);
    rng.rng_table[RNGtype::KNUTH_TAOCP2 as usize].i_seed[KT_KK] = 100;
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

// ---------------------------------------------------------------------------
// GetRNGstate / PutRNGstate
// ---------------------------------------------------------------------------

/// Get the .Random.seed into proper variables.
pub unsafe fn GetRNGstate() {
    unsafe {
        let seeds_sym = R_SeedsSymbol();
        let seeds = R_findVarInFrame(R_GlobalEnv(), seeds_sym);

        if seeds == R_UnboundValue() {
            // No .Random.seed -- randomize
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, TimeToSeed() as i64);
            });
            return;
        }

        // Check if PROMSXP
        let ty = TYPEOF(seeds);
        if ty == SEXPTYPE::PROMSXP {
            // Would need eval -- for now just randomize
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, TimeToSeed() as i64);
            });
            return;
        }

        if ty != SEXPTYPE::INTSXP {
            with_rng_state(|rng| {
                RNG_Init(rng, RNG_DEFAULT, TimeToSeed() as i64);
                rng.rng_kind.set(RNG_DEFAULT);
                rng.n01_kind.set(N01_DEFAULT);
                rng.sample_kind.set(Sample_DEFAULT);
            });
            return;
        }

        let is = INTEGER(seeds);
        let tmp = *is.add(0);
        if tmp == NA_INTEGER || tmp < 0 || tmp > 11000 {
            with_rng_state(|rng| {
                RNG_Init(rng, RNG_DEFAULT, TimeToSeed() as i64);
                rng.rng_kind.set(RNG_DEFAULT);
                rng.n01_kind.set(N01_DEFAULT);
                rng.sample_kind.set(Sample_DEFAULT);
            });
            return;
        }

        let new_rng = tmp % 100;
        let new_n01 = (tmp % 10000) / 100;
        let new_sample = tmp / 10000;

        if new_n01 > 5 || new_sample > 1 {
            with_rng_state(|rng| {
                RNG_Init(rng, RNG_DEFAULT, TimeToSeed() as i64);
                rng.rng_kind.set(RNG_DEFAULT);
                rng.n01_kind.set(N01_DEFAULT);
                rng.sample_kind.set(Sample_DEFAULT);
            });
            return;
        }

        let rng_kind = match new_rng {
            0 => RNGtype::WICHMANN_HILL,
            1 => RNGtype::MARSAGLIA_MULTICARRY,
            2 => RNGtype::SUPER_DUPER,
            3 => RNGtype::MERSENNE_TWISTER,
            4 => RNGtype::KNUTH_TAOCP,
            5 => RNGtype::USER_UNIF,
            6 => RNGtype::KNUTH_TAOCP2,
            7 => RNGtype::LECUYER_CMRG,
            _ => {
                with_rng_state(|rng| {
                    RNG_Init(rng, RNG_DEFAULT, TimeToSeed() as i64);
                });
                return;
            }
        };

        let n01_kind = match new_n01 {
            0 => N01type::BUGGY_KINDERMAN_RAMAGE,
            1 => N01type::AHRENS_DIETER,
            2 => N01type::BOX_MULLER,
            3 => N01type::USER_NORM,
            4 => N01type::INVERSION,
            5 => N01type::KINDERMAN_RAMAGE,
            _ => N01_DEFAULT,
        };

        let sample_kind = if new_sample == 0 {
            Sampletype::ROUNDING
        } else {
            Sampletype::REJECTION
        };

        let len_seed = match rng_kind {
            RNGtype::WICHMANN_HILL => 3,
            RNGtype::MARSAGLIA_MULTICARRY => 2,
            RNGtype::SUPER_DUPER => 2,
            RNGtype::MERSENNE_TWISTER => 1 + MT_N,
            RNGtype::KNUTH_TAOCP | RNGtype::KNUTH_TAOCP2 => 1 + KT_KK,
            RNGtype::USER_UNIF => 0,
            RNGtype::LECUYER_CMRG => 6,
        };

        let seeds_len = XLENGTH(seeds) as usize;

        with_rng_state(|rng| {
            rng.rng_kind.set(rng_kind);
            rng.n01_kind.set(n01_kind);
            rng.sample_kind.set(sample_kind);

            if seeds_len == 1 && rng_kind != RNGtype::USER_UNIF {
                RNG_Init(rng, rng_kind, TimeToSeed() as i64);
            } else if seeds_len > 1 {
                // Copy seeds in
                for j in 0..len_seed {
                    if j + 1 < seeds_len {
                        rng.rng_table[rng_kind as usize].i_seed[j] = *is.add(j + 1) as u32;
                    }
                }
                // For MT, also copy into mt array
                if rng_kind == RNGtype::MERSENNE_TWISTER {
                    for j in 0..MT_N {
                        if j + 1 < seeds_len {
                            rng.mt[j] = *is.add(j + 1) as u32;
                        }
                    }
                }
                // For Knuth TAOCP, copy into kt_x
                if rng_kind == RNGtype::KNUTH_TAOCP || rng_kind == RNGtype::KNUTH_TAOCP2 {
                    for j in 0..KT_KK {
                        if j + 1 < seeds_len {
                            rng.kt_x[j] = *is.add(j + 1) as i64;
                        }
                    }
                }
                FixupSeeds(rng, rng_kind, false);
            }
        });
    }
}

/// Copy seeds out to .Random.seed.
pub unsafe fn PutRNGstate() {
    unsafe {
        let (rng_kind, n01_kind, sample_kind, len_seed, seeds_vec) = with_rng_state(|rng| {
            let kind = rng.rng_kind.get();
            let n01 = rng.n01_kind.get();
            let samp = rng.sample_kind.get();
            let ls = rng.rng_table[kind as usize].n_seed;

            // Build the full seed vector: [kinds, i_seed[0], i_seed[1], ...]
            let mut sv = vec![0i32; ls + 1];
            sv[0] = kind as i32 + 100 * n01 as i32 + 10000 * samp as i32;
            for j in 0..ls {
                sv[j + 1] = rng.rng_table[kind as usize].i_seed[j] as i32;
            }
            (kind, n01, samp, ls, sv)
        });

        if rng_kind as i32 > 7 || n01_kind as i32 > 5 || sample_kind as i32 > 1 {
            return;
        }

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
    let kind = newkind;

    // Validate
    match kind {
        RNGtype::WICHMANN_HILL
        | RNGtype::MARSAGLIA_MULTICARRY
        | RNGtype::SUPER_DUPER
        | RNGtype::MERSENNE_TWISTER
        | RNGtype::KNUTH_TAOCP
        | RNGtype::USER_UNIF
        | RNGtype::KNUTH_TAOCP2
        | RNGtype::LECUYER_CMRG => {}
    }

    // Get current state and generate a seed from it
    let u = r_unif_rand();
    let seed = if u < 0.0 || u > 1.0 {
        TimeToSeed() as i64
    } else {
        (u * u32::MAX as f64) as i64
    };

    with_rng_state(|rng| {
        RNG_Init(rng, kind, seed);
        rng.rng_kind.set(kind);
    });

    // Put the new state
    unsafe {
        PutRNGstate();
    }
}

fn r_Norm_kind(kind: N01type) {
    if kind as i32 > 5 {
        return;
    }

    if kind == N01type::BOX_MULLER {
        with_rng_state(|rng| rng.bm_norm_keep.set(0.0));
    }

    with_rng_state(|rng| rng.n01_kind.set(kind));
    unsafe {
        PutRNGstate();
    }
}

fn r_Samp_kind(kind: Sampletype) {
    if kind as i32 > 1 {
        return;
    }
    with_rng_state(|rng| rng.sample_kind.set(kind));
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
// do_RNGkind -- R's RNGkind() function
// ---------------------------------------------------------------------------

pub unsafe fn do_RNGkind(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        GetRNGstate();

        let (rng_kind, n01_kind, sample_kind) = with_rng_state(|rng| {
            (
                rng.rng_kind.get() as c_int,
                rng.n01_kind.get() as c_int,
                rng.sample_kind.get() as c_int,
            )
        });

        let ans = Rf_allocVector(SEXPTYPE::INTSXP, 3);
        let _ans_guard = protect(ans);
        *INTEGER(ans).add(0) = rng_kind;
        *INTEGER(ans).add(1) = n01_kind;
        *INTEGER(ans).add(2) = sample_kind;

        let rng_arg = CAR(args);
        let norm_arg = CADR(args);
        let sample_arg = CADDR(args);

        if !isNull(rng_arg) {
            let v = asInteger_local(rng_arg);
            if v != NA_INTEGER {
                let kind = match v {
                    0 => RNGtype::WICHMANN_HILL,
                    1 => RNGtype::MARSAGLIA_MULTICARRY,
                    2 => RNGtype::SUPER_DUPER,
                    3 => RNGtype::MERSENNE_TWISTER,
                    4 => RNGtype::KNUTH_TAOCP,
                    5 => RNGtype::USER_UNIF,
                    6 => RNGtype::KNUTH_TAOCP2,
                    7 => RNGtype::LECUYER_CMRG,
                    _ => RNGtype::MERSENNE_TWISTER,
                };
                r_RNGkind(kind);
            }
        }
        if !isNull(norm_arg) {
            let v = asInteger_local(norm_arg);
            if v != NA_INTEGER {
                let kind = match v {
                    0 => N01type::BUGGY_KINDERMAN_RAMAGE,
                    1 => N01type::AHRENS_DIETER,
                    2 => N01type::BOX_MULLER,
                    3 => N01type::USER_NORM,
                    4 => N01type::INVERSION,
                    5 => N01type::KINDERMAN_RAMAGE,
                    _ => N01type::INVERSION,
                };
                r_Norm_kind(kind);
            }
        }
        if !isNull(sample_arg) {
            let v = asInteger_local(sample_arg);
            if v != NA_INTEGER {
                let kind = if v == 0 {
                    Sampletype::ROUNDING
                } else {
                    Sampletype::REJECTION
                };
                r_Samp_kind(kind);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_setseed -- R's set.seed() function
// ---------------------------------------------------------------------------

pub unsafe fn do_setseed(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let seed: i64;
        if !isNull(CAR(args)) {
            let v = asInteger_local(CAR(args));
            if v == NA_INTEGER {
                error("supplied seed is not a valid integer");
            }
            seed = v as i64;
        } else {
            seed = TimeToSeed() as i64;
        }

        let skind = CADR(args);
        let nkind = CADDR(args);
        let sampkind = CADDDR(args);

        if !isNull(skind) {
            let v = asInteger_local(skind);
            if v != NA_INTEGER {
                let kind = match v {
                    0 => RNGtype::WICHMANN_HILL,
                    1 => RNGtype::MARSAGLIA_MULTICARRY,
                    2 => RNGtype::SUPER_DUPER,
                    3 => RNGtype::MERSENNE_TWISTER,
                    4 => RNGtype::KNUTH_TAOCP,
                    5 => RNGtype::USER_UNIF,
                    6 => RNGtype::KNUTH_TAOCP2,
                    7 => RNGtype::LECUYER_CMRG,
                    _ => RNGtype::MERSENNE_TWISTER,
                };
                r_RNGkind(kind);
            }
        }
        if !isNull(nkind) {
            let v = asInteger_local(nkind);
            if v != NA_INTEGER {
                let kind = match v {
                    0 => N01type::BUGGY_KINDERMAN_RAMAGE,
                    1 => N01type::AHRENS_DIETER,
                    2 => N01type::BOX_MULLER,
                    3 => N01type::USER_NORM,
                    4 => N01type::INVERSION,
                    5 => N01type::KINDERMAN_RAMAGE,
                    _ => N01type::INVERSION,
                };
                r_Norm_kind(kind);
            }
        }
        if !isNull(sampkind) {
            let v = asInteger_local(sampkind);
            if v != NA_INTEGER {
                let kind = if v == 0 {
                    Sampletype::ROUNDING
                } else {
                    Sampletype::REJECTION
                };
                r_Samp_kind(kind);
            }
        }

        // Initialize the RNG with the seed
        with_rng_state(|rng| {
            let kind = rng.rng_kind.get();
            RNG_Init(rng, kind, seed);
        });
        PutRNGstate();

        R_NilValue()
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
