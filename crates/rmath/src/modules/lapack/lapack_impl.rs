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

use crate::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};

use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_current_instance;
use crate::sexp::memory::with_arena_in;
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
    unsafe {
        if TYPEOF(jobu) != STRSXP_C || XLENGTH(jobu) < 1 {
            crate::sexp::context::r_error("'jobu' must be a character string");
        }
        if TYPEOF(x) != REALSXP_C {
            crate::sexp::context::r_error("'x' must be a numeric matrix");
        }
        if TYPEOF(s) != REALSXP_C {
            crate::sexp::context::r_error("'s' must be a numeric vector");
        }
        if TYPEOF(u) != REALSXP_C {
            crate::sexp::context::r_error("'u' must be a numeric matrix");
        }
        if TYPEOF(vt) != REALSXP_C {
            crate::sexp::context::r_error("'vt' must be a numeric matrix");
        }
        let _jobu_guard = protect(jobu);
        let _x_guard = protect(x);
        let _s_guard = protect(s);
        let _u_guard = protect(u);
        let _vt_guard = protect(vt);

        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let p = INTEGER(dim).add(1).read();
        if n < 0 || p < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }

        let Some(len) = (n as usize).checked_mul(p as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(x) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let min_np = if n < p { n } else { p };
        if XLENGTH(s) as usize != min_np as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let u_dims = getAttrib(u, R_DimSymbol());
        if u_dims.is_null() || TYPEOF(u_dims) != INTSXP_C || XLENGTH(u_dims) != 2 {
            crate::sexp::context::r_error("'u' must be a matrix");
        }
        let ldu = INTEGER(u_dims).add(0).read();
        let u_cols = INTEGER(u_dims).add(1).read();
        if ldu < 0 || u_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_u) = (ldu as usize).checked_mul(u_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_u > c_int::MAX as usize || XLENGTH(u) as usize != len_u {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let vt_dims = getAttrib(vt, R_DimSymbol());
        if vt_dims.is_null() || TYPEOF(vt_dims) != INTSXP_C || XLENGTH(vt_dims) != 2 {
            crate::sexp::context::r_error("'vt' must be a matrix");
        }
        let ldvt = INTEGER(vt_dims).add(0).read();
        let vt_cols = INTEGER(vt_dims).add(1).read();
        if ldvt < 0 || vt_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_vt) = (ldvt as usize).checked_mul(vt_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_vt > c_int::MAX as usize || XLENGTH(vt) as usize != len_vt {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(iwork_len) = (min_np as usize).checked_mul(8) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| {
                bytes.checked_add(iwork_len.checked_mul(std::mem::size_of::<c_int>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native SVD workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut x_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(x), x_copy.as_mut_ptr(), len);
        }
        let mut iwork = vec![0 as c_int; iwork_len];

        let ju = CHAR(STRING_ELT(jobu, 0)) as *const u8;
        let mut tmp: f64 = 0.0;
        let mut info: c_int = 0;
        let mut lwork: c_int = -1;

        super::backend::dgesdd_(
            ju,
            &n,
            &p,
            x_copy.as_mut_ptr(),
            &n,
            REAL(s),
            REAL(u),
            &ldu,
            REAL(vt),
            &ldvt,
            &mut tmp,
            &lwork,
            iwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgesdd'");
        }

        if n > 0 && p > 0 {
            if !tmp.is_finite() || tmp < 1.0 || tmp > c_int::MAX as f64 {
                crate::sexp::context::r_error(
                    "invalid workspace size from Lapack routine 'dgesdd'",
                );
            }
            lwork = tmp as c_int;
            let work_bytes = (lwork as usize)
                .checked_mul(std::mem::size_of::<f64>())
                .unwrap_or_else(|| crate::sexp::context::r_error("invalid SVD workspace size"));
            let work_reservation = with_current_instance(|instance| {
                with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
            });
            if matches!(work_reservation, Some(None)) {
                crate::sexp::context::r_error(
                    "allocation failed: native SVD workspace exceeds resource limit",
                );
            }
            let _work_reservation = work_reservation.flatten();
            let mut work = vec![0.0f64; lwork as usize];

            super::backend::dgesdd_(
                ju,
                &n,
                &p,
                x_copy.as_mut_ptr(),
                &n,
                REAL(s),
                REAL(u),
                &ldu,
                REAL(vt),
                &ldvt,
                work.as_mut_ptr(),
                &lwork,
                iwork.as_mut_ptr(),
                &mut info,
            );

            if info != 0 {
                crate::sexp::context::r_error("error code from Lapack routine 'dgesdd'");
            }
        }

        // Build result list: list(d=s, u=u, vt=vt)
        let val = Rf_allocVector(VECSXP_C, 3);
        let _val_guard = protect(val);
        let nm = Rf_allocVector(STRSXP_C, 3);
        let _names_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"u\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"vt\0".as_ptr() as *const c_char));
        setAttrib(val, R_NamesSymbol(), nm);
        SET_VECTOR_ELT(val, 0, s);
        SET_VECTOR_ELT(val, 1, u);
        SET_VECTOR_ELT(val, 2, vt);

        val
    }
}

/// La_rs - real symmetric eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rs(SEXP x, SEXP only_values)
pub unsafe fn La_rs(x: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
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
        let mut guards = Vec::new();
        if TYPEOF(x) != 14 {
            x = coerceVector(x, REALSXP_C);
            guards.push(protect(x));
            rx = REAL(x);
        } else {
            rx = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
            ptr::copy_nonoverlapping(REAL(x), rx, (n as usize) * (n as usize));
        }
        guards.push(protect(x));

        let values = Rf_allocVector(REALSXP_C, n as c_int);
        guards.push(protect(values));
        let rvalues = REAL(values);

        let mut z = R_NilValue();
        let mut rz: *mut f64 = ptr::null_mut();
        if ov == 0 {
            z = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
            guards.push(protect(z));
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
            &jobv, &range, &uplo, &n, rx, &n, &0.0f64, &0.0f64, &0, &0, &0.0f64, &mut m, rvalues,
            rz, &n, isuppz, &mut tmp, &lwork, &mut itmp, &liwork, &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dsyevr'\0".as_ptr() as *const c_char);
        }

        lwork = tmp as c_int;
        liwork = itmp;

        let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;
        let iwork = R_alloc(liwork as usize, std::mem::size_of::<c_int>()) as *mut c_int;

        super::backend::dsyevr_(
            &jobv, &range, &uplo, &n, rx, &n, &0.0f64, &0.0f64, &0, &0, &0.0f64, &mut m, rvalues,
            rz, &n, isuppz, work, &lwork, iwork, &liwork, &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dsyevr'\0".as_ptr() as *const c_char);
        }

        let ret;
        let nm;
        if ov == 0 {
            ret = Rf_allocVector(VECSXP_C, 2);
            guards.push(protect(ret));
            nm = Rf_allocVector(STRSXP_C, 2);
            guards.push(protect(nm));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 1, z);
        } else {
            ret = Rf_allocVector(VECSXP_C, 1);
            guards.push(protect(ret));
            nm = Rf_allocVector(STRSXP_C, 1);
            guards.push(protect(nm));
        }
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        setAttrib(ret, R_NamesSymbol(), nm);
        SET_VECTOR_ELT(ret, 0, values);

        ret
    }
}

