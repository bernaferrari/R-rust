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

use crate::attrib_core::{R_DimSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asReal};
use crate::main::errors::Rf_error;
use crate::main::random::{GetRNGstate, PutRNGstate};
use crate::modules::lapack::backend;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

fn isMatrix(x: SEXP) -> bool {
    let dn = getAttrib(x, R_DimSymbol());
    !dn.is_null() && LENGTH(dn) >= 2
}

fn isReal(x: SEXP) -> bool {
    crate::main::coerce::isReal(x) != 0
}

fn alloc3DArray(sexptype: c_int, nrow: c_int, ncol: c_int, ndepth: c_int) -> SEXP {
    let total = nrow as isize * ncol as isize * ndepth as isize;
    let ans = Rf_allocVector(sexptype, total as c_int);
    let _ans_guard = protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 3);
    let _dim_guard = protect(dim);
    crate::library::stats::checked::set_integers(dim, &[nrow, ncol, ndepth]);
    setAttrib(ans, R_DimSymbol(), dim);
    ans
}

fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    Rf_error(c_msg.as_ptr());
}

// ---------------------------------------------------------------------------
// Fortran BLAS declarations used only by the fortran-backend Wishart path.
// Cholesky factorization goes through modules::lapack::backend so the default
// Rust backend never reaches a fake self-extern.
// ---------------------------------------------------------------------------

