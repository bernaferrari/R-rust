//! Essentials domain module `matrix` — extracted verbatim from essentials.rs.

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

fn supported_matrix_type(t: c_int) -> bool {
    t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::VECSXP
}

unsafe fn set_matrix_na_or_zero(x: SEXP, i: R_xlen_t) {
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
unsafe fn set_matrix_zero(x: SEXP, i: R_xlen_t) {
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

unsafe fn set_diagonal_identity_value(x: SEXP, i: R_xlen_t) {
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

unsafe fn string_vector_contains_value(x: SEXP, needle: &str) -> bool {
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

/// R's `drop(x)` — remove extent-one dimensions from arrays and matrices.
pub unsafe fn do_drop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            return x;
        }

        let dim_count = XLENGTH(dim);
        let mut kept_axes = Vec::new();
        for i in 0..dim_count {
            if *INTEGER(dim).add(i as usize) != 1 {
                kept_axes.push(i);
            }
        }
        if kept_axes.len() == dim_count as usize {
            return x;
        }

        let result = crate::mainutils::duplicate::shallow_duplicate(x);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let _result_guard = protect(result);

        let dimnames =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());

        match kept_axes.len() {
            0 => {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
            1 => {
                let names = retained_dimname(dimnames, kept_axes[0]);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            _ => {
                let new_dim = Rf_allocVector3(SEXPTYPE::INTSXP, kept_axes.len() as R_xlen_t);
                if new_dim.is_null() {
                    return R_NilValue();
                }
                let _dim_guard = protect(new_dim);
                for (out_i, axis) in kept_axes.iter().enumerate() {
                    *INTEGER(new_dim).add(out_i) = *INTEGER(dim).add(*axis as usize);
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    new_dim,
                );

                let new_dimnames = retained_dimnames(dimnames, &kept_axes);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    new_dimnames,
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
        }

        result
    }
}

/// R's `t(x)` — transpose a matrix.
///
/// Ported from R's `do_transpose` in array.c (line 1569). Plain vectors
/// (dim of length 0 or 1) transpose as column vectors; dimnames are swapped
/// (including their names), and all other attributes are copied via
/// copyMostAttrib.
pub unsafe fn do_transpose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(x);
        if !supported_matrix_type(t) {
            not_matrix();
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(x, R_DimSymbol());
        let ldim = if !dim_attr.is_null()
            && dim_attr != R_NilValue()
            && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
        {
            LENGTH(dim_attr) as R_xlen_t
        } else {
            0
        };

        // (nrow, ncol, rnames, cnames, names(dimnames), whether to attach
        // dimnames at all) as in the C ldim switch.
        let (nrow, ncol, rnames, cnames, dimnames_names, have_dimnames) = match ldim {
            0 => {
                let rnames = crate::sexp::attrib_core::getAttrib(x, R_NamesSymbol());
                let have = !rnames.is_null() && rnames != R_NilValue();
                (XLENGTH(x), 1, rnames, R_NilValue(), R_NilValue(), have)
            }
            1 | 2 => {
                let dimnames = crate::sexp::attrib_core::getAttrib(x, R_DimNamesSymbol());
                let have = !dimnames.is_null() && dimnames != R_NilValue();
                if !have {
                    if ldim == 2 {
                        (
                            *INTEGER(dim_attr) as R_xlen_t,
                            *INTEGER(dim_attr).add(1) as R_xlen_t,
                            R_NilValue(),
                            R_NilValue(),
                            R_NilValue(),
                            false,
                        )
                    } else {
                        (XLENGTH(x), 1, R_NilValue(), R_NilValue(), R_NilValue(), false)
                    }
                } else {
                    let (nrow, ncol, rnames, cnames) = if ldim == 2 {
                        (
                            *INTEGER(dim_attr) as R_xlen_t,
                            *INTEGER(dim_attr).add(1) as R_xlen_t,
                            VECTOR_ELT(dimnames, 0),
                            VECTOR_ELT(dimnames, 1),
                        )
                    } else {
                        (
                            XLENGTH(x),
                            1,
                            VECTOR_ELT(dimnames, 0),
                            R_NilValue(),
                        )
                    };
                    let names = crate::sexp::attrib_core::getAttrib(dimnames, R_NamesSymbol());
                    (nrow, ncol, rnames, cnames, names, true)
                }
            }
            _ => not_matrix(),
        };

        let len = nrow * ncol;
        let result = Rf_allocVector3(t, len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // Source and destination are both column-major:
        // src(row, col) = row + col * nrow
        // dst(col, row) = col + row * ncol
        for row in 0..nrow {
            for col in 0..ncol {
                let src = row + col * nrow;
                let dst = col + row * ncol;
                copy_matrix_element(result, dst, x, src);
            }
        }

        // Set transposed dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = ncol as c_int;
            *INTEGER(dim).add(1) = nrow as c_int;
            crate::sexp::attrib_core::setAttrib(result, R_DimSymbol(), dim);
        }

        if have_dimnames {
            let new_dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if !new_dimnames.is_null() {
                let _dimnames_guard = protect(new_dimnames);
                SET_VECTOR_ELT(new_dimnames, 0, cnames);
                SET_VECTOR_ELT(new_dimnames, 1, rnames);
                if !dimnames_names.is_null() && dimnames_names != R_NilValue() {
                    let new_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
                    if !new_names.is_null() {
                        let _names_guard = protect(new_names);
                        SET_STRING_ELT(new_names, 1, STRING_ELT(dimnames_names, 0));
                        SET_STRING_ELT(
                            new_names,
                            0,
                            if ldim == 2 {
                                STRING_ELT(dimnames_names, 1)
                            } else {
                                // R_BlankString
                                Rf_mkChar(c"".as_ptr())
                            },
                        );
                        crate::sexp::attrib_core::setAttrib(
                            new_dimnames,
                            R_NamesSymbol(),
                            new_names,
                        );
                    }
                }
                crate::sexp::attrib_core::setAttrib(result, R_DimNamesSymbol(), new_dimnames);
            }
        }

        crate::mainutils::array::copyMostAttrib(x, result);
        result
    }
}

fn not_matrix() -> ! {
    std::panic::panic_any(RError {
        message: "argument is not a matrix".to_string(),
    });
}

/// R's `nrow(x)` — number of rows.
pub unsafe fn do_nrow(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_object(x) {
            return Rf_ScalarInteger(data_frame_row_count(x) as c_int);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
            Rf_ScalarInteger(*INTEGER(dim_attr))
        } else {
            Rf_ScalarInteger(XLENGTH(x) as c_int)
        }
    }
}

/// R's `ncol(x)` — number of columns.
pub unsafe fn do_ncol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_object(x) {
            return Rf_ScalarInteger(XLENGTH(x) as c_int);
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

/// R's `dim(x)` — dimensions as integer vector.
pub unsafe fn do_dim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if is_data_frame_object(x) {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if dim.is_null() {
                return R_NilValue();
            }
            *INTEGER(dim) = data_frame_row_count(x) as c_int;
            *INTEGER(dim).add(1) = XLENGTH(x) as c_int;
            return dim;
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() {
            dim_attr
        } else {
            R_NilValue()
        }
    }
}

/// R's `tsp(x)` — time-series parameter attribute.
pub unsafe fn do_tsp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_TspSymbol())
    }
}

