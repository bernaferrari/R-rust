//! Port of R's src/main/array.c
//!
//! This module provides array/matrix manipulation functions including
//! matrix creation, array allocation, transpose, crossprod, colSums/rowSums,
//! diag, backsolve, maxcol, asplit, and related utilities.
//!
//! Original file: 2,483 lines.
//! Key functions:
//!   GetRowNames, GetColNames, do_matrix, allocMatrix, alloc3DArray, allocArray,
//!   DropDims, do_drop, do_length (conflict in inspect.rs), dispatch_length,
//!   dispatch_xlength, do_lengths, do_rowscols, do_matprod, do_transpose,
//!   do_aperm, do_colsum, do_array, do_diag, do_backsolve, do_maxcol, do_asplit

#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, INTEGER, LENGTH, TYPEOF, VECTOR_ELT, XLENGTH};
use crate::sexp::attrib_core::{R_DimSymbol, setAttrib};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3, Rf_cons};
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

fn array_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn valid_array_storage_type(mode: c_int) -> bool {
    matches!(mode, 10 | 13 | 14 | 15 | 16 | 19 | 20 | 24)
}

unsafe fn dim_product(dims: SEXP) -> Result<R_xlen_t, &'static str> {
    unsafe {
        if dims.is_null() || dims == R_NilValue() || TYPEOF(dims) != SEXPTYPE::INTSXP {
            return Err("'dims' must be an integer vector");
        }
        let mut total: R_xlen_t = 1;
        for i in 0..XLENGTH(dims) {
            let dim = *INTEGER(dims).add(i as usize);
            if dim < 0 {
                return Err("negative extents are not allowed");
            }
            total = total
                .checked_mul(dim as R_xlen_t)
                .ok_or("array is too large")?;
        }
        Ok(total)
    }
}

unsafe fn set_dim_attr(x: SEXP, dims: SEXP) -> SEXP {
    unsafe {
        if !x.is_null() {
            setAttrib(x, R_DimSymbol(), dims);
        }
        x
    }
}

// ---------------------------------------------------------------------------
// GetRowNames / GetColNames
// ---------------------------------------------------------------------------

/// Retrieve row names from a dimnames attribute (vector-based list).
///
/// Ported from R's `GetRowNames` in array.c.
/// Returns `VECTOR_ELT(dimnames, 0)` if dimnames is a VECSXP, else R_NilValue.
pub unsafe fn GetRowNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        if XLENGTH(dimnames) < 1 {
            R_NilValue()
        } else {
            VECTOR_ELT(dimnames, 0)
        }
    }
}

/// Retrieve column names from a dimnames attribute (vector-based list).
///
/// Ported from R's `GetColNames` in array.c.
/// Returns `VECTOR_ELT(dimnames, 1)` if dimnames is a VECSXP, else R_NilValue.
pub unsafe fn GetColNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        if XLENGTH(dimnames) < 2 {
            R_NilValue()
        } else {
            VECTOR_ELT(dimnames, 1)
        }
    }
}

// ---------------------------------------------------------------------------
// do_matrix
// ---------------------------------------------------------------------------

