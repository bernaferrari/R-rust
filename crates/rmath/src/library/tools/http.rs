/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2092--2025   The R Core Team.
 *
 *  Ported to Rust for the crates project.
 *  Based on R's r-source/src/library/tools/src/http.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::ffi::CStr;
use std::ptr;

use crate::main::errors::{Rf_error, Rf_error_cstr};
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
pub fn startHTTPD(sIP: SEXP, sPort: SEXP) -> SEXP {
    let _ = (sIP, sPort);
    unsupported("tools::startHTTPD")
}

/// Stop the HTTP daemon.
pub fn stopHTTPD() -> SEXP {
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


// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    crate::sexp::accessors::SET_STRING_ELT(x, i, val);
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
