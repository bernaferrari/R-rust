/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2092--2025   The R Core Team.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/http.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::ffi::CStr;
use std::ptr;

use crate::main::errors::Rf_error;
use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::accessors::{CHAR, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// startHTTPD / stopHTTPD
// ---------------------------------------------------------------------------

/// Start an HTTP daemon on the given IP and port.
pub unsafe fn startHTTPD(sIP: SEXP, sPort: SEXP) -> SEXP {
    let _ = (sIP, sPort);
    unsupported("tools::startHTTPD")
}

/// Stop the HTTP daemon.
pub unsafe fn stopHTTPD() -> SEXP {
    unsupported("tools::stopHTTPD")
}

// ---------------------------------------------------------------------------
// remove_dot_segments (RFC 3986, Section 5.2.4)
// ---------------------------------------------------------------------------

/// Remove . and (most) .. from a path following RFC 3986, 5.2.4.
fn remove_dot_segments(p: &str) -> String {
    let input = p.as_bytes();
    let mut output: Vec<u8> = Vec::with_capacity(input.len());
    let mut idx: usize = 0;

    while idx < input.len() {
        let rest = &input[idx..];

        if rest.starts_with(b"../") {
            idx += 3;
            continue;
        }
        if rest.starts_with(b"./") {
            idx += 2;
            continue;
        }
        if rest.starts_with(b"/./") {
            idx += 2;
            continue;
        }
        if rest == b"/." {
            output.push(b'/');
            break;
        }
        if rest.starts_with(b"/../") {
            idx += 3;
            pop_last_segment(&mut output);
            continue;
        }
        if rest == b"/.." {
            pop_last_segment(&mut output);
            output.push(b'/');
            break;
        }
        if rest == b"." || rest == b".." {
            break;
        }

        if rest[0] == b'/' {
            output.push(b'/');
            idx += 1;
        }
        while idx < input.len() && input[idx] != b'/' {
            output.push(input[idx]);
            idx += 1;
        }
    }

    String::from_utf8(output).unwrap_or_default()
}

/// Wrapper for remove_dot_segments that operates on a character vector.
pub unsafe fn remove_dot_segments_wrapper(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            Rf_error(b"non-character argument\0".as_ptr() as *const _);
        }
        let n = XLENGTH(x);
        let y = Rf_allocVector(SEXPTYPE::STRSXP, n as i32);
        let _y_guard = protect(y);

        for i in 0..n as usize {
            let s = STRING_ELT(x, i as R_xlen_t);
            if s.is_null() {
                let na_char = Rf_mkChar(ptr::null());
                SET_STRING_ELT(y, i as R_xlen_t, na_char);
                continue;
            }
            let p = CHAR(s);
            if p.is_null() {
                let empty_char = Rf_mkChar(c"".as_ptr());
                SET_STRING_ELT(y, i as R_xlen_t, empty_char);
                continue;
            }
            let path = match CStr::from_ptr(p).to_str() {
                Ok(s) => s,
                Err(_) => {
                    let empty_char = Rf_mkChar(c"".as_ptr());
                    SET_STRING_ELT(y, i as R_xlen_t, empty_char);
                    continue;
                }
            };
            let cleaned = remove_dot_segments(path);
            let c_string = std::ffi::CString::new(cleaned).unwrap_or_default();
            let char_sxp = Rf_mkChar(c_string.as_ptr());
            SET_STRING_ELT(y, i as R_xlen_t, char_sxp);
        }

        y
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
unsafe fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    unsafe {
        crate::sexp::accessors::SET_STRING_ELT(x, i, val);
    }
}

fn pop_last_segment(output: &mut Vec<u8>) {
    while output.last().copied().is_some_and(|b| b != b'/') {
        output.pop();
    }
    if !output.is_empty() {
        output.pop();
    }
}

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned")
}