/// La_rg - real eigenvalues/eigenvectors (general, non-symmetric).
///
/// Port of: static SEXP La_rg(SEXP x, SEXP only_values)
pub unsafe fn La_rg(x: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
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
            x = coerceVector(x, REALSXP_C);
            xvals = REAL(x);
        } else {
            xvals = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
            ptr::copy_nonoverlapping(REAL(x), xvals, (n as usize) * (n as usize));
        }
        let _x_guard = protect(x);

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
                let values = Rf_allocVector(CPLXSXP_C, n as c_int);
                let _values_guard = protect(values);
                for i in 0..n as usize {
                    let c = COMPLEX(values).add(i);
                    (*c).r = *wR.add(i);
                    (*c).i = *wI.add(i);
                }

                // Build complex eigenvector matrix
                let z = Rf_allocVector(CPLXSXP_C, (n as c_int) * (n as c_int));
                let _z_guard = protect(z);
                for i in 0..cmplx_vecs.len() {
                    let c = COMPLEX(z).add(i);
                    (*c).r = cmplx_vecs[i].r;
                    (*c).i = cmplx_vecs[i].i;
                }

                ret = Rf_allocVector(VECSXP_C, 2);
                let _ret_guard = protect(ret);
                nm = Rf_allocVector(STRSXP_C, 2);
                let _nm_guard = protect(nm);
                SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
                SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
                SET_VECTOR_ELT(ret, 0, values);
                SET_VECTOR_ELT(ret, 1, z);
                setAttrib(ret, R_NamesSymbol(), nm);

                return ret;
            } else {
                // All real eigenvalues
                let values = Rf_allocVector(REALSXP_C, n as c_int);
                let _values_guard = protect(values);
                ptr::copy_nonoverlapping(wR, REAL(values), n as usize);

                let z = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
                let _z_guard = protect(z);
                ptr::copy_nonoverlapping(right, REAL(z), (n as usize) * (n as usize));

                ret = Rf_allocVector(VECSXP_C, 2);
                let _ret_guard = protect(ret);
                nm = Rf_allocVector(STRSXP_C, 2);
                let _nm_guard = protect(nm);
                SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
                SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
                SET_VECTOR_ELT(ret, 0, values);
                SET_VECTOR_ELT(ret, 1, z);
                setAttrib(ret, R_NamesSymbol(), nm);

                return ret;
            }
        } else {
            // Only values
            let values = Rf_allocVector(REALSXP_C, n as c_int);
            let _values_guard = protect(values);
            ptr::copy_nonoverlapping(wR, REAL(values), n as usize);

            ret = Rf_allocVector(VECSXP_C, 1);
            let _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 1);
            let _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            setAttrib(ret, R_NamesSymbol(), nm);

            ret
        }
    }
}

