/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-2016  The R Core Team
 *
 *  Ported to Rust from r-source/src/library/stats/src/integrate.c
 */

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::appl::integrate::{Rdqagi, Rdqags};
use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// IntStruct -- integration callback state
// ---------------------------------------------------------------------------

struct IntStruct {
    f: SEXP,
    env: SEXP,
}

// ---------------------------------------------------------------------------
// Rintfn -- the integrand function called by the quadrature routines
// ---------------------------------------------------------------------------

fn Rintfn(is: &IntStruct, x: &mut [c_double]) {
    {
        let n = x.len() as c_int;

        let args = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _args_guard = protect(args);
        for i in 0..n {
            SET_REAL_ELT(args, i as i32, x[i as usize]);
        }

        let tmp = Rf_lang2(is.f, args);
        let _tmp_guard = protect(tmp);
        let resultsxp = eval(tmp, is.env);
        let _resultsxp_guard = protect(resultsxp);

        // Check length
        let rlen = LENGTH(resultsxp);
        if rlen != n {
            Rf_error_cstr(c"evaluation of function gave a result of wrong length");
        }

        // Check type and coerce if needed
        let mut coerced_guard = None;
        let resultsxp = if TYPEOF(resultsxp) == SEXPTYPE::INTSXP {
            let coerced = coerceVector(resultsxp, SEXPTYPE::REALSXP.as_c_int());
            coerced_guard = Some(protect(coerced));
            coerced
        } else if TYPEOF(resultsxp) != SEXPTYPE::REALSXP {
            Rf_error_cstr(c"evaluation of function gave a result of wrong type");
            unreachable!();
        } else {
            resultsxp
        };

        for i in 0..n {
            x[i as usize] = REAL_ELT(resultsxp, i as i32);
            if !R_FINITE(x[i as usize]) {
                Rf_error_cstr(c"non-finite function value");
            }
        }
        drop(coerced_guard);
    }
}

// ---------------------------------------------------------------------------
// Helper: asReal
// ---------------------------------------------------------------------------

