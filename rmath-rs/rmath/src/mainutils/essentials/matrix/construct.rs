//! Matrix construction: lower/upper.tri, matrix(), array(), diag(), element access/coercion helpers, as.matrix — extracted verbatim from the former single-file module.
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
// Matrix: lower.tri, upper.tri
// ---------------------------------------------------------------------------

/// R's `lower.tri(x, diag=FALSE)` — TRUE for lower triangle of matrix.
pub unsafe fn do_lower_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let diag_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let include_diag = !diag_arg.is_null()
            && diag_arg != R_NilValue()
            && real_or_default(diag_arg, 0.0) != 0.0;
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        let (nrow, ncol) =
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr).add(1) as R_xlen_t,
                )
            } else {
                let n = XLENGTH(x);
                (n, 1)
            };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                let is_lower = if include_diag { i >= j } else { i > j };
                *dst.add(idx) = if is_lower { TRUE } else { FALSE };
            }
        }
        result
    }
}

/// R's `upper.tri(x, diag=FALSE)` — TRUE for upper triangle of matrix.
pub unsafe fn do_upper_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let diag_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let include_diag = !diag_arg.is_null()
            && diag_arg != R_NilValue()
            && real_or_default(diag_arg, 0.0) != 0.0;
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        let (nrow, ncol) =
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr).add(1) as R_xlen_t,
                )
            } else {
                let n = XLENGTH(x);
                (n, 1)
            };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                let is_upper = if include_diag { i <= j } else { i < j };
                *dst.add(idx) = if is_upper { TRUE } else { FALSE };
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Matrix operations: matrix(), t(), nrow(), ncol(), dim(), diag()
// ---------------------------------------------------------------------------

pub fn supported_matrix_type(t: c_int) -> bool {
    t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::VECSXP
}

pub unsafe fn set_matrix_na_or_zero(x: SEXP, i: R_xlen_t) {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => *REAL(x).add(i as usize) = NA_REAL,
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(i as usize) = NA_INTEGER,
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) = NA_INTEGER,
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(x).add(i as usize) = Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                };
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(i as usize) = 0 as Rbyte,
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(x, i, crate::sexp::globals::R_NaString());
            }
            t if t == SEXPTYPE::VECSXP => SET_VECTOR_ELT(x, i, R_NilValue()),
            _ => {}
        }
    }
}

/// Read element `i` of `x` as a CHARSXP, coercing non-character atomics via
/// their string form. Returns R_NaString when the source element is NA or the
/// index is out of range.
pub(crate) unsafe fn str_elt_or_na(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return crate::sexp::globals::R_NaString();
        }
        let n = XLENGTH(x);
        if n == 0 {
            return crate::sexp::globals::R_NaString();
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let charsxp = STRING_ELT(x, i % n);
            return if charsxp.is_null() {
                crate::sexp::globals::R_NaString()
            } else {
                charsxp
            };
        }
        if as_character_element_is_na(x, i % n) {
            return crate::sexp::globals::R_NaString();
        }
        let s = elt_to_string(x, i % n);
        let cstr = CString::new(s).unwrap_or_default();
        let charsxp = Rf_mkChar(cstr.as_ptr());
        if charsxp.is_null() {
            crate::sexp::globals::R_NaString()
        } else {
            charsxp
        }
    }
}

/// Read element `i` of `x` as an integer, coercing logical/raw; NA otherwise.
pub(crate) unsafe fn int_elt_or_na(x: SEXP, i: R_xlen_t) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_INTEGER;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return NA_INTEGER;
        }
        let idx = (i % n) as usize;
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(idx),
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(idx),
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(idx) as c_int,
            _ => NA_INTEGER,
        }
    }
}

/// Read element `i` of a raw vector as a byte (0 when out of range).
pub(crate) unsafe fn raw_elt_or_zero(x: SEXP, i: R_xlen_t) -> Rbyte {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP || XLENGTH(x) == 0 {
            return 0;
        }
        *RAW(x).add((i % XLENGTH(x)) as usize)
    }
}

