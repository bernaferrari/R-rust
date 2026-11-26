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
use crate::sexp::protect::protect;

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
    unsafe {
        let k = K as usize;

        // Convolution rows as owned vectors: row 0 starts as the unit mass,
        // each stratum appends a wider zeroed row convolved from its predecessor.
        let mut rows: Vec<Vec<c_double>> = Vec::with_capacity(k + 1);
        rows.push(vec![1.0]);

        let mut l: c_int = 0;
        let mut y: c_int = 0;
        let mut z: c_int = 0;

        let mut m_ptr = m;
        let mut n_ptr = n;
        let mut t_ptr = t;

        for i in 0..k {
            y = imax2(0, (*t_ptr - *n_ptr) as c_int);
            z = std::cmp::min(*m_ptr as c_int, *t_ptr as c_int);
            rows.push(vec![0.0; (l + z - y + 1) as usize]);

            // Convolution step: split so the predecessor and the fresh row can
            // be read and written in the same iteration.
            let (before, after) = rows.split_at_mut(i + 1);
            let c_prev = &before[i];
            let c_next = &mut after[0];
            for j in 0..=(z - y) as usize {
                let u = dhyper_inner((j as c_int + y) as f64, *m_ptr, *n_ptr, *t_ptr, false);
                for w in 0..=l as usize {
                    c_next[w + j] += c_prev[w] * u;
                }
            }

            l = l + z - y;
            m_ptr = m_ptr.add(1);
            n_ptr = n_ptr.add(1);
            t_ptr = t_ptr.add(1);
        }

        // Normalize
        let mut u: c_double = 0.0;
        let c_k = &rows[k];
        for j in 0..=l as usize {
            u += c_k[j];
        }
        for j in 0..=l as usize {
            *d.add(j) = c_k[j] / u;
        }
    }
}

/// SEXP wrapper for d2x2xk.
///
/// Computes the exact conditional distribution for the
/// Cochran-Mantel-Haenszel test.
///
/// # Safety
/// sK, m, n, t, srn must be valid SEXP pointers.
pub fn d2x2xk(sK: SEXP, m: SEXP, n: SEXP, t: SEXP, srn: SEXP) -> SEXP {
    let K = asInteger(sK);
    let rn = asInteger(srn);

    let m = coerceVector(m, SEXPTYPE::REALSXP.as_c_int());
    let _m_guard = protect(m);
    let n = coerceVector(n, SEXPTYPE::REALSXP.as_c_int());
    let _n_guard = protect(n);
    let t = coerceVector(t, SEXPTYPE::REALSXP.as_c_int());
    let _t_guard = protect(t);

    let ans = Rf_allocVector(SEXPTYPE::REALSXP, rn as i32);
    let _ans_guard = protect(ans);

    // SAFETY: `m`/`n`/`t` are protected REALSXPs of K cells and the protected
    // `ans` holds the `rn` cells the convolution writes.
    unsafe {
        int_d2x2xk(K, REAL(m), REAL(n), REAL(t), REAL(ans));
    }

    ans
}