fn as_real(x: SEXP) -> c_double {
    if x.is_null() {
        return NA_REAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::REALSXP {
        return REAL_ELT(x, 0);
    }
    if t == SEXPTYPE::INTSXP {
        let v = INTEGER_ELT(x, 0);
        if v == NA_INTEGER {
            return NA_REAL;
        }
        return v as c_double;
    }
    if t == SEXPTYPE::LGLSXP {
        let v = INTEGER_ELT(x, 0);
        if v == NA_INTEGER {
            return NA_REAL;
        }
        return if v != 0 { 1.0 } else { 0.0 };
    }
    NA_REAL
}

// ---------------------------------------------------------------------------
// Helper: asInteger
// ---------------------------------------------------------------------------

fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        return INTEGER_ELT(x, 0);
    }
    if t == SEXPTYPE::REALSXP {
        let v = REAL_ELT(x, 0);
        if v.is_nan() || v < c_int::MIN as c_double || v > c_int::MAX as c_double {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP {
        return INTEGER_ELT(x, 0);
    }
    NA_INTEGER
}

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

fn eval(call: SEXP, rho: SEXP) -> SEXP {
    crate::eval::eval::eval_inner(call, rho)
}

fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

// ---------------------------------------------------------------------------
// Helper: build result list
// ---------------------------------------------------------------------------

fn build_integrate_result(result: c_double, abserr: c_double, last: c_int, ier: c_int) -> SEXP {
    let ans = Rf_allocVector(SEXPTYPE::VECSXP, 4);
    let _ans_guard = protect(ans);
    let ansnames = Rf_allocVector(SEXPTYPE::STRSXP, 4);
    let _ansnames_guard = protect(ansnames);

    SET_STRING_ELT(
        ansnames,
        0,
        crate::library::stats::checked::mk_char("value"),
    );
    SET_VECTOR_ELT(ans, 0, Rf_allocVector(SEXPTYPE::REALSXP, 1));
    SET_REAL_ELT(VECTOR_ELT(ans, 0), 0, result);

    SET_STRING_ELT(
        ansnames,
        1,
        crate::library::stats::checked::mk_char("abs.error"),
    );
    SET_VECTOR_ELT(ans, 1, Rf_allocVector(SEXPTYPE::REALSXP, 1));
    SET_REAL_ELT(VECTOR_ELT(ans, 1), 0, abserr);

    SET_STRING_ELT(
        ansnames,
        2,
        crate::library::stats::checked::mk_char("subdivisions"),
    );
    SET_VECTOR_ELT(ans, 2, Rf_allocVector(SEXPTYPE::INTSXP, 1));
    SET_INTEGER_ELT(VECTOR_ELT(ans, 2), 0, last);

    SET_STRING_ELT(ansnames, 3, crate::library::stats::checked::mk_char("ierr"));
    SET_VECTOR_ELT(ans, 3, Rf_allocVector(SEXPTYPE::INTSXP, 1));
    SET_INTEGER_ELT(VECTOR_ELT(ans, 3), 0, ier);

    setAttrib(ans, R_NamesSymbol(), ansnames);
    ans
}

// ---------------------------------------------------------------------------
// call_dqags -- finite interval integration
// ---------------------------------------------------------------------------

pub fn call_dqags(args: SEXP) -> SEXP {
    let mut is = IntStruct {
        f: ptr::null_mut(),
        env: ptr::null_mut(),
    };

    let mut args = CDR(args);
    is.f = CAR(args);
    args = CDR(args);
    is.env = CAR(args);
    args = CDR(args);

    if LENGTH(CAR(args)) > 1 {
        Rf_error_cstr(c"'lower' must be of length one");
    }
    let mut lower = as_real(CAR(args));
    args = CDR(args);

    if LENGTH(CAR(args)) > 1 {
        Rf_error_cstr(c"'upper' must be of length one");
    }
    let mut upper = as_real(CAR(args));
    args = CDR(args);

    let mut epsabs = as_real(CAR(args));
    args = CDR(args);
    let mut epsrel = as_real(CAR(args));
    args = CDR(args);
    let mut limit = as_integer(CAR(args));
    args = CDR(args);

    let lenw = 4 * limit;
    let mut iwork = vec![0 as c_int; limit.max(0) as usize];
    let mut work = vec![0.0 as c_double; lenw.max(0) as usize];

    let mut result: c_double = 0.0;
    let mut abserr: c_double = 0.0;
    let mut neval: c_int = 0;
    let mut ier: c_int = 0;
    let mut last: c_int = 0;
    let mut lenw_out: c_int = lenw;

    Rdqags(
        &mut |x| Rintfn(&is, x),
        lower,
        upper,
        epsabs,
        epsrel,
        &mut result,
        &mut abserr,
        &mut neval,
        &mut ier,
        limit,
        lenw_out,
        &mut last,
        &mut iwork,
        &mut work,
    );

    build_integrate_result(result, abserr, last, ier)
}

// ---------------------------------------------------------------------------
// call_dqagi -- infinite interval integration
// ---------------------------------------------------------------------------

pub fn call_dqagi(args: SEXP) -> SEXP {
    let mut is = IntStruct {
        f: ptr::null_mut(),
        env: ptr::null_mut(),
    };

    let mut args = CDR(args);
    is.f = CAR(args);
    args = CDR(args);
    is.env = CAR(args);
    args = CDR(args);

    if LENGTH(CAR(args)) > 1 {
        Rf_error_cstr(c"'bound' must be of length one");
    }
    let mut bound = as_real(CAR(args));
    args = CDR(args);
    let mut inf = as_integer(CAR(args));
    args = CDR(args);

    let mut epsabs = as_real(CAR(args));
    args = CDR(args);
    let mut epsrel = as_real(CAR(args));
    args = CDR(args);
    let mut limit = as_integer(CAR(args));
    args = CDR(args);

    let lenw = 4 * limit;
    let mut iwork = vec![0 as c_int; limit.max(0) as usize];
    let mut work = vec![0.0 as c_double; lenw.max(0) as usize];

    let mut result: c_double = 0.0;
    let mut abserr: c_double = 0.0;
    let mut neval: c_int = 0;
    let mut ier: c_int = 0;
    let mut last: c_int = 0;
    let mut lenw_out: c_int = lenw;

    Rdqagi(
        &mut |x| Rintfn(&is, x),
        bound,
        inf,
        epsabs,
        epsrel,
        &mut result,
        &mut abserr,
        &mut neval,
        &mut ier,
        limit,
        lenw_out,
        &mut last,
        &mut iwork,
        &mut work,
    );

    build_integrate_result(result, abserr, last, ier)
}

/// GNU `integrate(f, lower, upper)`.
pub fn do_integrate(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let f = CAR(args);
    let lower = CAR(CDR(args));
    let upper = CAR(CDR(CDR(args)));
    let epsabs = Rf_ScalarReal(1.49e-8);
    let _ea = protect(epsabs);
    let epsrel = Rf_ScalarReal(1.49e-8);
    let _er = protect(epsrel);
    let limit = Rf_ScalarInteger(100);
    let _lim = protect(limit);
    let internal = Rf_cons(
        R_NilValue(),
        Rf_cons(
            f,
            Rf_cons(
                rho,
                Rf_cons(
                    lower,
                    Rf_cons(
                        upper,
                        Rf_cons(epsabs, Rf_cons(epsrel, Rf_cons(limit, R_NilValue()))),
                    ),
                ),
            ),
        ),
    );
    let _i = protect(internal);
    let ans = call_dqags(internal);
    let _a = protect(ans);
    crate::sexp::attrib_core::setAttrib(
        ans,
        crate::sexp::attrib_core::R_ClassSymbol(),
        crate::library::stats::checked::mk_string("integrate"),
    );
    ans
}
