#![cfg(feature = "altrep")]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/altclasses.c — ALTREP class implementations.
//!
//! Provides specific ALTREP classes:
//! - compact_intseq: compact integer sequences
//! - compact_realseq: compact real sequences
//! - deferred_string: deferred string operations
//! - deferred_names: deferred names vectors

use std::os::raw::c_int;

use crate::sexp::accessors::{ALTREP, TYPEOF, XLENGTH};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::symbol::Rf_install;

unsafe fn class_symbol(name: &'static [u8]) -> SEXP {
    unsafe { Rf_install(name.as_ptr() as *const std::os::raw::c_char) }
}

unsafe fn altrep_class_is(x: SEXP, class: SEXP) -> bool {
    unsafe {
        !x.is_null() && ALTREP(x) != 0 && crate::mainutils::altrep::R_altrep_class(x) == class
    }
}

// ---------------------------------------------------------------------------
// compact_intseq — compact integer sequence ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the compact integer sequence ALTREP class.
pub unsafe fn R_init_compact_intseq() -> SEXP {
    unsafe { class_symbol(b"compact_intseq\0") }
}

/// Check if an SEXP is a compact integer sequence.
pub unsafe fn R_compact_intseq_check(x: SEXP) -> c_int {
    unsafe {
        if !altrep_class_is(x, R_init_compact_intseq()) || TYPEOF(x) != SEXPTYPE::INTSXP {
            return 0;
        }
        let data = crate::mainutils::altrep::R_altrep_data1(x);
        (!data.is_null() && TYPEOF(data) == SEXPTYPE::INTSXP && XLENGTH(data) == 3) as c_int
    }
}

// ---------------------------------------------------------------------------
// compact_realseq — compact real sequence ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the compact real sequence ALTREP class.
pub unsafe fn R_init_compact_realseq() -> SEXP {
    unsafe { class_symbol(b"compact_realseq\0") }
}

/// Check if an SEXP is a compact real sequence.
pub unsafe fn R_compact_realseq_check(x: SEXP) -> c_int {
    unsafe {
        if !altrep_class_is(x, R_init_compact_realseq()) || TYPEOF(x) != SEXPTYPE::REALSXP {
            return 0;
        }
        let data = crate::mainutils::altrep::R_altrep_data1(x);
        (!data.is_null() && TYPEOF(data) == SEXPTYPE::REALSXP && XLENGTH(data) == 3) as c_int
    }
}

// ---------------------------------------------------------------------------
// deferred_string — deferred string ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the deferred string ALTREP class.
pub unsafe fn R_init_deferred_string() -> SEXP {
    unsafe { class_symbol(b"deferred_string\0") }
}

/// Check if an SEXP is a deferred string.
pub unsafe fn R_deferred_string_check(x: SEXP) -> c_int {
    unsafe {
        (altrep_class_is(x, R_init_deferred_string()) && TYPEOF(x) == SEXPTYPE::STRSXP) as c_int
    }
}

// ---------------------------------------------------------------------------
// deferred_names — deferred names ALTREP class
// ---------------------------------------------------------------------------

/// Initialize the deferred names ALTREP class.
pub unsafe fn R_init_deferred_names() -> SEXP {
    unsafe { class_symbol(b"deferred_names\0") }
}

/// Check if an SEXP is a deferred names vector.
pub unsafe fn R_deferred_names_check(x: SEXP) -> c_int {
    unsafe {
        (altrep_class_is(x, R_init_deferred_names()) && TYPEOF(x) == SEXPTYPE::STRSXP) as c_int
    }
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
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::session::RSession;

    #[test]
    fn test_compact_intseq_init() {
        let _session = RSession::new();
        unsafe {
            let cls = R_init_compact_intseq();
            assert!(!cls.is_null());
            assert_ne!(cls, R_NilValue());
            assert_eq!(cls, R_init_compact_intseq());
        }
    }

    #[test]
    fn test_compact_realseq_init() {
        let _session = RSession::new();
        unsafe {
            let cls = R_init_compact_realseq();
            assert!(!cls.is_null());
            assert_ne!(cls, R_NilValue());
            assert_eq!(cls, R_init_compact_realseq());
        }
    }

    #[test]
    fn test_deferred_string_init() {
        let _session = RSession::new();
        unsafe {
            let cls = R_init_deferred_string();
            assert!(!cls.is_null());
            assert_ne!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_deferred_names_init() {
        let _session = RSession::new();
        unsafe {
            let cls = R_init_deferred_names();
            assert!(!cls.is_null());
            assert_ne!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_compact_intseq_check_null() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_compact_intseq_check(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_compact_realseq_check_null() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_compact_realseq_check(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_compact_sequence_checks_recognize_builtin_altreps() {
        let _session = RSession::new();
        unsafe {
            let int_seq = crate::mainutils::altrep::R_compact_intseq(1, 3);
            let real_seq = crate::mainutils::altrep::R_compact_realseq(1.0, 0.5, 3);

            assert_eq!(R_compact_intseq_check(int_seq), 1);
            assert_eq!(R_compact_realseq_check(real_seq), 1);
            assert_eq!(R_compact_intseq_check(real_seq), 0);
            assert_eq!(R_compact_realseq_check(int_seq), 0);
        }
    }

    #[test]
    fn test_init_altrep_classes() {
        let _session = RSession::new();
        unsafe {
            R_init_altrep_classes();
        }
    }
}
