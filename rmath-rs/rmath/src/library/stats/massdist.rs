//! Mass distribution for density estimation.
//! Port of r-source/src/library/stats/src/massdist.c
//!
//! BinDist distributes weighted observations into bins via linear interpolation.
//! The result is a vector of length 2*n where each pair (y[2*i], y[2*i+1])
//! represents the left and right bin densities. Only the lower half (indices
//! 0..n) is populated; the upper half (indices n..2*n) remains zero-padded.

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::{INTEGER, LENGTH, REAL, XLENGTH};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{NA_INTEGER, R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Helper: asInteger -- extract a scalar integer from an SEXP
// ---------------------------------------------------------------------------

/// Extract a scalar integer value from an SEXP.
/// Returns NA_INTEGER if the SEXP is NULL or not an integer/real vector.
unsafe fn as_integer(s: SEXP) -> c_int {
    if s.is_null() {
        return NA_INTEGER;
    }
    if INTEGER(s).is_null() {
        return NA_INTEGER;
    }
    *INTEGER(s)
}

// ---------------------------------------------------------------------------
// Helper: asReal -- extract a scalar double from an SEXP
// ---------------------------------------------------------------------------

/// Extract a scalar double value from an SEXP.
/// Returns NaN if the SEXP is NULL or has no real data.
unsafe fn as_real(s: SEXP) -> c_double {
    if s.is_null() {
        return f64::NAN; // NaN
    }
    if REAL(s).is_null() {
        return f64::NAN; // NaN
    }
    *REAL(s)
}

// ---------------------------------------------------------------------------
// BinDist -- binned density estimation
// ---------------------------------------------------------------------------

/// BinDist - mass distribution for density estimation.
///
/// Distributes weighted observations into bins via linear interpolation.
/// Each observation x[i] with weight w[i] is distributed between two
/// adjacent bin edges: the fraction (1-fx) goes to bin ix and the
/// fraction fx goes to bin ix+1, where fx is the fractional position
/// within the bin.
///
/// # Arguments
/// * `sx` - SEXP containing observation values (REALSXP vector)
/// * `sw` - SEXP containing weights (REALSXP vector)
/// * `slo` - SEXP containing the lower bound of the bin range (scalar REALSXP)
/// * `shi` - SEXP containing the upper bound of the bin range (scalar REALSXP)
/// * `sn` - SEXP containing the number of bins (scalar INTSXP)
///
/// # Returns
/// A REALSXP vector of length 2*n with the binned density values.
/// Only indices 0..n are populated; indices n..2*n are zero.
///
/// # Safety
/// All SEXP arguments must be valid, non-null pointers to properly
/// allocated R objects of the expected types.
pub unsafe fn BinDist(sx: SEXP, sw: SEXP, slo: SEXP, shi: SEXP, sn: SEXP) -> SEXP {
    let n = as_integer(sn);
    if n == NA_INTEGER || n <= 0 {
        // Return a length-0 real vector on error (matches R's error behavior
        // in this C-level function; the R wrapper calls error() itself).
        return Rf_allocVector(SEXPTYPE::REALSXP, 0);
    }

    let n_xlen = n as R_xlen_t;
    let ans = Rf_allocVector(SEXPTYPE::REALSXP, 2 * n);
    Rf_protect(ans);

    let xlo = as_real(slo);
    let xhi = as_real(shi);

    let x = REAL(sx);
    let w = REAL(sw);
    let y = REAL(ans);

    let ixmin: c_int = 0;
    let ixmax: c_int = n - 2;
    let xdelta: c_double = (xhi - xlo) / (n - 1) as c_double;

    let len = XLENGTH(sx);

    // Zero-initialize the output (the upper half is always zero-padded).
    // Rf_allocVector already zeroes memory in our arena, but we do it
    // explicitly to match the C code's behavior exactly.
    for i in 0..(2 * n_xlen) {
        *y.add(i as usize) = 0.0;
    }

    for i in 0..len {
        let xi = *x.add(i as usize);
        if R_FINITE(xi) {
            let xpos = (xi - xlo) / xdelta;
            // Avoid integer overflows for ix.
            if xpos > c_int::MAX as c_double || xpos < c_int::MIN as c_double {
                continue;
            }
            let ix = xpos.floor() as c_int;
            let fx = xpos - ix as c_double;
            let wi = *w.add(i as usize);
            if ixmin <= ix && ix <= ixmax {
                *y.add(ix as usize) += (1.0 - fx) * wi;
                *y.add((ix + 1) as usize) += fx * wi;
            } else if ix == -1 {
                *y.add(0) += fx * wi;
            } else if ix == ixmax + 1 {
                *y.add(ix as usize) += (1.0 - fx) * wi;
            }
        }
    }

    Rf_unprotect(1);
    ans
}