/// `.Internal(matrix(data, nrow, ncol, byrow, dimnames, missing(nrow), missing(ncol)))`
///
/// Ported from R's `do_matrix` in array.c (line 82).
/// Creates a matrix from the given data, dimensions, and byrow flag.
pub unsafe fn do_matrix(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            array_error("matrix() requires arguments");
        }
        crate::mainutils::essentials::do_matrix(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// allocMatrix
// ---------------------------------------------------------------------------

/// Allocate a 2-dimensional array (matrix) of the given type and dimensions.
///
/// Ported from R's `allocMatrix` in array.c (line 221).
pub unsafe fn allocMatrix(mode: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        if nrow < 0 || ncol < 0 {
            array_error("negative extents are not allowed");
        }
        if !valid_array_storage_type(mode) {
            array_error("invalid matrix storage mode");
        }
        let len = (nrow as R_xlen_t)
            .checked_mul(ncol as R_xlen_t)
            .unwrap_or_else(|| array_error("matrix is too large"));
        let result = Rf_allocVector3(mode, len);
        if result.is_null() {
            return R_NilValue();
        }
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nrow;
            *INTEGER(dim).add(1) = ncol;
            setAttrib(result, R_DimSymbol(), dim);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// alloc3DArray
// ---------------------------------------------------------------------------

/// Allocate a 3-dimensional array.
///
/// Ported from R's `alloc3DArray` in array.c (line 255).
pub unsafe fn alloc3DArray(mode: c_int, nrow: c_int, ncol: c_int, nface: c_int) -> SEXP {
    unsafe {
        if nrow < 0 || ncol < 0 || nface < 0 {
            array_error("negative extents are not allowed");
        }
        let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        if dims.is_null() {
            return R_NilValue();
        }
        *INTEGER(dims) = nrow;
        *INTEGER(dims).add(1) = ncol;
        *INTEGER(dims).add(2) = nface;
        allocArray(mode, dims)
    }
}

// ---------------------------------------------------------------------------
// allocArray
// ---------------------------------------------------------------------------

/// Allocate a general array with dimensions given by the integer vector `dims`.
///
/// Ported from R's `allocArray` in array.c (line 281).
pub unsafe fn allocArray(mode: c_int, dims: SEXP) -> SEXP {
    unsafe {
        if !valid_array_storage_type(mode) {
            array_error("invalid array storage mode");
        }
        let len = match dim_product(dims) {
            Ok(len) => len,
            Err(message) => array_error(message),
        };
        let result = Rf_allocVector3(mode, len);
        if result.is_null() {
            return R_NilValue();
        }
        set_dim_attr(result, dims)
    }
}

// ---------------------------------------------------------------------------
// DropDims
// ---------------------------------------------------------------------------

/// Strip away redundant (extent-1) dimension information from an array.
///
/// Ported from R's `DropDims` in array.c (line 313).
/// Note: this function mutates `x` in place; duplication should occur before
/// calling it.
pub(crate) unsafe fn DropDims(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::mainutils::essentials::do_drop(
            R_NilValue(),
            R_NilValue(),
            Rf_cons(x, R_NilValue()),
            R_NilValue(),
        )
    }
}

// ---------------------------------------------------------------------------
// do_drop
// ---------------------------------------------------------------------------

/// `.Internal(drop(x))` -- drop redundant dimensions from an array/matrix.
///
/// Ported from R's `do_drop` in array.c (line 430).
pub unsafe fn do_drop(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            array_error("drop() requires an argument");
        }
        crate::mainutils::essentials::do_drop(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_length (CONFLICT with inspect.rs)
// ---------------------------------------------------------------------------

/// `length(x)` -- return the length of a primitive object.
///
/// Ported from R's `do_length` in array.c (line 452).
///
/// **CONFLICT**: This symbol is also defined as `#[unsafe(no_mangle)]` in inspect.rs.
/// Using `pub(crate) unsafe fn` here (no `#[unsafe(no_mangle)]`) to avoid duplicate
/// symbol errors at link time.
pub(crate) unsafe fn do_length(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            array_error("length() requires an argument");
        }
        Rf_ScalarInteger(LENGTH(CAR(args)))
    }
}

// ---------------------------------------------------------------------------
// dispatch_length / dispatch_xlength
// ---------------------------------------------------------------------------

/// Dispatch to the `length` method for objects, returning R_len_t.
///
/// Ported from R's `dispatch_length` in array.c (line 483).
pub(crate) unsafe fn dispatch_length(x: SEXP, _call: SEXP, _rho: SEXP) -> c_int {
    unsafe { LENGTH(x) }
}

/// Dispatch to the `length` method for objects, returning R_xlen_t.
///
/// Ported from R's `dispatch_xlength` in array.c (line 491).
pub(crate) unsafe fn dispatch_xlength(x: SEXP, _call: SEXP, _rho: SEXP) -> usize {
    unsafe { XLENGTH(x) as usize }
}

// ---------------------------------------------------------------------------
// do_lengths
// ---------------------------------------------------------------------------

