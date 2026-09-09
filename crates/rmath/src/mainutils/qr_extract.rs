//! R-facing extraction of the triangular R factor from a `qr` object.

#![allow(non_snake_case)]

use std::os::raw::c_int;

use crate::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::main::coerce::asLogical;
use crate::mainutils::duplicate::Rf_duplicate;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

const REALSXP_C: c_int = 14;
const INTSXP_C: c_int = 13;
const STRSXP_C: c_int = 16;
const VECSXP_C: c_int = 19;

fn qr_r_error(message: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

unsafe fn match_args(args: SEXP) -> [Option<SEXP>; 2] {
    unsafe {
        let mut matched = [None; 2];
        let mut cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) != R_NilValue() && !TAG(cell).is_null() {
                let tag = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(TAG(cell)))).to_string_lossy();
                let index = ["qr", "complete"]
                    .iter()
                    .position(|name| !tag.is_empty() && name.starts_with(tag.as_ref()))
                    .unwrap_or_else(|| qr_r_error("unused argument"));
                if matched[index].replace(CAR(cell)).is_some() {
                    qr_r_error("formal argument matched by multiple actual arguments");
                }
            }
            cell = CDR(cell);
        }
        cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) == R_NilValue() || TAG(cell).is_null() {
                let slot = matched
                    .iter_mut()
                    .find(|slot| slot.is_none())
                    .unwrap_or_else(|| qr_r_error("unused argument"));
                *slot = Some(CAR(cell));
            }
            cell = CDR(cell);
        }
        matched
    }
}

unsafe fn has_qr_class(value: SEXP) -> bool {
    unsafe {
        let class = getAttrib(value, R_ClassSymbol());
        if class == R_NilValue() || TYPEOF(class) != STRSXP_C {
            return false;
        }
        for i in 0..XLENGTH(class) {
            let text = CHAR(STRING_ELT(class, i));
            if std::ffi::CStr::from_ptr(text).to_bytes() == b"qr" {
                return true;
            }
        }
        false
    }
}

unsafe fn copy_dimname_prefix(value: SEXP, count: usize) -> SEXP {
    unsafe {
        if value == R_NilValue() {
            return value;
        }
        if TYPEOF(value) != STRSXP_C || XLENGTH(value) < count as R_xlen_t {
            return R_NilValue();
        }
        let result = Rf_allocVector(STRSXP_C, count as c_int);
        for i in 0..count {
            SET_STRING_ELT(result, i as R_xlen_t, STRING_ELT(value, i as R_xlen_t));
        }
        result
    }
}

/// Extract the upper-triangular R factor from a validated `qr` object.
pub unsafe fn do_qr_R(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let [object, complete_arg] = match_args(args);
        let object =
            object.unwrap_or_else(|| qr_r_error("argument 'qr' is missing, with no default"));
        if object == R_NilValue() || TYPEOF(object) != VECSXP_C || !has_qr_class(object) {
            qr_r_error("argument is not a QR decomposition");
        }
        let names = getAttrib(object, R_NamesSymbol());
        let mut factor = R_NilValue();
        if TYPEOF(names) == STRSXP_C && XLENGTH(names) == XLENGTH(object) {
            for i in 0..XLENGTH(names) {
                if std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_bytes() == b"qr" {
                    factor = VECTOR_ELT(object, i);
                    break;
                }
            }
        }
        if factor == R_NilValue() || TYPEOF(factor) != REALSXP_C {
            qr_r_error("invalid QR decomposition");
        }
        let dim = getAttrib(factor, R_DimSymbol());
        if dim == R_NilValue() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            qr_r_error("invalid QR matrix dimensions");
        }
        let m_i = *INTEGER(dim);
        let n_i = *INTEGER(dim).add(1);
        if m_i < 0 || n_i < 0 {
            qr_r_error("invalid QR matrix dimensions");
        }
        let m = m_i as usize;
        let n = n_i as usize;
        match m.checked_mul(n) {
            Some(length) if length == XLENGTH(factor) as usize => {}
            _ => qr_r_error("invalid QR matrix length"),
        }
        let complete = complete_arg.is_some_and(|value| {
            let value = asLogical(value);
            if value == NA_LOGICAL {
                qr_r_error("invalid 'complete' argument");
            }
            value != 0
        });
        let rows = if complete { m } else { m.min(n) };
        if rows > c_int::MAX as usize || n > c_int::MAX as usize {
            qr_r_error("QR result is too large");
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, (rows * n) as R_xlen_t);
        let _result_guard = protect(result);
        for col in 0..n {
            crate::eval::limits::poll_computation();
            for row in 0..rows {
                let value = if row <= col && row < m {
                    *REAL(factor).add(row + col * m)
                } else {
                    0.0
                };
                *REAL(result).add(row + col * rows) = value;
            }
        }
        let out_dim = Rf_allocVector(INTSXP_C, 2);
        let _dim_guard = protect(out_dim);
        *INTEGER(out_dim) = rows as c_int;
        *INTEGER(out_dim).add(1) = n as c_int;
        setAttrib(result, R_DimSymbol(), out_dim);

        let input_dimnames = getAttrib(factor, R_DimNamesSymbol());
        if input_dimnames != R_NilValue()
            && TYPEOF(input_dimnames) == VECSXP_C
            && XLENGTH(input_dimnames) == 2
        {
            let output_dimnames = Rf_allocVector(VECSXP_C, 2);
            let _dimnames_guard = protect(output_dimnames);
            let row_names = copy_dimname_prefix(VECTOR_ELT(input_dimnames, 0), rows);
            SET_VECTOR_ELT(output_dimnames, 0, row_names);
            let col_names = Rf_duplicate(VECTOR_ELT(input_dimnames, 1));
            SET_VECTOR_ELT(output_dimnames, 1, col_names);
            setAttrib(result, R_DimNamesSymbol(), output_dimnames);
        }
        result
    }
}
