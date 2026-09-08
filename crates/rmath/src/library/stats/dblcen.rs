//! Double Centering for Classical Multidimensional Scaling.
//! Port of r-source/src/library/stats/src/dblcen.c

use std::os::raw::c_double;
use std::slice;

use crate::main::util_main::nrows;
use crate::sexp::accessors::REAL;
use crate::sexp::ffi::SEXP;

fn double_centre_square(a: &mut [c_double], n: usize) {
    let n_f = n as c_double;

    for row in 0..n {
        let mut sum = 0.0;
        for col in 0..n {
            sum += a[row + col * n];
        }
        let mean = sum / n_f;
        for col in 0..n {
            a[row + col * n] -= mean;
        }
    }

    for col in 0..n {
        let column = &mut a[col * n..(col + 1) * n];
        let mean = column.iter().sum::<c_double>() / n_f;
        for value in column {
            *value -= mean;
        }
    }
}

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
    let n = unsafe { nrows(A as *const std::ffi::c_void) };
    if n <= 0 {
        return A;
    }
    let n_s = n as usize;
    let len = n_s * n_s;
    let a = unsafe { slice::from_raw_parts_mut(REAL(A), len) };
    double_centre_square(a, n_s);

    A
}
