#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Partial autocorrelation sum and integration helpers
//! Port of r-source/src/library/stats/src/PPsum.c

use std::os::raw::{c_double, c_int};

use crate::main::coerce::{asInteger, coerceVector};
use crate::sexp::accessors::{LENGTH, REAL, TYPEOF};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

unsafe fn R_pp_sum(u: *const c_double, n: c_int, l: c_int) -> c_double {
    let mut tmp1: c_double = 0.0;
    let mut i: c_int = 1;
    while i <= l {
        let mut tmp2: c_double = 0.0;
        let mut j: c_int = i;
        while j < n {
            tmp2 += *u.add(j as usize) * *u.add((j - i) as usize);
            j += 1;
        }
        tmp2 *= 1.0 - i as c_double / (l as c_double + 1.0);
        tmp1 += tmp2;
        i += 1;
    }
    2.0 * tmp1 / n as c_double
}

pub unsafe fn pp_sum(u: SEXP, sl: SEXP) -> SEXP {
    let u = Rf_protect(coerceVector(u, SEXPTYPE::REALSXP.0));
    let n = LENGTH(u);
    let l = asInteger(sl);
    let trm = R_pp_sum(REAL(u), n, l);
    Rf_unprotect(1);
    Rf_ScalarReal(trm)
}

pub unsafe fn intgrt_vec(x: SEXP, xi: SEXP, slag: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(x, SEXPTYPE::REALSXP.0));
    let xi = Rf_protect(coerceVector(xi, SEXPTYPE::REALSXP.0));
    let n = LENGTH(x);
    let lag = asInteger(slag);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n + lag));
    let rx = REAL(x);
    let y = REAL(ans);

    let mut i: c_int = 0;
    while i < n + lag {
        *y.add(i as usize) = 0.0;
        i += 1;
    }
    let mut i: c_int = 0;
    while i < lag {
        *y.add(i as usize) = *REAL(xi).add(i as usize);
        i += 1;
    }
    let mut i = lag;
    while i < lag + n {
        *y.add(i as usize) = *rx.add((i - lag) as usize) + *y.add((i - lag) as usize);
        i += 1;
    }
    Rf_unprotect(3);
    ans
}