/// La_dlange - real matrix norm.
///
/// Port of: static SEXP La_dlange(SEXP a, SEXP type_)
pub unsafe fn La_dlange(a: SEXP, type_: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(type_) != 16 {
            Rf_error(b"'type' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _type_guard = protect(type_);

        let typ_str = CStr::from_ptr(CHAR(STRING_ELT(type_, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_norm_type(typ_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if norm_c == b'I' || norm_c == b'O' {
            m as usize
        } else {
            0
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native matrix-norm workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();
        let mut work = vec![0.0f64; work_len];

        let anorm = super::backend::dlange_(&norm_c, &m, &n, REAL(a), &m, work.as_mut_ptr());

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = anorm;
        ans
    }
}

/// La_dgecon - real matrix condition number estimate.
///
/// Port of: static SEXP La_dgecon(SEXP a, SEXP norm)
pub unsafe fn La_dgecon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_norm_len = if norm_c == b'I' { n as usize } else { 0 };
        let Some(work_len) = (n as usize).checked_mul(4) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(work_norm_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        // Compute the norm of A
        let mut work_norm = vec![0.0f64; work_norm_len];
        let anorm = super::backend::dlange_(&norm_c, &n, &n, REAL(a), &n, work_norm.as_mut_ptr());

        if anorm == 0.0 {
            let ans = Rf_allocVector(REALSXP_C, 1);
            *REAL(ans) = 0.0;
            return ans;
        }

        // Work on a copy
        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len);
        }

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
        let mut work = vec![0.0f64; work_len];
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
}

/// La_dtrcon - real triangular condition number.
///
/// Port of: static SEXP La_dtrcon(SEXP a, SEXP norm)
pub unsafe fn La_dtrcon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        let Some(work_len) = (n as usize).checked_mul(3) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![0.0f64; work_len];
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
}

/// La_dtrcon3 - real triangular condition number with explicit uplo.
///
/// Port of: static SEXP La_dtrcon3(SEXP a, SEXP norm, SEXP uplo)
pub unsafe fn La_dtrcon3(a: SEXP, norm: SEXP, uplo: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(uplo) != 16 {
            Rf_error(b"'uplo' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);
        let _uplo_guard = protect(uplo);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);
        let uplo_str = CStr::from_ptr(CHAR(STRING_ELT(uplo, 0)))
            .to_str()
            .unwrap_or("U");
        let uplo_c = La_valid_uplo(uplo_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        let Some(work_len) = (n as usize).checked_mul(3) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![0.0f64; work_len];
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
}

/// La_zlange - complex matrix norm.
///
/// Port of: static SEXP La_zlange(SEXP a, SEXP type_)
pub unsafe fn La_zlange(a: SEXP, type_: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(type_) != 16 {
            Rf_error(b"'type' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _type_guard = protect(type_);

        let typ_str = CStr::from_ptr(CHAR(STRING_ELT(type_, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_norm_type(typ_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if norm_c == b'I' || norm_c == b'O' {
            m as usize
        } else {
            0
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex matrix-norm workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();
        let mut work = vec![0.0f64; work_len];

        let a_ptr = COMPLEX(a) as *const LapRcomplex;
        let anorm = super::backend::zlange_(&norm_c, &m, &n, a_ptr, &m, work.as_mut_ptr());

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = anorm;
        ans
    }
}

/// La_zgecon - complex matrix condition number estimate.
///
/// Port of: static SEXP La_zgecon(SEXP a, SEXP norm)
pub unsafe fn La_zgecon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_norm_len = if norm_c == b'I' { n as usize } else { 0 };
        let Some(work_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(work_norm_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<LapRcomplex>())?))
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut work_norm = vec![0.0f64; work_norm_len];
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
            *REAL(ans) = 0.0;
            return ans;
        }

        // Work on a copy
        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(a) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

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
        let mut work = vec![LapRcomplex::default(); work_len];
        let mut rwork = vec![0.0f64; work_len];

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
}

/// La_ztrcon - complex triangular condition number.
///
/// Port of: static SEXP La_ztrcon(SEXP a, SEXP norm)
pub unsafe fn La_ztrcon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        let Some(work_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let rwork_len = n as usize;
        let Some(scratch_bytes) = work_len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![LapRcomplex::default(); work_len];
        let mut rwork = vec![0.0f64; rwork_len];
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
}

/// La_ztrcon3 - complex triangular condition number with explicit uplo.
///
/// Port of: static SEXP La_ztrcon3(SEXP a, SEXP norm, SEXP uplo)
pub unsafe fn La_ztrcon3(a: SEXP, norm: SEXP, uplo: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(uplo) != 16 {
            Rf_error(b"'uplo' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);
        let _uplo_guard = protect(uplo);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);
        let uplo_str = CStr::from_ptr(CHAR(STRING_ELT(uplo, 0)))
            .to_str()
            .unwrap_or("U");
        let uplo_c = La_valid_uplo(uplo_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        let Some(work_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let rwork_len = n as usize;
        let Some(scratch_bytes) = work_len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![LapRcomplex::default(); work_len];
        let mut rwork = vec![0.0f64; rwork_len];
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
}

/// La_chol - real Cholesky decomposition.
///
/// Port of: static SEXP La_chol(SEXP a, SEXP pivot, SEXP stol)
pub unsafe fn La_chol(a: SEXP, pivot: SEXP, stol: SEXP) -> SEXP {
    unsafe {
        let piv = asLogical(pivot);
        if piv == NA_INTEGER {
            Rf_error(b"invalid 'pivot' argument\0".as_ptr() as *const c_char);
        }

        let tol = asReal(stol);

        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if piv != 0 {
            (n as usize)
                .checked_mul(2)
                .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"))
        } else {
            0
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native Cholesky workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len);
        }

        let mut info: c_int = 0;
        let uplo = b'U';

        if piv != 0 {
            // pivoted Cholesky: dpstrf
            let piv_arr = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
            for i in 0..n as usize {
                *piv_arr.add(i) = i as c_int + 1;
            }
            let mut rank: c_int = 0;
            let mut work = vec![0.0f64; work_len];
            let uplo_s = c"U";

            super::backend::dpstrf_(
                uplo_s.as_ptr() as *const u8,
                &n,
                a_copy.as_mut_ptr(),
                &n,
                piv_arr,
                &mut rank,
                &tol,
                work.as_mut_ptr(),
                &mut info,
            );

            if info != 0 {
                let msg = std::ffi::CString::new(
                    "the matrix is either rank-deficient or not positive definite",
                )
                .unwrap();
                crate::mainutils::errors::warningcall(R_NilValue(), msg.as_ptr());
                if rank <= 0 {
                    rank = 0;
                }
            }

            let nn = n as usize;
            for j in rank as usize..nn {
                for i in rank as usize..=j {
                    a_copy[i + nn * j] = 0.0;
                }
            }
            for j in 0..nn {
                for i in (j + 1)..nn {
                    a_copy[i + j * nn] = 0.0;
                }
            }
            let ans = Rf_allocVector(REALSXP_C, len as c_int);
            let _ans = protect(ans);
            if len != 0 {
                ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), len);
            }
            let dims = Rf_allocVector(INTSXP_C, 2);
            let _d = protect(dims);
            *INTEGER(dims) = n;
            *INTEGER(dims).add(1) = n;
            setAttrib(ans, R_DimSymbol(), dims);
            let pivot_s = Rf_allocVector(INTSXP_C, n as c_int);
            let _ps = protect(pivot_s);
            for i in 0..nn {
                *INTEGER(pivot_s).add(i) = *piv_arr.add(i);
            }
            setAttrib(ans, crate::sexp::symbol::Rf_install(c"pivot".as_ptr()), pivot_s);
            setAttrib(
                ans,
                crate::sexp::symbol::Rf_install(c"rank".as_ptr()),
                Rf_ScalarInteger(rank),
            );
            ans
        } else {
            super::backend::dpotrf_(&uplo, &n, a_copy.as_mut_ptr(), &n, &mut info);
            if info > 0 {
                crate::sexp::context::r_error(&format!(
                    "the leading minor of order {info} is not positive"
                ));
            }
            if info != 0 {
                crate::sexp::context::r_error("error code from Lapack routine 'dpotrf'");
            }
            for j in 0..n as usize {
                for i in (j + 1)..n as usize {
                    a_copy[i + j * n as usize] = 0.0;
                }
            }
            let ans = Rf_allocVector(REALSXP_C, len as c_int);
            if len != 0 {
                ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), len);
            }
            ans
        }

    }
}

/// La_chol2inv - real inverse from Cholesky factor.
///
/// Port of: static SEXP La_chol2inv(SEXP a, SEXP size)
pub unsafe fn La_chol2inv(a: SEXP, size: SEXP) -> SEXP {
    unsafe {
        let n = asInteger(size);
        if n == NA_INTEGER || n <= 0 {
            Rf_error(b"'size' must be a positive integer\0".as_ptr() as *const c_char);
        }

        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let p = INTEGER(dim).add(1).read();
        if m < 0 || p < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        // GNU: size may be rank < ncol; use the leading size×size of R.
        if n > p {
            crate::sexp::context::r_error(format!("'size' cannot exceed ncol(x) = {p}"));
        }
        if n > m {
            crate::sexp::context::r_error(format!("'size' cannot exceed nrow(x) = {m}"));
        }

        let Some(src_len) = (m as usize).checked_mul(p as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if src_len > c_int::MAX as usize || XLENGTH(a) as usize != src_len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native Cholesky inverse workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        let lda = m as usize;
        let sz = n as usize;
        for j in 0..sz {
            for i in 0..=j {
                a_copy[i + j * sz] = *REAL(a).add(i + j * lda);
            }
        }

        let mut info: c_int = 0;
        let uplo = b'U';

        super::backend::dpotri_(&uplo, &n, a_copy.as_mut_ptr(), &n, &mut info);

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dpotri'\0".as_ptr() as *const c_char);
        }

        // Copy upper triangle to lower (dpotri U writes i<=j only).
        for j in 1..sz {
            for i in 0..j {
                a_copy[j + i * sz] = a_copy[i + j * sz];
            }
        }

        let ans = crate::mainutils::array::allocMatrix(REALSXP_C, n, n);
        if len != 0 {
            ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), len);
        }
        ans
    }
}

/// La_solve - real linear solve.
///
/// Port of: static SEXP La_solve(SEXP a, SEXP bin, SEXP tolin)
pub unsafe fn La_solve(a: SEXP, bin: SEXP, tolin: SEXP) -> SEXP {
    unsafe {
        let tol = asReal(tolin);

        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        if TYPEOF(bin) != REALSXP_C {
            crate::sexp::context::r_error("'b' must be a numeric matrix");
        }
        let _a_guard = protect(a);
        let _b_guard = protect(bin);

        let a_dim = getAttrib(a, R_DimSymbol());
        if a_dim.is_null() || TYPEOF(a_dim) != INTSXP_C || XLENGTH(a_dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(a_dim).add(0).read();
        let n2 = INTEGER(a_dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'b' must be a matrix");
        }

        let m_b = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if m_b < 0 || nrhs < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if m_b != n {
            crate::sexp::context::r_error("'b' must have same row dimension as 'a'");
        }

        let Some(len_a) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(len_b) = (n as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_a > c_int::MAX as usize || XLENGTH(a) as usize != len_a {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if tol > 0.0 {
            (n as usize)
                .checked_mul(4)
                .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"))
        } else {
            0
        };
        let iwork_len = if tol > 0.0 { n as usize } else { 0 };
        let Some(scratch_bytes) = len_a
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| {
                bytes.checked_add(iwork_len.checked_mul(std::mem::size_of::<c_int>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native solve workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len_a];
        if len_a != 0 {
            ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len_a);
        }

        let mut b_copy = vec![0.0f64; len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);
        }

        let anorm = {
            let n0 = (n as usize).max(1);
            let mut max_col = 0.0;
            let mut saw_nan = false;
            for col in a_copy.chunks(n0) {
                let mut sum = 0.0;
                for v in col {
                    if v.is_nan() {
                        saw_nan = true;
                    }
                    sum += v.abs();
                }
                if sum > max_col {
                    max_col = sum;
                }
            }
            if saw_nan {
                f64::NAN
            } else {
                max_col
            }
        };
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

        if tol > 0.0 {
            let mut rcond = 0.0;
            let mut work = vec![0.0; work_len];
            let mut iwork = vec![0; iwork_len];
            super::backend::dgecon_(
                b"1".as_ptr(),
                &n,
                a_copy.as_ptr(),
                &n,
                &anorm,
                &mut rcond,
                work.as_mut_ptr(),
                iwork.as_mut_ptr(),
                &mut info,
            );
            // GNU: NaN rcond is not < tol (IEEE). All-NaN A must return NaN, not error.
            if rcond.is_finite() && rcond < tol {
                crate::sexp::context::r_error("system is computationally singular");
            }
        }
        let ans = Rf_allocVector(REALSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        if len_b != 0 {
            ptr::copy_nonoverlapping(b_copy.as_ptr(), REAL(ans), len_b);
        }
        ans

    }
}

/// La_solve_cmplx - complex linear solve.
///
/// Port of: static SEXP La_solve_cmplx(SEXP a, SEXP bin, SEXP tolin)
pub unsafe fn La_solve_cmplx(a: SEXP, bin: SEXP, tolin: SEXP) -> SEXP {
    unsafe {
        let tol = asReal(tolin);

        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        if TYPEOF(bin) != CPLXSXP_C {
            crate::sexp::context::r_error("'b' must be a complex matrix");
        }
        let _a_guard = protect(a);
        let _b_guard = protect(bin);

        let a_dim = getAttrib(a, R_DimSymbol());
        if a_dim.is_null() || TYPEOF(a_dim) != INTSXP_C || XLENGTH(a_dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(a_dim).add(0).read();
        let n2 = INTEGER(a_dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'b' must be a matrix");
        }

        let m_b = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if m_b < 0 || nrhs < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if m_b != n {
            crate::sexp::context::r_error("'b' must have same row dimension as 'a'");
        }

        let Some(len_a) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(len_b) = (n as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_a > c_int::MAX as usize || XLENGTH(a) as usize != len_a {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if tol > 0.0 {
            (n as usize)
                .checked_mul(2)
                .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"))
        } else {
            0
        };
        let rwork_len = work_len;
        let Some(scratch_bytes) = len_a
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| {
                bytes.checked_add(len_b.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
            .and_then(|bytes| {
                bytes.checked_add(work_len.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native solve workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_a];
        if len_a != 0 {
            ptr::copy_nonoverlapping(COMPLEX(a) as *const LapRcomplex, a_copy.as_mut_ptr(), len_a);
        }

        let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(bin) as *const LapRcomplex,
                b_copy.as_mut_ptr(),
                len_b,
            );
        }

        let anorm = {
            let n0 = (n as usize).max(1);
            let mut max_col = 0.0;
            let mut saw_nan = false;
            for col in a_copy.chunks(n0) {
                let mut sum = 0.0;
                for v in col {
                    if v.r.is_nan() || v.i.is_nan() {
                        saw_nan = true;
                    }
                    sum += v.r.hypot(v.i);
                }
                if sum > max_col {
                    max_col = sum;
                }
            }
            if saw_nan {
                f64::NAN
            } else {
                max_col
            }
        };
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

        if tol > 0.0 {
            let mut rcond = 0.0;
            let mut work = vec![LapRcomplex::default(); work_len];
            let mut rwork = vec![0.0; rwork_len];
            super::backend::zgecon_(
                b"1".as_ptr(),
                &n,
                a_copy.as_ptr(),
                &n,
                &anorm,
                &mut rcond,
                work.as_mut_ptr(),
                rwork.as_mut_ptr(),
                &mut info,
            );
            if rcond.is_finite() && rcond < tol {
                crate::sexp::context::r_error("system is computationally singular");
            }
        }
        let ans = Rf_allocVector(CPLXSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        if len_b != 0 {
            ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
        }
        ans
    }
}

/// La_qr - real QR decomposition.
///
/// Port of: static SEXP La_qr(SEXP ain)
pub unsafe fn La_qr(ain: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(ain) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(ain);
        let dim = getAttrib(ain, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let min_mn = if m < n { m } else { n };

        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(ain) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        let lda = m.max(1);
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| {
                bytes.checked_add((n as usize).checked_mul(std::mem::size_of::<c_int>())?)
            })
            .and_then(|bytes| {
                bytes.checked_add((min_mn as usize).checked_mul(std::mem::size_of::<f64>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(ain), a_copy.as_mut_ptr(), len);
        }
        let mut jpvt = vec![0 as c_int; n as usize];
        let mut tau = vec![0.0f64; min_mn as usize];

        let mut tmp: f64 = 0.0;
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;
        super::backend::dgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            &mut tmp,
            &lwork,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgeqp3'");
        }
        if !tmp.is_finite() || tmp < 1.0 || tmp > c_int::MAX as f64 {
            crate::sexp::context::r_error("invalid workspace size from Lapack routine 'dgeqp3'");
        }
        lwork = tmp as c_int;
        let work_bytes = (lwork as usize)
            .checked_mul(std::mem::size_of::<f64>())
            .unwrap_or_else(|| crate::sexp::context::r_error("invalid QR workspace size"));
        let work_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
        });
        if matches!(work_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _work_reservation = work_reservation.flatten();
        let mut work = vec![0.0f64; lwork as usize];

        super::backend::dgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            work.as_mut_ptr(),
            &lwork,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgeqp3'");
        }

        let qr = Rf_allocVector(REALSXP_C, len as c_int);
        let _qr_guard = protect(qr);
        if len != 0 {
            ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(qr), len);
        }

        let qraux = Rf_allocVector(REALSXP_C, min_mn as c_int);
        let _qraux_guard = protect(qraux);
        for i in 0..min_mn as usize {
            *REAL(qraux).add(i) = tau[i];
        }

        let pivot = Rf_allocVector(INTSXP_C, n as c_int);
        let _pivot_guard = protect(pivot);
        for i in 0..n as usize {
            *INTEGER(pivot).add(i) = jpvt[i];
        }

        let ret = Rf_allocVector(VECSXP_C, 4);
        let _ret_guard = protect(ret);
        let nm = Rf_allocVector(STRSXP_C, 4);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"qr\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 3, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, qr);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(min_mn));
        SET_VECTOR_ELT(ret, 2, qraux);
        SET_VECTOR_ELT(ret, 3, pivot);
        setAttrib(ret, R_NamesSymbol(), nm);

        ret
    }
}

/// La_qr_cmplx - complex QR decomposition.
///
/// Port of: static SEXP La_qr_cmplx(SEXP ain)
pub unsafe fn La_qr_cmplx(ain: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(ain) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(ain);
        let dim = getAttrib(ain, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let min_mn = if m < n { m } else { n };

        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(ain) as usize != len {
            crate::sexp::context::r_error("invalid complex matrix dimensions or length");
        }
        let Some(rwork_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let lda = m.max(1);
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| {
                bytes.checked_add((n as usize).checked_mul(std::mem::size_of::<c_int>())?)
            })
            .and_then(|bytes| {
                bytes
                    .checked_add((min_mn as usize).checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(ain) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let mut jpvt = vec![0 as c_int; n as usize];
        let mut tau = vec![LapRcomplex::default(); min_mn as usize];

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut lwork: c_int = -1;
        let mut rwork = vec![0.0f64; rwork_len];
        let mut info: c_int = 0;

        super::backend::zgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            &mut tmp,
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'zgeqp3'"));
        }

        if !tmp.r.is_finite() || tmp.r < 1.0 || tmp.r > c_int::MAX as f64 {
            crate::sexp::context::r_error("invalid workspace size from Lapack routine 'zgeqp3'");
        }
        lwork = tmp.r as c_int;
        let work_bytes = (lwork as usize)
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .unwrap_or_else(|| crate::sexp::context::r_error("invalid QR workspace size"));
        let work_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
        });
        if matches!(work_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _work_reservation = work_reservation.flatten();
        let mut work = vec![LapRcomplex::default(); lwork as usize];

        super::backend::zgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            work.as_mut_ptr(),
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'zgeqp3'"));
        }

        let qr = Rf_allocVector(CPLXSXP_C, len as c_int);
        let _qr_guard = protect(qr);
        if len != 0 {
            ptr::copy_nonoverlapping(a_copy.as_ptr(), COMPLEX(qr) as *mut LapRcomplex, len);
        }

        let qr_dim = Rf_allocVector(INTSXP_C, 2);
        let _qr_dim_guard = protect(qr_dim);
        *INTEGER(qr_dim) = m;
        *INTEGER(qr_dim).add(1) = n;
        setAttrib(qr, R_DimSymbol(), qr_dim);

        let qraux = Rf_allocVector(CPLXSXP_C, min_mn as c_int);
        let _qraux_guard = protect(qraux);
        for i in 0..min_mn as usize {
            *COMPLEX(qraux).add(i) = {
                // SAFETY: LapRcomplex and Rcomplex have identical layouts: #[repr(C)] struct { r: f64, i: f64 }
                std::mem::transmute::<LapRcomplex, Rcomplex>(tau[i])
            };
        }

        let pivot = Rf_allocVector(INTSXP_C, n as c_int);
        let _pivot_guard = protect(pivot);
        for i in 0..n as usize {
            *INTEGER(pivot).add(i) = jpvt[i];
        }

        // GNU pivots the input's column dimnames onto the factored matrix.
        let adn = getAttrib(ain, R_DimNamesSymbol());
        if adn != R_NilValue() && TYPEOF(adn) == VECSXP_C && XLENGTH(adn) == 2 {
            let adn2 = crate::mainutils::duplicate::Rf_duplicate(adn);
            let _adn2_guard = protect(adn2);
            let cn = VECTOR_ELT(adn, 1);
            let cn2 = VECTOR_ELT(adn2, 1);
            if cn != R_NilValue()
                && TYPEOF(cn) == STRSXP_C
                && TYPEOF(cn2) == STRSXP_C
                && (XLENGTH(cn) as usize) == n as usize
                && (XLENGTH(cn2) as usize) == n as usize
            {
                for j in 0..n as usize {
                    let source = *INTEGER(pivot).add(j);
                    if source >= 1 && (source as usize) <= n as usize {
                        SET_STRING_ELT(cn2, j as i64, STRING_ELT(cn, (source - 1) as i64));
                    }
                }
            }
            setAttrib(qr, R_DimNamesSymbol(), adn2);
        }
        let ret = Rf_allocVector(VECSXP_C, 4);
        let _ret_guard = protect(ret);
        let nm = Rf_allocVector(STRSXP_C, 4);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"qr\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 3, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, qr);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(min_mn));
        SET_VECTOR_ELT(ret, 2, qraux);
        SET_VECTOR_ELT(ret, 3, pivot);
        setAttrib(ret, R_NamesSymbol(), nm);

        ret
    }
}

/// La_svd_cmplx - complex singular value decomposition.
///
/// Port of: static SEXP La_svd_cmplx(SEXP jobu, SEXP x, SEXP s, SEXP u, SEXP v)
pub unsafe fn La_svd_cmplx(jobu: SEXP, x: SEXP, s: SEXP, u: SEXP, v: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(jobu) != STRSXP_C || XLENGTH(jobu) < 1 {
            crate::sexp::context::r_error("'jobu' must be a character string");
        }
        if TYPEOF(x) != CPLXSXP_C {
            crate::sexp::context::r_error("'x' must be a complex matrix");
        }
        if TYPEOF(s) != REALSXP_C {
            crate::sexp::context::r_error("'s' must be a numeric vector");
        }
        if TYPEOF(u) != CPLXSXP_C {
            crate::sexp::context::r_error("'u' must be a complex matrix");
        }
        if TYPEOF(v) != CPLXSXP_C {
            crate::sexp::context::r_error("'v' must be a complex matrix");
        }
        let _jobu_guard = protect(jobu);
        let _x_guard = protect(x);
        let _s_guard = protect(s);
        let _u_guard = protect(u);
        let _v_guard = protect(v);

        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let p = INTEGER(dim).add(1).read();
        if n < 0 || p < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }

        let Some(len) = (n as usize).checked_mul(p as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(x) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let min_np = if n < p { n } else { p };
        if XLENGTH(s) as usize != min_np as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let u_dims = getAttrib(u, R_DimSymbol());
        if u_dims.is_null() || TYPEOF(u_dims) != INTSXP_C || XLENGTH(u_dims) != 2 {
            crate::sexp::context::r_error("'u' must be a matrix");
        }
        let ldu = INTEGER(u_dims).add(0).read();
        let u_cols = INTEGER(u_dims).add(1).read();
        if ldu < 0 || u_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_u) = (ldu as usize).checked_mul(u_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_u > c_int::MAX as usize || XLENGTH(u) as usize != len_u {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let vt_dims = getAttrib(v, R_DimSymbol());
        if vt_dims.is_null() || TYPEOF(vt_dims) != INTSXP_C || XLENGTH(vt_dims) != 2 {
            crate::sexp::context::r_error("'v' must be a matrix");
        }
        let ldvt = INTEGER(vt_dims).add(0).read();
        let vt_cols = INTEGER(vt_dims).add(1).read();
        if ldvt < 0 || vt_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_vt) = (ldvt as usize).checked_mul(vt_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_vt > c_int::MAX as usize || XLENGTH(v) as usize != len_vt {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let ju = CHAR(STRING_ELT(jobu, 0)) as *const u8;

        let Some(iwork_len) = (min_np as usize).checked_mul(8) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let rwork_len = min_np as usize;
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| {
                bytes.checked_add(iwork_len.checked_mul(std::mem::size_of::<c_int>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native SVD workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut x_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(x) as *const LapRcomplex, x_copy.as_mut_ptr(), len);
        }
        let mut rwork = vec![0.0f64; rwork_len];
        let mut iwork = vec![0 as c_int; iwork_len];

        let mut tmp = LapRcomplex::default();
        let mut info: c_int = 0;
        let mut lwork: c_int = -1;

        super::backend::zgesdd_(
            ju,
            &n,
            &p,
            x_copy.as_mut_ptr(),
            &n,
            REAL(s),
            COMPLEX(u) as *mut LapRcomplex,
            &ldu,
            COMPLEX(v) as *mut LapRcomplex,
            &ldvt,
            &mut tmp,
            &lwork,
            rwork.as_mut_ptr(),
            iwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'zgesdd'");
        }

        if n > 0 && p > 0 {
            if !tmp.r.is_finite() || tmp.r < 1.0 || tmp.r > c_int::MAX as f64 {
                crate::sexp::context::r_error(
                    "invalid workspace size from Lapack routine 'zgesdd'",
                );
            }
            lwork = tmp.r as c_int;
            let work_bytes = (lwork as usize)
                .checked_mul(std::mem::size_of::<LapRcomplex>())
                .unwrap_or_else(|| crate::sexp::context::r_error("invalid SVD workspace size"));
            let work_reservation = with_current_instance(|instance| {
                with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
            });
            if matches!(work_reservation, Some(None)) {
                crate::sexp::context::r_error(
                    "allocation failed: native SVD workspace exceeds resource limit",
                );
            }
            let _work_reservation = work_reservation.flatten();
            let mut work = vec![LapRcomplex::default(); lwork as usize];

            super::backend::zgesdd_(
                ju,
                &n,
                &p,
                x_copy.as_mut_ptr(),
                &n,
                REAL(s),
                COMPLEX(u) as *mut LapRcomplex,
                &ldu,
                COMPLEX(v) as *mut LapRcomplex,
                &ldvt,
                work.as_mut_ptr(),
                &lwork,
                rwork.as_mut_ptr(),
                iwork.as_mut_ptr(),
                &mut info,
            );

            if info != 0 {
                crate::sexp::context::r_error("error code from Lapack routine 'zgesdd'");
            }
        }

        let val = Rf_allocVector(VECSXP_C, 3);
        let _val_guard = protect(val);
        let nm = Rf_allocVector(STRSXP_C, 3);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"u\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"vt\0".as_ptr() as *const c_char));
        setAttrib(val, R_NamesSymbol(), nm);
        SET_VECTOR_ELT(val, 0, s);
        SET_VECTOR_ELT(val, 1, u);
        SET_VECTOR_ELT(val, 2, v);

        val
    }
}

/// La_rs_cmplx - complex symmetric eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rs_cmplx(SEXP xin, SEXP only_values)
pub unsafe fn La_rs_cmplx(xin: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(xin) != CPLXSXP_C {
            crate::sexp::context::r_error("'x' must be a complex matrix");
        }
        let _input_guard = protect(xin);
        let _ov_guard = protect(only_values);
        let dim = getAttrib(xin, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'x' must be a square numeric matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(xin) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let ov = asLogical(only_values);
        if ov == NA_INTEGER {
            Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
        }

        let jobv = if ov != 0 { b'N' } else { b'V' };
        let uplo = b'U';

        let Some(rwork_len) = (n as usize).checked_mul(3) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex eigen workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(xin) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let values = Rf_allocVector(REALSXP_C, n as c_int);
        let _values_guard = protect(values);

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut rwork = vec![0.0f64; rwork_len];
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
        let _ret_guard;
        let _nm_guard;
        if ov == 0 {
            let z = Rf_allocVector(CPLXSXP_C, len as c_int);
            let _z_guard = protect(z);
            ptr::copy_nonoverlapping(a_copy.as_ptr(), COMPLEX(z) as *mut LapRcomplex, len);

            ret = Rf_allocVector(VECSXP_C, 2);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 2);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            SET_VECTOR_ELT(ret, 1, z);
        } else {
            ret = Rf_allocVector(VECSXP_C, 1);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 1);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
        }

        setAttrib(ret, R_NamesSymbol(), nm);
        ret
    }
}

/// La_rg_cmplx - complex eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rg_cmplx(SEXP x, SEXP only_values)
pub unsafe fn La_rg_cmplx(x: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) != CPLXSXP_C {
            crate::sexp::context::r_error("'x' must be a complex matrix");
        }
        let _input_guard = protect(x);
        let _ov_guard = protect(only_values);
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'x' must be a square numeric matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(x) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let ov = asLogical(only_values);
        if ov == NA_INTEGER {
            Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
        }

        let jobvl = b'N';
        let jobvr = if ov != 0 { b'N' } else { b'V' };

        let Some(rwork_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex eigen workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(x) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let values = Rf_allocVector(CPLXSXP_C, n as c_int);
        let _values_guard = protect(values);

        let mut vr: *mut LapRcomplex = ptr::null_mut();
        if ov == 0 {
            vr = R_alloc(len, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;
        }

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut rwork = vec![0.0f64; rwork_len];
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
        let _ret_guard;
        let _nm_guard;
        if ov == 0 {
            let z = Rf_allocVector(CPLXSXP_C, len as c_int);
            let _z_guard = protect(z);
            ptr::copy_nonoverlapping(vr, COMPLEX(z) as *mut LapRcomplex, len);

            ret = Rf_allocVector(VECSXP_C, 2);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 2);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            SET_VECTOR_ELT(ret, 1, z);
        } else {
            ret = Rf_allocVector(VECSXP_C, 1);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 1);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
        }

        setAttrib(ret, R_NamesSymbol(), nm);
        ret
    }
}

/// qr_coef_real - real QR coefficients.
///
/// Port of: static SEXP qr_coef_real(SEXP q, SEXP bin)
pub unsafe fn qr_coef_real(q: SEXP, bin: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 1 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != REALSXP_C {
            crate::sexp::context::r_error("'y' must be a numeric matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let qr = VECTOR_ELT(q, 0);
        if TYPEOF(qr) != REALSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a numeric matrix");
        }

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (m as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let k = if m < n { m } else { n };

        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR coefficient workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy = vec![0.0f64; len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(REAL(qr), r_copy.as_mut_ptr(), len_r);
        }
        let mut b_copy = vec![0.0f64; len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);
        }

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

        let ans = Rf_allocVector(REALSXP_C, ((k as usize) * (nrhs as usize)) as c_int);
        let _ans_guard = protect(ans);
        // Copy only the first k rows
        for j in 0..nrhs as usize {
            for i in 0..k as usize {
                *REAL(ans).add(i + j * k as usize) = b_copy[i + j * m as usize];
            }
        }

        ans
    }
}

/// qr_coef_cmplx - complex QR coefficients.
///
/// Port of: static SEXP qr_coef_cmplx(SEXP q, SEXP bin)
pub unsafe fn qr_coef_cmplx(q: SEXP, bin: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 1 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != CPLXSXP_C {
            crate::sexp::context::r_error("'y' must be a complex matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let qr = VECTOR_ELT(q, 0);
        if TYPEOF(qr) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a complex matrix");
        }
        let _qr_guard = protect(qr);

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (m as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        // GNU trusts the stored taus; validate them only after the visible
        // matrix arguments, right before zunmqr reads them.
        let qraux = VECTOR_ELT(q, 2);
        if TYPEOF(qraux) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qraux' must be a complex vector");
        }
        let _qraux_guard = protect(qraux);
        let k = if m < n { m } else { n };
        if (XLENGTH(qraux) as usize) < k as usize {
            crate::sexp::context::r_error("invalid QR decomposition fields");
        }

        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<LapRcomplex>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR coefficient workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(qr) as *const LapRcomplex,
                r_copy.as_mut_ptr(),
                len_r,
            );
        }
        let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(bin) as *const LapRcomplex,
                b_copy.as_mut_ptr(),
                len_b,
            );
        }

        // GNU applies Q^H with zunmqr first, then solves R X = B with
        // ztrtrs; B keeps its full n rows (the tail holds Q^H residuals).
        let mut info: c_int = 0;
        let mut tmp = LapRcomplex::default();
        let mut lwork: c_int = -1;
        super::backend::zunmqr_(
            b"L".as_ptr(),
            b"C".as_ptr(),
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
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'zunmqr'"));
        }
        if !tmp.r.is_finite() || tmp.r < 1.0 || tmp.r > c_int::MAX as f64 {
            crate::sexp::context::r_error("invalid workspace size from Lapack routine 'zunmqr'");
        }
        lwork = tmp.r as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;
        super::backend::zunmqr_(
            b"L".as_ptr(),
            b"C".as_ptr(),
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
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'zunmqr'"));
        }

        super::backend::ztrtrs_(
            b"U".as_ptr(),
            b"N".as_ptr(),
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
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'ztrtrs'"));
        }

        let ans = Rf_allocVector(CPLXSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        if len_b != 0 {
            ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
        }
        let out_dim = Rf_allocVector(INTSXP_C, 2);
        let _out_dim_guard = protect(out_dim);
        *INTEGER(out_dim) = m;
        *INTEGER(out_dim).add(1) = nrhs;
        setAttrib(ans, R_DimSymbol(), out_dim);
        let bin_dimnames = getAttrib(bin, R_DimNamesSymbol());
        if bin_dimnames != R_NilValue() {
            setAttrib(ans, R_DimNamesSymbol(), bin_dimnames);
        }
        ans
    }
}
/// qr_qy_real - real QR multiply Q*y.
///
/// Port of: static SEXP qr_qy_real(SEXP q, SEXP bin, SEXP trans)
pub unsafe fn qr_qy_real(q: SEXP, bin: SEXP, trans: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 3 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != REALSXP_C {
            crate::sexp::context::r_error("'y' must be a numeric matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let _trans_guard = protect(trans);
        let qr = VECTOR_ELT(q, 0);
        let qraux = VECTOR_ELT(q, 2);
        if TYPEOF(qr) != REALSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a numeric matrix");
        }
        if TYPEOF(qraux) != REALSXP_C {
            crate::sexp::context::r_error("'qr$qraux' must be a numeric vector");
        }

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (bm as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let tr = asLogical(trans);
        if tr == NA_INTEGER {
            Rf_error(b"invalid 'trans' argument\0".as_ptr() as *const c_char);
        }

        let k = if m < n { m } else { n };
        if (XLENGTH(qraux) as usize) < k as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        // Work on copies
        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR Qy workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy = vec![0.0f64; len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(REAL(qr), r_copy.as_mut_ptr(), len_r);
        }
        let mut b_copy = vec![0.0f64; len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);
        }

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

        let ans = Rf_allocVector(REALSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        ptr::copy_nonoverlapping(b_copy.as_ptr(), REAL(ans), len_b);
        ans
    }
}

/// qr_qy_cmplx - complex QR multiply Q*y.
///
/// Port of: static SEXP qr_qy_cmplx(SEXP q, SEXP bin, SEXP trans)
pub unsafe fn qr_qy_cmplx(q: SEXP, bin: SEXP, trans: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 3 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != CPLXSXP_C {
            crate::sexp::context::r_error("'y' must be a complex matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let _trans_guard = protect(trans);
        let qr = VECTOR_ELT(q, 0);
        let qraux = VECTOR_ELT(q, 2);
        if TYPEOF(qr) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a complex matrix");
        }
        if TYPEOF(qraux) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qraux' must be a complex vector");
        }

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (bm as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let tr = asLogical(trans);
        if tr == NA_INTEGER {
            Rf_error(b"invalid 'trans' argument\0".as_ptr() as *const c_char);
        }

        let k = if m < n { m } else { n };
        if (XLENGTH(qraux) as usize) < k as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<LapRcomplex>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR Qy workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(qr) as *const LapRcomplex,
                r_copy.as_mut_ptr(),
                len_r,
            );
        }
        let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(bin) as *const LapRcomplex,
                b_copy.as_mut_ptr(),
                len_b,
            );
        }

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
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'zunmqr'"));
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
            crate::sexp::context::r_error(format!("error code {info} from Lapack routine 'zunmqr'"));
        }

        let ans = Rf_allocVector(CPLXSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
        ans
    }
}

/// det_ge_real - real matrix determinant.
///
/// Port of: static SEXP det_ge_real(SEXP ain, SEXP logarithm)
pub unsafe fn det_ge_real(ain: SEXP, logarithm: SEXP) -> SEXP {
    unsafe {
        let ldet = asLogical(logarithm);
        if ldet == NA_INTEGER {
            Rf_error(b"invalid 'logarithm' argument\0".as_ptr() as *const c_char);
        }
        let ain = if TYPEOF(ain) == REALSXP_C {

            ain
        } else if TYPEOF(ain) == INTSXP_C || TYPEOF(ain) == LGLSXP_C {
            let coerced = coerceVector(ain, REALSXP_C);
            let _c = protect(coerced);
            coerced
        } else {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        };
        let _a_guard = protect(ain);
        let _log_guard = protect(logarithm);

        let dim = getAttrib(ain, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(ain) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(scratch_bytes) = len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native determinant workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(ain), a_copy.as_mut_ptr(), len);
        }

        let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut info: c_int = 0;
        super::backend::dgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);
        if info < 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgetrf'");
        }

        let use_log = ldet != 0;
        let mut sign: c_int = 1;
        let modulus = if info > 0 {
            if use_log {
                f64::NEG_INFINITY
            } else {
                0.0
            }
        } else {
            for i in 0..n as usize {
                if *ipiv.add(i) != (i as c_int + 1) {
                    sign = -sign;
                }
            }
            if use_log {
                let mut acc = 0.0;
                let n1 = n as usize + 1;
                for i in 0..n as usize {
                    let dii = a_copy[i * n1];
                    acc += dii.abs().ln();
                    if dii < 0.0 {
                        sign = -sign;
                    }
                }
                acc
            } else {
                let mut acc = 1.0;
                let n1 = n as usize + 1;
                for i in 0..n as usize {
                    acc *= a_copy[i * n1];
                }
                if acc < 0.0 {
                    sign = -sign;
                    -acc
                } else {
                    acc
                }
            }
        };

        let val = Rf_allocVector(VECSXP_C, 2);
        let _val = protect(val);
        let nm = Rf_allocVector(STRSXP_C, 2);
        let _nm = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"modulus\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"sign\0".as_ptr() as *const c_char));
        setAttrib(val, R_NamesSymbol(), nm);
        let modulus_s = Rf_ScalarReal(modulus);
        let _ms = protect(modulus_s);
        setAttrib(
            modulus_s,
            crate::sexp::symbol::Rf_install(c"logarithm".as_ptr()),
            Rf_ScalarLogical(if use_log { 1 } else { 0 }),
        );
        SET_VECTOR_ELT(val, 0, modulus_s);
        SET_VECTOR_ELT(val, 1, Rf_ScalarInteger(sign));
        setAttrib(
            val,
            R_ClassSymbol(),
            Rf_ScalarString(Rf_mkChar(b"det\0".as_ptr() as *const c_char)),
        );
        val

    }
}

/// mod_do_lapack - main LAPACK dispatcher.
///
/// Port of: SEXP mod_do_lapack(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn mod_do_lapack(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // This dispatches based on op symbol to the appropriate La_* function.
        // In R, this is done via the .Internal() mechanism.
        // For now, return nil as dispatch needs the full .Internal infrastructure.
        R_NilValue()
    }
}

/// R_init_lapack - LAPACK module initialization.
///
/// Port of: void R_init_lapack(DllInfo *dll)
pub unsafe fn R_init_lapack(_info: *mut std::ffi::c_void) {
    unsafe {
        // Registration would happen here in the full implementation
    }
}
