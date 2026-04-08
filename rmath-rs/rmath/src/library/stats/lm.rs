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
 *  Copyright (C) 2012-2025  The R Core Team
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
 *  https://www.R-project.org/Licenses/.
 *
 *  Ported from r-source/src/library/stats/src/lm.c
 */

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

unsafe fn asBool(x: SEXP) -> bool {
    let v = crate::main::coerce::asLogical(x);
    v != 0 && v != NA_INTEGER
}

unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::shallow_duplicate(x)
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    Rf_protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP.0, 2);
    Rf_protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim.add(1)) = ncol;
    crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
    Rf_unprotect(2);
    ans
}

unsafe extern "C" {
    fn R_alloc(size: usize, eltsize: usize) -> *mut std::ffi::c_void;
    /// LINPACK dqrls — Fortran name-mangled entry point
    #[link_name = "dqrls_"]
    fn dqrls(
        qr: *mut c_double,
        n: *mut c_int,
        p: *mut c_int,
        y: *mut c_double,
        ny: *mut c_int,
        tol: *mut c_double,
        coefficients: *mut c_double,
        residuals: *mut c_double,
        effects: *mut c_double,
        rank: *mut c_int,
        pivot: *mut c_int,
        qraux: *mut c_double,
        work: *mut c_double,
    );
}

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};

unsafe fn mkNamed(sexptype: c_int, names: &[&str]) -> SEXP {
    let nn = names.len() as c_int;
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, nn));
    let nm = Rf_allocVector(SEXPTYPE::STRSXP.0, nn);
    setAttrib(ans, R_NamesSymbol(), nm);
    for i in 0..(nn as usize) {
        SET_STRING_ELT(nm, i as R_xlen_t, Rf_mkChar(names[i].as_ptr() as *const i8));
    }
    Rf_unprotect(1);
    ans
}

pub unsafe fn Cdqrls(x: SEXP, y: SEXP, tol: SEXP, chk: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let mut x = x;
    let mut y = y;
    let mut nprotect: c_int = 4;

    let ans_dim = getAttrib(x, R_DimSymbol());
    if asBool(chk) && LENGTH(ans_dim) != 2 {
        Rf_error(b"'x' is not a matrix\0".as_ptr() as *const i8);
    }
    let dims = INTEGER(ans_dim);
    let n = *dims.add(0);
    let p = *dims.add(1);
    let mut ny: c_int = 0;
    if n != 0 {
        ny = (XLENGTH(y) as i64 / n as i64) as c_int;
    }
    if asBool(chk) && n * ny != XLENGTH(y) as c_int {
        Rf_error(b"dimensions of 'x' and 'y' do not match\0".as_ptr() as *const i8);
    }

    /* These lose attributes, so do after we have extracted dims */
    if TYPEOF(x) != SEXPTYPE::REALSXP.0 {
        x = coerceVector(x, SEXPTYPE::REALSXP.0);
        Rf_protect(x);
        nprotect += 1;
    }
    if TYPEOF(y) != SEXPTYPE::REALSXP.0 {
        y = coerceVector(y, SEXPTYPE::REALSXP.0);
        Rf_protect(y);
        nprotect += 1;
    }

    let rptr = REAL(x);
    for i in 0..(XLENGTH(x) as usize) {
        if !R_FINITE(*rptr.add(i)) {
            Rf_error(b"NA/NaN/Inf in 'x'\0".as_ptr() as *const i8);
        }
    }

    let rptr = REAL(y);
    for i in 0..(XLENGTH(y) as usize) {
        if !R_FINITE(*rptr.add(i)) {
            Rf_error(b"NA/NaN/Inf in 'y'\0".as_ptr() as *const i8);
        }
    }

    let ansNms = [
        "qr",
        "coefficients",
        "residuals",
        "effects",
        "rank",
        "pivot",
        "qraux",
        "tol",
        "pivoted",
    ];
    let ans = Rf_protect(mkNamed(SEXPTYPE::VECSXP.0, &ansNms));
    let qr = shallow_duplicate(x);
    SET_VECTOR_ELT(ans, 0, qr);

    let coefficients = if ny > 1 {
        allocMatrix(SEXPTYPE::REALSXP.0, p, ny)
    } else {
        Rf_allocVector(SEXPTYPE::REALSXP.0, p)
    };
    Rf_protect(coefficients);
    SET_VECTOR_ELT(ans, 1, coefficients);

    let residuals = shallow_duplicate(y);
    SET_VECTOR_ELT(ans, 2, residuals);
    let effects = shallow_duplicate(y);
    SET_VECTOR_ELT(ans, 3, effects);

    let pivot = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, p));
    let ip = INTEGER(pivot);
    for i in 0..(p as usize) {
        *ip.add(i) = (i + 1) as c_int;
    }
    SET_VECTOR_ELT(ans, 5, pivot);

    let qraux = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, p));
    SET_VECTOR_ELT(ans, 6, qraux);
    SET_VECTOR_ELT(ans, 7, tol);

    let work = R_alloc(2 * p as usize, std::mem::size_of::<c_double>()) as *mut c_double;

    let mut rank: c_int = 0;
    let mut n_mut = n;
    let mut p_mut = p;
    let mut ny_mut = ny;
    let mut rtol = asReal(tol);

    dqrls(
        REAL(qr),
        &mut n_mut,
        &mut p_mut,
        REAL(y),
        &mut ny_mut,
        &mut rtol,
        REAL(coefficients),
        REAL(residuals),
        REAL(effects),
        &mut rank,
        INTEGER(pivot),
        REAL(qraux),
        work,
    );

    SET_VECTOR_ELT(ans, 4, Rf_ScalarInteger(rank));
    let mut pivoted: c_int = 0;
    for i in 0..(p as usize) {
        if *ip.add(i) != (i + 1) as c_int {
            pivoted = 1;
            break;
        }
    }
    SET_VECTOR_ELT(ans, 8, Rf_ScalarLogical(pivoted));

    Rf_unprotect(nprotect);
    ans
}
