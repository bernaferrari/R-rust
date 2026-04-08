// Ported from R's nmath/chebyshev.c
// chebyshev_init and chebyshev_eval
//
// Original by W. Fullerton, Los Alamos Scientific Laboratory.
// Based on Fortran routine dcsevl. Adapted from R. Broucke, Algorithm 446, CACM., 16, 254 (1973).

use crate::error::*;
use libm::*;

/// Determines the number of terms for the double precision orthogonal series `dos`
/// needed to ensure the error is no larger than `eta`.
/// Ordinarily eta will be chosen to be one-tenth machine precision.
///
/// Returns the number of terms to use (0 if nos < 1).
#[inline]
pub(crate) fn chebyshev_init(dos: &[f64], nos: usize, eta: f64) -> usize {
    if nos < 1 {
        return 0;
    }

    let mut err: f64 = 0.0;
    let mut i: usize = 0; // just to avoid compiler warnings
    let mut ii: usize = 1;
    while ii <= nos {
        i = nos - ii;
        err += fabs(dos[i]);
        if err > eta {
            return i;
        }
        ii += 1;
    }
    i
}

/// Evaluates the n-term Chebyshev series `a` at `x`.
/// NaNs propagated correctly.
pub(crate) fn chebyshev_eval(x: f64, a: &[f64], n: i32) -> f64 {
    if n < 1 || n > 1000 {
        return ml_warn_return_nan();
    }

    if x < -1.1 || x > 1.1 {
        return ml_warn_return_nan();
    }

    let twox: f64 = x * 2.0;
    let mut b2: f64 = 0.0;
    let mut b1: f64 = 0.0;
    let mut b0: f64 = 0.0;
    let mut i: i32 = 1;
    while i <= n {
        b2 = b1;
        b1 = b0;
        b0 = twox * b1 - b2 + a[(n - i) as usize];
        i += 1;
    }
    (b0 - b2) * 0.5
}

pub extern "C" fn Rf_chebyshev_init(
    dos: *const f64,
    nos: std::os::raw::c_int,
    eta: f64,
) -> std::os::raw::c_int {
    let dos_slice = unsafe { std::slice::from_raw_parts(dos, nos as usize) };
    chebyshev_init(dos_slice, nos as usize, eta) as std::os::raw::c_int
}

#[must_use]
pub extern "C" fn Rf_chebyshev_eval(x: f64, a: *const f64, n: std::os::raw::c_int) -> f64 {
    let a_slice = unsafe { std::slice::from_raw_parts(a, n as usize) };
    chebyshev_eval(x, a_slice, n)
}
