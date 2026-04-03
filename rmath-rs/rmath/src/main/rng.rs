#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/RNG.c -- Random Number Generator infrastructure.
//!
//! NOTE: The original rng.rs was corrupted (pre-existing file never committed to git).
//! This module now re-exports from random.rs which has the complete implementation.
//!
//! RNG type constants are defined here for compatibility with code that imports
//! `crate::main::rng::*`.

use std::os::raw::c_int;

// ---------------------------------------------------------------------------
// RNG type enumerations (matching R_ext/Random.h)
// ---------------------------------------------------------------------------

pub const WICHMANN_HILL: c_int = 0;
pub const MARSAGLIA_MULTICARRY: c_int = 1;
pub const SUPER_DUPER: c_int = 2;
pub const MERSENNE_TWISTER: c_int = 3;
pub const KNUTH_TAOCP: c_int = 4;
pub const USER_UNIF: c_int = 5;
pub const KNUTH_TAOCP2: c_int = 6;
pub const LECUYER_CMRG: c_int = 7;

/// Normal generator types.
pub const BUGGY_KINDERMAN_RAMAGE: c_int = 0;
pub const AHRENS_DIETER: c_int = 1;
pub const BOX_MULLER: c_int = 2;
pub const USER_NORM: c_int = 3;
pub const INVERSION: c_int = 4;
pub const KINDERMAN_RAMAGE: c_int = 5;

/// Sample types.
pub const ROUNDING: c_int = 0;
pub const REJECTION: c_int = 1;

// ---------------------------------------------------------------------------
// Re-exports from random.rs (the complete implementation)
// ---------------------------------------------------------------------------
