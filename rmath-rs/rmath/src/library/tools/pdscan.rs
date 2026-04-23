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
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector, Rf_isString, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// Scan one package dependency string for package names.
///
/// A package dependency spec is a comma-separated list of package names
/// optionally followed by a comment in parentheses specifying a version
/// requirement. Package names match "[[:alpha:]][[:alnum:].]*[[:alnum:]]".
/// Single-char "R" is excluded as it refers to R itself.
unsafe fn package_dependencies_scan_one(this: SEXP) -> SEXP {
    if this.is_null() {
        // NA_STRING case: return empty character vector
        return Rf_allocVector(SEXPTYPE::STRSXP, 0);
    }

    let p = CHAR(this);
    if p.is_null() {
        return Rf_allocVector(SEXPTYPE::STRSXP, 0);
    }
    let s_bytes = CStr::from_ptr(p).to_bytes();

    let mut size: usize = 256;
    let mut beg: Vec<c_int> = vec![0; size];
    let mut end: Vec<c_int> = vec![0; size];
    let mut nb: usize = 0;
    let mut ne: usize = 0;

    let mut save = false;
    let mut q: u8 = 0;
    let mut i: c_int = 0;

    for &c in s_bytes.iter() {
        if save {
            if !c.is_ascii_alphanumeric() && c != b'.' {
                save = false;
                if q == b'R' && beg[ne] == i - 1 {
                    nb -= 1;
                } else {
                    end[ne] = i - 1;
                    ne += 1;
                }
            }
        } else {
            if c.is_ascii_alphabetic() {
                save = true;
                q = c;
                if nb >= size {
                    if size > c_int::MAX as usize / 2 {
                        Rf_error(b"too many items\0".as_ptr() as *const _);
                    }
                    size *= 2;
                    beg.resize(size, 0);
                    end.resize(size, 0);
                }
                beg[nb] = i;
                nb += 1;
            }
        }
        i += 1;
    }

    // Handle the last token
    if ne < nb {
        if q == b'R' && beg[ne] == i - 1 {
            nb -= 1;
        } else {
            end[ne] = i - 1;
        }
    }

    let y = Rf_allocVector(SEXPTYPE::STRSXP, nb as c_int);
    Rf_protect(y);

    let mut v: c_int = -1;
    for idx in 0..nb {
        let u = beg[idx];
        let v_end = end[idx];
        let w = (v_end - u + 1) as usize;

        let byte_start = u as usize;
        if byte_start + w <= s_bytes.len() {
            let substr = &s_bytes[byte_start..byte_start + w];
            let c_string = std::ffi::CString::new(substr).unwrap_or_default();
            let char_sxp = Rf_mkChar(c_string.as_ptr());
            SET_STRING_ELT(y, idx as R_xlen_t, char_sxp);
        }
        v = v_end;
    }

    Rf_unprotect(1);
    y
}

/// Scan package dependency strings for package names.
///
/// Takes a character vector and returns a character vector of all
/// package names found across all elements.
pub unsafe fn package_dependencies_scan(x: SEXP) -> SEXP {
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

    // Multiple elements: collect into a list, then unlist
    let z = Rf_allocVector(SEXPTYPE::VECSXP, nx);
    Rf_protect(z);
    let mut ny: R_xlen_t = 0;

    for i in 0..nx as usize {
        let this = package_dependencies_scan_one(STRING_ELT(x, i as R_xlen_t));
        SET_VECTOR_ELT(z, i as R_xlen_t, this);
        ny += LENGTH(this) as R_xlen_t;
    }

    // Unlist
    let y = Rf_allocVector(SEXPTYPE::STRSXP, ny as c_int);
    Rf_protect(y);
    let mut k: R_xlen_t = 0;

    for i in 0..nx as usize {
        let this = VECTOR_ELT(z, i as R_xlen_t);
        let this_len = LENGTH(this);
        for j in 0..this_len as usize {
            SET_STRING_ELT(y, k, STRING_ELT(this, j as R_xlen_t));
            k += 1;
        }
    }

    Rf_unprotect(2);
    y
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
unsafe fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    crate::sexp::accessors::SET_STRING_ELT(x, i, val);
}

#[inline]
unsafe fn SET_VECTOR_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    crate::sexp::accessors::SET_VECTOR_ELT(x, i, val);
}

#[inline]
unsafe fn VECTOR_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    crate::sexp::accessors::VECTOR_ELT(x, i)
}