/// Read element `i` of `x` as a complex value; NA complex otherwise.
pub(crate) unsafe fn cplx_elt_or_na(x: SEXP, i: R_xlen_t) -> Rcomplex {
    unsafe {
        if !x.is_null() && x != R_NilValue() && TYPEOF(x) == SEXPTYPE::CPLXSXP && XLENGTH(x) != 0 {
            return *COMPLEX(x).add((i % XLENGTH(x)) as usize);
        }
        Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        }
    }
}
pub unsafe fn set_matrix_zero(x: SEXP, i: R_xlen_t) {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => *REAL(x).add(i as usize) = 0.0,
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(i as usize) = 0,
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) = FALSE,
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(x).add(i as usize) = Rcomplex { r: 0.0, i: 0.0 };
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(i as usize) = 0 as Rbyte,
            t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(x, i, Rf_mkChar(c"".as_ptr())),
            t if t == SEXPTYPE::VECSXP => SET_VECTOR_ELT(x, i, R_NilValue()),
            _ => {}
        }
    }
}

pub(crate) unsafe fn copy_matrix_element(dst: SEXP, dst_i: R_xlen_t, src: SEXP, src_i: R_xlen_t) {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(dst_i as usize) = *REAL(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::INTSXP => {
                *INTEGER(dst).add(dst_i as usize) = *INTEGER(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::LGLSXP => {
                *LOGICAL(dst).add(dst_i as usize) = *LOGICAL(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(dst).add(dst_i as usize) = *COMPLEX(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_i as usize) = *RAW(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(dst, dst_i, STRING_ELT(src, src_i)),
            t if t == SEXPTYPE::VECSXP => SET_VECTOR_ELT(dst, dst_i, VECTOR_ELT(src, src_i)),
            _ => {}
        }
    }
}

pub(crate) unsafe fn set_two_dim_attr(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t) {
    unsafe {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if dim.is_null() {
            return;
        }
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow as c_int;
        *INTEGER(dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
    }
}

pub unsafe fn set_diagonal_identity_value(x: SEXP, i: R_xlen_t) {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => *REAL(x).add(i as usize) = 1.0,
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(i as usize) = 1,
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) = TRUE,
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(x).add(i as usize) = Rcomplex { r: 1.0, i: 0.0 };
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(i as usize) = 1 as Rbyte,
            _ => {}
        }
    }
}

pub unsafe fn string_vector_contains_value(x: SEXP, needle: &str) -> bool {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(x) {
            let elt = STRING_ELT(x, i);
            if elt.is_null() {
                continue;
            }
            let ptr = CHAR(elt);
            if !ptr.is_null() && CStr::from_ptr(ptr).to_str().ok() == Some(needle) {
                return true;
            }
        }
        false
    }
}

pub(crate) unsafe fn is_data_frame_object(x: SEXP) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        string_vector_contains_value(class, "data.frame")
    }
}

pub(crate) unsafe fn data_frame_row_count(x: SEXP) -> R_xlen_t {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        );
        if !row_names.is_null() && TYPEOF(row_names) == SEXPTYPE::INTSXP && LENGTH(row_names) == 2 {
            let first = *INTEGER(row_names);
            let second = *INTEGER(row_names).add(1);
            if first == NA_INTEGER && second < 0 {
                return -(second as R_xlen_t);
            }
        }

        let mut rows = 0;
        for i in 0..XLENGTH(x) {
            let col = VECTOR_ELT(x, i);
            if !col.is_null() {
                rows = rows.max(XLENGTH(col));
            }
        }
        rows
    }
}

