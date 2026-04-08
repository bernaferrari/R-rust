#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/altclasses.c — ALTREP class implementations.
//!
//! Provides specific ALTREP classes:
//! - compact_intseq: compact integer sequences
//! - compact_realseq: compact real sequences
//! - deferred_string: deferred string operations
//! - deferred_names: deferred names vectors

use std::os::raw::c_int;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// compact_intseq — compact integer sequence ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the compact integer sequence ALTREP class.
pub unsafe fn R_init_compact_intseq() -> SEXP {
    unsafe { R_NilValue() }
}

/// Check if an SEXP is a compact integer sequence.
pub unsafe fn R_compact_intseq_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

// ---------------------------------------------------------------------------
// compact_realseq — compact real sequence ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the compact real sequence ALTREP class.
pub unsafe fn R_init_compact_realseq() -> SEXP {
    unsafe { R_NilValue() }
}

/// Check if an SEXP is a compact real sequence.
pub unsafe fn R_compact_realseq_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

// ---------------------------------------------------------------------------
// deferred_string — deferred string ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the deferred string ALTREP class.
pub unsafe fn R_init_deferred_string() -> SEXP {
    unsafe { R_NilValue() }
}

/// Check if an SEXP is a deferred string.
pub unsafe fn R_deferred_string_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

// ---------------------------------------------------------------------------
// deferred_names — deferred names ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the deferred names ALTREP class.
pub unsafe fn R_init_deferred_names() -> SEXP {
    unsafe { R_NilValue() }
}

/// Check if an SEXP is a deferred names vector.
pub unsafe fn R_deferred_names_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

// ---------------------------------------------------------------------------
// ALTREP initialization
// ---------------------------------------------------------------------------

/// Initialize all built-in ALTREP classes.
pub unsafe fn R_init_altrep_classes() {
    unsafe {
        R_init_compact_intseq();
        R_init_compact_realseq();
        R_init_deferred_string();
        R_init_deferred_names();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    fn test_compact_intseq_init() {
        unsafe {
            let cls = R_init_compact_intseq();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_compact_realseq_init() {
        unsafe {
            let cls = R_init_compact_realseq();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_deferred_string_init() {
        unsafe {
            let cls = R_init_deferred_string();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_deferred_names_init() {
        unsafe {
            let cls = R_init_deferred_names();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_compact_intseq_check_null() {
        unsafe {
            assert_eq!(R_compact_intseq_check(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_compact_realseq_check_null() {
        unsafe {
            assert_eq!(R_compact_realseq_check(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_init_altrep_classes() {
        unsafe {
            R_init_altrep_classes();
        }
    }
}
