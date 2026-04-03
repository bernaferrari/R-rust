//! R's nmath statistical library — pure mathematical core.
//!
//! Distributions, special functions, random number generation, and
//! density/quantile/probability helpers. Ported from R's `src/nmath/`.

#![allow(non_snake_case, non_upper_case_globals)]

pub mod constants;
pub mod dist;
pub mod dpq;
pub mod error;
pub mod fprec;
pub mod rng;
pub mod special;
pub mod utils;
