//! Matrix linear algebra: crossprod, tcrossprod, det, solve, which() for arrays — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::Path;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol};

// ---------------------------------------------------------------------------
// Matrix/linear algebra
// ---------------------------------------------------------------------------

/// R's `crossprod(x, y)` — computes t(x) %*% y.
/// If y is NULL, computes t(x) %*% x.
pub unsafe fn do_crossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::array::do_matprod_kind(
            crate::mainutils::array::MatProductKind::Cross,
            args,
            "crossprod",
        )
    }
}

/// R's `tcrossprod(x, y)` — computes x %*% t(y).
/// If y is NULL, computes x %*% t(x).
pub unsafe fn do_tcrossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::array::do_matprod_kind(
            crate::mainutils::array::MatProductKind::TransposedCross,
            args,
            "tcrossprod",
        )
    }
}

/// R's `det(x)` — determinant of a square matrix (simplified via LU-like approach).
/// For a 2x2 matrix: det = a*d - b*c. For larger, uses LU decomposition concept.
pub unsafe fn do_det(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
            return Rf_ScalarReal(NA_REAL);
        }
        let n = *INTEGER(dim_attr) as usize;
        let m = *INTEGER(dim_attr).add(1) as usize;
        if n != m || n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return Rf_ScalarReal(NA_REAL);
        }

        // Compute determinant using LU decomposition (without pivoting for simplicity)
        let src = REAL(x);
        // Copy matrix data
        let mut mat: Vec<f64> = Vec::with_capacity(n * n);
        for i in 0..n * n {
            mat.push(*src.add(i));
        }

        let mut det_val = 1.0_f64;
        for i in 0..n {
            // Find pivot
            let mut max_val = mat[i * n + i].abs();
            let mut max_row = i;
            for k in (i + 1)..n {
                let v = mat[k * n + i].abs();
                if v > max_val {
                    max_val = v;
                    max_row = k;
                }
            }
            if max_val == 0.0 {
                return Rf_ScalarReal(0.0);
            }
            // Swap rows
            if max_row != i {
                for j in 0..n {
                    let tmp = mat[i * n + j];
                    mat[i * n + j] = mat[max_row * n + j];
                    mat[max_row * n + j] = tmp;
                }
                det_val = -det_val;
            }
            det_val *= mat[i * n + i];
            // Eliminate
            let pivot = mat[i * n + i];
            for k in (i + 1)..n {
                let factor = mat[k * n + i] / pivot;
                mat[k * n + i] = 0.0;
                for j in (i + 1)..n {
                    mat[k * n + j] -= factor * mat[i * n + j];
                }
            }
        }

        Rf_ScalarReal(det_val)
    }
}

/// R's `solve(a, b)` — solve the linear system a %*% x = b.
/// If b is omitted, computes the inverse of a (simplified).
/// Uses Gaussian elimination with partial pivoting.
pub unsafe fn do_solve(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b_cdr = CDR(args);
        let b = if b_cdr.is_null() || b_cdr == R_NilValue() {
            R_NilValue()
        } else {
            CAR(b_cdr)
        };

        if a.is_null() || a == R_NilValue() {
            return R_NilValue();
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(
            a,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
            return R_NilValue();
        }
        let n = *INTEGER(dim_attr) as usize;
        let m = *INTEGER(dim_attr).add(1) as usize;
        if n != m || n == 0 {
            return R_NilValue();
        }
        if TYPEOF(a) != SEXPTYPE::REALSXP {
            return R_NilValue();
        }

        let src = REAL(a);
        // Build augmented matrix [A | I] or [A | b]
        let nrhs = if b == R_NilValue() {
            n // inverse
        } else {
            let b_dim = crate::sexp::attrib_core::getAttrib(
                b,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            if !b_dim.is_null() && TYPEOF(b_dim) == SEXPTYPE::INTSXP && LENGTH(b_dim) == 2 {
                *INTEGER(b_dim).add(1) as usize
            } else {
                1
            }
        };

        let aug_cols = n + nrhs;
        let mut aug: Vec<f64> = vec![0.0; n * aug_cols];

        // Fill A
        for i in 0..n {
            for j in 0..n {
                aug[i * aug_cols + j] = *src.add(i * n + j);
            }
        }

        // Fill right-hand side
        if b == R_NilValue() {
            // Identity matrix for inverse
            for i in 0..n {
                aug[i * aug_cols + n + i] = 1.0;
            }
        } else {
            let b_src = REAL(b);
            for i in 0..n {
                for j in 0..nrhs {
                    aug[i * aug_cols + n + j] = *b_src.add(i * nrhs + j);
                }
            }
        }

        // Gaussian elimination with partial pivoting
        for i in 0..n {
            // Find pivot
            let mut max_val = aug[i * aug_cols + i].abs();
            let mut max_row = i;
            for k in (i + 1)..n {
                let v = aug[k * aug_cols + i].abs();
                if v > max_val {
                    max_val = v;
                    max_row = k;
                }
            }
            if max_val == 0.0 {
                return R_NilValue(); // singular
            }
            // Swap rows
            if max_row != i {
                for j in 0..aug_cols {
                    let tmp = aug[i * aug_cols + j];
                    aug[i * aug_cols + j] = aug[max_row * aug_cols + j];
                    aug[max_row * aug_cols + j] = tmp;
                }
            }
            // Eliminate below
            let pivot = aug[i * aug_cols + i];
            for k in (i + 1)..n {
                let factor = aug[k * aug_cols + i] / pivot;
                aug[k * aug_cols + i] = 0.0;
                for j in (i + 1)..aug_cols {
                    aug[k * aug_cols + j] -= factor * aug[i * aug_cols + j];
                }
            }
        }

        // Back substitution
        for i in (0..n).rev() {
            let diag = aug[i * aug_cols + i];
            for j in (n)..aug_cols {
                aug[i * aug_cols + j] /= diag;
            }
            aug[i * aug_cols + i] = 1.0;
            for k in 0..i {
                let factor = aug[k * aug_cols + i];
                for j in n..aug_cols {
                    aug[k * aug_cols + j] -= factor * aug[i * aug_cols + j];
                }
                aug[k * aug_cols + i] = 0.0;
            }
        }

        // Extract result
        let result_len = (n * nrhs) as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            for j in 0..nrhs {
                *dst.add(i * nrhs + j) = aug[i * aug_cols + n + j];
            }
        }

        // Set dim if multi-column
        if nrhs > 1 {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let _dim_guard = protect(dim);
                *INTEGER(dim) = n as i32;
                *INTEGER(dim).add(1) = nrhs as i32;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Matrix helpers
// ---------------------------------------------------------------------------

/// R's `which(x)` variant for arrays — returns 1-based row-major indices where x is TRUE.
pub unsafe fn do_which_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Same as do_which for now — array-aware which is equivalent for logical vectors
        do_which(_call, _op, args, _rho)
    }
}
