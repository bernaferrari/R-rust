/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001--2025  The R Core Team.
 *
 *  Ported to Rust from R's src/modules/lapack/Lapack.c
 *
 *  Interface routines for LAPACK, callable from R via .Internal.
 */

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::*;

use super::backend::{
    La_norm_type, La_rcond_type, La_valid_uplo, LapRcomplex, fort_char, fort_str, unscramble,
};

// Local SEXPTYPE constants (as c_int for coerceVector etc.)
const REALSXP_C: c_int = 14;
const INTSXP_C: c_int = 13;
const CPLXSXP_C: c_int = 15;
const STRSXP_C: c_int = 16;
const LGLSXP_C: c_int = 10;
const VECSXP_C: c_int = 19;

/// La_svd - real singular value decomposition.
///
/// Port of: static SEXP La_svd(SEXP jobu, SEXP x, SEXP s, SEXP u, SEXP vt)
pub unsafe fn La_svd(jobu: SEXP, x: SEXP, s: SEXP, u: SEXP, vt: SEXP) -> SEXP {
    // Validate jobu is a string
    if TYPEOF(jobu) != 16 {
        Rf_error(b"'jobu' must be a character string\0".as_ptr() as *const c_char);
    }

    // Get matrix dimensions from dim attribute
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let p = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    let mut nprot: c_int = 2;

    // Work on a copy of x
    let xvals: *mut f64;
    let mut x = x;
    if TYPEOF(x) != 14 {
        x = Rf_protect(coerceVector(x, REALSXP_C));
        nprot += 1;
        xvals = REAL(x);
    } else {
        let len = (n as usize) * (p as usize);
        xvals = R_alloc(len, std::mem::size_of::<f64>()) as *mut f64;
        ptr::copy_nonoverlapping(REAL(x), xvals, len);
    }

    // Get leading dimensions from u and vt
    let u_dims = getAttrib(u, R_DimSymbol());
    let ldu = INTEGER(coerceVector(u_dims, INTSXP_C)).add(0).read() as i32;

    let vt_dims = getAttrib(vt, R_DimSymbol());
    let ldvt = INTEGER(coerceVector(vt_dims, INTSXP_C)).add(0).read() as i32;

    let mut tmp: f64 = 0.0;
    let min_np = if n < p { n } else { p };
    let iwork = R_alloc(8 * min_np as usize, std::mem::size_of::<c_int>()) as *mut c_int;

    let ju = CHAR(STRING_ELT(jobu, 0)) as *const u8;
    let mut info: c_int = 0;
    let mut lwork: c_int = -1;

    // Query optimal work size
    super::backend::dgesdd_(
        ju,
        &n,
        &p,
        xvals,
        &n,
        REAL(s),
        REAL(u),
        &ldu,
        REAL(vt),
        &ldvt,
        &mut tmp,
        &lwork,
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgesdd'\0".as_ptr() as *const c_char);
    }

    lwork = tmp as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;

    // Actual computation
    super::backend::dgesdd_(
        ju,
        &n,
        &p,
        xvals,
        &n,
        REAL(s),
        REAL(u),
        &ldu,
        REAL(vt),
        &ldvt,
        work,
        &lwork,
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgesdd'\0".as_ptr() as *const c_char);
    }

    // Build result list: list(d=s, u=u, vt=vt)
    let val = Rf_protect(Rf_allocVector(VECSXP_C, 3));
    let nm = Rf_protect(Rf_allocVector(STRSXP_C, 3));
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"u\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"vt\0".as_ptr() as *const c_char));
    setAttrib(val, R_NamesSymbol(), nm);
    SET_VECTOR_ELT(val, 0, s);
    SET_VECTOR_ELT(val, 1, u);
    SET_VECTOR_ELT(val, 2, vt);

    Rf_unprotect(nprot);
    val
}

/// La_rs - real symmetric eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rs(SEXP x, SEXP only_values)
pub unsafe fn La_rs(x: SEXP, only_values: SEXP) -> SEXP {
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'x' must be a square numeric matrix\0".as_ptr() as *const c_char);
    }

    let ov = asLogical(only_values);
    if ov == NA_INTEGER {
        Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
    }

    let jobv = if ov != 0 { b'N' } else { b'V' };
    let uplo = b'L';
    let range = b'A';

    // Work on a copy of x
    let rx: *mut f64;
    let mut x = x;
    if TYPEOF(x) != 14 {
        x = Rf_protect(coerceVector(x, REALSXP_C));
        rx = REAL(x);
    } else {
        rx = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
        ptr::copy_nonoverlapping(REAL(x), rx, (n as usize) * (n as usize));
    }
    Rf_protect(x);

    let values = Rf_protect(Rf_allocVector(REALSXP_C, n as c_int));
    let rvalues = REAL(values);

    let mut z = R_NilValue();
    let mut rz: *mut f64 = ptr::null_mut();
    if ov == 0 {
        z = Rf_protect(Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int)));
        rz = REAL(z);
    }

    let isuppz = R_alloc(2 * n as usize, std::mem::size_of::<c_int>()) as *mut c_int;

    // Query optimal work sizes
    let mut tmp: f64 = 0.0;
    let mut itmp: c_int = 0;
    let mut lwork: c_int = -1;
    let mut liwork: c_int = -1;
    let mut m: c_int = 0;
    let mut info: c_int = 0;

    super::backend::dsyevr_(
        &jobv, &range, &uplo, &n, rx, &n, &0.0f64, &0.0f64, &0, &0, &0.0f64, &mut m, rvalues, rz,
        &n, isuppz, &mut tmp, &lwork, &mut itmp, &liwork, &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dsyevr'\0".as_ptr() as *const c_char);
    }

    lwork = tmp as c_int;
    liwork = itmp;

    let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;
    let iwork = R_alloc(liwork as usize, std::mem::size_of::<c_int>()) as *mut c_int;

    super::backend::dsyevr_(
        &jobv, &range, &uplo, &n, rx, &n, &0.0f64, &0.0f64, &0, &0, &0.0f64, &mut m, rvalues, rz,
        &n, isuppz, work, &lwork, iwork, &liwork, &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dsyevr'\0".as_ptr() as *const c_char);
    }

    let ret;
    let nm;
    if ov == 0 {
        ret = Rf_protect(Rf_allocVector(VECSXP_C, 2));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 2));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 1, z);
    } else {
        ret = Rf_protect(Rf_allocVector(VECSXP_C, 1));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 1));
    }
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
    setAttrib(ret, R_NamesSymbol(), nm);
    SET_VECTOR_ELT(ret, 0, values);

    Rf_unprotect(if ov != 0 { 4 } else { 5 });
    ret
}

