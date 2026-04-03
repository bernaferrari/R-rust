//! R Mathematical Library - Rust Implementation
//!
//! This crate provides a Rust implementation of R's nmath library,
//! containing special mathematical functions, statistical distributions,
//! and random number generation.
//!
//! # Architecture
//!
//! - `constants`: Mathematical constants and error codes
//! - `error`: Error handling (ML_WARNING, etc.)
//! - `utils`: Basic utility functions (fmax2, sign, etc.)
//! - `special`: Special mathematical functions (gamma, beta, bessel)
//! - `dist`: Statistical distributions (normal, gamma, beta, etc.)
//! - `rng`: Random number generation
//!
//! # Compatibility
//!
//! This implementation aims for bit-exact compatibility with R's nmath
//! for all reference inputs, enabling differential testing.

#![cfg_attr(not(feature = "standalone"), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod constants;
pub mod error;
pub mod utils;

pub mod dist;
pub mod rng;
pub mod special;

// Re-export commonly used items
pub use constants::*;
pub use error::{ml_warn_return_nan, ml_warning};
pub use utils::{cospi, fmax2, fmin2, fsign, ftrunc, imax2, imin2, sign, sinpi, tanpi};

#[cfg(feature = "standalone")]
pub use utils::{fprec, fround, R_finite, R_pow, R_pow_di};

/// Version of this library
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert!(ML_POSINF.is_infinite());
        assert!(ML_NEGINF.is_infinite());
        assert!(ML_NAN.is_nan());
        assert!(r_finite(1.0));
        assert!(!r_finite(ML_POSINF));
        assert!(!r_finite(ML_NAN));
    }
}
