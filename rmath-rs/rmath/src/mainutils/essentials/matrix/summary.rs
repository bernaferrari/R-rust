//! Row/column summaries and extents: rowSums/colSums/rowMeans/colMeans, row(), col(), NROW/NCOL, lengths — extracted verbatim from the former single-file module.
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
// S3 dispatch helpers — NROW, NCOL, lengths, rownames, colnames, names, class
// ---------------------------------------------------------------------------

/// R's `NROW(x)` — number of rows; falls back to length(x) if no dim.
pub unsafe fn do_NROW(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_like(x) {
            return Rf_ScalarInteger(data_frame_row_count(x) as i32);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
            Rf_ScalarInteger(*INTEGER(dim_attr))
        } else {
            Rf_ScalarInteger(XLENGTH(x) as i32)
        }
    }
}

/// R's `NCOL(x)` — number of columns; returns 1 for vectors.
pub unsafe fn do_NCOL(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_like(x) {
            return Rf_ScalarInteger(XLENGTH(x) as i32);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            Rf_ScalarInteger(*INTEGER(dim_attr).add(1))
        } else {
            Rf_ScalarInteger(1)
        }
    }
}

/// R's `lengths(x)` — length of each element in a list/vector.
pub unsafe fn do_lengths(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            for i in 0..n {
                let elem = VECTOR_ELT(x, i as i64);
                *dst.add(i as usize) = if elem.is_null() {
                    0
                } else {
                    XLENGTH(elem) as i32
                };
            }
        } else {
            for i in 0..n {
                *dst.add(i as usize) = 1;
            }
        }
        result
    }
}

/// R's `length(x) <- value` — resize vectors with R's missing-value fill rules.
pub unsafe fn do_length_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            std::panic::panic_any(RError {
                message: "cannot set length of NULL".to_string(),
            });
        }
        let new_len = match length_replacement_size(value) {
            Some(len) => len,
            None => {
                std::panic::panic_any(RError {
                    message: "invalid value".to_string(),
                });
            }
        };

        let result = resize_vector(x, new_len);
        resize_names(x, result, new_len);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

pub unsafe fn resize_vector(x: SEXP, new_len: R_xlen_t) -> SEXP {
    unsafe {
        if XLENGTH(x) == new_len {
            return x;
        }
        let kind = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE(kind), new_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let copy_len = XLENGTH(x).min(new_len);

        if kind == SEXPTYPE::LGLSXP.as_c_int() {
            for i in 0..copy_len {
                *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, i as c_int);
            }
            for i in copy_len..new_len {
                *LOGICAL(result).add(i as usize) = NA_INTEGER;
            }
        } else if kind == SEXPTYPE::INTSXP.as_c_int() {
            for i in 0..copy_len {
                *INTEGER(result).add(i as usize) = INTEGER_ELT(x, i as c_int);
            }
            for i in copy_len..new_len {
                *INTEGER(result).add(i as usize) = NA_INTEGER;
            }
        } else if kind == SEXPTYPE::REALSXP.as_c_int() {
            for i in 0..copy_len {
                *REAL(result).add(i as usize) = REAL_ELT(x, i as c_int);
            }
            for i in copy_len..new_len {
                *REAL(result).add(i as usize) = NA_REAL;
            }
        } else if kind == SEXPTYPE::CPLXSXP.as_c_int() {
            for i in 0..copy_len {
                *COMPLEX(result).add(i as usize) = *COMPLEX(x).add(i as usize);
            }
            for i in copy_len..new_len {
                *COMPLEX(result).add(i as usize) = Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                };
            }
        } else if kind == SEXPTYPE::STRSXP.as_c_int() {
            for i in 0..copy_len {
                SET_STRING_ELT(result, i, STRING_ELT(x, i));
            }
            for i in copy_len..new_len {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            }
        } else if kind == SEXPTYPE::RAWSXP.as_c_int() {
            for i in 0..copy_len {
                *RAW(result).add(i as usize) = *RAW(x).add(i as usize);
            }
        } else if kind == SEXPTYPE::VECSXP.as_c_int() || kind == SEXPTYPE::EXPRSXP.as_c_int() {
            for i in 0..copy_len {
                SET_VECTOR_ELT(result, i, VECTOR_ELT(x, i));
            }
        } else {
            std::panic::panic_any(RError {
                message: "unsupported type for length assignment".to_string(),
            });
        }
        result
    }
}

