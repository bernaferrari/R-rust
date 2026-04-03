#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/altrep.c — ALTREP (alternative representations).
//!
//! ALTREP provides a mechanism for lazy/delayed computation of R vectors.
//! This module provides stubs for the ALTREP API.

use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// ALTREP class structure stubs
// ---------------------------------------------------------------------------

/// ALTREP class descriptor (stub).
pub const R_ALTREP_CLASS_TYPE: c_int = 255;

/// Get the ALTREP class.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_class(x: SEXP) -> SEXP {
    unsafe { if ALTREP(x) != 0 { x } else { R_NilValue() } }
}

/// Get the ALTREP data1 field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_data1(x: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the ALTREP data2 field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_data2(x: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Set the ALTREP data1 field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_data1(_x: SEXP, _v: SEXP) {}

/// Set the ALTREP data2 field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_data2(_x: SEXP, _v: SEXP) {}

/// Get the ALTREP length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_length(x: SEXP) -> R_xlen_t {
    unsafe { if ALTREP(x) != 0 { XLENGTH(x) } else { 0 } }
}

// ---------------------------------------------------------------------------
// ALTREP constructors (stubs)
// ---------------------------------------------------------------------------

/// Create a new ALTREP object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_new_altrep(_class: SEXP, _data1: SEXP, _data2: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Create a compact integer sequence ALTREP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_compact_intseq(_from: R_xlen_t, _to: R_xlen_t) -> SEXP {
    unsafe { R_NilValue() }
}

/// Create a compact real sequence ALTREP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_compact_realseq(_from: f64, _by: f64, _length: R_xlen_t) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// ALTREP realization (stubs)
// ---------------------------------------------------------------------------

/// Realize (materialize) an ALTREP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_realize(x: SEXP) -> SEXP {
    x
}

/// Duplicate an ALTREP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_duplicate(_x: SEXP, _deep: c_int) -> SEXP {
    unsafe { R_NilValue() }
}

/// Inspect an ALTREP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_altrep_inspect(_x: SEXP, _pre: c_int, _deep: c_int) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// ALTREP type-specific accessors (stubs)
// ---------------------------------------------------------------------------

/// ALTINTEGER_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTINTEGER_ELT(_x: SEXP, _i: R_xlen_t) -> c_int {
    NA_INTEGER
}

/// ALTINTEGER_SET_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTINTEGER_SET_ELT(_x: SEXP, _i: R_xlen_t, _v: c_int) {}

/// ALTREAL_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTREAL_ELT(_x: SEXP, _i: R_xlen_t) -> f64 {
    crate::sexp::ffi::NA_REAL
}

/// ALTREAL_SET_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTREAL_SET_ELT(_x: SEXP, _i: R_xlen_t, _v: f64) {}

/// ALTLOGICAL_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTLOGICAL_ELT(_x: SEXP, _i: R_xlen_t) -> c_int {
    crate::sexp::ffi::NA_LOGICAL
}

/// ALTLOGICAL_SET_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTLOGICAL_SET_ELT(_x: SEXP, _i: R_xlen_t, _v: c_int) {}

/// ALTRAW_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTRAW_ELT(_x: SEXP, _i: R_xlen_t) -> u8 {
    0
}

/// ALTRAW_SET_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTRAW_SET_ELT(_x: SEXP, _i: R_xlen_t, _v: u8) {}

/// ALTSTRING_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTSTRING_ELT(_x: SEXP, _i: R_xlen_t) -> SEXP {
    unsafe { R_NilValue() }
}

/// ALTSTRING_SET_ELT stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTSTRING_SET_ELT(_x: SEXP, _i: R_xlen_t, _v: SEXP) {}

// ---------------------------------------------------------------------------
// ALTREP finalizer (stub)
// ---------------------------------------------------------------------------

/// Register ALTREP finalizer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_finalizer(
    _class: SEXP,
    _finalizer: Option<unsafe extern "C" fn(SEXP)>,
) {
}

/// Register ALTREP duplicate method.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_duplicate_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
) {
}

/// Register ALTREP inspect method.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_inspect_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP, c_int, c_int) -> c_int>,
) {
}

/// Register ALTREP length method.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_length_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP) -> R_xlen_t>,
) {
}

// ---------------------------------------------------------------------------
// ALTREP coerce method (stub)
// ---------------------------------------------------------------------------

/// Register ALTREP coerce method.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_altrep_coerce_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
) {
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_altrep_class_stub() {
        unsafe {
            let result = R_altrep_class(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_altrep_length_null() {
        unsafe {
            assert_eq!(R_altrep_length(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_altrep_data_null() {
        unsafe {
            assert_eq!(R_altrep_data1(ptr::null_mut()), R_NilValue());
            assert_eq!(R_altrep_data2(ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_new_altrep_stub() {
        unsafe {
            let result = R_new_altrep(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_altinteger_elt() {
        unsafe {
            assert_eq!(ALTINTEGER_ELT(ptr::null_mut(), 0), NA_INTEGER);
        }
    }

    #[test]
    fn test_altreal_elt() {
        unsafe {
            assert!(ALTREAL_ELT(ptr::null_mut(), 0).is_nan());
        }
    }
}
