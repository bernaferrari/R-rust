//! Answer-filling passes: AnswerType dispatch and per-type fill (list/string/logical/integer/real/complex/raw) — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_data_class, getAttrib, isObject, setAttrib};
use crate::eval::dispatch::DispatchOrEval;
use crate::eval::dispatch::promiseArgs;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// AnswerType -- determine the result type of unlist() / c()
// ---------------------------------------------------------------------------

/// Walk SEXP `x`, updating `data->ans_flags` and `data->ans_length` to reflect
/// the coercion type and total length required.
///
/// ans_flags bit assignments:
///   1   = RAWSXP
///   2   = LGLSXP
///   16  = INTSXP
///   32  = REALSXP
///   64  = CPLXSXP
///   128 = STRSXP
///   256 = VECSXP (generic list)
///   512 = EXPRSXP (expression)
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn AnswerType(x: SEXP, recurse: bool, usenames: bool, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        if crate::mainutils::objects::isS4(x) != 0 {
            (*data).ans_flags |= 256;
            (*data).ans_length += 1;
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            RAWSXP_I => {
                (*data).ans_flags |= 1;
                (*data).ans_length += xlength(x);
            }
            LGLSXP_I => {
                (*data).ans_flags |= 2;
                (*data).ans_length += xlength(x);
            }
            INTSXP_I => {
                (*data).ans_flags |= 16;
                (*data).ans_length += xlength(x);
            }
            REALSXP_I => {
                (*data).ans_flags |= 32;
                (*data).ans_length += xlength(x);
            }
            CPLXSXP_I => {
                (*data).ans_flags |= 64;
                (*data).ans_length += xlength(x);
            }
            STRSXP_I => {
                (*data).ans_flags |= 128;
                (*data).ans_length += xlength(x);
            }
            VECSXP_I | EXPRSXP_I => {
                if recurse {
                    let n = xlength(x);
                    if usenames && (*data).ans_nnames == 0 {
                        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
                        if !Rf_isNull(getAttrib(x, names_sym)) != 0 {
                            (*data).ans_nnames = 1;
                        }
                    }
                    for i in 0..n {
                        if usenames && (*data).ans_nnames == 0 {
                            (*data).ans_nnames = HasNames(VECTOR_ELT(x, i)) as R_xlen_t;
                        }
                        AnswerType(VECTOR_ELT(x, i), recurse, usenames, data, call);
                    }
                } else {
                    if t == EXPRSXP_I {
                        (*data).ans_flags |= 512;
                    } else {
                        (*data).ans_flags |= 256;
                    }
                    (*data).ans_length += xlength(x);
                }
            }
            LISTSXP_I => {
                if recurse {
                    let mut current = x;
                    while !current.is_null() && current != R_NilValue() {
                        if usenames && (*data).ans_nnames == 0 {
                            if !Rf_isNull(TAG(current)) != 0 {
                                (*data).ans_nnames = 1;
                            } else {
                                (*data).ans_nnames = HasNames(CAR(current)) as R_xlen_t;
                            }
                        }
                        AnswerType(CAR(current), recurse, usenames, data, call);
                        current = CDR(current);
                    }
                } else {
                    (*data).ans_flags |= 256;
                    (*data).ans_length += length(x) as R_xlen_t;
                }
            }
            _ => {
                (*data).ans_flags |= 256;
                (*data).ans_length += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ListAnswer -- fill a list/expression vector result
// ---------------------------------------------------------------------------

/// Copy elements from `x` into `data->ans_ptr` as list elements.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn ListAnswer(x: SEXP, recurse: c_int, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        if crate::mainutils::objects::isS4(x) != 0 {
            list_assign(data, lazy_duplicate(x));
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarLogical(*LOGICAL(x).add(i as usize)));
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarRaw(*RAW(x).add(i as usize)));
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarInteger(*INTEGER(x).add(i as usize)));
                }
            }
            REALSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarReal(*REAL(x).add(i as usize)));
                }
            }
            CPLXSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarComplex(*COMPLEX(x).add(i as usize)));
                }
            }
            STRSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarString(STRING_ELT(x, i)));
                }
            }
            VECSXP_I | EXPRSXP_I => {
                if recurse != 0 {
                    for i in 0..xlength(x) {
                        ListAnswer(VECTOR_ELT(x, i), recurse, data, call);
                    }
                } else {
                    for i in 0..xlength(x) {
                        list_assign(data, lazy_duplicate(VECTOR_ELT(x, i)));
                    }
                }
            }
            LISTSXP_I => {
                if recurse != 0 {
                    let mut current = x;
                    while !current.is_null() && current != R_NilValue() {
                        ListAnswer(CAR(current), recurse, data, call);
                        current = CDR(current);
                    }
                } else {
                    let mut current = x;
                    while !current.is_null() && current != R_NilValue() {
                        list_assign(data, lazy_duplicate(CAR(current)));
                        current = CDR(current);
                    }
                }
            }
            _ => {
                list_assign(data, lazy_duplicate(x));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// StringAnswer -- fill a character (STRSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to strings and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn StringAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    StringAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    StringAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            STRSXP_I => {
                // Already strings, copy directly
                for i in 0..xlength(x) {
                    SET_STRING_ELT((*data).ans_ptr, (*data).ans_length, STRING_ELT(x, i));
                    (*data).ans_length += 1;
                }
            }
            _ => {
                // For other types, coerce to string first
                let coerced = coerceVector(x, SEXPTYPE::STRSXP);
                let _coerced_guard = protect(coerced);
                for i in 0..xlength(coerced) {
                    SET_STRING_ELT((*data).ans_ptr, (*data).ans_length, STRING_ELT(coerced, i));
                    (*data).ans_length += 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LogicalAnswer -- fill a logical (LGLSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to logicals and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn LogicalAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    LogicalAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    LogicalAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    *LOGICAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        *LOGICAL(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    let v = *INTEGER(x).add(i as usize);
                    let lv = if v == NA_INTEGER {
                        NA_LOGICAL
                    } else if v != 0 {
                        TRUE
                    } else {
                        FALSE
                    };
                    *LOGICAL((*data).ans_ptr).add((*data).ans_length as usize) = lv;
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *LOGICAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        if *RAW(x).add(i as usize) != 0 {
                            TRUE
                        } else {
                            FALSE
                        };
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'LogicalAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IntegerAnswer -- fill an integer (INTSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to integers and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn IntegerAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    IntegerAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    IntegerAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    *INTEGER((*data).ans_ptr).add((*data).ans_length as usize) =
                        *LOGICAL(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    *INTEGER((*data).ans_ptr).add((*data).ans_length as usize) =
                        *INTEGER(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *INTEGER((*data).ans_ptr).add((*data).ans_length as usize) =
                        *RAW(x).add(i as usize) as c_int;
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'IntegerAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RealAnswer -- fill a double (REALSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to doubles and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn RealAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    RealAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            VECSXP_I | EXPRSXP_I => {
                for i in 0..xlength(x) {
                    RealAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            REALSXP_I => {
                for i in 0..xlength(x) {
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        *REAL(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *LOGICAL(x).add(i as usize);
                    let v = if xi == NA_LOGICAL {
                        NA_REAL
                    } else {
                        xi as c_double
                    };
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) = v;
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *INTEGER(x).add(i as usize);
                    let v = if xi == NA_INTEGER {
                        NA_REAL
                    } else {
                        xi as c_double
                    };
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) = v;
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        *RAW(x).add(i as usize) as c_double;
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'RealAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ComplexAnswer -- fill a complex (CPLXSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to complex and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn ComplexAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    ComplexAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    ComplexAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            REALSXP_I => {
                for i in 0..xlength(x) {
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    (*c).r = *REAL(x).add(i as usize);
                    (*c).i = 0.0;
                    (*data).ans_length += 1;
                }
            }
            CPLXSXP_I => {
                for i in 0..xlength(x) {
                    *COMPLEX((*data).ans_ptr).add((*data).ans_length as usize) =
                        *COMPLEX(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *LOGICAL(x).add(i as usize);
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    if xi == NA_LOGICAL {
                        (*c).r = NA_REAL;
                        (*c).i = 0.0;
                    } else {
                        (*c).r = xi as c_double;
                        (*c).i = 0.0;
                    }
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *INTEGER(x).add(i as usize);
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    if xi == NA_INTEGER {
                        (*c).r = NA_REAL;
                        (*c).i = 0.0;
                    } else {
                        (*c).r = xi as c_double;
                        (*c).i = 0.0;
                    }
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    (*c).r = *RAW(x).add(i as usize) as c_double;
                    (*c).i = 0.0;
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'ComplexAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RawAnswer -- fill a raw (RAWSXP) result
// ---------------------------------------------------------------------------

/// Copy raw bytes from `x` into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
pub unsafe fn RawAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    RawAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    RawAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *RAW((*data).ans_ptr).add((*data).ans_length as usize) =
                        *RAW(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'RawAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}