/// `lengths(x, use.names)` -- return a vector of the lengths of elements.
///
/// Ported from R's `do_lengths` in array.c (line 536).
pub unsafe fn do_lengths(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { crate::mainutils::essentials::do_lengths(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// do_rowscols
// ---------------------------------------------------------------------------

/// `row()` / `col()` -- create matrices of row/column indices.
///
/// Ported from R's `do_rowscols` in array.c (line 597).
/// PRIMVAL(op) == 1 for row(), == 2 for col().
pub unsafe fn do_rowscols(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("row()/col() are not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// do_matprod
// ---------------------------------------------------------------------------

/// `%*%`, `crossprod`, `tcrossprod` -- matrix multiplication.
///
/// Ported from R's `do_matprod` in array.c (line 1250).
/// PRIMVAL(op) == 0 for `%*%`, == 1 for `crossprod`, == 2 for `tcrossprod`.
pub unsafe fn do_matprod(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("matrix products are not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// do_transpose
// ---------------------------------------------------------------------------

/// `t(x)` -- transpose a matrix.
///
/// Ported from R's `do_transpose` in array.c (line 1569).
pub unsafe fn do_transpose(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { crate::mainutils::essentials::do_transpose(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// do_aperm
// ---------------------------------------------------------------------------

/// `aperm(a, perm, resize = TRUE)` -- array transposition by permutation.
///
/// Ported from R's `do_aperm` in array.c (line 1704).
pub unsafe fn do_aperm(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("aperm() is not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// do_colsum (handles colSums, colMeans, rowSums, rowMeans via PRIMVAL)
// ---------------------------------------------------------------------------

/// `colSums`, `colMeans`, `rowSums`, `rowMeans` -- column/row sum and mean.
///
/// Ported from R's `do_colsum` in array.c (line 1894).
/// PRIMVAL(op): 0 = colSums, 1 = colMeans, 2 = rowSums, 3 = rowMeans.
pub unsafe fn do_colsum(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("row/column summaries are not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// do_array
// ---------------------------------------------------------------------------

/// `array(data, dim, dimnames)` -- create a multi-dimensional array.
///
/// Ported from R's `do_array` in array.c (line 2145).
pub unsafe fn do_array(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            array_error("array() requires arguments");
        }
        crate::mainutils::essentials::do_array(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_diag
// ---------------------------------------------------------------------------

/// `diag(x, nrow, ncol)` -- extract or construct a diagonal matrix.
///
/// Ported from R's `do_diag` in array.c (line 2259).
pub unsafe fn do_diag(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { crate::mainutils::essentials::do_diag(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// do_backsolve
// ---------------------------------------------------------------------------

/// `backsolve(r, b, k, upper.tri, transpose)` -- solve triangular systems.
///
/// Ported from R's `do_backsolve` in array.c (line 2357).
pub unsafe fn do_backsolve(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("backsolve() is not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// do_maxcol
// ---------------------------------------------------------------------------

/// `max.col(m, ties.method)` -- find maximum position per row.
///
/// Ported from R's `do_maxcol` in array.c (line 2403).
pub unsafe fn do_maxcol(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("max.col() is not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// do_asplit
// ---------------------------------------------------------------------------

/// `asplit(x, m)` -- split an array into a list of sub-arrays.
///
/// Ported from R's `do_asplit` in array.c (line 2433).
pub unsafe fn do_asplit(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    array_error("asplit() is not implemented in the Rust array port yet")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::sexp::accessors::SET_VECTOR_ELT;
    use crate::sexp::constructors::Rf_allocVector3;
    use crate::sexp::session::RSession;

    fn assert_r_error(action: impl FnOnce()) -> RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<RError>()
            .expect("expected RError payload")
            .clone()
    }

    #[test]
    fn test_get_rownames_returns_nil() {
        let _session = RSession::new();
        unsafe {
            let result = GetRowNames(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_get_colnames_returns_nil() {
        let _session = RSession::new();
        unsafe {
            let result = GetColNames(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_get_row_and_col_names_extract_components() {
        let _session = RSession::new();
        unsafe {
            let rows = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
            let cols = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
            let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(dimnames, 0, rows);
            SET_VECTOR_ELT(dimnames, 1, cols);
            assert_eq!(GetRowNames(dimnames), rows);
            assert_eq!(GetColNames(dimnames), cols);
        }
    }

    #[test]
    fn test_do_matrix_delegates_to_real_implementation() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 4);
            for i in 0..4 {
                *INTEGER(data).add(i) = (i + 1) as c_int;
            }
            let args = Rf_cons(
                data,
                Rf_cons(
                    Rf_ScalarInteger(2),
                    Rf_cons(Rf_ScalarInteger(2), Rf_cons(R_NilValue(), R_NilValue())),
                ),
            );
            let result = do_matrix(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(XLENGTH(result), 4);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 2);
        }
    }

    #[test]
    fn test_do_matrix_errors_without_args() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_matrix(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("matrix() requires arguments"));
    }

    #[test]
    fn test_alloc_matrix_allocates_dimmed_vector() {
        let _session = RSession::new();
        unsafe {
            let result = allocMatrix(SEXPTYPE::INTSXP.as_c_int(), 2, 3);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(XLENGTH(result), 6);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(XLENGTH(dim), 2);
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 3);
        }
    }

    #[test]
    fn test_alloc_matrix_rejects_invalid_mode() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            allocMatrix(0, 0, 0);
        });
        assert!(err.message.contains("invalid matrix storage mode"));
    }

    #[test]
    fn test_alloc_3d_array_allocates_dimmed_vector() {
        let _session = RSession::new();
        unsafe {
            let result = alloc3DArray(SEXPTYPE::REALSXP.as_c_int(), 2, 3, 4);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(XLENGTH(result), 24);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(XLENGTH(dim), 3);
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 3);
            assert_eq!(*INTEGER(dim).add(2), 4);
        }
    }

    #[test]
    fn test_alloc_array_allocates_dimmed_vector() {
        let _session = RSession::new();
        unsafe {
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(dims) = 2;
            *INTEGER(dims).add(1) = 4;
            let result = allocArray(SEXPTYPE::LGLSXP.as_c_int(), dims);
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
            assert_eq!(XLENGTH(result), 8);
            assert_eq!(
                crate::sexp::attrib_core::getAttrib(result, R_DimSymbol()),
                dims
            );
        }
    }

    #[test]
    fn test_drop_dims_returns_nil() {
        let _session = RSession::new();
        unsafe {
            let result = DropDims(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_drop_errors_without_args() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_drop(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("drop() requires an argument"));
    }

    #[test]
    fn test_do_length_returns_real_length() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 5);
            let args = Rf_cons(data, R_NilValue());
            let result = do_length(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(*INTEGER(result), 5);
        }
    }

    #[test]
    fn test_dispatch_length_returns_vector_length() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::REALSXP, 7);
            assert_eq!(dispatch_length(data, ptr::null_mut(), ptr::null_mut()), 7);
        }
    }

    #[test]
    fn test_dispatch_xlength_returns_vector_length() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::REALSXP, 7);
            assert_eq!(dispatch_xlength(data, ptr::null_mut(), ptr::null_mut()), 7);
        }
    }

    #[test]
    fn test_do_lengths_delegates_to_real_implementation() {
        let _session = RSession::new();
        unsafe {
            let first = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let second = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let list = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(list, 0, first);
            SET_VECTOR_ELT(list, 1, second);
            let args = Rf_cons(list, Rf_cons(R_NilValue(), R_NilValue()));
            let result = do_lengths(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 2);
            assert_eq!(*INTEGER(result).add(1), 3);
        }
    }

    #[test]
    fn test_do_rowscols_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_rowscols(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("row()/col()"));
    }

    #[test]
    fn test_do_matprod_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_matprod(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("matrix products"));
    }

    #[test]
    fn test_do_transpose_delegates_to_real_implementation() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 4);
            for i in 0..4 {
                *INTEGER(data).add(i) = (i + 1) as c_int;
            }
            let matrix_args = Rf_cons(
                data,
                Rf_cons(
                    Rf_ScalarInteger(2),
                    Rf_cons(Rf_ScalarInteger(2), Rf_cons(R_NilValue(), R_NilValue())),
                ),
            );
            let matrix = do_matrix(
                ptr::null_mut(),
                ptr::null_mut(),
                matrix_args,
                ptr::null_mut(),
            );
            let result = do_transpose(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(matrix, R_NilValue()),
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(XLENGTH(result), 4);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 2);
        }
    }

    #[test]
    fn test_do_transpose_returns_nil_for_null_legacy_call() {
        let _session = RSession::new();
        unsafe {
            let result = do_transpose(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_aperm_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_aperm(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("aperm()"));
    }

    #[test]
    fn test_do_colsum_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_colsum(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("row/column summaries"));
    }

    #[test]
    fn test_do_array_delegates_to_real_implementation() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 4);
            for i in 0..4 {
                *INTEGER(data).add(i) = (i + 1) as c_int;
            }
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(dims) = 2;
            *INTEGER(dims).add(1) = 2;
            let args = Rf_cons(data, Rf_cons(dims, Rf_cons(R_NilValue(), R_NilValue())));
            let result = do_array(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(XLENGTH(result), 4);
            let result_dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(XLENGTH(result_dim), 2);
            assert_eq!(*INTEGER(result_dim), 2);
            assert_eq!(*INTEGER(result_dim).add(1), 2);
        }
    }

    #[test]
    fn test_do_array_errors_without_args() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_array(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("array() requires arguments"));
    }

    #[test]
    fn test_do_diag_delegates_to_real_implementation() {
        let _session = RSession::new();
        unsafe {
            let x = Rf_ScalarInteger(3);
            let args = Rf_cons(
                x,
                Rf_cons(
                    Rf_ScalarInteger(3),
                    Rf_cons(Rf_ScalarInteger(3), R_NilValue()),
                ),
            );
            let result = do_diag(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(XLENGTH(result), 9);
        }
    }

    #[test]
    fn test_do_backsolve_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_backsolve(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("backsolve()"));
    }

    #[test]
    fn test_do_maxcol_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_maxcol(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("max.col()"));
    }

    #[test]
    fn test_do_asplit_errors_until_implemented() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            do_asplit(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("asplit()"));
    }
}