/// R's `dim(x) <- value` — replace an object's dimension attribute.
pub unsafe fn do_dim_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_DimSymbol(),
                R_NilValue(),
            );
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                R_NilValue(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let dim = match dimension_attribute(value, XLENGTH(x)) {
            Ok(dim) => dim,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };

        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `tsp(x) <- value` — replace or remove time-series parameters.
pub unsafe fn do_tsp_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_TspSymbol(),
                R_NilValue(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let tsp = match tsp_attribute(value) {
            Ok(tsp) => tsp,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_TspSymbol(), tsp);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

unsafe fn tsp_attribute(value: SEXP) -> Result<SEXP, String> {
    unsafe {
        if XLENGTH(value) != 3
            || (TYPEOF(value) != SEXPTYPE::INTSXP && TYPEOF(value) != SEXPTYPE::REALSXP)
        {
            return Err("'tsp' attribute must be numeric of length three".to_string());
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        if result.is_null() {
            return Ok(R_NilValue());
        }
        let _result_guard = protect(result);
        for i in 0..3 {
            *REAL(result).add(i) = if TYPEOF(value) == SEXPTYPE::INTSXP {
                let n = INTEGER_ELT(value, i as c_int);
                if n == NA_INTEGER { NA_REAL } else { n as f64 }
            } else {
                REAL_ELT(value, i as c_int)
            };
        }

        let start = *REAL(result);
        let end = *REAL(result).add(1);
        let frequency = *REAL(result).add(2);
        if frequency.is_infinite() || start.is_infinite() || end.is_infinite() {
            return Err("invalid time series parameters specified (1)".to_string());
        }
        if !frequency.is_nan() && frequency <= 0.0 {
            return Err("invalid time series parameters specified (0)".to_string());
        }
        if start.is_finite() && end.is_finite() && frequency.is_finite() && end < start {
            return Err("invalid time series parameters specified (1)".to_string());
        }
        Ok(result)
    }
}

unsafe fn dimension_attribute(value: SEXP, object_len: R_xlen_t) -> Result<SEXP, String> {
    unsafe {
        if !is_atomic_vector_type(TYPEOF(value)) {
            return Err("invalid second argument, must be vector or NULL".to_string());
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(value));
        let _dim_guard = protect(dim);
        for i in 0..XLENGTH(value) {
            *INTEGER(dim).add(i as usize) = dimension_component(value, i);
        }

        // dimgets(): dims are validated and totaled by array.c's dim2total()
        // (trunk attrib.c delegates there — it errors on length-0/missing/
        // negative dims itself).
        let total = crate::mainutils::array::dim2total(dim)
            .map_err(|()| "too many elements specified".to_string())?;

        if total != object_len {
            return Err(format!(
                "dims [product {total}] do not match the length of object [{object_len}]"
            ));
        }

        Ok(dim)
    }
}

/// Coerce the `dim=` argument of `array()` to an integer vector, as
/// upstream's `coerceVector(dims, INTSXP)`. The dim2total() checks
/// (length-0 / NA / negative / overflow) run in do_array on the coerced
/// vector, exactly as in trunk array.c.
unsafe fn array_dimension_attribute(value: SEXP, data_len: R_xlen_t) -> Result<SEXP, String> {
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

unsafe fn valid_array_dimnames(dimnames: SEXP, dim: SEXP) -> bool {
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

unsafe fn retained_dimname(dimnames: SEXP, axis: R_xlen_t) -> SEXP {
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

unsafe fn retained_dimnames(dimnames: SEXP, axes: &[R_xlen_t]) -> SEXP {
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

unsafe fn dimension_component(value: SEXP, i: R_xlen_t) -> c_int {
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

fn real_to_dimension(value: f64) -> c_int {
    if !value.is_finite() || value < c_int::MIN as f64 || value > c_int::MAX as f64 {
        return NA_INTEGER;
    }
    value.trunc() as c_int
}

fn is_atomic_vector_type(kind: c_int) -> bool {
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

unsafe fn resize_vector(x: SEXP, new_len: R_xlen_t) -> SEXP {
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

unsafe fn resize_names(x: SEXP, result: SEXP, new_len: R_xlen_t) {
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

unsafe fn length_replacement_size(value: SEXP) -> Option<R_xlen_t> {
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

/// R's `storage.mode(x) <- value` — coerce storage while preserving attributes.
pub unsafe fn do_storage_mode_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        let target_type = match storage_mode_target(value) {
            Ok(target_type) => target_type,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };

        if TYPEOF(x) == target_type {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        if inherits_class(x, "factor") {
            std::panic::panic_any(RError {
                message: "invalid to change the storage mode of a factor".to_string(),
            });
        }

        let result = crate::mainutils::coerce::coerceVector(x, target_type);
        let _result_guard = protect(result);
        crate::sexp::accessors::SET_ATTRIB(result, ATTRIB(x));
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn storage_mode_target(value: SEXP) -> Result<c_int, String> {
    unsafe {
        if value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || XLENGTH(value) < 1
            || is_string_na(value, 0)
        {
            return Err("'value' must be non-null character string".to_string());
        }

        let mode = elt_to_string(value, 0);
        match mode.as_str() {
            "logical" => Ok(SEXPTYPE::LGLSXP.as_c_int()),
            "integer" => Ok(SEXPTYPE::INTSXP.as_c_int()),
            "double" => Ok(SEXPTYPE::REALSXP.as_c_int()),
            "complex" => Ok(SEXPTYPE::CPLXSXP.as_c_int()),
            "character" => Ok(SEXPTYPE::STRSXP.as_c_int()),
            "raw" => Ok(SEXPTYPE::RAWSXP.as_c_int()),
            "list" => Ok(SEXPTYPE::VECSXP.as_c_int()),
            "expression" => Ok(SEXPTYPE::EXPRSXP.as_c_int()),
            "real" => Err("use of 'real' is defunct: use 'double' instead".to_string()),
            "single" => Err("use of 'single' is defunct: use mode<- instead".to_string()),
            _ => Err("invalid value".to_string()),
        }
    }
}

/// R's `rownames(x)` — get row names attribute.
pub unsafe fn do_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 1 {
            return VECTOR_ELT(dimnames, 0);
        }
        if is_data_frame_like(x) {
            return string_vector(&data_frame_row_names(x));
        }
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `colnames(x)` — get column names attribute.
pub unsafe fn do_colnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 2 {
            VECTOR_ELT(dimnames, 1)
        } else {
            R_NilValue()
        }
    }
}

/// R's `names(x)` — get names attribute (alias for do_names).
pub unsafe fn do_names_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_names(_call, _op, args, _rho) }
}

/// R's `names(x) <- value` — set names attribute.
pub unsafe fn do_names_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `dimnames(x) <- value` — set matrix/array dimension names.
pub unsafe fn do_dimnames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `rownames(x) <- value` — set matrix row names through dimnames[[1]].
pub unsafe fn do_rownames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { set_matrix_dimname(args, 0) }
}

/// R's `colnames(x) <- value` — set matrix column names through dimnames[[2]].
pub unsafe fn do_colnames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { set_matrix_dimname(args, 1) }
}

unsafe fn set_matrix_dimname(args: SEXP, axis: i64) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let dimnames_sym = Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr());
        let mut dimnames = crate::sexp::attrib_core::getAttrib(x, dimnames_sym);
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if dimnames.is_null() {
                return x;
            }
            let _dimnames_guard = protect(dimnames);
            SET_VECTOR_ELT(dimnames, 0, R_NilValue());
            SET_VECTOR_ELT(dimnames, 1, R_NilValue());
            crate::sexp::attrib_core::setAttrib(x, dimnames_sym, dimnames);
        }

        if LENGTH(dimnames) > axis as i32 {
            SET_VECTOR_ELT(dimnames, axis, value);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `class(x)` — get class attribute.
pub unsafe fn do_class_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if class.is_null() || class == R_NilValue() {
            let dim =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
            if !dim.is_null() && dim != R_NilValue() && TYPEOF(dim) == SEXPTYPE::INTSXP {
                if XLENGTH(dim) == 2 {
                    let result = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
                    if result.is_null() {
                        return result;
                    }
                    let _result_guard = protect(result);
                    SET_STRING_ELT(result, 0, Rf_mkChar(c"matrix".as_ptr()));
                    SET_STRING_ELT(result, 1, Rf_mkChar(c"array".as_ptr()));
                    return result;
                }
                return Rf_mkString(c"array".as_ptr());
            }
            let t = TYPEOF(x);
            let name = if t == SEXPTYPE::REALSXP {
                "numeric"
            } else if t == SEXPTYPE::INTSXP {
                "integer"
            } else if t == SEXPTYPE::LGLSXP {
                "logical"
            } else if t == SEXPTYPE::STRSXP {
                "character"
            } else if t == SEXPTYPE::VECSXP {
                "list"
            } else {
                "NULL"
            };
            let cstr = CString::new(name).unwrap_or_default();
            Rf_mkString(cstr.as_ptr())
        } else {
            class
        }
    }
}

/// R's `.class2(x)` — class vector including implicit primitive inheritance.
pub unsafe fn do_class2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"NULL".as_ptr());
        }

        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && class != R_NilValue() {
            return class;
        }

        let implicit: &[&std::ffi::CStr] = match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP => &[c"integer", c"numeric"],
            t if t == SEXPTYPE::REALSXP => &[c"numeric"],
            t if t == SEXPTYPE::LGLSXP => &[c"logical"],
            t if t == SEXPTYPE::CPLXSXP => &[c"complex"],
            t if t == SEXPTYPE::STRSXP => &[c"character"],
            t if t == SEXPTYPE::RAWSXP => &[c"raw"],
            t if t == SEXPTYPE::VECSXP => &[c"list"],
            t if t == SEXPTYPE::LANGSXP => &[c"call"],
            _ => &[c"NULL"],
        };

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, implicit.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for (i, name) in implicit.iter().enumerate() {
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(name.as_ptr()));
        }
        result
    }
}

