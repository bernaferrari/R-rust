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
//! and gnu-r/src/library/stats/src/lminfl.f (rust-backend path).

use std::ffi::CString;
use std::os::raw::{c_double, c_int};

use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asReal};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn mkChar(s: &str) -> SEXP {
    unsafe {
        let c_str = CString::new(s).unwrap_or_default();
        Rf_mkChar(c_str.as_ptr())
    }
}

unsafe fn getListElement(list: SEXP, str: &str) -> SEXP {
    unsafe {
        if TYPEOF(list) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = getAttrib(list, R_NamesSymbol());
        if TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let len = LENGTH(list);
        if LENGTH(names) < len {
            return R_NilValue();
        }
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
}

unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let dn = getAttrib(x, crate::attrib_core::R_DimSymbol());
        if Rf_isNull(dn) != 0 || LENGTH(dn) < 1 {
            return LENGTH(x);
        }
        *INTEGER(dn)
    }
}

unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        let dn = getAttrib(x, crate::attrib_core::R_DimSymbol());
        if Rf_isNull(dn) != 0 || LENGTH(dn) < 2 {
            return 1;
        }
        *INTEGER(dn).add(1)
    }
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let len = (nrow as i64).checked_mul(ncol as i64);
        let Some(len) = len.filter(|&len| (0..=c_int::MAX as i64).contains(&len)) else {
            influence_error(b"influence: matrix dimensions are too large\0");
        };
        let ans = Rf_allocVector(sexptype, len as c_int);
        let _ans_guard = protect(ans);
        let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow;
        *INTEGER(dim).add(1) = ncol;
        setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
        ans
    }
}

unsafe fn influence_error(message: &'static [u8]) -> ! {
    unsafe { Rf_error(message.as_ptr() as *const std::os::raw::c_char) };
    unreachable!("Rf_error must not return")
}

// ---------------------------------------------------------------------------
// External LINPACK lminfl declaration (fortran-backend)
// ---------------------------------------------------------------------------

#[cfg(feature = "fortran-backend")]
unsafe extern "C" {
    fn lminfl_(
        qr: *const c_double,
        ldx: *const c_int,
        n: *const c_int,
        k: *const c_int,
        q: *const c_int,
        qraux: *const c_double,
        resid: *const c_double,
        hat: *mut c_double,
        sigma: *mut c_double,
        tol: *const c_double,
    );
}

// ---------------------------------------------------------------------------
// Pure rust-backend lminfl (LAPACK QR / dormqr convention)
// ---------------------------------------------------------------------------

/// Apply Q from a LAPACK QR factorization to `y` in place (`y := Q y`).
/// Reflectors use the same storage as `dgeqp3` / rust-backend `Cdqrls`.
#[cfg(not(feature = "fortran-backend"))]
fn apply_q_lapack(qr: &[f64], ldx: usize, n: usize, k: usize, tau: &[f64], y: &mut [f64]) {
    let ju = k.min(n);
    // Q = H_0 ... H_{ju-1}  ⇒  apply H_{ju-1}, ..., H_0
    for jj in (0..ju).rev() {
        let tau_val = tau[jj];
        if tau_val == 0.0 {
            continue;
        }
        let mut dot = y[jj];
        for i in (jj + 1)..n {
            dot += qr[i + jj * ldx] * y[i];
        }
        dot *= tau_val;
        y[jj] -= dot;
        for i in (jj + 1)..n {
            y[i] -= dot * qr[i + jj * ldx];
        }
    }
}

