#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

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
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector, Rf_isString, Rf_mkChar};
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// startHTTPD / stopHTTPD
// ---------------------------------------------------------------------------

/// Start an HTTP daemon on the given IP and port.
pub unsafe fn startHTTPD(sIP: SEXP, sPort: SEXP) -> SEXP {
    let ip: *const c_char = ptr::null();
    if !sIP.is_null() && sIP != R_NilValue() {
        if Rf_isString(sIP) == 0 || LENGTH(sIP) != 1 {
            Rf_error(b"invalid bind address specification\0".as_ptr() as *const _);
        }
        let elt = STRING_ELT(sIP, 0);
        if !elt.is_null() {
            let _ = ip; // suppress unused warning - IP is extracted but not used in stub
        }
    }

    let port = coerce_to_int(sPort);
    if port < 0 || port > 65535 {
        let msg = std::ffi::CString::new(format!(
            "Invalid port number {}: should be in 0:65535, typically above 1024",
            port
        ))
        .unwrap_or_default();
        Rf_error(msg.as_ptr());
    }

    // extR_HTTPDCreate is an external function not available in this port
    // Return -1 (failure) since the HTTP daemon is not implemented
    Rf_ScalarInteger(-1)
}

/// Stop the HTTP daemon.
pub unsafe fn stopHTTPD() -> SEXP {
    // extR_HTTPDStop is an external function not available in this port
    R_NilValue()
}

// ---------------------------------------------------------------------------
// remove_dot_segments (RFC 3986, Section 5.2.4)
// ---------------------------------------------------------------------------

/// Remove . and (most) .. from a path following RFC 3986, 5.2.4.
fn remove_dot_segments(p: &str) -> String {
    let mut inbuf: Vec<char> = p.chars().collect();
    let mut outbuf: Vec<char> = Vec::with_capacity(inbuf.len() + 1);
    let mut idx: usize = 0;

    while idx < inbuf.len() {
        // A. If the input buffer begins with "../" or "./", remove that prefix
        if idx + 2 < inbuf.len()
            && inbuf[idx] == '.'
            && inbuf[idx + 1] == '.'
            && inbuf[idx + 2] == '/'
        {
            idx += 3;
            continue;
        }
        if idx + 1 < inbuf.len() && inbuf[idx] == '.' && inbuf[idx + 1] == '/' {
            idx += 2;
            continue;
        }

        // B. If the input buffer begins with "/./" or "/.", replace with "/"
        if idx + 2 < inbuf.len()
            && inbuf[idx] == '/'
            && inbuf[idx + 1] == '.'
            && inbuf[idx + 2] == '/'
        {
            idx += 2;
            continue;
        }
        if idx + 1 < inbuf.len()
            && inbuf[idx] == '/'
            && inbuf[idx + 1] == '.'
            && idx + 2 >= inbuf.len()
        {
            // trailing "/." -> "/"
            inbuf.truncate(idx + 1);
            continue;
        }

        // C. If the input buffer begins with "/../" or "/..", replace with "/"
        //    and remove the last segment from output
        if idx + 3 < inbuf.len()
            && inbuf[idx] == '/'
            && inbuf[idx + 1] == '.'
            && inbuf[idx + 2] == '.'
            && inbuf[idx + 3] == '/'
        {
            idx += 3;
            // remove trailing "/segment" from output
            while !outbuf.is_empty() && outbuf.last().copied().unwrap_or('\0') != '/' {
                outbuf.pop();
            }
            if !outbuf.is_empty() {
                outbuf.pop(); // remove the '/'
            }
            continue;
        }
        if idx + 2 < inbuf.len()
            && inbuf[idx] == '/'
            && inbuf[idx + 1] == '.'
            && inbuf[idx + 2] == '.'
            && idx + 3 >= inbuf.len()
        {
            // trailing "/.." -> "/"
            inbuf.truncate(idx + 1);
            // remove trailing "/segment" from output
            while !outbuf.is_empty() && outbuf.last().copied().unwrap_or('\0') != '/' {
                outbuf.pop();
            }
            if !outbuf.is_empty() {
                outbuf.pop(); // remove the '/'
            }
            continue;
        }

        // D. If the input buffer consists only of "." or "..", remove that
        if idx + 1 >= inbuf.len() && inbuf[idx] == '.' {
            idx += 1;
            continue;
        }
        if idx + 2 >= inbuf.len() && inbuf[idx] == '.' && inbuf[idx + 1] == '.' {
            idx += 2;
            continue;
        }

        // E. Move the first path segment to the end of the output buffer
        if inbuf[idx] == '/' {
            outbuf.push('/');
            idx += 1;
        }
        while idx < inbuf.len() && inbuf[idx] != '/' {
            outbuf.push(inbuf[idx]);
            idx += 1;
        }
    }

    outbuf.into_iter().collect()
}

/// Wrapper for remove_dot_segments that operates on a character vector.
pub unsafe fn remove_dot_segments_wrapper(x: SEXP) -> SEXP {
    if x.is_null() {
        return R_NilValue();
    }
    if TYPEOF(x) != SEXPTYPE::STRSXP {
        Rf_error(b"non-character argument\0".as_ptr() as *const _);
    }
    let n = XLENGTH(x);
    let y = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
    Rf_protect(y);

    for i in 0..n as usize {
        let s = STRING_ELT(x, i as R_xlen_t);
        if s.is_null() {
            // NA case - set null CHARSXP
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
        let char_sxp = Rf_mkChar(std::ffi::CString::new(cleaned).unwrap_or_default().as_ptr());
        SET_STRING_ELT(y, i as R_xlen_t, char_sxp);
    }

    Rf_unprotect(1);
    y
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
unsafe fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    crate::sexp::accessors::SET_STRING_ELT(x, i, val);
}

unsafe fn coerce_to_int(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        let p = INTEGER(x);
        if !p.is_null() {
            return *p;
        }
    } else if t == SEXPTYPE::LGLSXP {
        let p = crate::sexp::accessors::LOGICAL(x);
        if !p.is_null() {
            return *p;
        }
    } else if t == SEXPTYPE::REALSXP {
        let p = crate::sexp::accessors::REAL(x);
        if !p.is_null() {
            let v = *p;
            if v.is_nan() {
                return NA_INTEGER;
            }
            return v as c_int;
        }
    }
    NA_INTEGER
}