/// R's `matrix(data, nrow, ncol, byrow, dimnames)` — create a matrix.
pub unsafe fn do_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = arg_by_name_or_position(args, &["data"], 0);
        let nrow_arg = arg_by_name_or_position(args, &["nrow"], 1);
        let ncol_arg = arg_by_name_or_position(args, &["ncol"], 2);
        let byrow_arg = arg_by_name_or_position(args, &["byrow"], 3);
        let dimnames = arg_by_name_or_position(args, &["dimnames"], 4);

        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }

        let data_len = XLENGTH(data);
        let nrow = if nrow_arg.is_null() || nrow_arg == R_NilValue() {
            data_len
        } else {
            real_or_default(nrow_arg, data_len as f64) as R_xlen_t
        };
        let ncol = if ncol_arg.is_null() || ncol_arg == R_NilValue() {
            if nrow == 0 {
                0
            } else {
                (data_len + nrow - 1) / nrow
            }
        } else {
            real_or_default(ncol_arg, 1.0) as R_xlen_t
        };
        let byrow = if byrow_arg.is_null() || byrow_arg == R_NilValue() {
            false
        } else {
            TYPEOF(byrow_arg) == SEXPTYPE::LGLSXP && *LOGICAL(byrow_arg) != 0
        };

        let t = TYPEOF(data);
        if !supported_matrix_type(t) || nrow < 0 || ncol < 0 {
            return R_NilValue();
        }
        let result = Rf_allocVector3(t, nrow * ncol);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // R stores matrices in column-major order. If data has length zero, keep
        // the requested shape and fill with the type-appropriate missing value.
        for i in 0..(nrow * ncol) {
            if data_len == 0 {
                set_matrix_na_or_zero(result, i);
            } else {
                let src_idx = if byrow && ncol > 0 {
                    let row = i % nrow;
                    let col = i / nrow;
                    row * ncol + col
                } else {
                    i
                } % data_len;
                copy_matrix_element(result, i, data, src_idx);
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nrow as c_int;
            *INTEGER(dim).add(1) = ncol as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }

        if !dimnames.is_null() && dimnames != R_NilValue() {
            if !valid_array_dimnames(dimnames, dim) {
                std::panic::panic_any(RError {
                    message: "length of 'dimnames' not equal to array extent".to_string(),
                });
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }

        result
    }
}

/// R's `array(data, dim, dimnames)` — create an array with recycled data.
pub unsafe fn do_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = arg_by_name_or_position(args, &["data"], 0);
        let dim_arg = arg_by_name_or_position(args, &["dim"], 1);
        let dimnames = arg_by_name_or_position(args, &["dimnames"], 2);

        let data_missing = data.is_null() || data == R_NilValue();
        let data_type = if data_missing {
            SEXPTYPE::LGLSXP.as_c_int()
        } else {
            TYPEOF(data)
        };
        if !supported_matrix_type(data_type) {
            std::panic::panic_any(RError {
                message: "'data' must be a vector type".to_string(),
            });
        }

        let data_len = if data_missing { 1 } else { XLENGTH(data) };
        let dim = match array_dimension_attribute(dim_arg, data_len) {
            Ok(dim) => dim,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };
        let _dim_guard = protect(dim);
        // dim2total(dims, &err) with all checks (trunk array.c): errors on
        // length-0, NA, and negative dims; "too many elements specified"
        // once the product passes R_XLEN_T_MAX.
        let total_len = match crate::mainutils::array::dim2total(dim) {
            Ok(total) => total,
            Err(()) => std::panic::panic_any(RError {
                message: "too many elements specified".to_string(),
            }),
        };

        if !valid_array_dimnames(dimnames, dim) {
            std::panic::panic_any(RError {
                message: "length of 'dimnames' not equal to array extent".to_string(),
            });
        }

        let result = Rf_allocVector3(data_type, total_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..total_len {
            if data_missing || data_len == 0 {
                set_matrix_na_or_zero(result, i);
            } else {
                copy_matrix_element(result, i, data, i % data_len);
            }
        }

        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);
        if !dimnames.is_null() && dimnames != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }

        result
    }
}