/// R's `class(x) <- value` — set class attribute.
pub unsafe fn do_class_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `oldClass(x)` — direct S3 class attribute without implicit defaults.
pub unsafe fn do_oldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol())
    }
}

/// R's `oldClass(x) <- value` — set or remove the direct S3 class attribute.
pub unsafe fn do_oldClass_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// Attribute access helpers
// ---------------------------------------------------------------------------

/// R's `attr(x, which)` — get arbitrary attribute by name.
pub unsafe fn do_attr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `attr(x, which) <- value` — set or remove a single attribute.
pub unsafe fn do_attr_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `attributes(x) <- value` — replace all attributes from a named list.
pub unsafe fn do_attributes_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        crate::sexp::accessors::SET_ATTRIB(x, R_NilValue());
        if value.is_null() || value == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        if TYPEOF(value) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "attributes must be a list or NULL".to_string(),
            });
        }

        let names =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            std::panic::panic_any(RError {
                message: "attributes must be named".to_string(),
            });
        }

        for i in (0..XLENGTH(value)).rev() {
            let name_elt = STRING_ELT(names, i);
            if name_elt.is_null() || name_elt == crate::sexp::globals::R_NaString() {
                continue;
            }
            let name = CHAR(name_elt);
            if name.is_null() || CStr::from_ptr(name).to_bytes().is_empty() {
                continue;
            }
            crate::sexp::attrib_core::setAttrib(x, Rf_install(name), VECTOR_ELT(value, i));
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `comment(x)` — get the comment attribute.
pub unsafe fn do_comment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, comment_symbol())
    }
}

