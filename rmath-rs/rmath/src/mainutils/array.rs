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

use std::ffi::CStr;
use std::os::raw::c_int;

use crate::sexp::accessors::{
    CAR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME, RAW, REAL, SET_STRING_ELT,
    SET_VECTOR_ELT, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{R_DimSymbol, getAttrib, setAttrib};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3, Rf_cons};
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

unsafe fn primitive_name(op: SEXP) -> Option<String> {
    unsafe {
        let name = crate::mainutils::relop::PRIMNAME(op);
        if name.is_null() {
            None
        } else {
            let name = CStr::from_ptr(name).to_string_lossy().into_owned();
            if name.is_empty() { None } else { Some(name) }
        }
    }
}

fn array_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn valid_array_storage_type(mode: c_int) -> bool {
    matches!(mode, 10 | 13 | 14 | 15 | 16 | 19 | 20 | 24)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MatProductKind {
    Matrix,
    Cross,
    TransposedCross,
}

fn is_numeric_type(x: SEXP) -> bool {
    unsafe {
        matches!(
            TYPEOF(x),
            tag if tag == SEXPTYPE::INTSXP.as_c_int()
                || tag == SEXPTYPE::REALSXP.as_c_int()
                || tag == SEXPTYPE::LGLSXP.as_c_int()
        )
    }
}

unsafe fn numeric_at(x: SEXP, index: usize) -> f64 {
    unsafe {
        match TYPEOF(x) {
            tag if tag == SEXPTYPE::REALSXP.as_c_int() => *REAL(x).add(index),
            tag if tag == SEXPTYPE::INTSXP.as_c_int() => {
                let value = *INTEGER(x).add(index);
                if value == NA_INTEGER {
                    f64::NAN
                } else {
                    value as f64
                }
            }
            tag if tag == SEXPTYPE::LGLSXP.as_c_int() => {
                let value = *LOGICAL(x).add(index);
                if value == NA_LOGICAL {
                    f64::NAN
                } else {
                    value as f64
                }
            }
            _ => f64::NAN,
        }
    }
}

unsafe fn matrix_dims(x: SEXP) -> Option<(usize, usize)> {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || LENGTH(dim) != 2
        {
            return None;
        }
        let nrow = *INTEGER(dim);
        let ncol = *INTEGER(dim).add(1);
        if nrow < 0 || ncol < 0 {
            None
        } else {
            Some((nrow as usize, ncol as usize))
        }
    }
}

unsafe fn matrix_value(x: SEXP, row: usize, col: usize, nrow: usize) -> f64 {
    unsafe { numeric_at(x, row + col * nrow) }
}

unsafe fn first_arg(args: SEXP, name: &str) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            array_error(format!("{name} requires an argument"));
        }
        CAR(args)
    }
}

unsafe fn second_arg_or_first(args: SEXP, first: SEXP) -> SEXP {
    unsafe {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            first
        } else {
            let second = CAR(rest);
            if second.is_null() || second == R_NilValue() {
                first
            } else {
                second
            }
        }
    }
}

