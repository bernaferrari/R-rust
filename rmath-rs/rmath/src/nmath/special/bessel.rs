// Unified Bessel function module.
//
// Provides a clean API for all four Bessel function families:
//   I (modified Bessel of the first kind)
//   J (Bessel of the first kind)
//   K (modified Bessel of the third kind)
//   Y (Bessel of the second kind)
//
// The underlying implementations live in bessel_i.rs, bessel_j.rs,
// bessel_k.rs, and bessel_y.rs, ported from R's nmath library.

use crate::nmath::special::bessel_i::bessel_i as bessel_i_impl;
use crate::nmath::special::bessel_j::bessel_j as bessel_j_impl;
use crate::nmath::special::bessel_k::bessel_k as bessel_k_impl;
use crate::nmath::special::bessel_y::bessel_y as bessel_y_impl;

/// Modified Bessel function of the first kind, I_alpha(x).
///
/// When `expo` is true, returns exp(-x) * I_alpha(x) (exponentially scaled).
///
/// # Arguments
/// * `x`     - Non-negative argument
/// * `alpha` - Order (may be negative)
/// * `expo`  - If true, return exp(-x)*I(x); if false, return I(x)
pub fn bessel_i(x: f64, alpha: f64, expo: bool) -> f64 {
    let expo_val = if expo { 2.0 } else { 1.0 };
    bessel_i_impl(x, alpha, expo_val)
}

/// Bessel function of the first kind, J_alpha(x).
///
/// # Arguments
/// * `x`     - Non-negative argument
/// * `alpha` - Order (may be negative)
pub fn bessel_j(x: f64, alpha: f64) -> f64 {
    bessel_j_impl(x, alpha)
}

/// Modified Bessel function of the third kind, K_alpha(x).
///
/// When `expo` is true, returns exp(x) * K_alpha(x) (exponentially scaled).
///
/// # Arguments
/// * `x`     - Non-negative argument
/// * `alpha` - Order (may be negative; absolute value is used)
/// * `expo`  - If true, return exp(x)*K(x); if false, return K(x)
pub fn bessel_k(x: f64, alpha: f64, expo: bool) -> f64 {
    let expo_val = if expo { 2.0 } else { 1.0 };
    bessel_k_impl(x, alpha, expo_val)
}

/// Bessel function of the second kind, Y_alpha(x).
///
/// # Arguments
/// * `x`     - Non-negative argument
/// * `alpha` - Order (may be negative)
pub fn bessel_y(x: f64, alpha: f64) -> f64 {
    bessel_y_impl(x, alpha)
}

// =====================================================================
// C FFI shims
//
// Placed in a private submodule to avoid name conflicts with the
// Rust functions above (Rust does not allow overloading by ABI).
// =====================================================================

mod ffi {
    use std::os::raw::{c_double, c_int};

    /// C FFI shim: Rf_bessel_i(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub extern "C" fn Rf_bessel_i(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_i(x, alpha, expo != 0)
    }

    /// C FFI shim: bessel_i(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub extern "C" fn bessel_i(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_i(x, alpha, expo != 0)
    }

    /// C FFI shim: Rf_bessel_j(x, alpha)
    pub extern "C" fn Rf_bessel_j(x: c_double, alpha: c_double) -> c_double {
        super::bessel_j(x, alpha)
    }

    /// C FFI shim: bessel_j(x, alpha)
    pub extern "C" fn bessel_j(x: c_double, alpha: c_double) -> c_double {
        super::bessel_j(x, alpha)
    }

    /// C FFI shim: Rf_bessel_k(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub extern "C" fn Rf_bessel_k(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_k(x, alpha, expo != 0)
    }

    /// C FFI shim: bessel_k(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub extern "C" fn bessel_k(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_k(x, alpha, expo != 0)
    }

    /// C FFI shim: Rf_bessel_y(x, alpha)
    pub extern "C" fn Rf_bessel_y(x: c_double, alpha: c_double) -> c_double {
        super::bessel_y(x, alpha)
    }

    /// C FFI shim: bessel_y(x, alpha)
    pub extern "C" fn bessel_y(x: c_double, alpha: c_double) -> c_double {
        super::bessel_y(x, alpha)
    }
}
