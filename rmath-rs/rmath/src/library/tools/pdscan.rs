/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2020-2022 The R Core Team.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/pdscan.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::ffi::CStr;
use std::os::raw::c_int;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

/// Scan one package dependency string for package names.
///
/// A package dependency spec is a comma-separated list of package names
/// optionally followed by a comment in parentheses specifying a version
/// requirement. Package names match "[[:alpha:]][[:alnum:].]*[[:alnum:]]".
/// Single-char "R" is excluded as it refers to R itself.
fn package_dependencies_scan_one(this: SEXP) -> SEXP {
    unsafe {
        if this.is_null() {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        let p = CHAR(this);
        if p.is_null() {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }
        let s_bytes = CStr::from_ptr(p).to_bytes();

        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut token_start: Option<usize> = None;
        let mut token_first: u8 = 0;

        for (i, &c) in s_bytes.iter().enumerate() {
            match token_start {
                Some(start) => {
                    if !c.is_ascii_alphanumeric() && c != b'.' {
                        if !(token_first == b'R' && i == start + 1) {
                            spans.push((start, i));
                        }
                        token_start = None;
                    }
                }
                None => {
                    if c.is_ascii_alphabetic() {
                        token_start = Some(i);
                        token_first = c;
                    }
                }
            }
        }

        if let Some(start) = token_start {
            if !(token_first == b'R' && start + 1 == s_bytes.len()) {
                spans.push((start, s_bytes.len()));
            }
        }

        let y = Rf_allocVector(SEXPTYPE::STRSXP, spans.len() as c_int);
        let _y_guard = protect(y);

        for (idx, (start, end)) in spans.into_iter().enumerate() {
            let substr = &s_bytes[start..end];
            let c_string = std::ffi::CString::new(substr).unwrap_or_default();
            let char_sxp = Rf_mkChar(c_string.as_ptr());
            SET_STRING_ELT(y, idx as R_xlen_t, char_sxp);
        }

        y
    }
}

/// Scan package dependency strings for package names.
///
/// Takes a character vector and returns a character vector of all
/// package names found across all elements.
pub unsafe fn package_dependencies_scan(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            Rf_error(b"non-character argument\0".as_ptr() as *const _);
        }

        let nx = LENGTH(x);
        if nx < 1 {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }
        if nx == 1 {
            return package_dependencies_scan_one(STRING_ELT(x, 0));
        }

        let z = Rf_allocVector(SEXPTYPE::VECSXP, nx);
        let _z_guard = protect(z);
        let mut ny: R_xlen_t = 0;

        for i in 0..nx as usize {
            let this = package_dependencies_scan_one(STRING_ELT(x, i as R_xlen_t));
            SET_VECTOR_ELT(z, i as R_xlen_t, this);
            ny += LENGTH(this) as R_xlen_t;
        }

        let y = Rf_allocVector(SEXPTYPE::STRSXP, ny as c_int);
        let _y_guard = protect(y);
        let mut k: R_xlen_t = 0;

        for i in 0..nx as usize {
            let this = VECTOR_ELT(z, i as R_xlen_t);
            let this_len = LENGTH(this);
            for j in 0..this_len as usize {
                SET_STRING_ELT(y, k, STRING_ELT(this, j as R_xlen_t));
                k += 1;
            }
        }

        y
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    unsafe {
        crate::sexp::accessors::SET_STRING_ELT(x, i, val);
    }
}

#[inline]
fn SET_VECTOR_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    unsafe {
        crate::sexp::accessors::SET_VECTOR_ELT(x, i, val);
    }
}

#[inline]
fn VECTOR_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe { crate::sexp::accessors::VECTOR_ELT(x, i) }
}