/// R's `comment(x) <- value` — set the comment attribute.
pub unsafe fn do_comment_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(x, comment_symbol(), value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

unsafe fn comment_symbol() -> SEXP {
    unsafe { Rf_install(CString::new("comment").unwrap_or_default().as_ptr()) }
}

/// R's namespace lookup operators, `pkg::name` and `pkg:::name`.
pub unsafe fn do_namespace_get(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package = CAR(args);
        let name = CAR(CDR(args));
        if package.is_null() || name.is_null() || package == R_NilValue() || name == R_NilValue() {
            return R_NilValue();
        }

        let package_name = if TYPEOF(package) == SEXPTYPE::SYMSXP {
            let pname = PRINTNAME(package);
            if pname.is_null() {
                String::new()
            } else {
                CStr::from_ptr(CHAR(pname)).to_string_lossy().into_owned()
            }
        } else {
            elt_to_string(package, 0)
        };

        if TYPEOF(name) != SEXPTYPE::SYMSXP {
            std::panic::panic_any(RError {
                message: "namespace lookup requires a name".to_string(),
            });
        }
        let lookup_name = symbol_name(name).unwrap_or_default();

        if package_name == "tools" {
            if lookup_name == "langElts" {
                let values = crate::sexp::init::LANGUAGE_ELEMENTS;
                let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let _result_guard = protect(result);
                for (i, value) in values.iter().enumerate() {
                    let c_value =
                        CString::new(*value).expect("static language element has no NUL byte");
                    SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(c_value.as_ptr()));
                }
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
                return result;
            }
            std::panic::panic_any(RError {
                message: format!("object '{lookup_name}' not found in tools namespace"),
            });
        }

        if package_name != "base" {
            let namespace = match load_package_namespace_by_name(&package_name) {
                Ok(env) => env,
                Err(message) => {
                    std::panic::panic_any(RError { message });
                }
            };
            let private_lookup = symbol_name(CAR(call)).as_deref() == Some(":::")
                || crate::eval::builtin::PRIMNAME(op) == ":::";
            if !private_lookup {
                let package_path = find_package_path(&package_name);
                let directives = read_namespace_directives(Path::new(&package_path))
                    .ok()
                    .flatten();
                let exports = namespace_exports(directives.as_ref(), namespace);
                if !exports.iter().any(|export| export == &lookup_name) {
                    std::panic::panic_any(RError {
                        message: format!(
                            "'{lookup_name}' is not an exported object from namespace '{package_name}'"
                        ),
                    });
                }
            }
            let value = crate::sexp::envir::R_findVarInFrame(namespace, name);
            if value == crate::sexp::globals::R_UnboundValue() {
                std::panic::panic_any(RError {
                    message: format!(
                        "object '{lookup_name}' not found in namespace '{package_name}'"
                    ),
                });
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
            return value;
        }

        let value = crate::sexp::envir::R_findVar(name, crate::sexp::globals::R_BaseEnv());
        if value == crate::sexp::globals::R_UnboundValue() {
            std::panic::panic_any(RError {
                message: format!("object '{lookup_name}' not found in base namespace"),
            });
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        value
    }
}

/// R's `attributes(x)` — return attributes as a named list.
pub unsafe fn do_attributes(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let attrs = ATTRIB(x);
        if attrs.is_null() || attrs == R_NilValue() {
            return R_NilValue();
        }

        let mut count = 0;
        let mut current = attrs;
        while !current.is_null() && current != R_NilValue() {
            count += 1;
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, count);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, count);
        if names.is_null() {
            return R_NilValue();
        }
        let _names_guard = protect(names);

        current = attrs;
        let mut i = 0;
        while !current.is_null() && current != R_NilValue() {
            SET_VECTOR_ELT(result, i, CAR(current));
            let name = tag_name(current).unwrap_or_default();
            SET_STRING_ELT(
                names,
                i,
                Rf_mkChar(CString::new(name).unwrap_or_default().as_ptr()),
            );
            i += 1;
            current = CDR(current);
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

fn structure_attr_name(name: &str) -> &str {
    match name {
        ".Dim" => "dim",
        ".Dimnames" => "dimnames",
        ".Names" => "names",
        ".Tsp" => "tsp",
        ".Label" => "levels",
        other => other,
    }
}

/// R's `structure(.Data, ...)` — attach attributes to an object.
pub unsafe fn do_structure(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let attr_name = structure_attr_name(&name);
                crate::sexp::attrib_core::setAttrib(
                    x,
                    Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
                    CAR(current),
                );
            }
            current = CDR(current);
        }

        x
    }
}