pub unsafe fn resize_names(x: SEXP, result: SEXP, new_len: R_xlen_t) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }

        let resized = Rf_allocVector3(SEXPTYPE::STRSXP, new_len);
        if resized.is_null() {
            return;
        }
        let _resized_guard = protect(resized);
        let copy_len = XLENGTH(names).min(new_len);
        for i in 0..copy_len {
            SET_STRING_ELT(resized, i, STRING_ELT(names, i));
        }
        for i in copy_len..new_len {
            SET_STRING_ELT(resized, i, Rf_mkChar(c"".as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            resized,
        );
    }
}

pub unsafe fn length_replacement_size(value: SEXP) -> Option<R_xlen_t> {
    unsafe {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::INTSXP {
            INTEGER_ELT(value, 0) as f64
        } else if TYPEOF(value) == SEXPTYPE::LGLSXP {
            LOGICAL_ELT(value, 0) as f64
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            REAL_ELT(value, 0)
        } else if TYPEOF(value) == SEXPTYPE::STRSXP {
            elt_to_string(value, 0).trim().parse::<f64>().ok()?
        } else {
            return None;
        };
        if !raw.is_finite() || raw < 0.0 || raw > R_xlen_t::MAX as f64 {
            None
        } else {
            Some(raw.trunc() as R_xlen_t)
        }
    }
}
// ---------------------------------------------------------------------------
// Complete base R — colSums, rowSums, colMeans, rowMeans, col, row
// ---------------------------------------------------------------------------

/// R's `colSums(x, na.rm = FALSE, dims = 1)` — column sums of a matrix or array.
pub unsafe fn do_colSums(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_array_margin_summary(args, false, false) }
}

/// R's `rowSums(x, na.rm = FALSE, dims = 1)` — row sums of a matrix or array.
pub unsafe fn do_rowSums(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_array_margin_summary(args, true, false) }
}

/// R's `colMeans(x, na.rm = FALSE, dims = 1)` — column means of a matrix or array.
pub unsafe fn do_colMeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_array_margin_summary(args, false, true) }
}

/// R's `rowMeans(x, na.rm = FALSE, dims = 1)` — row means of a matrix or array.
pub unsafe fn do_rowMeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_array_margin_summary(args, true, true) }
}

pub unsafe fn do_array_margin_summary(args: SEXP, rows: bool, mean: bool) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let na_rm = named_logical_arg(args, "na.rm").unwrap_or_else(|| {
            let arg = arg_by_name_or_position(args, &[], 1);
            !arg.is_null() && arg != R_NilValue() && real_or_default(arg, 0.0) != 0.0
        });
        let dims_arg = {
            let named = arg_by_name_or_position(args, &["dims"], 2);
            if !named.is_null() && named != R_NilValue() {
                named
            } else {
                R_NilValue()
            }
        };

        let dim_attr =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let dim_len = if !dim_attr.is_null()
            && dim_attr != R_NilValue()
            && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
        {
            XLENGTH(dim_attr)
        } else {
            0
        };

        let dims = margin_summary_dims(dims_arg, dim_len, rows);
        let mut leading = 1 as R_xlen_t;
        let mut trailing = 1 as R_xlen_t;
        let mut result_axes = Vec::new();

        if dim_len == 0 {
            leading = XLENGTH(x);
        } else {
            for axis in 0..dim_len {
                let extent = *INTEGER(dim_attr).add(axis as usize) as R_xlen_t;
                if axis < dims {
                    leading = leading.saturating_mul(extent);
                } else {
                    trailing = trailing.saturating_mul(extent);
                }
            }
            let axis_range: Box<dyn Iterator<Item = R_xlen_t>> = if rows {
                Box::new(0..dims)
            } else {
                Box::new(dims..dim_len)
            };
            result_axes.extend(axis_range);
        }

        let result_len = if rows { leading } else { trailing };
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        if rows {
            for row in 0..leading {
                let value = summarize_margin_cells(x, row, leading, trailing, na_rm, mean);
                *REAL(result).add(row as usize) = value;
            }
        } else {
            for col in 0..trailing {
                let value = summarize_contiguous_cells(x, col * leading, leading, na_rm, mean);
                *REAL(result).add(col as usize) = value;
            }
        }

        set_margin_summary_attrs(result, dim_attr, &result_axes, x);
        result
    }
}

