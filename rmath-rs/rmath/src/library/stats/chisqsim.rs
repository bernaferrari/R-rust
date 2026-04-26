//! Chi-squared simulation for contingency tables
//! Port of r-source/src/library/stats/src/chisqsim.c

use std::os::raw::{c_double, c_int};

use crate::library::stats::rcont::rcont2;
use crate::main::coerce::{asInteger, coerceVector};
use crate::main::random::{GetRNGstate, PutRNGstate};
use crate::sexp::accessors::{INTEGER, LENGTH, REAL, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

unsafe fn chisqsim(
    nrow: c_int,
    ncol: c_int,
    nrowt: *const c_int,
    ncolt: *const c_int,
    n: c_int,
    B: c_int,
    expected: *const c_double,
    observed: *mut c_int,
    fact: *mut c_double,
    jwork: *mut c_int,
    results: *mut c_double,
) {
    unsafe {
        // Calculate log-factorials: fact[i] = lgamma(i+1)
        *fact = 0.0;
        *fact.add(1) = 0.0;
        let mut i: c_int = 2;
        while i <= n {
            *fact.add(i as usize) = *fact.add((i - 1) as usize) + (i as c_double).ln();
            i += 1;
        }

        GetRNGstate();

        let mut iter: c_int = 0;
        while iter < B {
            rcont2(nrow, ncol, nrowt, ncolt, n, fact, jwork, observed);
            // Calculate chi-squared value from the random table
            let mut chisq: c_double = 0.0;
            let mut j: c_int = 0;
            while j < ncol {
                let mut i: c_int = 0;
                let mut ii = j * nrow;
                while i < nrow {
                    let e = *expected.add(ii as usize);
                    let o = *observed.add(ii as usize) as c_double;
                    chisq += (o - e) * (o - e) / e;
                    i += 1;
                    ii += 1;
                }
                j += 1;
            }
            *results.add(iter as usize) = chisq;
            iter += 1;
        }

        PutRNGstate();
    }
}

unsafe fn fisher_sim(
    nrow: c_int,
    ncol: c_int,
    nrowt: *const c_int,
    ncolt: *const c_int,
    n: c_int,
    B: c_int,
    observed: *mut c_int,
    fact: *mut c_double,
    jwork: *mut c_int,
    results: *mut c_double,
) {
    unsafe {
        // Calculate log-factorials: fact[i] = lgamma(i+1)
        *fact = 0.0;
        *fact.add(1) = 0.0;
        let mut i: c_int = 2;
        while i <= n {
            *fact.add(i as usize) = *fact.add((i - 1) as usize) + (i as c_double).ln();
            i += 1;
        }

        GetRNGstate();

        let mut iter: c_int = 0;
        while iter < B {
            rcont2(nrow, ncol, nrowt, ncolt, n, fact, jwork, observed);
            // Calculate log-prob value from the random table
            let mut ans: c_double = 0.0;
            let mut j: c_int = 0;
            while j < ncol {
                let mut i: c_int = 0;
                let mut ii = j * nrow;
                while i < nrow {
                    ans -= *fact.add(*observed.add(ii as usize) as usize);
                    i += 1;
                    ii += 1;
                }
                j += 1;
            }
            *results.add(iter as usize) = ans;
            iter += 1;
        }

        PutRNGstate();
    }
}

pub unsafe fn Fisher_sim(sr: SEXP, sc: SEXP, sB: SEXP) -> SEXP {
    unsafe {
        let sr = Rf_protect(coerceVector(sr, SEXPTYPE::INTSXP.as_c_int()));
        let sc = Rf_protect(coerceVector(sc, SEXPTYPE::INTSXP.as_c_int()));
        let nr = LENGTH(sr);
        let nc = LENGTH(sc);
        let B = asInteger(sB);
        let isr = INTEGER(sr);
        let mut n: c_int = 0;
        let mut i: c_int = 0;
        while i < nr {
            n += *isr.add(i as usize);
            i += 1;
        }
        let mut observed = vec![0i32; (nr * nc) as usize];
        let mut fact = vec![0.0f64; (n + 1) as usize];
        let mut jwork = vec![0i32; nc as usize];
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, B));
        fisher_sim(
            nr,
            nc,
            isr,
            INTEGER(sc),
            n,
            B,
            observed.as_mut_ptr(),
            fact.as_mut_ptr(),
            jwork.as_mut_ptr(),
            REAL(ans),
        );
        Rf_unprotect(3);
        ans
    }
}

pub unsafe fn chisq_sim(sr: SEXP, sc: SEXP, sB: SEXP, E: SEXP) -> SEXP {
    unsafe {
        let sr = Rf_protect(coerceVector(sr, SEXPTYPE::INTSXP.as_c_int()));
        let sc = Rf_protect(coerceVector(sc, SEXPTYPE::INTSXP.as_c_int()));
        let E = Rf_protect(coerceVector(E, SEXPTYPE::REALSXP.as_c_int()));
        let nr = LENGTH(sr);
        let nc = LENGTH(sc);
        let B = asInteger(sB);
        let isr = INTEGER(sr);
        let mut n: c_int = 0;
        let mut i: c_int = 0;
        while i < nr {
            n += *isr.add(i as usize);
            i += 1;
        }
        let mut observed = vec![0i32; (nr * nc) as usize];
        let mut fact = vec![0.0f64; (n + 1) as usize];
        let mut jwork = vec![0i32; nc as usize];
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, B));
        chisqsim(
            nr,
            nc,
            isr,
            INTEGER(sc),
            n,
            B,
            REAL(E),
            observed.as_mut_ptr(),
            fact.as_mut_ptr(),
            jwork.as_mut_ptr(),
            REAL(ans),
        );
        Rf_unprotect(4);
        ans
    }
}