/// Coerce the `dim=` argument of `array()` to an integer vector, as
/// upstream's `coerceVector(dims, INTSXP)`. The dim2total() checks
/// (length-0 / NA / negative / overflow) run in do_array on the coerced
/// vector, exactly as in trunk array.c.
pub unsafe fn array_dimension_attribute(value: SEXP, data_len: R_xlen_t) -> Result<SEXP, String> {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            if dim.is_null() {
                return Ok(R_NilValue());
            }
            *INTEGER(dim) = data_len as c_int;
            return Ok(dim);
        }

        if !is_atomic_vector_type(TYPEOF(value)) {
            return Err("'dim' must be a numeric vector".to_string());
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(value));
        if dim.is_null() {
            return Ok(R_NilValue());
        }
        let _dim_guard = protect(dim);
        for i in 0..XLENGTH(value) {
            *INTEGER(dim).add(i as usize) = dimension_component(value, i);
        }
        Ok(dim)
    }
}

pub unsafe fn valid_array_dimnames(dimnames: SEXP, dim: SEXP) -> bool {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return true;
        }
        if TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return false;
        }
        let dim_count = XLENGTH(dim);
        if XLENGTH(dimnames) > dim_count {
            return false;
        }
        for i in 0..XLENGTH(dimnames) {
            let names = VECTOR_ELT(dimnames, i);
            if names.is_null() || names == R_NilValue() {
                continue;
            }
            if XLENGTH(names) != *INTEGER(dim).add(i as usize) as R_xlen_t {
                return false;
            }
        }
        true
    }
}

pub unsafe fn retained_dimname(dimnames: SEXP, axis: R_xlen_t) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        if axis >= XLENGTH(dimnames) {
            return R_NilValue();
        }
        VECTOR_ELT(dimnames, axis)
    }
}

pub unsafe fn retained_dimnames(dimnames: SEXP, axes: &[R_xlen_t]) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, axes.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut has_names = false;
        for (out_i, axis) in axes.iter().enumerate() {
            let names = if *axis < XLENGTH(dimnames) {
                VECTOR_ELT(dimnames, *axis)
            } else {
                R_NilValue()
            };
            if !names.is_null() && names != R_NilValue() {
                has_names = true;
            }
            SET_VECTOR_ELT(result, out_i as R_xlen_t, names);
        }
        if has_names { result } else { R_NilValue() }
    }
}

pub unsafe fn dimension_component(value: SEXP, i: R_xlen_t) -> c_int {
    unsafe {
        let kind = TYPEOF(value);
        if kind == SEXPTYPE::INTSXP.as_c_int() || kind == SEXPTYPE::LGLSXP.as_c_int() {
            INTEGER_ELT(value, i as c_int)
        } else if kind == SEXPTYPE::REALSXP.as_c_int() {
            real_to_dimension(REAL_ELT(value, i as c_int))
        } else if kind == SEXPTYPE::CPLXSXP.as_c_int() {
            real_to_dimension((*COMPLEX(value).add(i as usize)).r)
        } else if kind == SEXPTYPE::STRSXP.as_c_int() {
            let elt = STRING_ELT(value, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                NA_INTEGER
            } else {
                let text = CStr::from_ptr(CHAR(elt)).to_string_lossy();
                text.trim()
                    .parse::<f64>()
                    .map(real_to_dimension)
                    .unwrap_or(NA_INTEGER)
            }
        } else if kind == SEXPTYPE::RAWSXP.as_c_int() {
            *RAW(value).add(i as usize) as c_int
        } else {
            NA_INTEGER
        }
    }
}

pub fn real_to_dimension(value: f64) -> c_int {
    if !value.is_finite() || value < c_int::MIN as f64 || value > c_int::MAX as f64 {
        return NA_INTEGER;
    }
    value.trunc() as c_int
}

pub fn is_atomic_vector_type(kind: c_int) -> bool {
    kind == SEXPTYPE::LGLSXP.as_c_int()
        || kind == SEXPTYPE::INTSXP.as_c_int()
        || kind == SEXPTYPE::REALSXP.as_c_int()
        || kind == SEXPTYPE::CPLXSXP.as_c_int()
        || kind == SEXPTYPE::STRSXP.as_c_int()
        || kind == SEXPTYPE::RAWSXP.as_c_int()
}

