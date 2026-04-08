#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Stub port of R's src/main/gram-ex.c
//!
//! Formerly in gram.y, this file provides `R_fgetc`, a wrapper around
//! standard `fgetc()` that normalises CRLF line termination and, on
//! non-Windows platforms, translates hard EOF into `R_EOF`.
//!
//! The original C implementation depends on:
//! - `<stdio.h>` for `fgetc`, `ungetc`, `feof`
//! - `R_EOF` constant (defined in R's internal headers)
//! - Platform `#ifdef Win32` branching
//!
//! A faithful port would need a concrete definition of `R_EOF` and a
//! real `FILE *` wrapper.  This module provides an FFI-compatible stub.

use std::os::raw::{c_int, c_void};

/// Stub for `R_fgetc` -- R's wrapper around `fgetc`.
///
/// In the full R implementation this reads a single character from *fp*,
/// strips CR from CRLF pairs, and on non-Windows platforms returns
/// `R_EOF` when the stream is exhausted.  The stub simply returns `R_EOF`
/// (represented as -1 here) unconditionally.
pub unsafe fn R_fgetc(_fp: *mut c_void) -> c_int {
    // R_EOF is typically -1; return that as a safe stub.
    -1
}