#[cfg(feature = "fortran-backend")]
unsafe extern "C" {
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

fn std_rWishart_factor(nu: c_double, p: c_int, upper: c_int, ans_slice: &mut [c_double]) {
    let pp1 = p + 1;

    if nu < (p as c_double) || p <= 0 {
        error("inconsistent degrees of freedom and dimension");
        return;
    }

    ans_slice.fill(0.0);

    for j in 0..p as usize {
        // diagonal element: sqrt(chisq(nu - j))
        let chi = crate::nmath::dist::chisq::rchisq_inner(nu - j as c_double);
        ans_slice[j * pp1 as usize] = chi.sqrt();

        for i in 0..j {
            let uind = i + j * p as usize;
            let lind = j + i * p as usize;
            let norm_val = crate::nmath::dist::normal::norm_rand();
            if upper != 0 {
                ans_slice[uind] = norm_val;
                ans_slice[lind] = 0.0;
            } else {
                ans_slice[lind] = norm_val;
                ans_slice[uind] = 0.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// rWishart: generate random Wishart matrices
// ---------------------------------------------------------------------------

pub fn rWishart(ns: SEXP, nuP: SEXP, scal: SEXP) -> SEXP {
    let dims = getAttrib(scal, R_DimSymbol());
    let n = asInteger(ns);
    let nu = asReal(nuP);

    if !isMatrix(scal) || !isReal(scal) || INTEGER_ELT(dims, 0) != INTEGER_ELT(dims, 1) {
        error("'scal' must be a square, real matrix");
        return R_NilValue();
    }

    let n = if n <= 0 { 1 } else { n };
    let p = INTEGER_ELT(dims, 0);
    let p_usize = p as usize;
    let psqr = p_usize * p_usize;

    let ans = alloc3DArray(SEXPTYPE::REALSXP.into(), p, p, n);
    let _ans_guard = protect(ans);

    let mut tmp = vec![0.0f64; psqr];
    let mut sc_cp = crate::library::stats::checked::real_vec(scal);
    sc_cp.truncate(psqr);

    // Cholesky factorization: scCp <- U'U where scal = U'U
    let mut info: c_int = 0;
    let uplo_u: [u8; 1] = [b'U'];
    backend::dpotrf_(uplo_u.as_ptr(), &p, sc_cp.as_mut_ptr(), &p, &mut info);
    if info != 0 {
        error("'scal' matrix is not positive-definite");
        return R_NilValue();
    }

    let mut ans_values = vec![0.0f64; psqr * n as usize];
    let ans_slice = ans_values.as_mut_slice();
    #[cfg(feature = "fortran-backend")]
    let one: c_double = 1.0;
    #[cfg(feature = "fortran-backend")]
    let zero: c_double = 0.0;
    #[cfg(feature = "fortran-backend")]
    let side_r: [u8; 1] = [b'R'];
    #[cfg(feature = "fortran-backend")]
    let uplo_u2: [u8; 1] = [b'U'];
    #[cfg(feature = "fortran-backend")]
    let trans_n: [u8; 1] = [b'N'];
    #[cfg(feature = "fortran-backend")]
    let diag_n: [u8; 1] = [b'N'];
    #[cfg(feature = "fortran-backend")]
    let uplo_u3: [u8; 1] = [b'U'];
    #[cfg(feature = "fortran-backend")]
    let trans_t: [u8; 1] = [b'T'];

    GetRNGstate();
    for ansj in ans_slice.chunks_exact_mut(psqr) {
        std_rWishart_factor(nu, p, 1, &mut tmp);

        // tmp := tmp * U  (dtrmm: R, U, N, N)
        #[cfg(feature = "fortran-backend")]
        // SAFETY: every operand is a live local p x p buffer or scalar.
        unsafe {
            dtrmm_(
                side_r.as_ptr(),
                uplo_u2.as_ptr(),
                trans_n.as_ptr(),
                diag_n.as_ptr(),
                &p,
                &p,
                &one,
                sc_cp.as_ptr(),
                &p,
                tmp.as_mut_ptr(),
                &p,
            );
        }
        #[cfg(not(feature = "fortran-backend"))]
        {
            // Pure-Rust fallback: tmp = tmp * scCp (upper triangular)
            let mut prod = vec![0.0f64; psqr];
            for i in 0..p_usize {
                for j in 0..p_usize {
                    let mut s = 0.0;
                    for k in 0..=j {
                        s += tmp[i + k * p_usize] * sc_cp[k + j * p_usize];
                    }
                    prod[i + j * p_usize] = s;
                }
            }
            tmp.copy_from_slice(&prod);
        }

        // ansj := tmp' * tmp  (dsyrk: U, T)
        #[cfg(feature = "fortran-backend")]
        // SAFETY: every operand is a live local p x p buffer or scalar.
        unsafe {
            dsyrk_(
                uplo_u3.as_ptr(),
                trans_t.as_ptr(),
                &p,
                &p,
                &one,
                tmp.as_ptr(),
                &p,
                &zero,
                ansj.as_mut_ptr(),
                &p,
            );
        }
        #[cfg(not(feature = "fortran-backend"))]
        {
            // Pure-Rust fallback: ansj = tmp^T * tmp (symmetric, store upper)
            for j in 0..p_usize {
                for i in 0..=j {
                    let mut s = 0.0;
                    for k in 0..p_usize {
                        s += tmp[k + i * p_usize] * tmp[k + j * p_usize];
                    }
                    ansj[i + j * p_usize] = s;
                }
            }
        }

        // Copy upper triangle to lower triangle
        for i in 1..p_usize {
            for k in 0..i {
                ansj[i + k * p_usize] = ansj[k + i * p_usize];
            }
        }
    }
    PutRNGstate();
    crate::library::stats::checked::set_reals(ans, &ans_values);

    ans
}

pub fn do_rWishart_r(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let n = CAR(args);
    if n.is_null() || n == R_NilValue() || n == crate::sexp::globals::R_MissingArg() {
        crate::main::errors::errorcall_str(call, "argument \"n\" is missing, with no default");
    }
    let nu = CADR(args);
    if nu.is_null() || nu == R_NilValue() || nu == crate::sexp::globals::R_MissingArg() {
        crate::main::errors::errorcall_str(call, "argument \"df\" is missing, with no default");
    }
    let scal = CADDR(args);
    if scal.is_null() || scal == R_NilValue() || scal == crate::sexp::globals::R_MissingArg() {
        crate::main::errors::errorcall_str(call, "argument \"Sigma\" is missing, with no default");
    }
    rWishart(n, nu, scal)
}

pub unsafe extern "C-unwind" fn c_rWishart(n: SEXP, df: SEXP, sigma: SEXP) -> SEXP {
    rWishart(n, df, sigma)
}