/// R's `diag(x)` — extract diagonal or create diagonal matrix.
pub unsafe fn do_diag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        // Check if x is a matrix
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            // Extract diagonal
            let nrow = *INTEGER(dim_attr) as usize;
            let ncol = *INTEGER(dim_attr).add(1) as usize;
            let n = nrow.min(ncol);
            if !supported_matrix_type(TYPEOF(x)) {
                return R_NilValue();
            }
            let result = Rf_allocVector3(TYPEOF(x), n as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..n {
                let src = i + i * nrow;
                copy_matrix_element(result, i as R_xlen_t, x, src as R_xlen_t);
            }
            result
        } else {
            if XLENGTH(x) == 1 {
                let n = real_or_default(x, 0.0).max(0.0) as usize;
                let t = TYPEOF(x);
                if !supported_matrix_type(t) {
                    return R_NilValue();
                }
                let result = Rf_allocVector3(t, (n * n) as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let _result_guard = protect(result);

                for i in 0..n * n {
                    set_matrix_zero(result, i as R_xlen_t);
                }
                for i in 0..n {
                    let dst = i + i * n;
                    set_diagonal_identity_value(result, dst as R_xlen_t);
                }

                let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                if !dim.is_null() {
                    *INTEGER(dim) = n as c_int;
                    *INTEGER(dim).add(1) = n as c_int;
                    crate::sexp::attrib_core::setAttrib(
                        result,
                        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                        dim,
                    );
                }
                return result;
            }

            // Create diagonal matrix from vector
            let n = XLENGTH(x) as usize;
            let t = TYPEOF(x);
            if !supported_matrix_type(t) {
                return R_NilValue();
            }
            let result = Rf_allocVector3(t, (n * n) as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);

            for i in 0..n * n {
                set_matrix_zero(result, i as R_xlen_t);
            }
            for i in 0..n {
                let dst = i + i * n;
                copy_matrix_element(result, dst as R_xlen_t, x, i as R_xlen_t);
            }

            // Set dim
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                *INTEGER(dim) = n as c_int;
                *INTEGER(dim).add(1) = n as c_int;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
            result
        }
    }
}
// ---------------------------------------------------------------------------
// Complete S3 generics — as.matrix, as.numeric
// ---------------------------------------------------------------------------

/// R's `as.matrix(x)` — convert to matrix (simplified).
/// For vectors, wraps as a single-column matrix.
/// For lists/data.frames, wraps as a matrix.
pub unsafe fn do_as_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            // Simple vector — copy and set dim attribute
            let n = XLENGTH(x);
            let result = Rf_allocVector3(t, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            if t == SEXPTYPE::REALSXP {
                let src = REAL(x);
                let dst = REAL(result);
                for i in 0..n {
                    *dst.add(i as usize) = *src.add(i as usize);
                }
            } else {
                let src = INTEGER(x);
                let dst = INTEGER(result);
                for i in 0..n {
                    *dst.add(i as usize) = *src.add(i as usize);
                }
            }
            // Set dim = c(n, 1)
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let _p2 = protect(dim);
                let d = INTEGER(dim);
                *d.add(0) = n as i32;
                *d.add(1) = 1;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
            result
        } else if t == SEXPTYPE::STRSXP {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            // Copy string elements
            for i in 0..n {
                let charsxp = STRING_ELT(x, i);
                if !charsxp.is_null() {
                    SET_STRING_ELT(result, i, charsxp);
                }
            }
            // Set dim = c(n, 1)
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let _p2 = protect(dim);
                let d = INTEGER(dim);
                *d.add(0) = n as i32;
                *d.add(1) = 1;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
            result
        } else {
            // For other types, return as-is
            x
        }
    }
}

/// R's `as.numeric(x)` — alias for as.double.
pub unsafe fn do_as_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Delegate to do_as_double
        do_as_double(_call, _op, args, _rho)
    }
}