/// R's `attr(x, which) <- value` — set arbitrary attribute by name.
pub unsafe fn do_setattr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

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

unsafe fn do_array_margin_summary(args: SEXP, rows: bool, mean: bool) -> SEXP {
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

unsafe fn margin_summary_dims(dims_arg: SEXP, dim_len: R_xlen_t, rows: bool) -> R_xlen_t {
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

unsafe fn summarize_contiguous_cells(
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

unsafe fn summarize_margin_cells(
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

unsafe fn numeric_margin_value(x: SEXP, index: R_xlen_t) -> Option<f64> {
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

unsafe fn set_margin_summary_attrs(result: SEXP, dim: SEXP, axes: &[R_xlen_t], source: SEXP) {
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

// ---------------------------------------------------------------------------
// Complete R runtime — cbind, rbind, t (transpose), and other critical functions
// ---------------------------------------------------------------------------

/// R's `cbind(...)` — combine vectors/matrices by columns.
pub unsafe fn do_cbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut result_type = SEXPTYPE::LGLSXP;
        let mut col_names = Vec::new();
        let mut has_col_names = false;
        let mut entries = Vec::new();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                result_type = bind_common_type(result_type, SEXPTYPE(TYPEOF(arg)));
                let (arg_nrow, arg_ncol) = bind_dims(arg, true);
                let name = tag_name(current).unwrap_or_default();
                if !name.is_empty() {
                    has_col_names = true;
                }
                entries.push((arg, arg_nrow, arg_ncol, name));
            }
            current = CDR(current);
        }

        let has_nonzero_extent = entries
            .iter()
            .any(|&(_, nrow, ncol, _)| nrow > 0 && ncol > 0);
        let mut ncols: R_xlen_t = 0;
        let mut nrows: R_xlen_t = 0;
        for &(_, arg_nrow, arg_ncol, ref name) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            nrows = nrows.max(arg_nrow);
            ncols += arg_ncol;
            for j in 0..arg_ncol {
                if arg_ncol == 1 {
                    col_names.push(name.clone());
                } else if name.is_empty() {
                    col_names.push(String::new());
                } else {
                    col_names.push(format!("{name}.{j_plus}", j_plus = j + 1));
                }
            }
        }

        if nrows == 0 || ncols == 0 {
            let result = Rf_allocVector3(result_type, 0);
            if result.is_null() {
                return R_NilValue();
            }
            set_two_dim_attr(result, nrows, ncols);
            if has_col_names {
                set_bind_dimnames(result, R_NilValue(), string_vector(&col_names));
            }
            return result;
        }

        let total = nrows * ncols;
        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut col_offset: R_xlen_t = 0;
        for &(arg, arg_nrow, arg_ncol, _) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            let arg_len = XLENGTH(arg);
            if arg_len == 0 {
                continue;
            }

            for j in 0..arg_ncol {
                for i in 0..nrows {
                    let src_idx = ((j * arg_nrow + (i % arg_nrow)) % arg_len) as R_xlen_t;
                    let dst_idx = ((col_offset + j) * nrows + i) as R_xlen_t;
                    copy_bind_value(result, dst_idx, result_type, arg, src_idx);
                }
            }
            col_offset += arg_ncol;
        }

        set_two_dim_attr(result, nrows, ncols);
        if has_col_names {
            set_bind_dimnames(result, R_NilValue(), string_vector(&col_names));
        }
        result
    }
}