unsafe fn arg_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() {
            return None;
        }
        Some(
            CStr::from_ptr(CHAR(printname))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn formal_match(name: &str, formals: &[&str]) -> Option<usize> {
    if let Some(index) = formals.iter().position(|formal| *formal == name) {
        return Some(index);
    }
    let mut matches = formals
        .iter()
        .enumerate()
        .filter(|(_, formal)| formal.starts_with(name))
        .map(|(index, _)| index);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

unsafe fn match_primitive_args(args: SEXP, formals: &[&str], name: &str) -> Vec<SEXP> {
    unsafe {
        let mut matched = vec![R_NilValue(); formals.len()];
        let mut positional = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if let Some(tag_name) = arg_tag_name(cell) {
                let Some(index) = formal_match(&tag_name, formals) else {
                    array_error(format!("unused argument ({tag_name} = ...)"));
                };
                if matched[index] != R_NilValue() {
                    array_error(format!(
                        "formal argument \"{}\" matched by multiple actual arguments",
                        formals[index]
                    ));
                }
                matched[index] = CAR(cell);
            } else {
                positional.push(CAR(cell));
            }
            cell = CDR(cell);
        }

        let mut next = 0;
        for value in positional {
            while next < matched.len() && matched[next] != R_NilValue() {
                next += 1;
            }
            if next == matched.len() {
                array_error(format!("{name}() called with too many arguments"));
            }
            matched[next] = value;
            next += 1;
        }
        matched
    }
}

unsafe fn matrix_dims_for_product(
    kind: MatProductKind,
    x: SEXP,
    y: SEXP,
) -> ((usize, usize), (usize, usize)) {
    unsafe {
        let x_dim = matrix_dims(x);
        let y_dim = matrix_dims(y);
        let x_len = XLENGTH(x) as usize;
        let y_len = XLENGTH(y) as usize;
        match kind {
            MatProductKind::Matrix => {
                let x_shape = x_dim.unwrap_or({
                    if let Some((y_rows, _)) = y_dim {
                        if x_len == y_rows {
                            (1, x_len)
                        } else {
                            (x_len, 1)
                        }
                    } else {
                        (1, x_len)
                    }
                });
                let y_shape = y_dim.unwrap_or({
                    if x_shape.1 == y_len {
                        (y_len, 1)
                    } else {
                        (1, y_len)
                    }
                });
                (x_shape, y_shape)
            }
            MatProductKind::Cross | MatProductKind::TransposedCross => {
                (x_dim.unwrap_or((x_len, 1)), y_dim.unwrap_or((y_len, 1)))
            }
        }
    }
}

unsafe fn set_matrix_dim(result: SEXP, nrow: usize, ncol: usize) {
    unsafe {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nrow as c_int;
            *INTEGER(dim).add(1) = ncol as c_int;
            setAttrib(result, R_DimSymbol(), dim);
        }
    }
}

unsafe fn copy_vector_element(dst: SEXP, dst_index: usize, src: SEXP, src_index: usize) {
    unsafe {
        match TYPEOF(src) {
            tag if tag == SEXPTYPE::LGLSXP.as_c_int() => {
                *LOGICAL(dst).add(dst_index) = *LOGICAL(src).add(src_index);
            }
            tag if tag == SEXPTYPE::INTSXP.as_c_int() => {
                *INTEGER(dst).add(dst_index) = *INTEGER(src).add(src_index);
            }
            tag if tag == SEXPTYPE::REALSXP.as_c_int() => {
                *REAL(dst).add(dst_index) = *REAL(src).add(src_index);
            }
            tag if tag == SEXPTYPE::CPLXSXP.as_c_int() => {
                *COMPLEX(dst).add(dst_index) = *COMPLEX(src).add(src_index);
            }
            tag if tag == SEXPTYPE::RAWSXP.as_c_int() => {
                *RAW(dst).add(dst_index) = *RAW(src).add(src_index);
            }
            tag if tag == SEXPTYPE::STRSXP.as_c_int() => {
                SET_STRING_ELT(
                    dst,
                    dst_index as R_xlen_t,
                    STRING_ELT(src, src_index as R_xlen_t),
                );
            }
            tag if tag == SEXPTYPE::VECSXP.as_c_int() || tag == SEXPTYPE::EXPRSXP.as_c_int() => {
                SET_VECTOR_ELT(
                    dst,
                    dst_index as R_xlen_t,
                    VECTOR_ELT(src, src_index as R_xlen_t),
                );
            }
            _ => array_error("unsupported array storage mode"),
        }
    }
}

fn column_major_strides(dims: &[usize]) -> Vec<usize> {
    let mut strides = Vec::with_capacity(dims.len());
    let mut stride = 1usize;
    for dim in dims {
        strides.push(stride);
        stride = stride.saturating_mul(*dim);
    }
    strides
}

fn unravel_column_major(mut index: usize, dims: &[usize]) -> Vec<usize> {
    dims.iter()
        .map(|dim| {
            let coord = if *dim == 0 { 0 } else { index % *dim };
            if *dim != 0 {
                index /= *dim;
            }
            coord
        })
        .collect()
}

fn ravel_column_major(coords: &[usize], strides: &[usize]) -> usize {
    coords
        .iter()
        .zip(strides.iter())
        .map(|(coord, stride)| coord.saturating_mul(*stride))
        .sum()
}

unsafe fn array_dimensions(x: SEXP, name: &str) -> Vec<usize> {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || LENGTH(dim) == 0
        {
            array_error(format!("{name} requires an array"));
        }
        (0..LENGTH(dim) as usize)
            .map(|index| {
                let value = *INTEGER(dim).add(index);
                if value < 0 {
                    array_error("negative extents are not allowed");
                }
                value as usize
            })
            .collect()
    }
}

