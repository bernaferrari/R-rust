#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/gram-ex.c
//!
//! Formerly in gram.y, this file provides `R_fgetc`, a wrapper around
//! standard `fgetc()` that normalises CRLF line termination and, on
//! non-Windows platforms, translates hard EOF into `R_EOF`.

use std::os::raw::{c_int, c_void};

/// R_EOF: the value R uses to signal end-of-input (distinct from C's EOF).
const R_EOF: c_int = -1;

/// R's wrapper around `fgetc`.
///
/// Reads a single character from the stream `fp` (a `*mut FILE`), strips
/// CR from CRLF pairs, and returns `R_EOF` when the stream is exhausted.
///
/// Ported from R's src/main/gram-ex.c `R_fgetc(FILE *fp)`.
/// This implements the non-Windows code path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_fgetc(fp: *mut c_void) -> c_int {
    let fp = fp as *mut libc::FILE;
    if fp.is_null() {
        return R_EOF;
    }

    let mut c = unsafe { libc::fgetc(fp) };

    // Get rid of CR in CRLF line termination.
    if c == '\r' as c_int {
        c = unsafe { libc::fgetc(fp) };
        // Retain CRs with no following linefeed.
        if c != '\n' as c_int {
            unsafe { libc::ungetc(c, fp) };
            return '\r' as c_int;
        }
        // c is now '\n' (the LF from the CRLF pair), fall through
    }

    if unsafe { libc::feof(fp) } != 0 {
        R_EOF
    } else {
        c
    }
}