/// La_rg - real eigenvalues/eigenvectors (general, non-symmetric).
///
/// Port of: static SEXP La_rg(SEXP x, SEXP only_values)
pub unsafe fn La_rg(x: SEXP, only_values: SEXP) -> SEXP {
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'x' must be a square numeric matrix\0".as_ptr() as *const c_char);
    }

    let ov = asLogical(only_values);
    if ov == NA_INTEGER {
        Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
    }

    let jobvl = b'N';
    let jobvr = if ov != 0 { b'N' } else { b'V' };

    // Work on a copy of x
    let xvals: *mut f64;
    let mut x = x;
    if TYPEOF(x) != 14 {
        x = Rf_protect(coerceVector(x, REALSXP_C));
        xvals = REAL(x);
    } else {
        xvals = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
        ptr::copy_nonoverlapping(REAL(x), xvals, (n as usize) * (n as usize));
    }
    Rf_protect(x);

    let wR = R_alloc(n as usize, std::mem::size_of::<f64>()) as *mut f64;
    let wI = R_alloc(n as usize, std::mem::size_of::<f64>()) as *mut f64;

    let mut right: *mut f64 = ptr::null_mut();
    if ov == 0 {
        right = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
    }

    // Query optimal work size
    let mut tmp: f64 = 0.0;
    let mut lwork: c_int = -1;
    let mut info: c_int = 0;

    super::backend::dgeev_(
        &jobvl,
        &jobvr,
        &n,
        xvals,
        &n,
        wR,
        wI,
        ptr::null_mut(),
        &1,
        right,
        &n,
        &mut tmp,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgeev'\0".as_ptr() as *const c_char);
    }

    lwork = tmp as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;

    super::backend::dgeev_(
        &jobvl,
        &jobvr,
        &n,
        xvals,
        &n,
        wR,
        wI,
        ptr::null_mut(),
        &1,
        right,
        &n,
        work,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgeev'\0".as_ptr() as *const c_char);
    }

    // Build the result
    let ret;
    let nm;

    if ov == 0 {
        // Check if any eigenvalues are complex
        let has_complex = (0..n as usize).any(|i| *wI.add(i) != 0.0);

        if has_complex {
            let imaginary = std::slice::from_raw_parts(wI, n as usize);
            let vecs = std::slice::from_raw_parts(right, (n as usize) * (n as usize));
            let cmplx_vecs = unscramble(imaginary, n, vecs);

            // Build complex eigenvalue vector
            let values = Rf_protect(Rf_allocVector(CPLXSXP_C, n as c_int));
            for i in 0..n as usize {
                let c = COMPLEX(values).add(i);
                (*c).r = *wR.add(i);
                (*c).i = *wI.add(i);
            }

            // Build complex eigenvector matrix
            let z = Rf_protect(Rf_allocVector(CPLXSXP_C, (n as c_int) * (n as c_int)));
            for i in 0..cmplx_vecs.len() {
                let c = COMPLEX(z).add(i);
                (*c).r = cmplx_vecs[i].r;
                (*c).i = cmplx_vecs[i].i;
            }

            ret = Rf_protect(Rf_allocVector(VECSXP_C, 2));
            nm = Rf_protect(Rf_allocVector(STRSXP_C, 2));
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            SET_VECTOR_ELT(ret, 1, z);
            setAttrib(ret, R_NamesSymbol(), nm);

            Rf_unprotect(6); // x, values, z, ret, nm, + values_protect
            return ret;
        } else {
            // All real eigenvalues
            let values = Rf_protect(Rf_allocVector(REALSXP_C, n as c_int));
            ptr::copy_nonoverlapping(wR, REAL(values), n as usize);

            let z = Rf_protect(Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int)));
            ptr::copy_nonoverlapping(right, REAL(z), (n as usize) * (n as usize));

            ret = Rf_protect(Rf_allocVector(VECSXP_C, 2));
            nm = Rf_protect(Rf_allocVector(STRSXP_C, 2));
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            SET_VECTOR_ELT(ret, 1, z);
            setAttrib(ret, R_NamesSymbol(), nm);

            Rf_unprotect(6);
            return ret;
        }
    } else {
        // Only values
        let values = Rf_protect(Rf_allocVector(REALSXP_C, n as c_int));
        ptr::copy_nonoverlapping(wR, REAL(values), n as usize);

        ret = Rf_protect(Rf_allocVector(VECSXP_C, 1));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 1));
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, values);
        setAttrib(ret, R_NamesSymbol(), nm);

        Rf_unprotect(5);
        ret
    }
}