/// R's `rbind(...)` — combine vectors/matrices by rows.
pub unsafe fn do_rbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut result_type = SEXPTYPE::LGLSXP;
        let mut row_names = Vec::new();
        let mut has_row_names = false;
        let mut entries = Vec::new();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                result_type = bind_common_type(result_type, SEXPTYPE(TYPEOF(arg)));
                let (arg_nrow, arg_ncol) = bind_dims(arg, false);
                let name = tag_name(current).unwrap_or_default();
                if !name.is_empty() {
                    has_row_names = true;
                }
                entries.push((arg, arg_nrow, arg_ncol, name));
            }
            current = CDR(current);
        }

        let has_nonzero_extent = entries
            .iter()
            .any(|&(_, nrow, ncol, _)| nrow > 0 && ncol > 0);
        let mut ncols: R_xlen_t = 0;
        let mut nrows: R_xlen_t = 0;
        for &(_, arg_nrow, arg_ncol, ref name) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            ncols = ncols.max(arg_ncol);
            nrows += arg_nrow;
            for i in 0..arg_nrow {
                if arg_nrow == 1 {
                    row_names.push(name.clone());
                } else if name.is_empty() {
                    row_names.push(String::new());
                } else {
                    row_names.push(format!("{name}.{i_plus}", i_plus = i + 1));
                }
            }
        }

        if nrows == 0 || ncols == 0 {
            let result = Rf_allocVector3(result_type, 0);
            if result.is_null() {
                return R_NilValue();
            }
            set_two_dim_attr(result, nrows, ncols);
            if has_row_names {
                set_bind_dimnames(result, string_vector(&row_names), R_NilValue());
            }
            return result;
        }

        let total = nrows * ncols;
        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut row_offset: R_xlen_t = 0;
        for &(arg, arg_nrow, arg_ncol, _) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            let arg_len = XLENGTH(arg);
            if arg_len == 0 {
                continue;
            }

            for j in 0..ncols {
                for i in 0..arg_nrow {
                    let src_idx = ((j * arg_nrow + i) % arg_len) as R_xlen_t;
                    let dst_idx = (j * nrows + row_offset + i) as R_xlen_t;
                    copy_bind_value(result, dst_idx, result_type, arg, src_idx);
                }
            }
            row_offset += arg_nrow;
        }

        set_two_dim_attr(result, nrows, ncols);
        if has_row_names {
            set_bind_dimnames(result, string_vector(&row_names), R_NilValue());
        }
        result
    }
}

fn bind_common_type(left: SEXPTYPE, right: SEXPTYPE) -> SEXPTYPE {
    if left == SEXPTYPE::STRSXP || right == SEXPTYPE::STRSXP {
        SEXPTYPE::STRSXP
    } else if left == SEXPTYPE::REALSXP || right == SEXPTYPE::REALSXP {
        SEXPTYPE::REALSXP
    } else if left == SEXPTYPE::INTSXP || right == SEXPTYPE::INTSXP {
        SEXPTYPE::INTSXP
    } else {
        left
    }
}

unsafe fn bind_dims(arg: SEXP, cbind: bool) -> (R_xlen_t, R_xlen_t) {
    unsafe {
        let dim_attr =
            crate::sexp::attrib_core::getAttrib(arg, crate::sexp::attrib_core::R_DimSymbol());
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr).add(1) as R_xlen_t,
            )
        } else if cbind {
            (XLENGTH(arg), 1)
        } else {
            (1, XLENGTH(arg))
        }
    }
}

unsafe fn copy_bind_value(
    dst: SEXP,
    dst_i: R_xlen_t,
    dst_type: SEXPTYPE,
    src: SEXP,
    src_i: R_xlen_t,
) {
    unsafe {
        match dst_type {
            SEXPTYPE::STRSXP => {
                if TYPEOF(src) == SEXPTYPE::STRSXP
                    && STRING_ELT(src, src_i) == crate::sexp::globals::R_NaString()
                {
                    SET_STRING_ELT(dst, dst_i, crate::sexp::globals::R_NaString());
                } else {
                    let value = elt_to_string(src, src_i);
                    let c_value = CString::new(value).unwrap_or_default();
                    SET_STRING_ELT(dst, dst_i, Rf_mkChar(c_value.as_ptr()));
                }
            }
            SEXPTYPE::REALSXP => {
                let value = match SEXPTYPE(TYPEOF(src)) {
                    SEXPTYPE::REALSXP => REAL_ELT(src, src_i as c_int),
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                        let value = INTEGER_ELT(src, src_i as c_int);
                        if value == NA_INTEGER {
                            NA_REAL
                        } else {
                            value as f64
                        }
                    }
                    _ => NA_REAL,
                };
                *REAL(dst).add(dst_i as usize) = value;
            }
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                let value = match SEXPTYPE(TYPEOF(src)) {
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => INTEGER_ELT(src, src_i as c_int),
                    _ => NA_INTEGER,
                };
                *INTEGER(dst).add(dst_i as usize) = value;
            }
            _ => {}
        }
    }
}