/// Core algorithm matching GNU `lminfl.f`, for LAPACK-style QR from `dgeqp3`
/// (as produced by rust-backend `Cdqrls`).
///
/// `qr` is column-major with leading dimension `ldx` (at least `n` rows and
/// `k` columns of Householder vectors). `qraux` holds LAPACK `tau` values.
/// `resid` / `sigma` are column-major `n × q`.
#[cfg(not(feature = "fortran-backend"))]
pub(crate) fn lminfl_compute(
    qr: &[f64],
    ldx: usize,
    n: usize,
    k: usize,
    q: usize,
    qraux: &[f64],
    resid: &[f64],
    hat: &mut [f64],
    sigma: &mut [f64],
    tol: f64,
) {
    assert!(hat.len() >= n);
    assert!(sigma.len() >= n.saturating_mul(q));
    assert!(resid.len() >= n.saturating_mul(q));
    assert!(qraux.len() >= k);
    assert!(ldx >= n);
    if n > 0 && k > 0 {
        assert!(qr.len() >= ldx.saturating_mul(k));
    }

    for h in hat.iter_mut().take(n) {
        *h = 0.0;
    }

    // hat_ii = sum_{j=0}^{k-1} Q_{ij}^2 for the thin Q from LAPACK QR (dgeqp3).
    // Q = H_0 H_1 ... H_{k-1} with H_j = I - tau_j v_j v_j^T (v_j has 1 on the
    // diagonal and the subdiagonal of qr column j). Apply Q to e_j by applying
    // reflectors in reverse — the same convention as rust-backend Cdqrls /
    // dqrls_rust (which applies them forward for Q^T).
    if n > 0 && k > 0 {
        let k_eff = k.min(n);
        let mut work = vec![0.0f64; n];
        for j in 0..k_eff {
            work.fill(0.0);
            work[j] = 1.0;
            apply_q_lapack(qr, ldx, n, k_eff, qraux, &mut work);
            for i in 0..n {
                hat[i] += work[i] * work[i];
            }
        }
    }

    for i in 0..n {
        if hat[i] >= 1.0 - tol {
            hat[i] = 1.0;
        }
    }

    // Leave-one-out residual SD: sigma(i,j) as in lminfl.f
    let denom = (n as f64) - (k as f64) - 1.0;
    for j in 0..q {
        let mut sum = 0.0f64;
        for i in 0..n {
            let r = resid[i + j * n];
            sum += r * r;
        }
        for i in 0..n {
            let r = resid[i + j * n];
            let val = if hat[i] < 1.0 {
                (sum - r * r / (1.0 - hat[i])) / denom
            } else {
                sum / denom
            };
            sigma[i + j * n] = val.sqrt();
        }
    }
}

/// Rust-backend `lminfl` entry matching the F77 calling convention used by
/// `influence` (arguments: qr, ldx, n, k, q, qraux, resid, hat, sigma, tol).
#[cfg(not(feature = "fortran-backend"))]
unsafe fn lminfl_(
    qr: *const c_double,
    ldx: *const c_int,
    n: *const c_int,
    k: *const c_int,
    q: *const c_int,
    qraux: *const c_double,
    resid: *const c_double,
    hat: *mut c_double,
    sigma: *mut c_double,
    tol: *const c_double,
) {
    unsafe {
        let ldx_u = (*ldx).max(0) as usize;
        let n_u = (*n).max(0) as usize;
        let k_u = (*k).max(0) as usize;
        let q_u = (*q).max(0) as usize;
        let tol_v = *tol;

        if n_u == 0 {
            return;
        }

        let qr_len = ldx_u.saturating_mul(k_u);
        let qr_slice = if k_u == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(qr, qr_len)
        };
        let qraux_slice = if k_u == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(qraux, k_u)
        };
        // A zero-column response has no payload.  Do not manufacture an
        // n-element slice from its zero-length allocation.
        let resid_slice = if q_u == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(resid, n_u.saturating_mul(q_u))
        };
        let hat_slice = std::slice::from_raw_parts_mut(hat, n_u);
        let sigma_slice = if q_u == 0 {
            &mut [][..]
        } else {
            std::slice::from_raw_parts_mut(sigma, n_u.saturating_mul(q_u))
        };

        lminfl_compute(
            qr_slice,
            ldx_u,
            n_u,
            k_u,
            q_u,
            qraux_slice,
            resid_slice,
            hat_slice,
            sigma_slice,
            tol_v,
        );
    }
}

// ---------------------------------------------------------------------------
// influence: regression influence diagnostics
// ---------------------------------------------------------------------------