/// La_dlange - real matrix norm.
///
/// Port of: static SEXP La_dlange(SEXP a, SEXP type_)
pub unsafe fn La_dlange(a: SEXP, type_: SEXP) -> SEXP {
    if TYPEOF(type_) != 16 {
        Rf_error(b"'type' must be a character string\0".as_ptr() as *const c_char);
    }

    let typ_str = CStr::from_ptr(CHAR(STRING_ELT(type_, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_norm_type(typ_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;

    let mut work = vec![
        0.0f64;
        if norm_c == b'I' || norm_c == b'O' {
            m as usize
        } else {
            0
        }
    ];

    let anorm = super::backend::dlange_(&norm_c, &m, &n, REAL(a), &m, work.as_mut_ptr());

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = anorm;
    ans
}

/// La_dgecon - real matrix condition number estimate.
///
/// Port of: static SEXP La_dgecon(SEXP a, SEXP norm)
pub unsafe fn La_dgecon(a: SEXP, norm: SEXP) -> SEXP {
    if TYPEOF(norm) != 16 {
        Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
    }

    let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_rcond_type(norm_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    // Compute the norm of A
    let mut work_norm = vec![0.0f64; if norm_c == b'I' { n as usize } else { 0 }];
    let anorm = super::backend::dlange_(&norm_c, &n, &n, REAL(a), &n, work_norm.as_mut_ptr());

    if anorm == 0.0 {
        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = f64::INFINITY;
        return ans;
    }

    // Work on a copy
    let mut a_copy = vec![0.0f64; (n as usize) * (n as usize)];
    ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), a_copy.len());

    let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;

    // LU factorization
    super::backend::dgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);
    if info > 0 {
        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = 0.0;
        return ans;
    }
    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgetrf'\0".as_ptr() as *const c_char);
    }

    let mut rcond: f64 = 0.0;
    let mut work = vec![0.0f64; 4 * n as usize];
    let iwork = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;

    super::backend::dgecon_(
        &norm_c,
        &n,
        a_copy.as_ptr(),
        &n,
        &anorm,
        &mut rcond,
        work.as_mut_ptr(),
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgecon'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = rcond;
    ans
}

/// La_dtrcon - real triangular condition number.
///
/// Port of: static SEXP La_dtrcon(SEXP a, SEXP norm)
pub unsafe fn La_dtrcon(a: SEXP, norm: SEXP) -> SEXP {
    if TYPEOF(norm) != 16 {
        Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
    }

    let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_rcond_type(norm_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let mut rcond: f64 = 0.0;
    let mut work = vec![0.0f64; 3 * n as usize];
    let iwork = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;

    let uplo = b'U'; // Default upper
    let diag = b'N'; // Non-unit triangular

    super::backend::dtrcon_(
        &norm_c,
        &uplo,
        &diag,
        &n,
        REAL(a),
        &n,
        &mut rcond,
        work.as_mut_ptr(),
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dtrcon'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = rcond;
    ans
}

/// La_dtrcon3 - real triangular condition number with explicit uplo.
///
/// Port of: static SEXP La_dtrcon3(SEXP a, SEXP norm, SEXP uplo)
pub unsafe fn La_dtrcon3(a: SEXP, norm: SEXP, uplo: SEXP) -> SEXP {
    if TYPEOF(norm) != 16 {
        Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
    }
    if TYPEOF(uplo) != 16 {
        Rf_error(b"'uplo' must be a character string\0".as_ptr() as *const c_char);
    }

    let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_rcond_type(norm_str);
    let uplo_str = CStr::from_ptr(CHAR(STRING_ELT(uplo, 0)))
        .to_str()
        .unwrap_or("U");
    let uplo_c = La_valid_uplo(uplo_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let mut rcond: f64 = 0.0;
    let mut work = vec![0.0f64; 3 * n as usize];
    let iwork = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let diag = b'N';
    let mut info: c_int = 0;

    super::backend::dtrcon_(
        &norm_c,
        &uplo_c,
        &diag,
        &n,
        REAL(a),
        &n,
        &mut rcond,
        work.as_mut_ptr(),
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dtrcon'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = rcond;
    ans
}

/// La_zlange - complex matrix norm.
///
/// Port of: static SEXP La_zlange(SEXP a, SEXP type_)
pub unsafe fn La_zlange(a: SEXP, type_: SEXP) -> SEXP {
    if TYPEOF(type_) != 16 {
        Rf_error(b"'type' must be a character string\0".as_ptr() as *const c_char);
    }

    let typ_str = CStr::from_ptr(CHAR(STRING_ELT(type_, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_norm_type(typ_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;

    let mut work = vec![
        0.0f64;
        if norm_c == b'I' || norm_c == b'O' {
            m as usize
        } else {
            0
        }
    ];

    let a_ptr = COMPLEX(a) as *const LapRcomplex;
    let anorm = super::backend::zlange_(&norm_c, &m, &n, a_ptr, &m, work.as_mut_ptr());

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = anorm;
    ans
}

/// La_zgecon - complex matrix condition number estimate.
///
/// Port of: static SEXP La_zgecon(SEXP a, SEXP norm)
pub unsafe fn La_zgecon(a: SEXP, norm: SEXP) -> SEXP {
    if TYPEOF(norm) != 16 {
        Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
    }

    let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_rcond_type(norm_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let mut work_norm = vec![0.0f64; if norm_c == b'I' { n as usize } else { 0 }];
    let anorm = super::backend::zlange_(
        &norm_c,
        &n,
        &n,
        COMPLEX(a) as *const LapRcomplex,
        &n,
        work_norm.as_mut_ptr(),
    );

    if anorm == 0.0 {
        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = f64::INFINITY;
        return ans;
    }

    // Work on a copy
    let len = (n as usize) * (n as usize);
    let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
    ptr::copy_nonoverlapping(COMPLEX(a) as *const LapRcomplex, a_copy.as_mut_ptr(), len);

    let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;

    super::backend::zgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);
    if info > 0 {
        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = 0.0;
        return ans;
    }
    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgetrf'\0".as_ptr() as *const c_char);
    }

    let mut rcond: f64 = 0.0;
    let mut work = vec![LapRcomplex::default(); 2 * n as usize];
    let mut rwork = vec![0.0f64; 2 * n as usize];

    super::backend::zgecon_(
        &norm_c,
        &n,
        a_copy.as_ptr(),
        &n,
        &anorm,
        &mut rcond,
        work.as_mut_ptr(),
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgecon'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = rcond;
    ans
}

/// La_ztrcon - complex triangular condition number.
///
/// Port of: static SEXP La_ztrcon(SEXP a, SEXP norm)
pub unsafe fn La_ztrcon(a: SEXP, norm: SEXP) -> SEXP {
    if TYPEOF(norm) != 16 {
        Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
    }

    let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_rcond_type(norm_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let mut rcond: f64 = 0.0;
    let mut work = vec![LapRcomplex::default(); 2 * n as usize];
    let mut rwork = vec![0.0f64; n as usize];
    let uplo = b'U';
    let diag = b'N';
    let mut info: c_int = 0;

    super::backend::ztrcon_(
        &norm_c,
        &uplo,
        &diag,
        &n,
        COMPLEX(a) as *const LapRcomplex,
        &n,
        &mut rcond,
        work.as_mut_ptr(),
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'ztrcon'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = rcond;
    ans
}

/// La_ztrcon3 - complex triangular condition number with explicit uplo.
///
/// Port of: static SEXP La_ztrcon3(SEXP a, SEXP norm, SEXP uplo)
pub unsafe fn La_ztrcon3(a: SEXP, norm: SEXP, uplo: SEXP) -> SEXP {
    if TYPEOF(norm) != 16 {
        Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
    }
    if TYPEOF(uplo) != 16 {
        Rf_error(b"'uplo' must be a character string\0".as_ptr() as *const c_char);
    }

    let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
        .to_str()
        .unwrap_or("O");
    let norm_c = La_rcond_type(norm_str);
    let uplo_str = CStr::from_ptr(CHAR(STRING_ELT(uplo, 0)))
        .to_str()
        .unwrap_or("U");
    let uplo_c = La_valid_uplo(uplo_str);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let mut rcond: f64 = 0.0;
    let mut work = vec![LapRcomplex::default(); 2 * n as usize];
    let mut rwork = vec![0.0f64; n as usize];
    let diag = b'N';
    let mut info: c_int = 0;

    super::backend::ztrcon_(
        &norm_c,
        &uplo_c,
        &diag,
        &n,
        COMPLEX(a) as *const LapRcomplex,
        &n,
        &mut rcond,
        work.as_mut_ptr(),
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'ztrcon'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_allocVector(REALSXP_C, 1);
    *REAL(ans) = rcond;
    ans
}

/// La_chol - real Cholesky decomposition.
///
/// Port of: static SEXP La_chol(SEXP a, SEXP pivot, SEXP stol)
pub unsafe fn La_chol(a: SEXP, pivot: SEXP, stol: SEXP) -> SEXP {
    let piv = asLogical(pivot);
    if piv == NA_INTEGER {
        Rf_error(b"invalid 'pivot' argument\0".as_ptr() as *const c_char);
    }

    let tol = asReal(stol);

    let dim = getAttrib(a, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    // Work on a copy
    let mut a_copy = vec![0.0f64; (n as usize) * (n as usize)];
    ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), a_copy.len());

    let mut info: c_int = 0;
    let uplo = b'U';

    if piv != 0 {
        // pivoted Cholesky: dpstrf
        let piv_arr = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut rank: c_int = 0;
        let mut work = vec![0.0f64; 2 * n as usize];

        super::backend::dpstrf_(
            &uplo,
            &n,
            a_copy.as_mut_ptr(),
            &n,
            piv_arr,
            &mut rank,
            &tol,
            work.as_mut_ptr(),
            &mut info,
        );

        if info < 0 {
            Rf_error(b"error code from Lapack routine 'dpstrf'\0".as_ptr() as *const c_char);
        }

        // Build result: list(rank, factors, pivot)
        let ret = Rf_protect(Rf_allocVector(VECSXP_C, 3));
        let nm = Rf_protect(Rf_allocVector(STRSXP_C, 3));
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"factors\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));

        let rank_s = Rf_allocVector(INTSXP_C, 1);
        *INTEGER(rank_s) = rank;
        SET_VECTOR_ELT(ret, 0, rank_s);

        let factors = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
        ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(factors), a_copy.len());
        SET_VECTOR_ELT(ret, 1, factors);

        let pivot_s = Rf_allocVector(INTSXP_C, n as c_int);
        for i in 0..n as usize {
            *INTEGER(pivot_s).add(i) = *piv_arr.add(i);
        }
        SET_VECTOR_ELT(ret, 2, pivot_s);

        setAttrib(ret, R_NamesSymbol(), nm);
        Rf_unprotect(2);
        ret
    } else {
        // Non-pivoted Cholesky: dpotrf
        super::backend::dpotrf_(&uplo, &n, a_copy.as_mut_ptr(), &n, &mut info);

        if info != 0 {
            Rf_error(b"not positive definite\0".as_ptr() as *const c_char);
        }

        // Zero out the lower triangle (R returns upper triangle only)
        for j in 0..n as usize {
            for i in (j + 1)..n as usize {
                a_copy[i + j * n as usize] = 0.0;
            }
        }

        let ans = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
        ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), a_copy.len());
        ans
    }
}

/// La_chol2inv - real inverse from Cholesky factor.
///
/// Port of: static SEXP La_chol2inv(SEXP a, SEXP size)
pub unsafe fn La_chol2inv(a: SEXP, size: SEXP) -> SEXP {
    let n = asInteger(size);
    if n == NA_INTEGER || n <= 0 {
        Rf_error(b"'size' must be a positive integer\0".as_ptr() as *const c_char);
    }

    // Work on a copy
    let mut a_copy = vec![0.0f64; (n as usize) * (n as usize)];
    ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), a_copy.len());

    let mut info: c_int = 0;
    let uplo = b'U';

    super::backend::dpotri_(&uplo, &n, a_copy.as_mut_ptr(), &n, &mut info);

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dpotri'\0".as_ptr() as *const c_char);
    }

    // Copy upper triangle to lower
    for j in 1..n as usize {
        for i in 0..j {
            a_copy[i + j * n as usize] = a_copy[j + i * n as usize];
        }
    }

    let ans = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
    ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), a_copy.len());
    ans
}

/// La_solve - real linear solve.
///
/// Port of: static SEXP La_solve(SEXP a, SEXP bin, SEXP tolin)
pub unsafe fn La_solve(a: SEXP, bin: SEXP, tolin: SEXP) -> SEXP {
    let _tol = asReal(tolin);

    let a_dim = getAttrib(a, R_DimSymbol());
    if a_dim.is_null() || a_dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(a_dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(a_dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let b_dim = getAttrib(bin, R_DimSymbol());
    if b_dim.is_null() || b_dim == R_NilValue() {
        Rf_error(b"'b' must be a matrix\0".as_ptr() as *const c_char);
    }

    let nrhs = INTEGER(coerceVector(b_dim, INTSXP_C)).add(1).read() as i32;
    let m_b = INTEGER(coerceVector(b_dim, INTSXP_C)).add(0).read() as i32;
    if m_b != n {
        Rf_error(b"'b' must have same row dimension as 'a'\0".as_ptr() as *const c_char);
    }

    // Work on copies
    let len_a = (n as usize) * (n as usize);
    let mut a_copy = vec![0.0f64; len_a];
    ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len_a);

    let len_b = (n as usize) * (nrhs as usize);
    let mut b_copy = vec![0.0f64; len_b];
    ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);

    let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;

    super::backend::dgesv_(
        &n,
        &nrhs,
        a_copy.as_mut_ptr(),
        &n,
        ipiv,
        b_copy.as_mut_ptr(),
        &n,
        &mut info,
    );

    if info > 0 {
        Rf_error(b"singular matrix in 'solve'\0".as_ptr() as *const c_char);
    }
    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgesv'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_protect(Rf_allocVector(REALSXP_C, len_b as c_int));
    ptr::copy_nonoverlapping(b_copy.as_ptr(), REAL(ans), len_b);
    Rf_unprotect(1);
    ans
}

/// La_solve_cmplx - complex linear solve.
///
/// Port of: static SEXP La_solve_cmplx(SEXP a, SEXP bin, SEXP tolin)
pub unsafe fn La_solve_cmplx(a: SEXP, bin: SEXP, tolin: SEXP) -> SEXP {
    let _tol = asReal(tolin);

    let a_dim = getAttrib(a, R_DimSymbol());
    if a_dim.is_null() || a_dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(a_dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(a_dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let b_dim = getAttrib(bin, R_DimSymbol());
    if b_dim.is_null() || b_dim == R_NilValue() {
        Rf_error(b"'b' must be a matrix\0".as_ptr() as *const c_char);
    }

    let nrhs = INTEGER(coerceVector(b_dim, INTSXP_C)).add(1).read() as i32;
    let m_b = INTEGER(coerceVector(b_dim, INTSXP_C)).add(0).read() as i32;
    if m_b != n {
        Rf_error(b"'b' must have same row dimension as 'a'\0".as_ptr() as *const c_char);
    }

    let len_a = (n as usize) * (n as usize);
    let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_a];
    ptr::copy_nonoverlapping(COMPLEX(a) as *const LapRcomplex, a_copy.as_mut_ptr(), len_a);

    let len_b = (n as usize) * (nrhs as usize);
    let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
    ptr::copy_nonoverlapping(
        COMPLEX(bin) as *const LapRcomplex,
        b_copy.as_mut_ptr(),
        len_b,
    );

    let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;

    super::backend::zgesv_(
        &n,
        &nrhs,
        a_copy.as_mut_ptr(),
        &n,
        ipiv,
        b_copy.as_mut_ptr(),
        &n,
        &mut info,
    );

    if info > 0 {
        Rf_error(b"singular matrix in 'solve'\0".as_ptr() as *const c_char);
    }
    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgesv'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_protect(Rf_allocVector(CPLXSXP_C, len_b as c_int));
    ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
    Rf_unprotect(1);
    ans
}

/// La_qr - real QR decomposition.
///
/// Port of: static SEXP La_qr(SEXP ain)
pub unsafe fn La_qr(ain: SEXP) -> SEXP {
    let dim = getAttrib(ain, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    let min_mn = if m < n { m } else { n };

    // Work on a copy
    let len = (m as usize) * (n as usize);
    let mut a_copy = vec![0.0f64; len];
    ptr::copy_nonoverlapping(REAL(ain), a_copy.as_mut_ptr(), len);

    let jpvt = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    // Zero out jpvt (no initial column selection)
    for i in 0..n as usize {
        *jpvt.add(i) = 0;
    }

    let tau = R_alloc(min_mn as usize, std::mem::size_of::<f64>()) as *mut f64;

    // Query optimal work size
    let mut tmp: f64 = 0.0;
    let mut lwork: c_int = -1;
    let mut info: c_int = 0;

    super::backend::dgeqp3_(
        &m,
        &n,
        a_copy.as_mut_ptr(),
        &m,
        jpvt,
        tau,
        &mut tmp,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgeqp3'\0".as_ptr() as *const c_char);
    }

    lwork = tmp as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;

    super::backend::dgeqp3_(
        &m,
        &n,
        a_copy.as_mut_ptr(),
        &m,
        jpvt,
        tau,
        work,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dgeqp3'\0".as_ptr() as *const c_char);
    }

    // Build result: list(qr=qr_matrix, rank=rank, qraux=qraux, pivot=pivot)
    let qr = Rf_protect(Rf_allocVector(REALSXP_C, len as c_int));
    ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(qr), len);

    let qraux = Rf_protect(Rf_allocVector(REALSXP_C, min_mn as c_int));
    // qraux stores: tau for first min(m,n) columns, then norms of remaining columns
    for i in 0..min_mn as usize {
        *REAL(qraux).add(i) = *tau.add(i);
    }

    let pivot = Rf_protect(Rf_allocVector(INTSXP_C, n as c_int));
    for i in 0..n as usize {
        *INTEGER(pivot).add(i) = *jpvt.add(i);
    }

    let ret = Rf_protect(Rf_allocVector(VECSXP_C, 4));
    let nm = Rf_protect(Rf_allocVector(STRSXP_C, 4));
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"qr\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 3, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));
    SET_VECTOR_ELT(ret, 0, qr);
    SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(min_mn));
    SET_VECTOR_ELT(ret, 2, qraux);
    SET_VECTOR_ELT(ret, 3, pivot);
    setAttrib(ret, R_NamesSymbol(), nm);

    Rf_unprotect(5);
    ret
}

/// La_qr_cmplx - complex QR decomposition.
///
/// Port of: static SEXP La_qr_cmplx(SEXP ain)
pub unsafe fn La_qr_cmplx(ain: SEXP) -> SEXP {
    let dim = getAttrib(ain, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    let min_mn = if m < n { m } else { n };

    let len = (m as usize) * (n as usize);
    let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
    ptr::copy_nonoverlapping(COMPLEX(ain) as *const LapRcomplex, a_copy.as_mut_ptr(), len);

    let jpvt = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    for i in 0..n as usize {
        *jpvt.add(i) = 0;
    }

    let tau = R_alloc(min_mn as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

    // Query optimal work size
    let mut tmp = LapRcomplex::default();
    let mut lwork: c_int = -1;
    let mut rwork = vec![0.0f64; 2 * n as usize];
    let mut info: c_int = 0;

    super::backend::zgeqp3_(
        &m,
        &n,
        a_copy.as_mut_ptr(),
        &m,
        jpvt,
        tau,
        &mut tmp,
        &lwork,
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgeqp3'\0".as_ptr() as *const c_char);
    }

    lwork = tmp.r as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

    super::backend::zgeqp3_(
        &m,
        &n,
        a_copy.as_mut_ptr(),
        &m,
        jpvt,
        tau,
        work,
        &lwork,
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgeqp3'\0".as_ptr() as *const c_char);
    }

    let qr = Rf_protect(Rf_allocVector(CPLXSXP_C, len as c_int));
    ptr::copy_nonoverlapping(a_copy.as_ptr(), COMPLEX(qr) as *mut LapRcomplex, len);

    let qraux = Rf_protect(Rf_allocVector(CPLXSXP_C, min_mn as c_int));
    for i in 0..min_mn as usize {
        *COMPLEX(qraux).add(i) = {
            // SAFETY: LapRcomplex and Rcomplex have identical layouts: #[repr(C)] struct { r: f64, i: f64 }
            std::mem::transmute::<LapRcomplex, Rcomplex>(*tau.add(i))
        };
    }

    let pivot = Rf_protect(Rf_allocVector(INTSXP_C, n as c_int));
    for i in 0..n as usize {
        *INTEGER(pivot).add(i) = *jpvt.add(i);
    }

    let ret = Rf_protect(Rf_allocVector(VECSXP_C, 4));
    let nm = Rf_protect(Rf_allocVector(STRSXP_C, 4));
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"qr\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 3, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));
    SET_VECTOR_ELT(ret, 0, qr);
    SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(min_mn));
    SET_VECTOR_ELT(ret, 2, qraux);
    SET_VECTOR_ELT(ret, 3, pivot);
    setAttrib(ret, R_NamesSymbol(), nm);

    Rf_unprotect(5);
    ret
}

/// La_svd_cmplx - complex singular value decomposition.
///
/// Port of: static SEXP La_svd_cmplx(SEXP jobu, SEXP x, SEXP s, SEXP u, SEXP v)
pub unsafe fn La_svd_cmplx(jobu: SEXP, x: SEXP, s: SEXP, u: SEXP, v: SEXP) -> SEXP {
    if TYPEOF(jobu) != 16 {
        Rf_error(b"'jobu' must be a character string\0".as_ptr() as *const c_char);
    }

    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let p = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    let mut nprot: c_int = 2;

    let xvals: *mut LapRcomplex;
    let mut x = x;
    if TYPEOF(x) != 15 {
        x = Rf_protect(coerceVector(x, CPLXSXP_C));
        nprot += 1;
        xvals = COMPLEX(x) as *mut LapRcomplex;
    } else {
        let len = (n as usize) * (p as usize);
        xvals = R_alloc(len, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;
        ptr::copy_nonoverlapping(COMPLEX(x) as *const LapRcomplex, xvals, len);
    }

    let u_dims = getAttrib(u, R_DimSymbol());
    let ldu = INTEGER(coerceVector(u_dims, INTSXP_C)).add(0).read() as i32;

    let vt_dims = getAttrib(v, R_DimSymbol());
    let ldvt = INTEGER(coerceVector(vt_dims, INTSXP_C)).add(0).read() as i32;

    let ju = CHAR(STRING_ELT(jobu, 0)) as *const u8;
    let min_np = if n < p { n } else { p };

    // Query optimal work sizes
    let mut tmp = LapRcomplex::default();
    let mut rwork = vec![0.0f64; min_np as usize];
    let iwork = R_alloc(8 * min_np as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;
    let mut lwork: c_int = -1;

    super::backend::zgesdd_(
        ju,
        &n,
        &p,
        xvals,
        &n,
        REAL(s),
        COMPLEX(u) as *mut LapRcomplex,
        &ldu,
        COMPLEX(v) as *mut LapRcomplex,
        &ldvt,
        &mut tmp,
        &lwork,
        rwork.as_mut_ptr(),
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgesdd'\0".as_ptr() as *const c_char);
    }

    lwork = tmp.r as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

    super::backend::zgesdd_(
        ju,
        &n,
        &p,
        xvals,
        &n,
        REAL(s),
        COMPLEX(u) as *mut LapRcomplex,
        &ldu,
        COMPLEX(v) as *mut LapRcomplex,
        &ldvt,
        work,
        &lwork,
        rwork.as_mut_ptr(),
        iwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgesdd'\0".as_ptr() as *const c_char);
    }

    let val = Rf_protect(Rf_allocVector(VECSXP_C, 3));
    let nm = Rf_protect(Rf_allocVector(STRSXP_C, 3));
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"u\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"vt\0".as_ptr() as *const c_char));
    setAttrib(val, R_NamesSymbol(), nm);
    SET_VECTOR_ELT(val, 0, s);
    SET_VECTOR_ELT(val, 1, u);
    SET_VECTOR_ELT(val, 2, v);

    Rf_unprotect(nprot);
    val
}

/// La_rs_cmplx - complex symmetric eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rs_cmplx(SEXP xin, SEXP only_values)
pub unsafe fn La_rs_cmplx(xin: SEXP, only_values: SEXP) -> SEXP {
    let dim = getAttrib(xin, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'x' must be a square numeric matrix\0".as_ptr() as *const c_char);
    }

    let ov = asLogical(only_values);
    if ov == NA_INTEGER {
        Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
    }

    let jobv = if ov != 0 { b'N' } else { b'V' };
    let uplo = b'U';

    // Work on a copy
    let len = (n as usize) * (n as usize);
    let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
    ptr::copy_nonoverlapping(COMPLEX(xin) as *const LapRcomplex, a_copy.as_mut_ptr(), len);

    let values = Rf_protect(Rf_allocVector(REALSXP_C, n as c_int));

    // Query optimal work size
    let mut tmp = LapRcomplex::default();
    let mut rwork = vec![0.0f64; 3 * n as usize];
    let mut lwork: c_int = -1;
    let mut info: c_int = 0;

    super::backend::zheev_(
        &jobv,
        &uplo,
        &n,
        a_copy.as_mut_ptr(),
        &n,
        REAL(values),
        &mut tmp,
        &lwork,
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zheev'\0".as_ptr() as *const c_char);
    }

    lwork = tmp.r as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

    super::backend::zheev_(
        &jobv,
        &uplo,
        &n,
        a_copy.as_mut_ptr(),
        &n,
        REAL(values),
        work,
        &lwork,
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zheev'\0".as_ptr() as *const c_char);
    }

    let ret;
    let nm;
    if ov == 0 {
        let z = Rf_protect(Rf_allocVector(CPLXSXP_C, len as c_int));
        ptr::copy_nonoverlapping(a_copy.as_ptr(), COMPLEX(z) as *mut LapRcomplex, len);

        ret = Rf_protect(Rf_allocVector(VECSXP_C, 2));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 2));
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, values);
        SET_VECTOR_ELT(ret, 1, z);

        Rf_unprotect(5);
    } else {
        ret = Rf_protect(Rf_allocVector(VECSXP_C, 1));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 1));
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, values);

        Rf_unprotect(4);
    }

    setAttrib(ret, R_NamesSymbol(), nm);
    ret
}

/// La_rg_cmplx - complex eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rg_cmplx(SEXP x, SEXP only_values)
pub unsafe fn La_rg_cmplx(x: SEXP, only_values: SEXP) -> SEXP {
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'x' must be a square numeric matrix\0".as_ptr() as *const c_char);
    }

    let ov = asLogical(only_values);
    if ov == NA_INTEGER {
        Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
    }

    let jobvl = b'N';
    let jobvr = if ov != 0 { b'N' } else { b'V' };

    let len = (n as usize) * (n as usize);
    let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
    ptr::copy_nonoverlapping(COMPLEX(x) as *const LapRcomplex, a_copy.as_mut_ptr(), len);

    let values = Rf_protect(Rf_allocVector(CPLXSXP_C, n as c_int));

    let mut vr: *mut LapRcomplex = ptr::null_mut();
    if ov == 0 {
        vr = R_alloc(len, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;
    }

    // Query optimal work size
    let mut tmp = LapRcomplex::default();
    let mut rwork = vec![0.0f64; 2 * n as usize];
    let mut lwork: c_int = -1;
    let mut info: c_int = 0;

    super::backend::zgeev_(
        &jobvl,
        &jobvr,
        &n,
        a_copy.as_mut_ptr(),
        &n,
        COMPLEX(values) as *mut LapRcomplex,
        ptr::null_mut(),
        &1,
        vr,
        &n,
        &mut tmp,
        &lwork,
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgeev'\0".as_ptr() as *const c_char);
    }

    lwork = tmp.r as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

    super::backend::zgeev_(
        &jobvl,
        &jobvr,
        &n,
        a_copy.as_mut_ptr(),
        &n,
        COMPLEX(values) as *mut LapRcomplex,
        ptr::null_mut(),
        &1,
        vr,
        &n,
        work,
        &lwork,
        rwork.as_mut_ptr(),
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zgeev'\0".as_ptr() as *const c_char);
    }

    let ret;
    let nm;
    if ov == 0 {
        let z = Rf_protect(Rf_allocVector(CPLXSXP_C, len as c_int));
        ptr::copy_nonoverlapping(vr, COMPLEX(z) as *mut LapRcomplex, len);

        ret = Rf_protect(Rf_allocVector(VECSXP_C, 2));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 2));
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, values);
        SET_VECTOR_ELT(ret, 1, z);

        Rf_unprotect(5);
    } else {
        ret = Rf_protect(Rf_allocVector(VECSXP_C, 1));
        nm = Rf_protect(Rf_allocVector(STRSXP_C, 1));
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, values);

        Rf_unprotect(4);
    }

    setAttrib(ret, R_NamesSymbol(), nm);
    ret
}