unsafe fn set_bind_dimnames(result: SEXP, row_names: SEXP, col_names: SEXP) {
    unsafe {
        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dimnames.is_null() {
            return;
        }
        let _dimnames_guard = protect(dimnames);
        SET_VECTOR_ELT(dimnames, 0, row_names);
        SET_VECTOR_ELT(dimnames, 1, col_names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );
    }
}
/// R's `var(x, y = NULL, na.rm = FALSE)` — variance or covariance.
pub unsafe fn do_var(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let y = CAR(CDR(args));
        let na_rm_arg = CAR(CDR(CDR(args)));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        if y.is_null() || y == R_NilValue() {
            // Variance of x
            let n = XLENGTH(x);
            let t = TYPEOF(x);
            let mut sum = 0.0f64;
            let mut sum_sq = 0.0f64;
            let mut count = 0i64;

            for i in 0..n {
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                    if !na_rm {
                        return Rf_ScalarReal(NA_REAL);
                    }
                } else {
                    sum += val;
                    sum_sq += val * val;
                    count += 1;
                }
            }

            if count < 2 {
                return Rf_ScalarReal(NA_REAL);
            }

            let mean = sum / count as f64;
            let variance = (sum_sq - count as f64 * mean * mean) / (count - 1) as f64;
            Rf_ScalarReal(variance)
        } else {
            // Covariance of x and y
            let n = XLENGTH(x).min(XLENGTH(y));
            let tx = TYPEOF(x);
            let ty = TYPEOF(y);
            let mut sum_x = 0.0f64;
            let mut sum_y = 0.0f64;
            let mut sum_xy = 0.0f64;
            let mut count = 0i64;

            for i in 0..n {
                let val_x = if tx == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                let val_y = if ty == SEXPTYPE::REALSXP {
                    *REAL(y).add(i as usize)
                } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(y).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                    || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                    || val_x.is_nan()
                    || val_y.is_nan()
                {
                    if !na_rm {
                        return Rf_ScalarReal(NA_REAL);
                    }
                } else {
                    sum_x += val_x;
                    sum_y += val_y;
                    sum_xy += val_x * val_y;
                    count += 1;
                }
            }

            if count < 2 {
                return Rf_ScalarReal(NA_REAL);
            }

            let mean_x = sum_x / count as f64;
            let mean_y = sum_y / count as f64;
            let covariance = (sum_xy - count as f64 * mean_x * mean_y) / (count - 1) as f64;
            Rf_ScalarReal(covariance)
        }
    }
}

/// R's `sd(x, na.rm = FALSE)` — standard deviation.
pub unsafe fn do_sd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Call do_var to get variance
        let var_args = Rf_cons(CAR(args), CDR(args));
        let var_result = do_var(_call, _op, var_args, _rho);
        if var_result.is_null() {
            return R_NilValue();
        }

        let v = real_or_default(var_result, NA_REAL);
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v < 0.0 {
            Rf_ScalarReal(NA_REAL)
        } else {
            Rf_ScalarReal(libm::sqrt(v))
        }
    }
}

/// R's `median(x, na.rm = FALSE)` — median value.
pub unsafe fn do_median(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals: Vec<f64> = Vec::new();

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                vals.push(val);
            }
        }

        if vals.is_empty() {
            return Rf_ScalarReal(NA_REAL);
        }

        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = vals.len() / 2;
        if vals.len().is_multiple_of(2) {
            Rf_ScalarReal((vals[mid - 1] + vals[mid]) / 2.0)
        } else {
            Rf_ScalarReal(vals[mid])
        }
    }
}

/// R's `IQR(x, na.rm = FALSE)` — interquartile range using quantile type 7.
pub unsafe fn do_iqr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals = Vec::new();

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == R_NA_BIT_PATTERN || val.is_nan() {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                vals.push(val);
            }
        }

        if vals.is_empty() {
            return Rf_ScalarReal(NA_REAL);
        }

        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Rf_ScalarReal(iqr_value(&mut vals))
    }
}

/// R's `cummin(x)` — cumulative minimum.
pub unsafe fn do_cummin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result_type = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            SEXPTYPE::INTSXP
        } else {
            SEXPTYPE::REALSXP
        };
        let result = Rf_allocVector3(result_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            let mut min_so_far = i32::MAX;
            let mut poisoned = false;
            for i in 0..n {
                let val = *INTEGER(x).add(i as usize);
                if val == NA_INTEGER {
                    poisoned = true;
                }
                if poisoned {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    min_so_far = min_so_far.min(val);
                    *dst.add(i as usize) = min_so_far;
                }
            }
            return result;
        }

        let dst = REAL(result);

        let mut min_so_far = f64::INFINITY;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                min_so_far = NA_REAL;
            } else if min_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                min_so_far = min_so_far.min(val);
            }
            *dst.add(i as usize) = min_so_far;
        }
        result
    }
}

