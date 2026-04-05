
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/axis_scales.c
 *
 *  Axis tick mark creation and axis parameter computation.
 */

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::main::coerce::{asInteger, asLogical, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

/// Stub: CreateAtVector - create axis tick positions.
unsafe fn CreateAtVector(
    _axp: *const c_double,
    _usr: *const c_double,
    _n: c_int,
    _log: c_int,
) -> SEXP {
    R_NilValue()
}

/// Stub: GAxisPars - compute axis parameters.
unsafe fn GAxisPars(
    _min: *mut c_double,
    _max: *mut c_double,
    _n: *mut c_int,
    _log: c_int,
    _axis: c_int,
) {
    // no-op
}

/// R_CreateAtVector - create an axis tick vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CreateAtVector(axp: SEXP, usr: SEXP, nint: SEXP, is_log: SEXP) -> SEXP {
    let nint_v = asInteger(nint);
    let logflag = asLogical(is_log);

    let axp = Rf_protect(coerceVector(axp, SEXPTYPE::REALSXP.0));
    let usr = Rf_protect(coerceVector(usr, SEXPTYPE::REALSXP.0));
    if LENGTH(axp) != 3 {
        Rf_error(b"'axp' must be numeric of length 3\0".as_ptr() as *const c_char);
    }
    if LENGTH(usr) != 2 {
        Rf_error(b"'usr' must be numeric of length 2\0".as_ptr() as *const c_char);
    }

    let res = CreateAtVector(REAL(axp), REAL(usr), nint_v, logflag);
    Rf_unprotect(2);
    res
}

/// R_GAxisPars - compute axis parameters (axp, n) from user range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GAxisPars(usr: SEXP, is_log: SEXP, nintLog: SEXP) -> SEXP {
    let usr = coerceVector(usr, SEXPTYPE::REALSXP.0);
    if LENGTH(usr) != 2 {
        Rf_error(b"'usr' must be numeric of length 2\0".as_ptr() as *const c_char);
    }
    let mut min = *REAL(usr).add(0);
    let mut max = *REAL(usr).add(1);
    let logflag = asLogical(is_log);
    let mut n = asInteger(nintLog);

    GAxisPars(&mut min, &mut max, &mut n, logflag, 0);

    // Build named list: list(axp = c(min, max), n = n)
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 2));
    let axp = Rf_allocVector(SEXPTYPE::REALSXP.0, 2);
    *REAL(axp).add(0) = min;
    *REAL(axp).add(1) = max;
    SET_VECTOR_ELT(ans, 0, axp);
    SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(n));
    Rf_unprotect(1);
    ans
}