/// qr_coef_real - real QR coefficients.
///
/// Port of: static SEXP qr_coef_real(SEXP q, SEXP bin)
pub unsafe fn qr_coef_real(q: SEXP, bin: SEXP) -> SEXP {
    // Extract QR decomposition components
    let qr = VECTOR_ELT(q, 0);
    let qraux = VECTOR_ELT(q, 2);

    let dim = getAttrib(qr, R_DimSymbol());
    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;

    let b_dim = getAttrib(bin, R_DimSymbol());
    let nrhs = INTEGER(coerceVector(b_dim, INTSXP_C)).add(1).read() as i32;

    let k = if m < n { m } else { n };

    // Work on a copy of the R part
    let len_r = (m as usize) * (n as usize);
    let mut r_copy = vec![0.0f64; len_r];
    ptr::copy_nonoverlapping(REAL(qr), r_copy.as_mut_ptr(), len_r);

    let len_b = (m as usize) * (nrhs as usize);
    let mut b_copy = vec![0.0f64; len_b];
    ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);

    let mut info: c_int = 0;

    // Solve R^T x = b^T using dtrtrs (transpose)
    super::backend::dtrtrs_(
        b"U".as_ptr(),
        b"T".as_ptr(),
        b"N".as_ptr(),
        &k,
        &nrhs,
        r_copy.as_ptr(),
        &m,
        b_copy.as_mut_ptr(),
        &m,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dtrtrs'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_protect(Rf_allocVector(
        REALSXP_C,
        ((k as usize) * (nrhs as usize)) as c_int,
    ));
    // Copy only the first k rows
    for j in 0..nrhs as usize {
        for i in 0..k as usize {
            *REAL(ans).add(i + j * k as usize) = b_copy[i + j * m as usize];
        }
    }

    Rf_unprotect(1);
    ans
}