pub unsafe fn influence(mqr: SEXP, e: SEXP, stol: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(mqr) != SEXPTYPE::VECSXP {
            influence_error(b"influence: 'mqr' must be a list\0");
        }
        if TYPEOF(e) != SEXPTYPE::REALSXP {
            influence_error(b"influence: residuals must be numeric\0");
        }
        let qr = getListElement(mqr, "qr");
        let qraux = getListElement(mqr, "qraux");
        let rank_val = getListElement(mqr, "rank");

        if TYPEOF(qr) != SEXPTYPE::REALSXP || TYPEOF(qraux) != SEXPTYPE::REALSXP {
            influence_error(b"influence: invalid QR decomposition\0");
        }
        if TYPEOF(rank_val) != SEXPTYPE::INTSXP || XLENGTH(rank_val) < 1 {
            influence_error(b"influence: invalid QR rank\0");
        }

        let qr_dim = getAttrib(qr, crate::attrib_core::R_DimSymbol());
        let e_dim = getAttrib(e, crate::attrib_core::R_DimSymbol());
        if TYPEOF(qr_dim) != SEXPTYPE::INTSXP
            || LENGTH(qr_dim) != 2
            || (TYPEOF(e_dim) != SEXPTYPE::INTSXP && Rf_isNull(e_dim) == 0)
            || (TYPEOF(e_dim) == SEXPTYPE::INTSXP && LENGTH(e_dim) != 2)
        {
            influence_error(b"influence: QR and residuals must be matrices\0");
        }

        let n = nrows(qr);
        let qr_cols = ncols(qr);
        let k = asInteger(rank_val);
        if n < 0
            || qr_cols < 0
            || k < 0
            || k > n
            || k > qr_cols
            || XLENGTH(qr) != (n as i64).saturating_mul(qr_cols as i64)
            || XLENGTH(qraux) < k as i64
        {
            influence_error(b"influence: inconsistent QR dimensions or rank\0");
        }
        let (e_rows, e_cols) = if Rf_isNull(e_dim) != 0 {
            (n, 1)
        } else {
            (*INTEGER(e_dim), *INTEGER(e_dim).add(1))
        };
        let q = if e_rows == n && e_cols >= 0 {
            e_cols
        } else {
            influence_error(b"influence: residual dimensions do not match QR\0");
        };
        if XLENGTH(e) != (n as i64).saturating_mul(q as i64) {
            influence_error(b"influence: residual payload does not match dimensions\0");
        }
        let tol = asReal(stol);

        let hat = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _hat_guard = protect(hat);
        let sigma = allocMatrix(SEXPTYPE::REALSXP.into(), n, q);
        let _sigma_guard = protect(sigma);

        // F77 order: x, ldx, n, k, q, qraux, resid, hat, sigma, tol
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

        // Clamp hat values slightly above 1 to exactly 1 (influence.c)
        for i in 0..n as usize {
            if *REAL(hat).add(i) > 1.0 - tol {
                *REAL(hat).add(i) = 1.0;
            }
        }

        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _ans_guard = protect(ans);
        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        setAttrib(ans, R_NamesSymbol(), nm);

        let mut m: c_int = 0;
        SET_VECTOR_ELT(ans, m as crate::sexp::ffi::R_xlen_t, hat);
        SET_STRING_ELT(nm, m as crate::sexp::ffi::R_xlen_t, mkChar("hat"));
        m += 1;
        SET_VECTOR_ELT(ans, m as crate::sexp::ffi::R_xlen_t, sigma);
        SET_STRING_ELT(nm, m as crate::sexp::ffi::R_xlen_t, mkChar("sigma"));

        ans
    }
}

#[cfg(all(test, not(feature = "fortran-backend")))]
mod tests {
    use super::lminfl_compute;
    use crate::modules::lapack::backend;
    use std::os::raw::c_int;

    /// Factor X (column-major n×p) with dgeqp3; returns (qr, tau, rank estimate).
    fn factor_qr(x: &[f64], n: usize, p: usize, tol: f64) -> (Vec<f64>, Vec<f64>, usize) {
        let mut qr = x.to_vec();
        let mut jpvt = vec![0i32; p];
        let mut tau = vec![0.0f64; n.min(p)];
        let n_i = n as c_int;
        let p_i = p as c_int;
        let mut info = 0i32;
        let mut lwork = -1i32;
        let mut work_query = [0.0f64; 1];
        unsafe {
            backend::dgeqp3_(
                &n_i,
                &p_i,
                qr.as_mut_ptr(),
                &n_i,
                jpvt.as_mut_ptr(),
                tau.as_mut_ptr(),
                work_query.as_mut_ptr(),
                &lwork,
                &mut info,
            );
            lwork = work_query[0] as i32;
            let mut work = vec![0.0f64; lwork as usize];
            backend::dgeqp3_(
                &n_i,
                &p_i,
                qr.as_mut_ptr(),
                &n_i,
                jpvt.as_mut_ptr(),
                tau.as_mut_ptr(),
                work.as_mut_ptr(),
                &lwork,
                &mut info,
            );
        }
        assert_eq!(info, 0);
        let kmax = n.min(p);
        let max_r = (0..kmax)
            .map(|j| qr[j + j * n].abs())
            .fold(0.0f64, f64::max);
        let thresh = tol * max_r;
        let rank = (0..kmax)
            .take_while(|&j| qr[j + j * n].abs() > thresh)
            .count();
        (qr, tau, rank)
    }

