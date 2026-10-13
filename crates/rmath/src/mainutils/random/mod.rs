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

mod generators;
mod rng_core;
#[cfg(test)]
mod tests;

pub use generators::*;
pub use rng_core::*;