/// qr_coef_cmplx - complex QR coefficients.
///
/// Port of: static SEXP qr_coef_cmplx(SEXP q, SEXP bin)
pub unsafe fn qr_coef_cmplx(q: SEXP, bin: SEXP) -> SEXP {
    let qr = VECTOR_ELT(q, 0);

    let dim = getAttrib(qr, R_DimSymbol());
    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;

    let b_dim = getAttrib(bin, R_DimSymbol());
    let nrhs = INTEGER(coerceVector(b_dim, INTSXP_C)).add(1).read() as i32;

    let k = if m < n { m } else { n };

    let len_r = (m as usize) * (n as usize);
    let mut r_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_r];
    ptr::copy_nonoverlapping(
        COMPLEX(qr) as *const LapRcomplex,
        r_copy.as_mut_ptr(),
        len_r,
    );

    let len_b = (m as usize) * (nrhs as usize);
    let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
    ptr::copy_nonoverlapping(
        COMPLEX(bin) as *const LapRcomplex,
        b_copy.as_mut_ptr(),
        len_b,
    );

    let mut info: c_int = 0;

    super::backend::ztrtrs_(
        b"U".as_ptr(),
        b"C".as_ptr(),
        b"N".as_ptr(),
        &k,
        &nrhs,
        r_copy.as_ptr(),
        &m,
        b_copy.as_mut_ptr(),
        &m,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'ztrtrs'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_protect(Rf_allocVector(
        CPLXSXP_C,
        ((k as usize) * (nrhs as usize)) as c_int,
    ));
    for j in 0..nrhs as usize {
        for i in 0..k as usize {
            *COMPLEX(ans).add(i + j * k as usize) = {
                // SAFETY: LapRcomplex and Rcomplex have identical #[repr(C)] layouts
                std::mem::transmute::<LapRcomplex, Rcomplex>(b_copy[i + j * m as usize])
            };
        }
    }

    Rf_unprotect(1);
    ans
}

