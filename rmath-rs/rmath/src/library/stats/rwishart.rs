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
 *  Copyright (C) 2012-2024  The R Core Team
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

//! Wishart distribution sampling
//! Port of r-source/src/library/stats/src/rWishart.c

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::main::random::{GetRNGstate, PutRNGstate};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_REAL, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn isMatrix(x: SEXP) -> bool {
    let dn = getAttrib(x, R_DimSymbol());
    if dn.is_null() {
        return false;
    }
    let len = LENGTH(dn);
    len >= 2
}

unsafe fn isReal(x: SEXP) -> bool {
    crate::main::coerce::isReal(x)
}

unsafe fn alloc3DArray(sexptype: c_int, nrow: c_int, ncol: c_int, ndepth: c_int) -> SEXP {
    let total = nrow as isize * ncol as isize * ndepth as isize;
    let ans = Rf_allocVector(sexptype, total as c_int);
    Rf_protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 3);
    Rf_protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim.add(1)) = ncol;
    *INTEGER(dim.add(2)) = ndepth;
    setAttrib(ans, R_DimSymbol(), dim);
    Rf_unprotect(2);
    ans
}

unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    Rf_error(c_msg.as_ptr());
}

// ---------------------------------------------------------------------------
// LAPACK / BLAS external declarations
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn dpotrf_(
        uplo: *const u8,
        n: *const c_int,
        a: *mut c_double,
        lda: *const c_int,
        info: *mut c_int,
    );
    fn dtrmm_(
        side: *const u8,
        uplo: *const u8,
        transa: *const u8,
        diag: *const u8,
        m: *const c_int,
        n: *const c_int,
        alpha: *const c_double,
        a: *const c_double,
        lda: *const c_int,
        b: *mut c_double,
        ldb: *const c_int,
    );
    fn dsyrk_(
        uplo: *const u8,
        trans: *const u8,
        n: *const c_int,
        k: *const c_int,
        alpha: *const c_double,
        a: *const c_double,
        lda: *const c_int,
        beta: *const c_double,
        c: *mut c_double,
        ldc: *const c_int,
    );
}

// ---------------------------------------------------------------------------
// std_rWishart_factor: simulate Cholesky factor of standardized Wishart
// ---------------------------------------------------------------------------

unsafe fn std_rWishart_factor(
    nu: c_double,
    p: c_int,
    upper: c_int,
    ans: *mut c_double,
) -> *mut c_double {
    let pp1 = p + 1;

    if nu < (p as c_double) || p <= 0 {
        error("inconsistent degrees of freedom and dimension");
        return ans;
    }

    let psqr = (p * p) as usize;
    for i in 0..psqr {
        *ans.add(i) = 0.0;
    }

    for j in 0..p as usize {
        // diagonal element: sqrt(chisq(nu - j))
        let chi = crate::nmath::dist::chisq::rchisq_inner(nu - j as c_double);
        *ans.add(j * pp1 as usize) = chi.sqrt();

        for i in 0..j {
            let uind = i + j * p as usize;
            let lind = j + i * p as usize;
            let norm_val = crate::nmath::dist::normal::norm_rand();
            if upper != 0 {
                *ans.add(uind) = norm_val;
                *ans.add(lind) = 0.0;
            } else {
                *ans.add(lind) = norm_val;
                *ans.add(uind) = 0.0;
            }
        }
    }

    ans
}

// ---------------------------------------------------------------------------
// rWishart: generate random Wishart matrices
// ---------------------------------------------------------------------------

pub unsafe fn rWishart(ns: SEXP, nuP: SEXP, scal: SEXP) -> SEXP {
    let dims = INTEGER(getAttrib(scal, R_DimSymbol()));
    let n = asInteger(ns);
    let nu = asReal(nuP);

    if !isMatrix(scal) || !isReal(scal) || *dims != *dims.add(1) {
        error("'scal' must be a square, real matrix");
        return R_NilValue();
    }

    let n = if n <= 0 { 1 } else { n };
    let p = *dims;
    let psqr = (p * p) as usize;

    let ans = Rf_protect(alloc3DArray(SEXPTYPE::REALSXP, p, p, n));

    // Allocate temporary arrays (replaces R_Calloc)
    let layout_tmp = std::alloc::Layout::array::<c_double>(psqr)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_double>()));
    let tmp = std::alloc::alloc(layout_tmp) as *mut c_double;
    if tmp.is_null() {
        std::alloc::handle_alloc_error(layout_tmp);
    }

    let layout_sccp = std::alloc::Layout::array::<c_double>(psqr)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<c_double>()));
    let scCp = std::alloc::alloc(layout_sccp) as *mut c_double;
    if scCp.is_null() {
        std::alloc::dealloc(tmp as *mut u8, layout_tmp);
        std::alloc::handle_alloc_error(layout_sccp);
    }

    // Copy scale matrix to scCp
    let scal_real = REAL(scal);
    for i in 0..psqr {
        *scCp.add(i) = *scal_real.add(i);
    }

    // Zero out tmp
    for i in 0..psqr {
        *tmp.add(i) = 0.0;
    }

    // Cholesky factorization: scCp <- U'U where scal = U'U
    let mut info: c_int = 0;
    let uplo_u: [u8; 1] = [b'U'];
    dpotrf_(uplo_u.as_ptr(), &p, scCp, &p, &mut info);
    if info != 0 {
        error("'scal' matrix is not positive-definite");
        std::alloc::dealloc(tmp as *mut u8, layout_tmp);
        std::alloc::dealloc(scCp as *mut u8, layout_sccp);
        Rf_unprotect(1);
        return R_NilValue();
    }

    let ansp = REAL(ans);
    let one: c_double = 1.0;
    let zero: c_double = 0.0;
    let side_r: [u8; 1] = [b'R'];
    let uplo_u2: [u8; 1] = [b'U'];
    let trans_n: [u8; 1] = [b'N'];
    let diag_n: [u8; 1] = [b'N'];
    let uplo_u3: [u8; 1] = [b'U'];
    let trans_t: [u8; 1] = [b'T'];

    GetRNGstate();
    for j in 0..n as usize {
        let ansj = ansp.add(j * psqr);

        std_rWishart_factor(nu, p, 1, tmp);

        // tmp := tmp * U  (dtrmm: R, U, N, N)
        dtrmm_(
            side_r.as_ptr(),
            uplo_u2.as_ptr(),
            trans_n.as_ptr(),
            diag_n.as_ptr(),
            &p,
            &p,
            &one,
            scCp,
            &p,
            tmp,
            &p,
        );

        // ansj := tmp' * tmp  (dsyrk: U, T)
        dsyrk_(
            uplo_u3.as_ptr(),
            trans_t.as_ptr(),
            &p,
            &p,
            &one,
            tmp,
            &p,
            &zero,
            ansj,
            &p,
        );

        // Copy upper triangle to lower triangle
        for i in 1..p as usize {
            for k in 0..i {
                *ansj.add(i + k * p as usize) = *ansj.add(k + i * p as usize);
            }
        }
    }
    PutRNGstate();

    std::alloc::dealloc(scCp as *mut u8, layout_sccp);
    std::alloc::dealloc(tmp as *mut u8, layout_tmp);
    Rf_unprotect(1);
    ans
}
