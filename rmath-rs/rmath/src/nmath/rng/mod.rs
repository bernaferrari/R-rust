//! Random number generation modules.
//!
//! This module provides the core RNG (Marsaglia-MultiCarry) and
//! distribution-specific random variate generators ported from R's nmath.

mod base;
pub use base::*;