/// qr_qy_real - real QR multiply Q*y.
///
/// Port of: static SEXP qr_qy_real(SEXP q, SEXP bin, SEXP trans)
pub unsafe fn qr_qy_real(q: SEXP, bin: SEXP, trans: SEXP) -> SEXP {
    let qr = VECTOR_ELT(q, 0);
    let qraux = VECTOR_ELT(q, 2);

    let dim = getAttrib(qr, R_DimSymbol());
    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;

    let b_dim = getAttrib(bin, R_DimSymbol());
    let b_rows = INTEGER(coerceVector(b_dim, INTSXP_C)).add(0).read() as i32;
    let nrhs = INTEGER(coerceVector(b_dim, INTSXP_C)).add(1).read() as i32;

    let tr = asLogical(trans);
    if tr == NA_INTEGER {
        Rf_error(b"invalid 'trans' argument\0".as_ptr() as *const c_char);
    }

    let k = if m < n { m } else { n };

    // Work on copies
    let len_r = (m as usize) * (n as usize);
    let mut r_copy = vec![0.0f64; len_r];
    ptr::copy_nonoverlapping(REAL(qr), r_copy.as_mut_ptr(), len_r);

    let len_b = (b_rows as usize) * (nrhs as usize);
    let mut b_copy = vec![0.0f64; len_b];
    ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);

    // Query optimal work size
    let mut tmp: f64 = 0.0;
    let mut lwork: c_int = -1;
    let mut info: c_int = 0;
    let side = b'L';
    let ctrans = if tr != 0 { b'T' } else { b'N' };

    super::backend::dormqr_(
        &side,
        &ctrans,
        &m,
        &nrhs,
        &k,
        r_copy.as_ptr(),
        &m,
        REAL(qraux),
        b_copy.as_mut_ptr(),
        &m,
        &mut tmp,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dormqr'\0".as_ptr() as *const c_char);
    }

    lwork = tmp as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;

    super::backend::dormqr_(
        &side,
        &ctrans,
        &m,
        &nrhs,
        &k,
        r_copy.as_ptr(),
        &m,
        REAL(qraux),
        b_copy.as_mut_ptr(),
        &m,
        work,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'dormqr'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_protect(Rf_allocVector(REALSXP_C, len_b as c_int));
    ptr::copy_nonoverlapping(b_copy.as_ptr(), REAL(ans), len_b);
    Rf_unprotect(1);
    ans
}

