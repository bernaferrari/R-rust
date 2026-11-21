//! Chi-squared simulation for contingency tables
//! Port of r-source/src/library/stats/src/chisqsim.c

use std::os::raw::{c_double, c_int};

use crate::library::stats::rcont::rcont2_slices;
use crate::main::coerce::{asInteger, coerceVector};
use crate::main::random::{GetRNGstate, PutRNGstate};
use crate::sexp::accessors::{INTEGER, LENGTH, REAL, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

fn chisqsim(
    nrow: c_int,
    ncol: c_int,
    nrowt: &[c_int],
    ncolt: &[c_int],
    n: c_int,
    B: c_int,
    expected: &[c_double],
    observed: &mut [c_int],
    fact: &mut [c_double],
    jwork: &mut [c_int],
    results: &mut [c_double],
) {
    {
        // Calculate log-factorials: fact[i] = lgamma(i+1)
        fact[0] = 0.0;
        fact[1] = 0.0;
        let mut i: c_int = 2;
        while i <= n {
            fact[i as usize] = fact[(i - 1) as usize] + (i as c_double).ln();
            i += 1;
        }

        // RNG state is process-global behind the interpreter lock.
        GetRNGstate();

        let mut iter: c_int = 0;
        while iter < B {
            // Buffers satisfy the dimension contract; degenerate margins are
            // rejected by the checked wrapper instead of producing garbage.
            let _ = rcont2_slices(nrow, ncol, nrowt, ncolt, n, fact, jwork, observed);
            // Calculate chi-squared value from the random table
            let mut chisq: c_double = 0.0;
            let mut j: c_int = 0;
            while j < ncol {
                let mut i: c_int = 0;
                let mut ii = j * nrow;
                while i < nrow {
                    let e = expected[ii as usize];
                    let o = observed[ii as usize] as c_double;
                    chisq += (o - e) * (o - e) / e;
                    i += 1;
                    ii += 1;
                }
                j += 1;
            }
            results[iter as usize] = chisq;
            iter += 1;
        }

        // RNG state is process-global behind the interpreter lock.
        PutRNGstate();
    }
}

fn fisher_sim(
    nrow: c_int,
    ncol: c_int,
    nrowt: &[c_int],
    ncolt: &[c_int],
    n: c_int,
    B: c_int,
    observed: &mut [c_int],
    fact: &mut [c_double],
    jwork: &mut [c_int],
    results: &mut [c_double],
) {
    {
        // Calculate log-factorials: fact[i] = lgamma(i+1)
        fact[0] = 0.0;
        fact[1] = 0.0;
        let mut i: c_int = 2;
        while i <= n {
            fact[i as usize] = fact[(i - 1) as usize] + (i as c_double).ln();
            i += 1;
        }

        // RNG state is process-global behind the interpreter lock.
        GetRNGstate();

        let mut iter: c_int = 0;
        while iter < B {
            // Buffers satisfy the dimension contract; degenerate margins are
            // rejected by the checked wrapper instead of producing garbage.
            let _ = rcont2_slices(nrow, ncol, nrowt, ncolt, n, fact, jwork, observed);
            // Calculate log-prob value from the random table
            let mut ans: c_double = 0.0;
            let mut j: c_int = 0;
            while j < ncol {
                let mut i: c_int = 0;
                let mut ii = j * nrow;
                while i < nrow {
                    ans -= fact[observed[ii as usize] as usize];
                    i += 1;
                    ii += 1;
                }
                j += 1;
            }
            results[iter as usize] = ans;
            iter += 1;
        }

        // RNG state is process-global behind the interpreter lock.
        PutRNGstate();
    }
}

pub unsafe fn Fisher_sim(sr: SEXP, sc: SEXP, sB: SEXP) -> SEXP {
    unsafe {
        let sr = coerceVector(sr, SEXPTYPE::INTSXP.as_c_int());
        let _sr_guard = protect(sr);
        let sc = coerceVector(sc, SEXPTYPE::INTSXP.as_c_int());
        let _sc_guard = protect(sc);
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
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, B);
        let _ans_guard = protect(ans);
        fisher_sim(
            nr,
            nc,
            std::slice::from_raw_parts(isr, nr as usize),
            std::slice::from_raw_parts(INTEGER(sc), nc as usize),
            n,
            B,
            &mut observed,
            &mut fact,
            &mut jwork,
            std::slice::from_raw_parts_mut(REAL(ans), B as usize),
        );
        ans
    }
}

pub unsafe fn chisq_sim(sr: SEXP, sc: SEXP, sB: SEXP, E: SEXP) -> SEXP {
    unsafe {
        let sr = coerceVector(sr, SEXPTYPE::INTSXP.as_c_int());
        let _sr_guard = protect(sr);
        let sc = coerceVector(sc, SEXPTYPE::INTSXP.as_c_int());
        let _sc_guard = protect(sc);
        let E = coerceVector(E, SEXPTYPE::REALSXP.as_c_int());
        let _e_guard = protect(E);
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
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, B);
        let _ans_guard = protect(ans);
        chisqsim(
            nr,
            nc,
            std::slice::from_raw_parts(isr, nr as usize),
            std::slice::from_raw_parts(INTEGER(sc), nc as usize),
            n,
            B,
            std::slice::from_raw_parts(REAL(E), (nr * nc) as usize),
            &mut observed,
            &mut fact,
            &mut jwork,
            std::slice::from_raw_parts_mut(REAL(ans), B as usize),
        );
        ans
    }
}