unsafe fn parse_aperm_perm(perm: SEXP, ndim: usize) -> Vec<usize> {
    unsafe {
        let parsed = if perm.is_null() || perm == R_NilValue() {
            (0..ndim).rev().collect::<Vec<_>>()
        } else if !is_numeric_type(perm) || XLENGTH(perm) as usize != ndim {
            array_error("'perm' is of wrong length");
        } else {
            let mut values = Vec::with_capacity(ndim);
            for index in 0..ndim {
                let value = numeric_at(perm, index);
                if !value.is_finite() || value.fract() != 0.0 {
                    array_error("'perm' must contain a permutation of 1:d");
                }
                let value = value as isize;
                if value < 1 || value as usize > ndim {
                    array_error("'perm' must contain a permutation of 1:d");
                }
                values.push(value as usize - 1);
            }
            values
        };

        let mut seen = vec![false; ndim];
        for axis in &parsed {
            if seen[*axis] {
                array_error("'perm' must contain a permutation of 1:d");
            }
            seen[*axis] = true;
        }
        parsed
    }
}

unsafe fn parse_bool_arg(value: SEXP, default: bool) -> bool {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return default;
        }
        match TYPEOF(value) {
            tag if tag == SEXPTYPE::LGLSXP.as_c_int() => {
                let value = *LOGICAL(value);
                if value == NA_LOGICAL {
                    default
                } else {
                    value != 0
                }
            }
            tag if tag == SEXPTYPE::INTSXP.as_c_int() => {
                let value = *INTEGER(value);
                if value == NA_INTEGER {
                    default
                } else {
                    value != 0
                }
            }
            _ => default,
        }
    }
}

unsafe fn parse_resize_arg(value: SEXP) -> bool {
    unsafe { parse_bool_arg(value, true) }
}

unsafe fn parse_positive_k(value: SEXP, default: usize, limit: usize) -> usize {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return default;
        }
        if !is_numeric_type(value) || XLENGTH(value) == 0 {
            array_error("invalid 'k' argument");
        }
        let k = numeric_at(value, 0);
        if !k.is_finite() || k < 1.0 {
            array_error("invalid 'k' argument");
        }
        let k = k as usize;
        if k > limit {
            array_error("invalid 'k' argument");
        }
        k
    }
}

pub(crate) unsafe fn do_matprod_kind(kind: MatProductKind, args: SEXP, name: &str) -> SEXP {
    unsafe {
        let x = first_arg(args, name);
        let y = second_arg_or_first(args, x);
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            array_error(format!("{name} requires numeric arguments"));
        }
        if !is_numeric_type(x) || !is_numeric_type(y) {
            array_error(format!(
                "{name} requires numeric/complex matrix/vector arguments"
            ));
        }

        let ((x_rows, x_cols), (y_rows, y_cols)) = matrix_dims_for_product(kind, x, y);
        let (out_rows, out_cols, inner) = match kind {
            MatProductKind::Matrix => {
                if x_cols != y_rows {
                    array_error("non-conformable arguments");
                }
                (x_rows, y_cols, x_cols)
            }
            MatProductKind::Cross => {
                if x_rows != y_rows {
                    array_error("non-conformable arguments");
                }
                (x_cols, y_cols, x_rows)
            }
            MatProductKind::TransposedCross => {
                if x_cols != y_cols {
                    array_error("non-conformable arguments");
                }
                (x_rows, y_rows, x_cols)
            }
        };

        let result_len = (out_rows as R_xlen_t)
            .checked_mul(out_cols as R_xlen_t)
            .unwrap_or_else(|| array_error("matrix product is too large"));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
        if result.is_null() {
            return R_NilValue();
        }
        let out = REAL(result);

        for col in 0..out_cols {
            for row in 0..out_rows {
                let mut sum = 0.0;
                for k in 0..inner {
                    let x_value = match kind {
                        MatProductKind::Matrix => matrix_value(x, row, k, x_rows),
                        MatProductKind::Cross => matrix_value(x, k, row, x_rows),
                        MatProductKind::TransposedCross => matrix_value(x, row, k, x_rows),
                    };
                    let y_value = match kind {
                        MatProductKind::Matrix => matrix_value(y, k, col, y_rows),
                        MatProductKind::Cross => matrix_value(y, k, col, y_rows),
                        MatProductKind::TransposedCross => matrix_value(y, col, k, y_rows),
                    };
                    sum += x_value * y_value;
                }
                *out.add(row + col * out_rows) = sum;
            }
        }

        set_matrix_dim(result, out_rows, out_cols);
        result
    }
}