/// qr_qy_cmplx - complex QR multiply Q*y.
///
/// Port of: static SEXP qr_qy_cmplx(SEXP q, SEXP bin, SEXP trans)
pub unsafe fn qr_qy_cmplx(q: SEXP, bin: SEXP, trans: SEXP) -> SEXP {
    let qr = VECTOR_ELT(q, 0);
    let qraux = VECTOR_ELT(q, 2);

    let dim = getAttrib(qr, R_DimSymbol());
    let m = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;

    let b_dim = getAttrib(bin, R_DimSymbol());
    let b_rows = INTEGER(coerceVector(b_dim, INTSXP_C)).add(0).read() as i32;
    let nrhs = INTEGER(coerceVector(b_dim, INTSXP_C)).add(1).read() as i32;

    let tr = asLogical(trans);
    if tr == NA_INTEGER {
        Rf_error(b"invalid 'trans' argument\0".as_ptr() as *const c_char);
    }

    let k = if m < n { m } else { n };

    let len_r = (m as usize) * (n as usize);
    let mut r_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_r];
    ptr::copy_nonoverlapping(
        COMPLEX(qr) as *const LapRcomplex,
        r_copy.as_mut_ptr(),
        len_r,
    );

    let len_b = (b_rows as usize) * (nrhs as usize);
    let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
    ptr::copy_nonoverlapping(
        COMPLEX(bin) as *const LapRcomplex,
        b_copy.as_mut_ptr(),
        len_b,
    );

    // Query optimal work size
    let mut tmp = LapRcomplex::default();
    let mut lwork: c_int = -1;
    let mut info: c_int = 0;
    let side = b'L';
    let ctrans = if tr != 0 { b'C' } else { b'N' };

    super::backend::zunmqr_(
        &side,
        &ctrans,
        &m,
        &nrhs,
        &k,
        r_copy.as_ptr(),
        &m,
        COMPLEX(qraux) as *const LapRcomplex,
        b_copy.as_mut_ptr(),
        &m,
        &mut tmp,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zunmqr'\0".as_ptr() as *const c_char);
    }

    lwork = tmp.r as c_int;
    let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

    super::backend::zunmqr_(
        &side,
        &ctrans,
        &m,
        &nrhs,
        &k,
        r_copy.as_ptr(),
        &m,
        COMPLEX(qraux) as *const LapRcomplex,
        b_copy.as_mut_ptr(),
        &m,
        work,
        &lwork,
        &mut info,
    );

    if info != 0 {
        Rf_error(b"error code from Lapack routine 'zunmqr'\0".as_ptr() as *const c_char);
    }

    let ans = Rf_protect(Rf_allocVector(CPLXSXP_C, len_b as c_int));
    ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
    Rf_unprotect(1);
    ans
}

