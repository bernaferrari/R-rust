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

unsafe fn coerceVector(x: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe { crate::main::coerce::coerceVector(x, type_.into()) }
}

unsafe fn asReal(x: SEXP) -> c_double {
    unsafe { crate::main::coerce::asReal(x) }
}

unsafe fn asBool(x: SEXP) -> bool {
    unsafe {
        let v = crate::main::coerce::asLogical(x);
        v != 0 && v != NA_INTEGER
    }
}

unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::shallow_duplicate(x) }
}

unsafe fn allocMatrix(sexptype: SEXPTYPE, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(sexptype, nrow * ncol);
        let _ans_guard = protect(ans);
        let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow;
        *INTEGER(dim).add(1) = ncol;
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
        ans
    }
}

use crate::sexp::memory_ext::R_alloc;

/// Pure-Rust replacement for LINPACK dqrls (QR least squares).
///
/// Computes QR factorisation with column pivoting of X (n x p),
/// then solves min ||X*b - y||_2.  On entry `pivot` must hold 1..p.
/// On exit `pivot` contains the column permutation, `qraux` the
/// Householder scalars, `effects = Q^T y`, and `rank` the numerical
/// rank determined by `tol`.
unsafe fn dqrls_rust(
    qr: *mut c_double,
    n: c_int,
    p: c_int,
    y: *mut c_double,
    ny: c_int,
    tol: c_double,
    coefficients: *mut c_double,
    residuals: *mut c_double,
    effects: *mut c_double,
    rank: *mut c_int,
    pivot: *mut c_int,
    qraux: *mut c_double,
) {
    unsafe {
        let n_us = n as usize;
        let p_us = p as usize;
        let ny_us = ny as usize;

        let mut work = vec![0.0f64; (2 * p_us).max(1)];
        let mut krank: c_int = 0;
        crate::appl::linpack_qr::dqrdc2(
            qr,
            n,
            n,
            p,
            tol,
            &mut krank,
            qraux,
            pivot,
            work.as_mut_ptr(),
        );
        *rank = krank;
        let rank_us = krank.max(0) as usize;

        for col in 0..ny_us {
            if rank_us == 0 {
                for i in 0..n_us {
                    *effects.add(i + col * n_us) = *y.add(i + col * n_us);
                    *residuals.add(i + col * n_us) = *y.add(i + col * n_us);
                }
                for j in 0..p_us {
                    *coefficients.add(j + col * p_us) = 0.0;
                }
                continue;
            }
            let ycol = y.add(col * n_us);
            let mut info = 0;
            let mut coef = vec![0.0f64; p_us.max(1)];
            crate::appl::linpack_qr::dqrsl(
                qr,
                n,
                n,
                rank_us as c_int,
                qraux,
                ycol,
                std::ptr::null_mut(),
                effects.add(col * n_us),
                coef.as_mut_ptr(),
                residuals.add(col * n_us),
                std::ptr::null_mut(),
                1110,
                &mut info,
            );
            for j in 0..rank_us.min(p_us) {
                *coefficients.add(j + col * p_us) = coef[j];
            }
            for j in rank_us..p_us {
                *coefficients.add(j + col * p_us) = 0.0;
            }
        }
    }
}

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};

unsafe fn mkNamed(sexptype: SEXPTYPE, names: &[&str]) -> SEXP {
    unsafe {
        let nn = names.len() as c_int;
        let ans = Rf_allocVector(sexptype, nn);
        let _ans_guard = protect(ans);
        let nm = Rf_allocVector(SEXPTYPE::STRSXP, nn);
        let _nm_guard = protect(nm);
        setAttrib(ans, R_NamesSymbol(), nm);
        for i in 0..(nn as usize) {
            let name = names[i];
            SET_STRING_ELT(
                nm,
                i as R_xlen_t,
                Rf_mkCharLen(
                    name.as_ptr() as *const core::ffi::c_char,
                    name.len() as c_int,
                ),
            );
        }
        ans
    }
}

pub unsafe fn Cdqrls(x: SEXP, y: SEXP, tol: SEXP, chk: SEXP) -> SEXP {
    unsafe {
        use crate::main::errors::Rf_error;

        let mut x = x;
        let mut y = y;
        let mut guards = Vec::with_capacity(6);

        let ans_dim = getAttrib(x, R_DimSymbol());
        if asBool(chk) && LENGTH(ans_dim) != 2 {
            Rf_error(b"'x' is not a matrix\0".as_ptr() as *const core::ffi::c_char);
        }
        let dims = INTEGER(ans_dim);
        let n = *dims.add(0);
        let p = *dims.add(1);
        let mut ny: c_int = 0;
        if n != 0 {
            ny = (XLENGTH(y) as i64 / n as i64) as c_int;
        }
        if asBool(chk) && n * ny != XLENGTH(y) as c_int {
            Rf_error(
                b"dimensions of 'x' and 'y' do not match\0".as_ptr() as *const core::ffi::c_char
            );
        }

        /* These lose attributes, so do after we have extracted dims */
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            x = coerceVector(x, SEXPTYPE::REALSXP);
            guards.push(protect(x));
        }
        if TYPEOF(y) != SEXPTYPE::REALSXP {
            y = coerceVector(y, SEXPTYPE::REALSXP);
            guards.push(protect(y));
        }

        let rptr = REAL(x);
        for i in 0..(XLENGTH(x) as usize) {
            if !R_FINITE(*rptr.add(i)) {
                Rf_error(b"NA/NaN/Inf in 'x'\0".as_ptr() as *const core::ffi::c_char);
            }
        }

        let rptr = REAL(y);
        for i in 0..(XLENGTH(y) as usize) {
            if !R_FINITE(*rptr.add(i)) {
                Rf_error(b"NA/NaN/Inf in 'y'\0".as_ptr() as *const core::ffi::c_char);
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
        let ans = mkNamed(SEXPTYPE::VECSXP, &ansNms);
        guards.push(protect(ans));
        let qr = shallow_duplicate(x);
        SET_VECTOR_ELT(ans, 0, qr);

        let coefficients = if ny > 1 {
            allocMatrix(SEXPTYPE::REALSXP, p, ny)
        } else {
            Rf_allocVector(SEXPTYPE::REALSXP, p)
        };
        guards.push(protect(coefficients));
        SET_VECTOR_ELT(ans, 1, coefficients);

        let residuals = shallow_duplicate(y);
        SET_VECTOR_ELT(ans, 2, residuals);
        let effects = shallow_duplicate(y);
        SET_VECTOR_ELT(ans, 3, effects);

        let pivot = Rf_allocVector(SEXPTYPE::INTSXP, p);
        guards.push(protect(pivot));
        let ip = INTEGER(pivot);
        for i in 0..(p as usize) {
            *ip.add(i) = (i + 1) as c_int;
        }
        SET_VECTOR_ELT(ans, 5, pivot);

        let qraux = Rf_allocVector(SEXPTYPE::REALSXP, p);
        guards.push(protect(qraux));
        SET_VECTOR_ELT(ans, 6, qraux);
        SET_VECTOR_ELT(ans, 7, tol);

        let _work = R_alloc(2 * p as usize, std::mem::size_of::<c_double>()) as *mut c_double;

        let mut rank: c_int = 0;
        let rtol = asReal(tol);

        dqrls_rust(
            REAL(qr),
            n,
            p,
            REAL(y),
            ny,
            rtol,
            REAL(coefficients),
            REAL(residuals),
            REAL(effects),
            &mut rank,
            INTEGER(pivot),
            REAL(qraux),
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

        ans
    }
}