unsafe fn parse_ties_method(value: SEXP) -> c_int {
    unsafe {
        if value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || LENGTH(value) == 0
        {
            return 1;
        }
        let method = CStr::from_ptr(CHAR(STRING_ELT(value, 0))).to_string_lossy();
        match method.as_ref() {
            "random" => 1,
            "first" => 2,
            "last" => 3,
            _ => array_error("'ties.method' must be \"random\", \"first\", or \"last\""),
        }
    }
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
pub unsafe fn do_rowscols(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        match primitive_name(op).as_deref() {
            Some("col") => crate::mainutils::essentials::do_col(call, op, args, env),
            Some("row") | None => crate::mainutils::essentials::do_row(call, op, args, env),
            Some(name) => array_error(format!("unsupported row/col primitive '{name}'")),
        }
    }
}

// ---------------------------------------------------------------------------
// do_matprod
// ---------------------------------------------------------------------------

/// `%*%`, `crossprod`, `tcrossprod` -- matrix multiplication.
///
/// Ported from R's `do_matprod` in array.c (line 1250).
/// PRIMVAL(op) == 0 for `%*%`, == 1 for `crossprod`, == 2 for `tcrossprod`.
pub unsafe fn do_matprod(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let (kind, name) = match primitive_name(op).as_deref() {
            Some("crossprod") => (MatProductKind::Cross, "crossprod"),
            Some("tcrossprod") => (MatProductKind::TransposedCross, "tcrossprod"),
            _ => (MatProductKind::Matrix, "%*%"),
        };
        do_matprod_kind(kind, args, name)
    }
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
pub unsafe fn do_aperm(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = first_arg(args, "aperm()");
        let dims = array_dimensions(x, "aperm()");
        let ndim = dims.len();
        let total = XLENGTH(x) as usize;

        let rest = CDR(args);
        let perm_arg = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CAR(rest)
        };
        let resize_arg = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            let tail = CDR(rest);
            if tail.is_null() || tail == R_NilValue() {
                R_NilValue()
            } else {
                CAR(tail)
            }
        };

        let perm = parse_aperm_perm(perm_arg, ndim);
        let resize = parse_resize_arg(resize_arg);
        let permuted_dims: Vec<_> = perm.iter().map(|axis| dims[*axis]).collect();
        let result_dims = if resize {
            permuted_dims.clone()
        } else {
            dims.clone()
        };

        let result = Rf_allocVector3(TYPEOF(x), total as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }

        let input_strides = column_major_strides(&dims);
        for output_index in 0..total {
            let output_coords = unravel_column_major(output_index, &permuted_dims);
            let mut input_coords = vec![0usize; ndim];
            for (output_axis, input_axis) in perm.iter().enumerate() {
                input_coords[*input_axis] = output_coords[output_axis];
            }
            let input_index = ravel_column_major(&input_coords, &input_strides);
            copy_vector_element(result, output_index, x, input_index);
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, ndim as R_xlen_t);
        if !dim.is_null() {
            for (index, value) in result_dims.iter().enumerate() {
                *INTEGER(dim).add(index) = *value as c_int;
            }
            setAttrib(result, R_DimSymbol(), dim);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_colsum (handles colSums, colMeans, rowSums, rowMeans via PRIMVAL)
// ---------------------------------------------------------------------------

/// `colSums`, `colMeans`, `rowSums`, `rowMeans` -- column/row sum and mean.
///
/// Ported from R's `do_colsum` in array.c (line 1894).
/// PRIMVAL(op): 0 = colSums, 1 = colMeans, 2 = rowSums, 3 = rowMeans.
pub unsafe fn do_colsum(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        match primitive_name(op).as_deref() {
            Some("rowSums") => crate::mainutils::essentials::do_rowSums(call, op, args, env),
            Some("colMeans") => crate::mainutils::essentials::do_colMeans(call, op, args, env),
            Some("rowMeans") => crate::mainutils::essentials::do_rowMeans(call, op, args, env),
            Some("colSums") | None => crate::mainutils::essentials::do_colSums(call, op, args, env),
            Some(name) => array_error(format!("unsupported row/column summary primitive '{name}'")),
        }
    }
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
pub unsafe fn do_backsolve(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let matched = match_primitive_args(
            args,
            &["r", "x", "k", "upper.tri", "transpose"],
            "backsolve",
        );
        let r = matched[0];
        if r.is_null() || r == R_NilValue() {
            array_error("backsolve() requires a triangular matrix");
        }
        let b = matched[1];
        if b.is_null() || b == R_NilValue() {
            array_error("backsolve() requires a right-hand side");
        }
        if !is_numeric_type(r) || !is_numeric_type(b) {
            array_error("backsolve() requires numeric arguments");
        }

        let Some((r_rows, r_cols)) = matrix_dims(r) else {
            array_error("'r' must be a matrix");
        };
        let k = parse_positive_k(matched[2], r_cols, r_rows.min(r_cols));
        let upper_tri = parse_bool_arg(matched[3], true);
        let transpose = parse_bool_arg(matched[4], false);

        let (b_rows, rhs_count, vector_rhs) = if let Some((rows, cols)) = matrix_dims(b) {
            (rows, cols, false)
        } else {
            (XLENGTH(b) as usize, 1, true)
        };
        if b_rows < k {
            array_error("right-hand side has too few rows");
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, (k * rhs_count) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let out = REAL(result);

        for rhs in 0..rhs_count {
            for row in 0..k {
                *out.add(row + rhs * k) = numeric_at(b, row + rhs * b_rows);
            }

            if upper_tri && !transpose {
                for row in (0..k).rev() {
                    let mut value = *out.add(row + rhs * k);
                    for col in (row + 1)..k {
                        value -= matrix_value(r, row, col, r_rows) * *out.add(col + rhs * k);
                    }
                    let diag = matrix_value(r, row, row, r_rows);
                    if diag == 0.0 {
                        array_error("singular matrix in 'backsolve'");
                    }
                    *out.add(row + rhs * k) = value / diag;
                }
            } else if upper_tri && transpose {
                for row in 0..k {
                    let mut value = *out.add(row + rhs * k);
                    for col in 0..row {
                        value -= matrix_value(r, col, row, r_rows) * *out.add(col + rhs * k);
                    }
                    let diag = matrix_value(r, row, row, r_rows);
                    if diag == 0.0 {
                        array_error("singular matrix in 'backsolve'");
                    }
                    *out.add(row + rhs * k) = value / diag;
                }
            } else if !upper_tri && !transpose {
                for row in 0..k {
                    let mut value = *out.add(row + rhs * k);
                    for col in 0..row {
                        value -= matrix_value(r, row, col, r_rows) * *out.add(col + rhs * k);
                    }
                    let diag = matrix_value(r, row, row, r_rows);
                    if diag == 0.0 {
                        array_error("singular matrix in 'backsolve'");
                    }
                    *out.add(row + rhs * k) = value / diag;
                }
            } else {
                for row in (0..k).rev() {
                    let mut value = *out.add(row + rhs * k);
                    for col in (row + 1)..k {
                        value -= matrix_value(r, col, row, r_rows) * *out.add(col + rhs * k);
                    }
                    let diag = matrix_value(r, row, row, r_rows);
                    if diag == 0.0 {
                        array_error("singular matrix in 'backsolve'");
                    }
                    *out.add(row + rhs * k) = value / diag;
                }
            }
        }

        if !vector_rhs {
            set_matrix_dim(result, k, rhs_count);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_maxcol
// ---------------------------------------------------------------------------

/// `max.col(m, ties.method)` -- find maximum position per row.
///
/// Ported from R's `do_maxcol` in array.c (line 2403).
pub unsafe fn do_maxcol(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = first_arg(args, "max.col()");
        if !is_numeric_type(x) {
            array_error("'m' must be a numeric matrix");
        }
        let Some((nrow, ncol)) = matrix_dims(x) else {
            array_error("'m' must be a matrix");
        };

        let ties_arg = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                R_NilValue()
            } else {
                CAR(rest)
            }
        };
        let ties_method = parse_ties_method(ties_arg);

        let mut values = Vec::with_capacity(nrow * ncol);
        for index in 0..(nrow * ncol) {
            values.push(numeric_at(x, index));
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nrow as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        crate::appl::maxcol::R_max_col(
            values.as_ptr(),
            nrow as c_int,
            ncol as c_int,
            INTEGER(result),
            ties_method,
        );
        result
    }
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
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_mkString};
    use crate::sexp::session::RSession;

    fn assert_r_error(action: impl FnOnce()) -> RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<RError>()
            .expect("expected RError payload")
            .clone()
    }

    unsafe fn real_matrix(values: &[f64], nrow: c_int, ncol: c_int) -> SEXP {
        unsafe {
            let matrix = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
            for (index, value) in values.iter().enumerate() {
                *REAL(matrix).add(index) = *value;
            }
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(dim) = nrow;
            *INTEGER(dim).add(1) = ncol;
            setAttrib(matrix, R_DimSymbol(), dim);
            matrix
        }
    }

    unsafe fn int_array(values: &[c_int], dims: &[c_int]) -> SEXP {
        unsafe {
            let array = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
            for (index, value) in values.iter().enumerate() {
                *INTEGER(array).add(index) = *value;
            }
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, dims.len() as R_xlen_t);
            for (index, value) in dims.iter().enumerate() {
                *INTEGER(dim).add(index) = *value;
            }
            setAttrib(array, R_DimSymbol(), dim);
            array
        }
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
    fn test_do_rowscols_delegates_to_row_by_default() {
        let _session = RSession::new();
        unsafe {
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
            let matrix_args = Rf_cons(
                data,
                Rf_cons(
                    Rf_ScalarInteger(2),
                    Rf_cons(Rf_ScalarInteger(3), Rf_cons(R_NilValue(), R_NilValue())),
                ),
            );
            let matrix = do_matrix(
                ptr::null_mut(),
                ptr::null_mut(),
                matrix_args,
                ptr::null_mut(),
            );
            let result = do_rowscols(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(matrix, R_NilValue()),
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 1);
            assert_eq!(*INTEGER(result).add(1), 2);
            assert_eq!(*INTEGER(result).add(2), 1);
        }
    }

    #[test]
    fn test_do_matprod_multiplies_column_major_matrices() {
        let _session = RSession::new();
        unsafe {
            let x = real_matrix(&[1.0, 2.0, 3.0, 4.0], 2, 2);
            let y = real_matrix(&[5.0, 6.0, 7.0, 8.0], 2, 2);
            let args = Rf_cons(x, Rf_cons(y, R_NilValue()));

            let result = do_matprod(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(*REAL(result), 23.0);
            assert_eq!(*REAL(result).add(1), 34.0);
            assert_eq!(*REAL(result).add(2), 31.0);
            assert_eq!(*REAL(result).add(3), 46.0);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 2);
        }
    }

    #[test]
    fn test_crossprod_and_tcrossprod_use_column_major_layout() {
        let _session = RSession::new();
        unsafe {
            let x = real_matrix(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 3);
            let args = Rf_cons(x, R_NilValue());

            let cross = do_matprod_kind(MatProductKind::Cross, args, "crossprod");
            assert_eq!(*REAL(cross), 5.0);
            assert_eq!(*REAL(cross).add(1), 11.0);
            assert_eq!(*REAL(cross).add(4), 25.0);
            assert_eq!(*REAL(cross).add(8), 61.0);

            let tcross = do_matprod_kind(MatProductKind::TransposedCross, args, "tcrossprod");
            assert_eq!(*REAL(tcross), 35.0);
            assert_eq!(*REAL(tcross).add(1), 44.0);
            assert_eq!(*REAL(tcross).add(2), 44.0);
            assert_eq!(*REAL(tcross).add(3), 56.0);
        }
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
    fn test_do_aperm_permutates_column_major_array() {
        let _session = RSession::new();
        unsafe {
            let values: Vec<c_int> = (1..=24).collect();
            let array = int_array(&values, &[2, 3, 4]);
            let perm = int_array(&[2, 1, 3], &[3]);
            let args = Rf_cons(
                array,
                Rf_cons(perm, Rf_cons(Rf_ScalarLogical(1), R_NilValue())),
            );
            let result = do_aperm(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 1);
            assert_eq!(*INTEGER(result).add(1), 3);
            assert_eq!(*INTEGER(result).add(2), 5);
            assert_eq!(*INTEGER(result).add(3), 2);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(*INTEGER(dim), 3);
            assert_eq!(*INTEGER(dim).add(1), 2);
            assert_eq!(*INTEGER(dim).add(2), 4);
        }
    }

    #[test]
    fn test_do_aperm_resize_false_keeps_original_dims() {
        let _session = RSession::new();
        unsafe {
            let values: Vec<c_int> = (1..=24).collect();
            let array = int_array(&values, &[2, 3, 4]);
            let perm = int_array(&[3, 1, 2], &[3]);
            let args = Rf_cons(
                array,
                Rf_cons(perm, Rf_cons(Rf_ScalarLogical(0), R_NilValue())),
            );
            let result = do_aperm(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(*INTEGER(result), 1);
            assert_eq!(*INTEGER(result).add(1), 7);
            assert_eq!(*INTEGER(result).add(2), 13);
            assert_eq!(*INTEGER(result).add(3), 19);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 3);
            assert_eq!(*INTEGER(dim).add(2), 4);
        }
    }

    #[test]
    fn test_do_colsum_delegates_to_col_sums_by_default() {
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
            let result = do_colsum(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(matrix, Rf_cons(R_NilValue(), R_NilValue())),
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(*crate::sexp::accessors::REAL(result), 3.0);
            assert_eq!(*crate::sexp::accessors::REAL(result).add(1), 7.0);
        }
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
    fn test_do_backsolve_solves_upper_triangular_system() {
        let _session = RSession::new();
        unsafe {
            let r = real_matrix(&[2.0, 0.0, 1.0, 3.0], 2, 2);
            let b = real_matrix(&[5.0, 9.0, 1.0, 2.0], 2, 2);
            let args = Rf_cons(
                r,
                Rf_cons(
                    b,
                    Rf_cons(
                        Rf_ScalarInteger(2),
                        Rf_cons(
                            Rf_ScalarLogical(1),
                            Rf_cons(Rf_ScalarLogical(0), R_NilValue()),
                        ),
                    ),
                ),
            );
            let result = do_backsolve(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(*REAL(result), 1.0);
            assert_eq!(*REAL(result).add(1), 3.0);
            assert!((*REAL(result).add(2) - (1.0 / 6.0)).abs() < 1e-12);
            assert!((*REAL(result).add(3) - (2.0 / 3.0)).abs() < 1e-12);
            let dim = crate::sexp::attrib_core::getAttrib(result, R_DimSymbol());
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 2);
        }
    }

    #[test]
    fn test_do_backsolve_handles_transpose_and_lower_triangular() {
        let _session = RSession::new();
        unsafe {
            let r = real_matrix(&[2.0, 0.0, 1.0, 3.0], 2, 2);
            let b = real_matrix(&[5.0, 9.0], 2, 1);
            let transposed = do_backsolve(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(
                    r,
                    Rf_cons(
                        b,
                        Rf_cons(
                            Rf_ScalarInteger(2),
                            Rf_cons(
                                Rf_ScalarLogical(1),
                                Rf_cons(Rf_ScalarLogical(1), R_NilValue()),
                            ),
                        ),
                    ),
                ),
                ptr::null_mut(),
            );
            assert_eq!(*REAL(transposed), 2.5);
            assert!((*REAL(transposed).add(1) - (13.0 / 6.0)).abs() < 1e-12);

            let lower = do_backsolve(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(
                    r,
                    Rf_cons(
                        b,
                        Rf_cons(
                            Rf_ScalarInteger(2),
                            Rf_cons(
                                Rf_ScalarLogical(0),
                                Rf_cons(Rf_ScalarLogical(0), R_NilValue()),
                            ),
                        ),
                    ),
                ),
                ptr::null_mut(),
            );
            assert_eq!(*REAL(lower), 2.5);
            assert_eq!(*REAL(lower).add(1), 3.0);
        }
    }

    #[test]
    fn test_do_maxcol_returns_first_or_last_ties() {
        let _session = RSession::new();
        unsafe {
            let matrix = real_matrix(&[1.0, 3.0, 2.0, 3.0, 2.0, 1.0], 2, 3);
            let first_args = Rf_cons(
                matrix,
                Rf_cons(Rf_mkString(c"first".as_ptr()), R_NilValue()),
            );
            let first = do_maxcol(
                ptr::null_mut(),
                ptr::null_mut(),
                first_args,
                ptr::null_mut(),
            );
            assert_eq!(*INTEGER(first), 2);
            assert_eq!(*INTEGER(first).add(1), 1);

            let last_args = Rf_cons(matrix, Rf_cons(Rf_mkString(c"last".as_ptr()), R_NilValue()));
            let last = do_maxcol(ptr::null_mut(), ptr::null_mut(), last_args, ptr::null_mut());
            assert_eq!(*INTEGER(last), 3);
            assert_eq!(*INTEGER(last).add(1), 2);
        }
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
