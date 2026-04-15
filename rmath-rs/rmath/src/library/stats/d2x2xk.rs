
//! Exact distribution for Cochran-Mantel-Haenszel test.
//! Port of r-source/src/library/stats/src/d2x2xk.c

use core::ffi::c_int;
use std::os::raw::c_double;
use std::os::raw::c_void;

use crate::main::coerce::{asInteger, coerceVector};
use crate::nmath::dist::hypergeometric::dhyper_inner;
use crate::nmath::utils::imax2;
use crate::sexp::accessors::REAL;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// Internal function: compute exact conditional distribution for
/// Cochran-Mantel-Haenszel test across K strata.
///
/// # Safety
/// m, n, t must be valid pointers to arrays of at least K doubles.
/// d must be a valid pointer to an array of at least rn doubles.
unsafe fn int_d2x2xk(
    K: c_int,
    m: *const c_double,
    n: *const c_double,
    t: *const c_double,
    d: *mut c_double,
) {
    let k = K as usize;

    // Allocate array of row pointers (K+1 entries)
    let c = R_alloc(std::mem::size_of::<*mut c_double>(), k + 1) as *mut *mut c_double;

    let mut l: c_int = 0;
    let mut y: c_int = 0;
    let mut z: c_int = 0;

    // c[0] has one element initialized to 1.0
    *c = R_alloc(std::mem::size_of::<c_double>(), 1) as *mut c_double;
    **c = 1.0;

    let mut m_ptr = m;
    let mut n_ptr = n;
    let mut t_ptr = t;

    for i in 0..k {
        y = imax2(0, (*t_ptr - *n_ptr) as c_int);
        z = std::cmp::min(*m_ptr as c_int, *t_ptr as c_int);
        *c.add(i + 1) =
            R_alloc(std::mem::size_of::<c_double>(), (l + z - y + 1) as usize) as *mut c_double;

        // Zero-initialize c[i+1][0..=l+z-y]
        let c_next = *c.add(i + 1);
        for j in 0..=(l + z - y) as usize {
            *c_next.add(j) = 0.0;
        }

        // Convolution step
        for j in 0..=(z - y) as usize {
            let u = dhyper_inner((j as c_int + y) as f64, *m_ptr, *n_ptr, *t_ptr, false);
            let c_prev = *c.add(i);
            for w in 0..=l as usize {
                *c_next.add(w + j) += *c_prev.add(w) * u;
            }
        }

        l = l + z - y;
        m_ptr = m_ptr.add(1);
        n_ptr = n_ptr.add(1);
        t_ptr = t_ptr.add(1);
    }

    // Normalize
    let mut u: c_double = 0.0;
    let c_k = *c.add(k);
    for j in 0..=l as usize {
        u += *c_k.add(j);
    }
    for j in 0..=l as usize {
        *d.add(j) = *c_k.add(j) / u;
    }
}

/// SEXP wrapper for d2x2xk.
///
/// Computes the exact conditional distribution for the
/// Cochran-Mantel-Haenszel test.
///
/// # Safety
/// sK, m, n, t, srn must be valid SEXP pointers.
pub unsafe fn d2x2xk(sK: SEXP, m: SEXP, n: SEXP, t: SEXP, srn: SEXP) -> SEXP {
    let K = asInteger(sK);
    let rn = asInteger(srn);

    let m = coerceVector(m, SEXPTYPE::REALSXP.0);
    let m = Rf_protect(m);
    let n = coerceVector(n, SEXPTYPE::REALSXP.0);
    let n = Rf_protect(n);
    let t = coerceVector(t, SEXPTYPE::REALSXP.0);
    let t = Rf_protect(t);

    let ans = Rf_allocVector(SEXPTYPE::REALSXP, rn as i32);
    let ans = Rf_protect(ans);

    int_d2x2xk(K, REAL(m), REAL(n), REAL(t), REAL(ans));

    Rf_unprotect(4);
    ans
}
