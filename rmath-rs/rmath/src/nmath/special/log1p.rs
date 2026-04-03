// Ported from R's nmath/log1p.c
//
// Compute the relative error logarithm: log(1 + x)
//
// On modern platforms with a working C99 log1p, R delegates to the
// system implementation. This module provides the same thin wrappers
// for Rust, delegating to libm::log1p and libm::expm1.

use libm::{expm1, log1p};

/// Compute log(1 + x) accurately for small x.
///
/// This is a direct wrapper around the system log1p implementation
/// (libm), which on modern platforms provides full double precision.
pub fn log1p_impl(x: f64) -> f64 {
    log1p(x)
}

/// Compute exp(x) - 1 accurately for small x.
pub fn expm1_impl(x: f64) -> f64 {
    expm1(x)
}

/// Compute log(1 + x) - x accurately for small x.
///
/// This is the same as log1pmx from gamma.rs, re-exported here
/// for convenience. It uses the continued fraction / series expansion
/// from Catherine Loader for high accuracy.
pub fn log1pmx(x: f64) -> f64 {
    crate::nmath::special::gamma::log1pmx(x)
}

/// Compute log(gamma(1 + x)) accurately also for small x (0 < x < 0.5).
///
/// This re-exports lgammafn1p from the gamma module.
pub fn lgamma1p_impl(x: f64) -> f64 {
    crate::nmath::special::gamma::lgammafn1p(x)
}

// =====================================================================
// C FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_log1p(x: f64) -> f64 {
    log1p(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log1p_c(x: f64) -> f64 {
    log1p(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_lgamma1p(x: f64) -> f64 {
    crate::nmath::special::gamma::lgammafn1p(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn lgamma1p_c(x: f64) -> f64 {
    crate::nmath::special::gamma::lgammafn1p(x)
}
