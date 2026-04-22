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
 *  Copyright (C) 2012--2019 The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

//! Regression influence diagnostics
//! Port of r-source/src/library/stats/src/influence.c

use std::ffi::CString;
use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    Rf_error(c_msg.as_ptr());
}

#[unsafe(no_mangle)]
unsafe fn mkChar(s: &str) -> SEXP {
    let c_str = CString::new(s).unwrap_or_default();
    Rf_mkChar(c_str.as_ptr())
}

unsafe fn getListElement(list: SEXP, str: &str) -> SEXP {
    if TYPEOF(list) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let names = getAttrib(list, R_NamesSymbol());
    if Rf_isNull(names) != 0 {
        return R_NilValue();
    }
    let len = LENGTH(list);
    let target = str.as_bytes();
    for i in 0..len {
        let name_sexp = STRING_ELT(names, i as crate::sexp::ffi::R_xlen_t);
        if name_sexp.is_null() {
            continue;
        }
        let name_ptr = CHAR(name_sexp);
        if name_ptr.is_null() {
            continue;
        }
        let name_bytes = std::ffi::CStr::from_ptr(name_ptr).to_bytes();
        if name_bytes == target {
            return VECTOR_ELT(list, i as crate::sexp::ffi::R_xlen_t);
        }
    }
    R_NilValue()
}

unsafe fn nrows(x: SEXP) -> c_int {
    let dn = getAttrib(x, crate::attrib_core::R_DimSymbol());
    if Rf_isNull(dn) != 0 || LENGTH(dn) < 1 {
        return LENGTH(x);
    }
    *INTEGER(dn)
}

unsafe fn ncols(x: SEXP) -> c_int {
    let dn = getAttrib(x, crate::attrib_core::R_DimSymbol());
    if Rf_isNull(dn) != 0 || LENGTH(dn) < 2 {
        return 1;
    }
    *INTEGER(dn.add(1))
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    Rf_protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    Rf_protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim.add(1)) = ncol;
    setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// External LINPACK lminfl declaration
// ---------------------------------------------------------------------------

#[cfg(feature = "fortran-backend")]
unsafe extern "C" {
    fn lminfl_(
        qr: *const c_double,
        n: *const c_int,
        ldqr: *const c_int,
        k: *const c_int,
        q: *const c_int,
        qraux: *const c_double,
        resid: *const c_double,
        hat: *mut c_double,
        sigma: *mut c_double,
        tol: *const c_double,
    );
}

#[cfg(not(feature = "fortran-backend"))]
unsafe fn lminfl_(_qr: *const c_double, _n: *const c_int, _ldqr: *const c_int, _k: *const c_int, _q: *const c_int, _qraux: *const c_double, _resid: *const c_double, _hat: *mut c_double, _sigma: *mut c_double, _tol: *const c_double) {}

// ---------------------------------------------------------------------------
// influence: regression influence diagnostics
// ---------------------------------------------------------------------------

pub unsafe fn influence(mqr: SEXP, e: SEXP, stol: SEXP) -> SEXP {
    let qr = getListElement(mqr, "qr");
    let qraux = getListElement(mqr, "qraux");
    let rank_val = getListElement(mqr, "rank");

    let n = nrows(qr);
    let k = asInteger(rank_val);
    let q = ncols(e);
    let tol = asReal(stol);

    let hat = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n));
    let sigma = Rf_protect(allocMatrix(SEXPTYPE::REALSXP.into(), n, q));

    lminfl_(
        REAL(qr),
        &n,
        &n,
        &k,
        &q,
        REAL(qraux),
        REAL(e),
        REAL(hat),
        REAL(sigma),
        &tol,
    );

    // Clamp hat values slightly above 1 to exactly 1
    for i in 0..n as usize {
        if *REAL(hat).add(i) > 1.0 - tol {
            *REAL(hat).add(i) = 1.0;
        }
    }

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 2));
    let nm = Rf_allocVector(SEXPTYPE::STRSXP, 2);
    setAttrib(ans, R_NamesSymbol(), nm);

    let mut m: c_int = 0;
    SET_VECTOR_ELT(ans, m as crate::sexp::ffi::R_xlen_t, hat);
    SET_STRING_ELT(nm, m as crate::sexp::ffi::R_xlen_t, mkChar("hat"));
    m += 1;
    SET_VECTOR_ELT(ans, m as crate::sexp::ffi::R_xlen_t, sigma);
    SET_STRING_ELT(nm, m as crate::sexp::ffi::R_xlen_t, mkChar("sigma"));

    Rf_unprotect(3);
    ans
}
