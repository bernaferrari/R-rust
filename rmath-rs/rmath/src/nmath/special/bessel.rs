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
    pub fn Rf_bessel_i(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_i(x, alpha, expo != 0)
    }

    /// C FFI shim: bessel_i(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub fn bessel_i(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_i(x, alpha, expo != 0)
    }

    /// C FFI shim: Rf_bessel_j(x, alpha)
    pub fn Rf_bessel_j(x: c_double, alpha: c_double) -> c_double {
        super::bessel_j(x, alpha)
    }

    /// C FFI shim: bessel_j(x, alpha)
    pub fn bessel_j(x: c_double, alpha: c_double) -> c_double {
        super::bessel_j(x, alpha)
    }

    /// C FFI shim: Rf_bessel_k(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub fn Rf_bessel_k(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_k(x, alpha, expo != 0)
    }

    /// C FFI shim: bessel_k(x, alpha, expo)
    ///
    /// `expo` is interpreted as a C int: 0 = unscaled, nonzero = exponentiated.
    pub fn bessel_k(x: c_double, alpha: c_double, expo: c_int) -> c_double {
        super::bessel_k(x, alpha, expo != 0)
    }

    /// C FFI shim: Rf_bessel_y(x, alpha)
    pub fn Rf_bessel_y(x: c_double, alpha: c_double) -> c_double {
        super::bessel_y(x, alpha)
    }

    /// C FFI shim: bessel_y(x, alpha)
    pub fn bessel_y(x: c_double, alpha: c_double) -> c_double {
        super::bessel_y(x, alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        if a.is_nan() && b.is_nan() {
            return true;
        }
        if a.is_infinite() && b.is_infinite() {
            return a.signum() == b.signum();
        }
        (a - b).abs() < tol * b.abs().max(1.0)
    }

    #[test]
    fn test_bessel_j_basic() {
        assert!(approx_eq(bessel_j(0.0, 0.0), 1.0, 1e-10));
        assert!(approx_eq(bessel_j(5.0, 0.0), -0.17759677131433830, 1e-10));
        assert!(approx_eq(bessel_j(2.0, 1.0), 0.5767248077568734, 1e-10));
    }

    #[test]
    fn test_bessel_y_basic() {
        assert!(bessel_y(0.0, 0.0).is_nan() || bessel_y(0.0, 0.0).is_infinite());
        assert!(approx_eq(bessel_y(5.0, 0.0), -0.3085176254852234, 1e-8));
    }

    #[test]
    fn test_bessel_i_basic() {
        assert!(approx_eq(bessel_i(0.0, 0.0, false), 1.0, 1e-10));
        assert!(approx_eq(
            bessel_i(2.0, 1.0, false),
            1.590636854637329,
            1e-10
        ));
    }

    #[test]
    fn test_bessel_k_basic() {
        assert!(approx_eq(
            bessel_k(2.0, 0.0, false),
            0.11389387274953344,
            1e-8
        ));
        assert!(approx_eq(
            bessel_k(1.0, 1.0, false),
            0.6019072301972347,
            1e-8
        ));
    }

    #[test]
    fn test_bessel_i_expo_scaled() {
        let unscaled = bessel_i(10.0, 0.0, false);
        let scaled = bessel_i(10.0, 0.0, true);
        assert!(scaled.is_finite());
        assert!(scaled < unscaled);
    }

    #[test]
    fn test_bessel_k_expo_scaled() {
        let unscaled = bessel_k(2.0, 1.0, false);
        let scaled = bessel_k(2.0, 1.0, true);
        assert!(scaled.is_finite());
        assert!(scaled > unscaled);
    }

    #[test]
    fn test_bessel_negative_order() {
        assert!(bessel_j(2.0, -1.0).is_finite());
        assert!(bessel_y(2.0, -1.0).is_finite());
        assert!(bessel_i(2.0, -1.0, false).is_finite());
        assert!(bessel_k(2.0, -1.0, false).is_finite());
    }

    #[test]
    fn test_bessel_negative_x() {
        assert!(bessel_j(-1.0, 0.0).is_nan() || bessel_j(-1.0, 0.0).is_infinite());
    }

    #[test]
    fn test_ffi_shims() {
        assert_eq!(ffi::Rf_bessel_i(2.0, 1.0, 0), bessel_i(2.0, 1.0, false));
        assert_eq!(ffi::bessel_i(2.0, 1.0, 1), bessel_i(2.0, 1.0, true));
        assert_eq!(ffi::Rf_bessel_j(2.0, 1.0), bessel_j(2.0, 1.0));
        assert_eq!(ffi::Rf_bessel_k(2.0, 1.0, 0), bessel_k(2.0, 1.0, false));
        assert_eq!(ffi::Rf_bessel_y(5.0, 0.0), bessel_y(5.0, 0.0));
    }
}