pub unsafe fn margin_summary_dims(dims_arg: SEXP, dim_len: R_xlen_t, rows: bool) -> R_xlen_t {
    unsafe {
        let default = if rows && dim_len > 0 { dim_len - 1 } else { 1 };
        if dims_arg.is_null() || dims_arg == R_NilValue() {
            return default;
        }
        let value = real_or_default(dims_arg, default as f64);
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
            base_error("invalid 'dims'");
        }
        let dims = value as R_xlen_t;
        if dim_len > 0 && dims > dim_len {
            base_error("invalid 'dims'");
        }
        dims
    }
}

pub unsafe fn summarize_contiguous_cells(
    x: SEXP,
    start: R_xlen_t,
    len: R_xlen_t,
    na_rm: bool,
    mean: bool,
) -> f64 {
    unsafe {
        let mut sum = 0.0;
        let mut count = 0i64;
        for offset in 0..len {
            match numeric_margin_value(x, start + offset) {
                Some(value) => {
                    sum += value;
                    count += 1;
                }
                None if !na_rm => return NA_REAL,
                None => {}
            }
        }
        if mean {
            if count > 0 {
                sum / count as f64
            } else {
                NA_REAL
            }
        } else {
            sum
        }
    }
}

pub unsafe fn summarize_margin_cells(
    x: SEXP,
    offset: R_xlen_t,
    stride: R_xlen_t,
    reps: R_xlen_t,
    na_rm: bool,
    mean: bool,
) -> f64 {
    unsafe {
        let mut sum = 0.0;
        let mut count = 0i64;
        for rep in 0..reps {
            match numeric_margin_value(x, rep * stride + offset) {
                Some(value) => {
                    sum += value;
                    count += 1;
                }
                None if !na_rm => return NA_REAL,
                None => {}
            }
        }
        if mean {
            if count > 0 {
                sum / count as f64
            } else {
                NA_REAL
            }
        } else {
            sum
        }
    }
}

pub unsafe fn numeric_margin_value(x: SEXP, index: R_xlen_t) -> Option<f64> {
    unsafe {
        let x_type = TYPEOF(x);
        let value = if x_type == SEXPTYPE::REALSXP {
            *REAL(x).add(index as usize)
        } else if x_type == SEXPTYPE::INTSXP || x_type == SEXPTYPE::LGLSXP {
            let value = *INTEGER(x).add(index as usize);
            if value == NA_INTEGER {
                return None;
            }
            value as f64
        } else {
            return None;
        };
        if value.is_nan() || value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            None
        } else {
            Some(value)
        }
    }
}

pub unsafe fn set_margin_summary_attrs(result: SEXP, dim: SEXP, axes: &[R_xlen_t], source: SEXP) {
    unsafe {
        if dim.is_null() || dim == R_NilValue() || axes.is_empty() {
            return;
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            source,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if axes.len() == 1 {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                retained_dimname(dimnames, axes[0]),
            );
            return;
        }

        let out_dim = Rf_allocVector3(SEXPTYPE::INTSXP, axes.len() as R_xlen_t);
        if out_dim.is_null() {
            return;
        }
        let _dim_guard = protect(out_dim);
        for (out_i, axis) in axes.iter().enumerate() {
            *INTEGER(out_dim).add(out_i) = *INTEGER(dim).add(*axis as usize);
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimSymbol(),
            out_dim,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            retained_dimnames(dimnames, axes),
        );
    }
}

/// R's `col(x)` — column indices for a matrix.
pub unsafe fn do_col(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || dim_attr == R_NilValue() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "a matrix-like object is required as argument to 'col'".to_string(),
            });
        }
        let nrow = *INTEGER(dim_attr) as R_xlen_t;
        let ncol = if LENGTH(dim_attr) >= 2 {
            *INTEGER(dim_attr).add(1) as R_xlen_t
        } else {
            1
        };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                *dst.add(idx) = (j + 1) as c_int;
            }
        }
        set_two_dim_attr(result, nrow, ncol);
        result
    }
}

/// R's `row(x)` — row indices for a matrix.
pub unsafe fn do_row(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || dim_attr == R_NilValue() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "a matrix-like object is required as argument to 'row'".to_string(),
            });
        }
        let nrow = *INTEGER(dim_attr) as R_xlen_t;
        let ncol = if LENGTH(dim_attr) >= 2 {
            *INTEGER(dim_attr).add(1) as R_xlen_t
        } else {
            1
        };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                *dst.add(idx) = (i + 1) as c_int;
            }
        }
        set_two_dim_attr(result, nrow, ncol);
        result
    }
}
