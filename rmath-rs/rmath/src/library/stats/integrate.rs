#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

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
use crate::main::errors::Rf_error;
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

unsafe extern "C" fn Rintfn(x: *mut c_double, n: c_int, ex: *mut std::ffi::c_void) {
    let is = &*(ex as *const IntStruct);

    let args = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n));
    for i in 0..n {
        *REAL(args).add(i as usize) = *x.add(i as usize);
    }

    let tmp = Rf_protect(Rf_lang2(is.f, args));
    let resultsxp = Rf_protect(eval(tmp, is.env));

    // Check length
    let rlen = LENGTH(resultsxp);
    if rlen != n {
        Rf_unprotect(3);
        Rf_error(b"evaluation of function gave a result of wrong length\0".as_ptr() as *const _);
    }

    // Check type and coerce if needed
    let resultsxp = if TYPEOF(resultsxp) == SEXPTYPE::INTSXP {
        coerceVector(resultsxp, SEXPTYPE::REALSXP.as_c_int())
    } else if TYPEOF(resultsxp) != SEXPTYPE::REALSXP {
        Rf_unprotect(3);
        Rf_error(b"evaluation of function gave a result of wrong type\0".as_ptr() as *const _);
        unreachable!();
    } else {
        resultsxp
    };

    for i in 0..n {
        *x.add(i as usize) = *REAL(resultsxp).add(i as usize);
        if !R_FINITE(*x.add(i as usize)) {
            Rf_unprotect(3);
            Rf_error(b"non-finite function value\0".as_ptr() as *const _);
        }
    }

    Rf_unprotect(3);
}

// ---------------------------------------------------------------------------
// Helper: asReal
// ---------------------------------------------------------------------------

unsafe fn as_real(x: SEXP) -> c_double {
    if x.is_null() {
        return NA_REAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::REALSXP {
        return *REAL(x);
    }
    if t == SEXPTYPE::INTSXP {
        let v = *INTEGER(x);
        if v == NA_INTEGER {
            return NA_REAL;
        }
        return v as c_double;
    }
    if t == SEXPTYPE::LGLSXP {
        let v = *INTEGER(x);
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

unsafe fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::REALSXP {
        let v = *REAL(x);
        if v.is_nan() || v < c_int::MIN as c_double || v > c_int::MAX as c_double {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP {
        return *INTEGER(x);
    }
    NA_INTEGER
}

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

unsafe fn eval(call: SEXP, rho: SEXP) -> SEXP {
    crate::eval::eval::Rf_eval(call, rho)
}

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    use crate::appl::integrate::{Rdqagi, Rdqags};
    crate::main::coerce::coerceVector(x, type_)
}

// ---------------------------------------------------------------------------
// Helper: allocate int/double arrays (replaces R_alloc)
// ---------------------------------------------------------------------------

unsafe fn alloc_int_array(n: usize) -> *mut c_int {
    let layout = std::alloc::Layout::array::<c_int>(n)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_int>()));
    let ptr = std::alloc::alloc(layout) as *mut c_int;
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

unsafe fn alloc_double_array(n: usize) -> *mut c_double {
    let layout = std::alloc::Layout::array::<c_double>(n)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_double>()));
    let ptr = std::alloc::alloc(layout) as *mut c_double;
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

// ---------------------------------------------------------------------------
// Helper: build result list
// ---------------------------------------------------------------------------

unsafe fn build_integrate_result(
    result: c_double,
    abserr: c_double,
    last: c_int,
    ier: c_int,
) -> SEXP {
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 4));
    let ansnames = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 4));

    SET_STRING_ELT(ansnames, 0, Rf_mkChar(b"value\0".as_ptr() as *const c_char));
    SET_VECTOR_ELT(ans, 0, Rf_allocVector(SEXPTYPE::REALSXP, 1));
    *REAL(VECTOR_ELT(ans, 0)).add(0) = result;

    SET_STRING_ELT(
        ansnames,
        1,
        Rf_mkChar(b"abs.error\0".as_ptr() as *const c_char),
    );
    SET_VECTOR_ELT(ans, 1, Rf_allocVector(SEXPTYPE::REALSXP, 1));
    *REAL(VECTOR_ELT(ans, 1)).add(0) = abserr;

    SET_STRING_ELT(
        ansnames,
        2,
        Rf_mkChar(b"subdivisions\0".as_ptr() as *const c_char),
    );
    SET_VECTOR_ELT(ans, 2, Rf_allocVector(SEXPTYPE::INTSXP, 1));
    *INTEGER(VECTOR_ELT(ans, 2)).add(0) = last;

    SET_STRING_ELT(ansnames, 3, Rf_mkChar(b"ierr\0".as_ptr() as *const c_char));
    SET_VECTOR_ELT(ans, 3, Rf_allocVector(SEXPTYPE::INTSXP, 1));
    *INTEGER(VECTOR_ELT(ans, 3)).add(0) = ier;

    setAttrib(ans, R_NamesSymbol(), ansnames);
    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// call_dqags -- finite interval integration
// ---------------------------------------------------------------------------

pub unsafe fn call_dqags(args: SEXP) -> SEXP {
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
        Rf_error(b"'lower' must be of length one\0".as_ptr() as *const _);
    }
    let mut lower = as_real(CAR(args));
    args = CDR(args);

    if LENGTH(CAR(args)) > 1 {
        Rf_error(b"'upper' must be of length one\0".as_ptr() as *const _);
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
    let iwork = alloc_int_array(limit as usize);
    let work = alloc_double_array(lenw as usize);

    let mut result: c_double = 0.0;
    let mut abserr: c_double = 0.0;
    let mut neval: c_int = 0;
    let mut ier: c_int = 0;
    let mut last: c_int = 0;
    let mut lenw_out: c_int = lenw;

    Rdqags(
        Rintfn,
        &mut is as *mut IntStruct as *mut std::ffi::c_void,
        &mut lower,
        &mut upper,
        &mut epsabs,
        &mut epsrel,
        &mut result,
        &mut abserr,
        &mut neval,
        &mut ier,
        &mut limit,
        &mut lenw_out,
        &mut last,
        iwork,
        work,
    );

    build_integrate_result(result, abserr, last, ier)
}

// ---------------------------------------------------------------------------
// call_dqagi -- infinite interval integration
// ---------------------------------------------------------------------------

pub unsafe fn call_dqagi(args: SEXP) -> SEXP {
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
        Rf_error(b"'bound' must be of length one\0".as_ptr() as *const _);
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
    let iwork = alloc_int_array(limit as usize);
    let work = alloc_double_array(lenw as usize);

    let mut result: c_double = 0.0;
    let mut abserr: c_double = 0.0;
    let mut neval: c_int = 0;
    let mut ier: c_int = 0;
    let mut last: c_int = 0;
    let mut lenw_out: c_int = lenw;

    Rdqagi(
        Rintfn,
        &mut is as *mut IntStruct as *mut std::ffi::c_void,
        &mut bound,
        &mut inf,
        &mut epsabs,
        &mut epsrel,
        &mut result,
        &mut abserr,
        &mut neval,
        &mut ier,
        &mut limit,
        &mut lenw_out,
        &mut last,
        iwork,
        work,
    );

    build_integrate_result(result, abserr, last, ier)
}