/// det_ge_real - real matrix determinant.
///
/// Port of: static SEXP det_ge_real(SEXP ain, SEXP logarithm)
pub unsafe fn det_ge_real(ain: SEXP, logarithm: SEXP) -> SEXP {
    let ldet = asLogical(logarithm);
    if ldet == NA_INTEGER {
        Rf_error(b"invalid 'logarithm' argument\0".as_ptr() as *const c_char);
    }

    let dim = getAttrib(ain, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        Rf_error(b"'a' must be a matrix\0".as_ptr() as *const c_char);
    }

    let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
    let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
    if n != n2 {
        Rf_error(b"'a' must be a square matrix\0".as_ptr() as *const c_char);
    }

    let len = (n as usize) * (n as usize);
    let mut a_copy = vec![0.0f64; len];
    ptr::copy_nonoverlapping(REAL(ain), a_copy.as_mut_ptr(), len);

    let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
    let mut info: c_int = 0;

    super::backend::dgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);

    if info != 0 {
        let modulus = Rf_protect(Rf_allocVector(REALSXP_C, 1));
        *REAL(modulus) = 0.0;
        let attr = Rf_protect(Rf_allocVector(INTSXP_C, 1));
        *INTEGER(attr) = 0;
        setAttrib(modulus, R_NilValue(), attr); // sign attribute
        Rf_unprotect(2);
        return modulus;
    }

    // Compute the determinant from the diagonal of U
    let mut det: f64 = 1.0;
    let mut sign: c_int = 1;
    for i in 0..n as usize {
        det *= a_copy[i * (n as usize + 1)]; // diagonal elements
        if *ipiv.add(i) != (i as c_int + 1) {
            sign = -sign;
        }
    }

    let ans = Rf_protect(Rf_allocVector(REALSXP_C, 1));
    if ldet != 0 {
        *REAL(ans) = det.abs().ln() + (if sign < 0 { std::f64::consts::PI } else { 0.0 });
    } else {
        *REAL(ans) = det * (sign as f64);
    }

    let attr = Rf_protect(Rf_allocVector(INTSXP_C, 1));
    *INTEGER(attr) = sign;
    setAttrib(ans, R_NilValue(), attr);

    Rf_unprotect(2);
    ans
}

/// mod_do_lapack - main LAPACK dispatcher.
///
/// Port of: SEXP mod_do_lapack(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn mod_do_lapack(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    // This dispatches based on op symbol to the appropriate La_* function.
    // In R, this is done via the .Internal() mechanism.
    // For now, return nil as dispatch needs the full .Internal infrastructure.
    R_NilValue()
}

/// R_init_lapack - LAPACK module initialization.
///
/// Port of: void R_init_lapack(DllInfo *dll)
pub unsafe fn R_init_lapack(_info: *mut std::ffi::c_void) {
    // Registration would happen here in the full implementation
}
