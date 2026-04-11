#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Stub port of R's src/main/agrep.c
//!
//! Approximate string matching (TRE-based grep).
//!
//! The original C implementation heavily depends on:
//! - TRE regex library (tre/tre.h)
//! - R's SEXP type system (Defn.h, Internal.h)
//! - R's memory management (R_Calloc, R_Free)
//! - R's internal string handling (CHAR, STRING_ELT, translateChar, etc.)
//!
//! A full port would require reimplementing all of these subsystems.
//! This module provides FFI-compatible stubs that return safe defaults.

use std::os::raw::{c_char, c_int};

/// Placeholder: `do_agrep` -- approximate grep.
///
/// In the full R implementation, this performs approximate (fuzzy) string
/// matching using the TRE regex library. It requires R's SEXP type system
/// and is not feasible to port without it.
///
/// Returns null (no match) as a safe stub.
pub unsafe fn R_agrep(
    _pattern: *const c_char,
    _text: *const c_char,
    _max_distance: c_int,
    _ignore_case: c_int,
) -> c_int {
    // always return "no match"
    0
}