/// R's `cummax(x)` — cumulative maximum.
pub unsafe fn do_cummax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result_type = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            SEXPTYPE::INTSXP
        } else {
            SEXPTYPE::REALSXP
        };
        let result = Rf_allocVector3(result_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            let mut max_so_far = i32::MIN;
            let mut poisoned = false;
            for i in 0..n {
                let val = *INTEGER(x).add(i as usize);
                if val == NA_INTEGER {
                    poisoned = true;
                }
                if poisoned {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    max_so_far = max_so_far.max(val);
                    *dst.add(i as usize) = max_so_far;
                }
            }
            return result;
        }

        let dst = REAL(result);

        let mut max_so_far = f64::NEG_INFINITY;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                max_so_far = NA_REAL;
            } else if max_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                max_so_far = max_so_far.max(val);
            }
            *dst.add(i as usize) = max_so_far;
        }
        result
    }
}

/// R's `dimnames(x)` — get dimension names of a matrix/array.
pub unsafe fn do_dimnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `%in%` operator — match operator.
pub unsafe fn do_in_operator(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let table = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || table.is_null() || table == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        for i in 0..n {
            let elem = elt_to_string(x, i);
            let table_len = XLENGTH(table);
            let mut found = false;
            for j in 0..table_len {
                let tbl_elem = elt_to_string(table, j);
                if elem == tbl_elem {
                    found = true;
                    break;
                }
            }
            *dst.add(i as usize) = if found { TRUE } else { FALSE };
        }
        result
    }
}

unsafe fn real_math1(args: SEXP, f: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            *dst.add(i as usize) = if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                f(val)
            };
        }
        result
    }
}

fn sinpi_value(x: f64) -> f64 {
    if x.is_finite() && x.fract() == 0.0 {
        0.0
    } else {
        (std::f64::consts::PI * x).sin()
    }
}

fn cospi_value(x: f64) -> f64 {
    if x.is_finite() && x.fract() == 0.0 {
        if (x as i64).rem_euclid(2) == 0 {
            1.0
        } else {
            -1.0
        }
    } else if x.is_finite() && (x - 0.5).fract() == 0.0 {
        0.0
    } else {
        (std::f64::consts::PI * x).cos()
    }
}

/// R's `expm1(x)` — accurate exp(x)-1.
pub unsafe fn do_expm1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::exp_m1) }
}

/// R's `log1p(x)` — accurate log(1+x).
pub unsafe fn do_log1p(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::ln_1p) }
}

/// R's `acosh(x)` — inverse hyperbolic cosine.
pub unsafe fn do_acosh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::acosh) }
}

/// R's `asinh(x)` — inverse hyperbolic sine.
pub unsafe fn do_asinh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::asinh) }
}

/// R's `atanh(x)` — inverse hyperbolic tangent.
pub unsafe fn do_atanh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::atanh) }
}

/// R's `sinpi(x)` — sin(pi*x), exact at integer arguments.
pub unsafe fn do_sinpi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, sinpi_value) }
}

/// R's `cospi(x)` — cos(pi*x), exact at integer and half-integer arguments.
pub unsafe fn do_cospi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, cospi_value) }
}

/// R's `tanpi(x)` — tan(pi*x), based on the exact sinpi/cospi helpers.
pub unsafe fn do_tanpi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        real_math1(args, |x| {
            if x.is_finite() && x.fract() == 0.0 {
                return 0.0;
            }
            if x.is_finite() {
                let cycle = x.rem_euclid(1.0);
                if cycle == 0.25 {
                    return 1.0;
                }
                if cycle == 0.75 {
                    return -1.0;
                }
            }
            let cos = cospi_value(x);
            if cos == 0.0 {
                f64::NAN
            } else {
                sinpi_value(x) / cos
            }
        })
    }
}

/// R's `sin(x)` — sine function.
pub unsafe fn do_sin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_sin,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.sin();
            }
        }
        result
    }
}

/// R's `cos(x)` — cosine function.
pub unsafe fn do_cos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_cos,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.cos();
            }
        }
        result
    }
}

/// R's `tan(x)` — tangent function.
pub unsafe fn do_tan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_tan,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.tan();
            }
        }
        result
    }
}

/// R's `asin(x)` — arc sine function.
pub unsafe fn do_asin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.asin();
            }
        }
        result
    }
}

/// R's `acos(x)` — arc cosine function.
pub unsafe fn do_acos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.acos();
            }
        }
        result
    }
}

/// R's `atan(x)` — arc tangent function.
pub unsafe fn do_atan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.atan();
            }
        }
        result
    }
}

/// R's `atan2(y, x)` — two-argument arc tangent function.
pub unsafe fn do_atan2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let x = CAR(CDR(args));

        if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(y).max(XLENGTH(x));
        let ty = TYPEOF(y);
        let tx = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let y_len = XLENGTH(y);
            let x_len = XLENGTH(x);
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let xi = if x_len > 0 { i % x_len } else { 0 };

            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val_y.atan2(val_x);
            }
        }
        result
    }
}