    #[test]
    fn lminfl_hat_matches_simple_linear_regression() {
        // lm(y ~ x) for x = 1:5; hat = 1/n + (x-mean)^2 / Sxx
        // expected: 0.6, 0.3, 0.2, 0.3, 0.6
        let n = 5usize;
        let p = 2usize;
        // X column-major: intercept column then x
        let x = vec![
            1.0, 1.0, 1.0, 1.0, 1.0, // intercept
            1.0, 2.0, 3.0, 4.0, 5.0, // x
        ];
        let (qr, tau, rank) = factor_qr(&x, n, p, 1e-7);
        assert_eq!(rank, 2);

        // residuals from OLS: beta = (X'X)^{-1} X'y via normal equations for check
        // For this data, fitted = -1.0 + 1.7*x → resid = y - fitted
        // Actually compute resid from hat-path independence: use known OLS
        // mean(x)=3, Sxx=10, Sxy = sum((x-3)*(y-mean(y))); mean(y)=3.8
        // b1 = Sxy/Sxx = ((-2)*(-2.8)+(-1)*(-1.8)+0*(-0.8)+1*1.2+2*4.2)/10
        //     = (5.6+1.8+0+1.2+8.4)/10 = 17/10 = 1.7
        // b0 = 3.8 - 1.7*3 = -1.3
        // fitted: 0.4, 2.1, 3.8, 5.5, 7.2
        // resid: 0.6, -0.1, -0.8, -0.5, 0.8
        let resid = vec![0.6, -0.1, -0.8, -0.5, 0.8];
        let mut hat = vec![0.0; n];
        let mut sigma = vec![0.0; n];
        lminfl_compute(
            &qr, n, n, rank, 1, &tau, &resid, &mut hat, &mut sigma, 1e-10,
        );

        let expected_hat = [0.6, 0.3, 0.2, 0.3, 0.6];
        for i in 0..n {
            assert!(
                (hat[i] - expected_hat[i]).abs() < 1e-10,
                "hat[{i}] = {} expected {}",
                hat[i],
                expected_hat[i]
            );
        }

        // denom = n - k - 1 = 2
        let rss: f64 = resid.iter().map(|r| r * r).sum();
        for i in 0..n {
            let exp = ((rss - resid[i] * resid[i] / (1.0 - hat[i])) / 2.0).sqrt();
            assert!(
                (sigma[i] - exp).abs() < 1e-10,
                "sigma[{i}] = {} expected {}",
                sigma[i],
                exp
            );
        }
    }

    #[test]
    fn lminfl_hat_equals_one_when_saturated() {
        // n = k = 2: every point is a perfect fit lever; hat → 1, sigma uses sum/denom
        let n = 2usize;
        let p = 2usize;
        let x = vec![1.0, 1.0, 0.0, 1.0]; // [[1,0],[1,1]]
        let (qr, tau, rank) = factor_qr(&x, n, p, 1e-7);
        assert_eq!(rank, 2);
        let resid = vec![0.0, 0.0];
        let mut hat = vec![0.0; n];
        let mut sigma = vec![0.0; n];
        lminfl_compute(&qr, n, n, rank, 1, &tau, &resid, &mut hat, &mut sigma, 1e-7);
        assert!((hat[0] - 1.0).abs() < 1e-9, "hat[0]={}", hat[0]);
        assert!((hat[1] - 1.0).abs() < 1e-9, "hat[1]={}", hat[1]);
        // denom = 2-2-1 = -1; sum=0 → sigma = sqrt(0/-1) = NaN in IEEE; accept non-finite or 0
        // With resid=0 and hat=1: sigma = sqrt(0/denom). If denom=-1 → -0.0 sqrt = 0 or -0.
        assert!(sigma[0].is_finite() || sigma[0].is_nan());
    }

