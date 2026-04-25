//! Double Centering for Classical Multidimensional Scaling.
//! Port of r-source/src/library/stats/src/dblcen.c

use core::ffi::c_int;
use std::os::raw::c_double;

use crate::main::util_main::nrows;
use crate::sexp::accessors::REAL;
use crate::sexp::ffi::SEXP;

/// DoubleCentre - double centering for classical MDS.
///
/// Takes a matrix SEXP, modifies in-place:
/// 1. Compute row means, subtract from each row
/// 2. Compute column means, subtract from each column
/// Returns the SEXP.
///
/// NB: this does not duplicate A.
///
/// # Safety
/// A must be a valid REALSXP matrix pointer.
pub unsafe fn DoubleCentre(A: SEXP) -> SEXP {
    let n = nrows(A as *const std::ffi::c_void);
    let a = REAL(A);
    let n_s = n as usize;
    let n_f = n as f64;

    // Subtract row means
    for i in 0..n_s {
        let mut sum: c_double = 0.0;
        for j in 0..n_s {
            sum += *a.add(i + j * n_s);
        }
        sum /= n_f;
        for j in 0..n_s {
            *a.add(i + j * n_s) -= sum;
        }
    }

    // Subtract column means
    for j in 0..n_s {
        let mut sum: c_double = 0.0;
        for i in 0..n_s {
            sum += *a.add(i + j * n_s);
        }
        sum /= n_f;
        for i in 0..n_s {
            *a.add(i + j * n_s) -= sum;
        }
    }

    A
}
