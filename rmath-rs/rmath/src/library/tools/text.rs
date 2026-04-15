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
 *  Copyright (C) 2003-2025   The R Core Team.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/text.c
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
use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, LOGICAL, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{
    Rf_ScalarLogical, Rf_ScalarString, Rf_allocVector, Rf_isString, Rf_mkChar,
};
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// delim_match
// ---------------------------------------------------------------------------

/// Match delimited substrings in a character vector x.
///
/// Returns an integer vector with the same length of x giving the
/// starting position of the match (including the start delimiter), or
/// -1 if there is none, with attribute "match.length" giving the
/// length of the matched text (including the end delimiter), or -1
/// for no match.
pub unsafe fn delim_match(x: SEXP, delims: SEXP) -> SEXP {
    if x.is_null() || delims.is_null() {
        return R_NilValue();
    }
    if Rf_isString(x) == 0 || Rf_isString(delims) == 0 || LENGTH(delims) != 2 {
        Rf_error(b"invalid argument type\0".as_ptr() as *const _);
    }

    let delim_start = STRING_ELT(delims, 0);
    let delim_end = STRING_ELT(delims, 1);
    let ds = if delim_start.is_null() {
        b"\0"
    } else {
        let p = CHAR(delim_start);
        if p.is_null() {
            b"\0"
        } else {
            CStr::from_ptr(p).to_bytes()
        }
    };
    let de = if delim_end.is_null() {
        b"\0"
    } else {
        let p = CHAR(delim_end);
        if p.is_null() {
            b"\0"
        } else {
            CStr::from_ptr(p).to_bytes()
        }
    };
    let lstart = ds.len();
    let lend = de.len();
    let equal_start_and_end_delims = ds == de;

    let n = LENGTH(x);
    let ans = Rf_allocVector(SEXPTYPE::INTSXP.0, n);
    Rf_protect(ans);
    let matchlen = Rf_allocVector(SEXPTYPE::INTSXP.0, n);
    Rf_protect(matchlen);

    let ans_ptr = INTEGER(ans);
    let matchlen_ptr = INTEGER(matchlen);

    for i in 0..n as usize {
        let mut start: c_int = -1;
        let mut end: c_int = -1;

        let s_elt = STRING_ELT(x, i as R_xlen_t);
        if s_elt.is_null() {
            *ans_ptr.add(i) = -1;
            *matchlen_ptr.add(i) = -1;
            continue;
        }
        let s_ptr = CHAR(s_elt);
        if s_ptr.is_null() {
            *ans_ptr.add(i) = -1;
            *matchlen_ptr.add(i) = -1;
            continue;
        }
        let s_bytes = CStr::from_ptr(s_ptr).to_bytes();

        let mut pos: c_int = 0;
        let mut delim_depth: c_int = 0;
        let mut is_escaped = false;
        let mut byte_idx: usize = 0;

        while byte_idx < s_bytes.len() {
            let c = s_bytes[byte_idx];

            if c == b'\n' {
                is_escaped = false;
            } else if c == b'\\' {
                is_escaped = !is_escaped;
            } else if is_escaped {
                is_escaped = false;
            } else if c == b'%' {
                // Skip to end of line (comment)
                loop {
                    if byte_idx >= s_bytes.len() {
                        break;
                    }
                    if s_bytes[byte_idx] == b'\n' {
                        break;
                    }
                    // Skip multi-byte chars
                    if s_bytes[byte_idx] >= 0x80 && s_bytes[byte_idx] <= 0xBF {
                        byte_idx += 1;
                        if byte_idx >= s_bytes.len() {
                            break;
                        }
                    } else {
                        byte_idx += 1;
                    }
                    pos += 1;
                }
            } else if byte_idx + lend <= s_bytes.len() && s_bytes[byte_idx..byte_idx + lend] == *de
            {
                if delim_depth > 1 {
                    delim_depth -= 1;
                } else if delim_depth == 1 {
                    end = pos;
                    break;
                } else if equal_start_and_end_delims {
                    start = pos;
                    delim_depth += 1;
                }
            } else if byte_idx + lstart <= s_bytes.len()
                && s_bytes[byte_idx..byte_idx + lstart] == *ds
            {
                if delim_depth == 0 {
                    start = pos;
                }
                delim_depth += 1;
            }

            // Advance past multi-byte character
            if s_bytes[byte_idx] >= 0x80 {
                // UTF-8 continuation: advance to start of next char
                byte_idx += 1;
                while byte_idx < s_bytes.len()
                    && s_bytes[byte_idx] >= 0x80
                    && s_bytes[byte_idx] <= 0xBF
                {
                    byte_idx += 1;
                }
            } else {
                byte_idx += 1;
            }
            pos += 1;
        }

        if end > -1 {
            *ans_ptr.add(i) = start + 1; /* index from one */
            *matchlen_ptr.add(i) = end - start + 1;
        } else {
            *ans_ptr.add(i) = -1;
            *matchlen_ptr.add(i) = -1;
        }
    }

    // setAttrib(ans, install("match.length"), matchlen);
    // Use a simplified approach since setAttrib may not be available
    // Just set the attribute directly on the node
    if !ans.is_null() {
        let attr_sym = crate::sexp::symbol::Rf_install(c"match.length".as_ptr());
        crate::attrib_core::setAttrib(ans, attr_sym, matchlen);
    }

    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// check_nonASCII
// ---------------------------------------------------------------------------

/// Check if all the lines in 'text' are ASCII, after removing
/// comments and ignoring the contents of quotes (unless ignore_quotes).
pub unsafe fn check_nonASCII(text: SEXP, ignore_quotes: SEXP) -> SEXP {
    if text.is_null() {
        return Rf_ScalarLogical(FALSE);
    }
    if TYPEOF(text) != SEXPTYPE::STRSXP {
        Rf_error(b"invalid input\0".as_ptr() as *const _);
    }
    let ign = coerce_to_logical(ignore_quotes);
    if ign == NA_LOGICAL {
        Rf_error(b"'ignore_quotes' must be TRUE or FALSE\0".as_ptr() as *const _);
        return Rf_ScalarLogical(FALSE);
    }

    let n = LENGTH(text);
    for i in 0..n as usize {
        let elt = STRING_ELT(text, i as R_xlen_t);
        if elt.is_null() {
            continue;
        }
        let p = CHAR(elt);
        if p.is_null() {
            continue;
        }
        let bytes = CStr::from_ptr(p).to_bytes();
        let mut inquote = false;
        let mut quote: u8 = 0;
        let mut nbslash: c_int = 0;

        for &c in bytes.iter() {
            if !inquote && c == b'#' {
                break;
            }
            if !inquote || ign == 0 {
                if (c as u32) > 127 {
                    return Rf_ScalarLogical(TRUE);
                }
            }
            if nbslash % 2 == 0 && (c == b'"' || c == b'\'') {
                if inquote && c == quote {
                    inquote = false;
                } else if !inquote {
                    quote = c;
                    inquote = true;
                }
            }
            if c == b'\\' {
                nbslash += 1;
            } else {
                nbslash = 0;
            }
        }
    }
    Rf_ScalarLogical(FALSE)
}

// ---------------------------------------------------------------------------
// check_nonASCII2
// ---------------------------------------------------------------------------

/// Return indices of lines containing non-ASCII characters.
pub unsafe fn check_nonASCII2(text: SEXP) -> SEXP {
    if text.is_null() {
        return R_NilValue();
    }
    if TYPEOF(text) != SEXPTYPE::STRSXP {
        Rf_error(b"invalid input\0".as_ptr() as *const _);
    }

    let n = LENGTH(text);
    let mut ind: Vec<c_int> = Vec::with_capacity(n as usize);
    let mut m: usize = 0;

    for i in 0..n as usize {
        let elt = STRING_ELT(text, i as R_xlen_t);
        if elt.is_null() {
            continue;
        }
        let p = CHAR(elt);
        if p.is_null() {
            continue;
        }
        let bytes = CStr::from_ptr(p).to_bytes();
        let mut found = false;
        for &c in bytes.iter() {
            if (c as u32) > 127 {
                found = true;
                break;
            }
        }
        if found {
            ind.push((i + 1) as c_int); /* R is 1-based */
            m += 1;
        }
    }

    if m > 0 {
        let ans = Rf_allocVector(SEXPTYPE::INTSXP.0, m as c_int);
        if !ans.is_null() {
            let ians = INTEGER(ans);
            for i in 0..m {
                *ians.add(i) = ind[i];
            }
        }
        ans
    } else {
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// doTabExpand
// ---------------------------------------------------------------------------

/// Tab expansion for UTF-8 strings only.
pub unsafe fn doTabExpand(strings: SEXP, starts: SEXP) -> SEXP {
    if strings.is_null() || starts.is_null() {
        return R_NilValue();
    }
    let n = LENGTH(strings);
    let result = Rf_allocVector(SEXPTYPE::STRSXP.0, n);
    Rf_protect(result);

    for i in 0..n as usize {
        let elt = STRING_ELT(strings, i as R_xlen_t);
        if elt.is_null() {
            let empty_char = Rf_mkChar(c"".as_ptr());
            SET_STRING_ELT(result, i as R_xlen_t, empty_char);
            continue;
        }
        let input_ptr = CHAR(elt);
        if input_ptr.is_null() {
            let empty_char = Rf_mkChar(c"".as_ptr());
            SET_STRING_ELT(result, i as R_xlen_t, empty_char);
            continue;
        }
        let input_bytes = CStr::from_ptr(input_ptr).to_bytes();
        let start_val = if !INTEGER(starts).is_null() {
            *INTEGER(starts).add(i)
        } else {
            0
        };

        let mut buffer: Vec<u8> = Vec::with_capacity(1024);
        let mut byte_idx: usize = 0;
        let mut start = start_val;

        while byte_idx < input_bytes.len() {
            let c = input_bytes[byte_idx];
            /* only the first byte of multi-byte chars counts */
            if (0x80..=0xBF).contains(&c) {
                start -= 1;
            } else if c == b'\n' {
                start = (buffer.len() as c_int) - 1;
            }
            if c == b'\t' {
                loop {
                    buffer.push(b' ');
                    if ((buffer.len() as c_int + start) & 7) == 0 {
                        break;
                    }
                }
            } else {
                buffer.push(c);
            }
            if buffer.len() >= buffer.capacity() - 8 {
                buffer.reserve(buffer.capacity());
            }
            byte_idx += 1;
        }

        // Create a null-terminated C string from the buffer
        buffer.push(0);
        let char_sxp = Rf_mkChar(buffer.as_ptr() as *const c_char);
        SET_STRING_ELT(result, i as R_xlen_t, char_sxp);
    }

    Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// splitString
// ---------------------------------------------------------------------------

/// Split a string by delimiter characters.
pub unsafe fn splitString(string: SEXP, delims: SEXP) -> SEXP {
    if string.is_null() || delims.is_null() {
        return R_NilValue();
    }
    if Rf_isString(string) == 0 || LENGTH(string) != 1 {
        Rf_error(b"first arg must be a single character string\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if Rf_isString(delims) == 0 || LENGTH(delims) != 1 {
        Rf_error(b"first arg must be a single character string\0".as_ptr() as *const _);
        return R_NilValue();
    }

    // Check for NA_STRING - simplified: just check if null
    let s_elt = STRING_ELT(string, 0);
    let d_elt = STRING_ELT(delims, 0);
    if s_elt.is_null() || d_elt.is_null() {
        // Return NA_STRING
        return Rf_ScalarString(ptr::null_mut());
    }

    let in_bytes = {
        let p = CHAR(s_elt);
        if p.is_null() {
            &[] as &[u8]
        } else {
            CStr::from_ptr(p).to_bytes()
        }
    };
    let del_bytes = {
        let p = CHAR(d_elt);
        if p.is_null() {
            &[] as &[u8]
        } else {
            CStr::from_ptr(p).to_bytes()
        }
    };
    let nc = in_bytes.len();

    // Over-allocate wildly (same as C code)
    let out = Rf_allocVector(SEXPTYPE::STRSXP.0, nc as c_int);
    Rf_protect(out);

    if nc > 0 {
        let mut tmp: Vec<u8> = vec![0u8; nc];
        let mut nthis: usize = 0;
        let mut used: usize = 0;

        for &c in in_bytes.iter() {
            if del_bytes.contains(&c) {
                // put out current string (if any)
                if nthis > 0 {
                    tmp[nthis] = 0;
                    let char_sxp = Rf_mkChar(tmp.as_ptr() as *const c_char);
                    SET_STRING_ELT(out, used as R_xlen_t, char_sxp);
                    used += 1;
                }
                // put out delimiter
                let delim_buf = [c, 0];
                let char_sxp = Rf_mkChar(delim_buf.as_ptr() as *const c_char);
                SET_STRING_ELT(out, used as R_xlen_t, char_sxp);
                used += 1;
                // restart
                nthis = 0;
            } else {
                tmp[nthis] = c;
                nthis += 1;
            }
        }
        if nthis > 0 {
            tmp[nthis] = 0;
            let char_sxp = Rf_mkChar(tmp.as_ptr() as *const c_char);
            SET_STRING_ELT(out, used as R_xlen_t, char_sxp);
            used += 1;
        }

        // lengthgets equivalent: truncate the vector to used elements
        // For now, just return the over-allocated vector (R code handles the length)
        // In a full implementation, we'd resize, but our allocator doesn't support shrinking
        let _ = used;
    }

    Rf_unprotect(1);
    out
}

// ---------------------------------------------------------------------------
// nonASCII
// ---------------------------------------------------------------------------

/// Return a logical vector indicating which strings contain non-ASCII characters.
pub unsafe fn nonASCII(text: SEXP) -> SEXP {
    if text.is_null() {
        return R_NilValue();
    }
    if TYPEOF(text) != SEXPTYPE::STRSXP {
        Rf_error(b"invalid input\0".as_ptr() as *const _);
    }
    let len = XLENGTH(text);
    let ans = Rf_allocVector(SEXPTYPE::LGLSXP.0, len as c_int);
    if ans.is_null() {
        return R_NilValue();
    }
    let lans = LOGICAL(ans);

    for i in 0..len as usize {
        let this = STRING_ELT(text, i as R_xlen_t);
        if this.is_null() {
            *lans.add(i) = 0;
            continue;
        }
        let p = CHAR(this);
        if p.is_null() {
            *lans.add(i) = 0;
            continue;
        }
        let bytes = CStr::from_ptr(p).to_bytes();
        let mut not_ok: c_int = 0;
        for &c in bytes.iter() {
            if (c as u32) > 127 {
                not_ok = 1;
                break;
            }
        }
        *lans.add(i) = not_ok;
    }
    ans
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// SET_STRING_ELT accessor.
#[inline]
unsafe fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    crate::sexp::accessors::SET_STRING_ELT(x, i, val);
}

/// Coerce SEXP to logical.
unsafe fn coerce_to_logical(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_LOGICAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        let p = LOGICAL(x);
        if !p.is_null() {
            return *p;
        }
    } else if t == SEXPTYPE::INTSXP {
        let p = INTEGER(x);
        if !p.is_null() {
            return *p;
        }
    }
    NA_LOGICAL
}