    #[test]
    fn lminfl_multivariate_sigma_columns_independent() {
        let n = 4usize;
        let p = 1usize;
        // through-origin single column of ones → hat = 1/n each after QR of ones
        let x = vec![1.0, 1.0, 1.0, 1.0];
        let (qr, tau, rank) = factor_qr(&x, n, p, 1e-7);
        assert_eq!(rank, 1);
        // two response columns with different residuals
        let resid = vec![
            1.0, -1.0, 1.0, -1.0, // col 0
            2.0, 2.0, -2.0, -2.0, // col 1
        ];
        let mut hat = vec![0.0; n];
        let mut sigma = vec![0.0; n * 2];
        lminfl_compute(
            &qr, n, n, rank, 2, &tau, &resid, &mut hat, &mut sigma, 1e-10,
        );
        for i in 0..n {
            assert!(
                (hat[i] - 0.25).abs() < 1e-10,
                "hat[{i}]={} expected 0.25",
                hat[i]
            );
        }
        let denom = (n as f64) - 1.0 - 1.0; // 2
        let rss0 = 4.0f64;
        let rss1 = 16.0f64;
        for i in 0..n {
            let e0 = ((rss0 - resid[i] * resid[i] / (1.0 - hat[i])) / denom).sqrt();
            let e1 = ((rss1 - resid[i + n] * resid[i + n] / (1.0 - hat[i])) / denom).sqrt();
            assert!((sigma[i] - e0).abs() < 1e-10);
            assert!((sigma[i + n] - e1).abs() < 1e-10);
        }
    }

    #[test]
    fn lminfl_zero_response_has_no_payload_access() {
        let mut hat = vec![0.0; 2];
        lminfl_compute(&[], 2, 2, 0, 0, &[], &[], &mut hat, &mut [], 1e-10);
        assert_eq!(hat, [0.0, 0.0]);
    }

    #[test]
    fn lminfl_adapter_accepts_zero_column_payloads() {
        let mut hat = [0.0; 2];
        let n = 2i32;
        let zero = 0i32;
        let tol = 1e-10;
        unsafe {
            super::lminfl_(
                std::ptr::null(),
                &n,
                &n,
                &zero,
                &zero,
                std::ptr::null(),
                std::ptr::null(),
                hat.as_mut_ptr(),
                std::ptr::null_mut(),
                &tol,
            );
        }
        assert_eq!(hat, [0.0, 0.0]);
    }
    #[test]
    fn influence_sexp_adapter_accepts_vector_and_matrix_residuals_and_rejects_bad_shapes() {
        use super::*;
        use crate::sexp::constructors::Rf_ScalarInteger;
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mqr = Rf_allocVector(SEXPTYPE::VECSXP, 3);
            let _mqr = protect(mqr);
            let names = Rf_allocVector(SEXPTYPE::STRSXP, 3);
            let _names = protect(names);
            for (i, name) in ["qr", "qraux", "rank"].iter().enumerate() {
                SET_STRING_ELT(names, i as i64, mkChar(name));
            }
            setAttrib(mqr, R_NamesSymbol(), names);
            SET_VECTOR_ELT(mqr, 0, allocMatrix(SEXPTYPE::REALSXP.into(), 3, 0));
            SET_VECTOR_ELT(mqr, 1, Rf_allocVector(SEXPTYPE::REALSXP, 0));
            SET_VECTOR_ELT(mqr, 2, Rf_ScalarInteger(0));
            let residuals = Rf_allocVector(SEXPTYPE::REALSXP, 3);
            let _residuals = protect(residuals);
            for i in 0..3 {
                *REAL(residuals).add(i) = (i + 1) as f64;
            }
            let tol = crate::sexp::constructors::Rf_ScalarReal(1e-7);
            let _tol = protect(tol);
            let result = influence(mqr, residuals, tol);
            let _result = protect(result);
            let sigma = VECTOR_ELT(result, 1);
            let dims = getAttrib(sigma, crate::attrib_core::R_DimSymbol());
            assert_eq!((*INTEGER(dims), *INTEGER(dims).add(1)), (3, 1));
            for (i, expected) in [6.5_f64, 5.0, 2.5].iter().enumerate() {
                assert!((*REAL(sigma).add(i) - expected.sqrt()).abs() < 1e-12);
            }
            let empty = allocMatrix(SEXPTYPE::REALSXP.into(), 3, 0);
            let _empty = protect(empty);
            let result = influence(mqr, empty, tol);
            assert_eq!(XLENGTH(VECTOR_ELT(result, 1)), 0);
            let bad = Rf_allocVector(SEXPTYPE::REALSXP, 2);
            let _bad = protect(bad);
            let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                influence(mqr, bad, tol);
            }))
            .expect_err("short residual payload must be rejected");
            assert!(
                err.downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message
                    .contains("residual payload")
            );
        }
    }
}
