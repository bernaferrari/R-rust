/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2002--2025     The R Core Team
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/getfmts.c
 *
 *  Formerly part of src/main/sprintf.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::ffi::CStr;
use std::os::raw::c_int;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF};
use crate::sexp::constructors::{Rf_allocVector, Rf_isString, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

const MAXLINE: usize = 8192;
const MAXNARGS: usize = 100;

/// Parse a format string and extract format specifications.
///
/// Returns a character vector of format strings, one per argument.
/// This is used by sprintf to determine the types of arguments needed.
pub unsafe fn getfmts(format: SEXP) -> SEXP {
    unsafe {
        if format.is_null() {
            return R_NilValue();
        }
        if Rf_isString(format) == 0 {
            Rf_error(b"'fmt' is not a character vector\0".as_ptr() as *const _);
        }
        let nfmt = LENGTH(format);
        if nfmt != 1 {
            Rf_error(b"'fmt' must be length 1\0".as_ptr() as *const _);
        }

        let res = Rf_allocVector(SEXPTYPE::STRSXP, MAXNARGS as c_int);
        let _res_guard = protect(res);

        let format_elt = STRING_ELT(format, 0);
        if format_elt.is_null() {
            return res;
        }
        let format_ptr = CHAR(format_elt);
        if format_ptr.is_null() {
            return res;
        }
        let format_bytes = CStr::from_ptr(format_ptr).to_bytes();
        let n = format_bytes.len();
        if n > MAXLINE {
            let msg = std::ffi::CString::new(format!(
                "'fmt' length exceeds maximal format length {}",
                MAXLINE
            ))
            .unwrap_or_default();
            Rf_error(msg.as_ptr());
            return res;
        }

        let mut maxlen: usize = 0;
        let mut cnt: c_int = 0;
        let mut cur: usize = 0;

        while cur < n {
            let cur_format = &format_bytes[cur..];

            if format_bytes[cur] == b'%' {
                if cur < n - 1 && format_bytes[cur + 1] == b'%' {
                    cur += 2;
                    continue;
                }

                let type_chars = b"diosfeEgGxXaAcupn";
                let mut chunk: usize = 1;
                let mut found_type = false;
                for j in 1..cur_format.len() {
                    if type_chars.contains(&cur_format[j]) {
                        chunk = j + 1;
                        found_type = true;
                        break;
                    }
                }
                if !found_type || cur + chunk > n {
                    Rf_error(b"unrecognised format specification\0".as_ptr() as *const _);
                    return res;
                }

                let mut fmt = String::from_utf8_lossy(&cur_format[..chunk]).into_owned();

                let mut nthis: c_int = -1;
                if fmt.len() > 3 {
                    let bytes = fmt.as_bytes();
                    if bytes.len() > 1 && bytes[1] >= b'1' && bytes[1] <= b'9' {
                        let v = (bytes[1] - b'0') as c_int;
                        if bytes.len() > 2 && bytes[2] == b'$' {
                            nthis = v - 1;
                            fmt.replace_range(1..3, "");
                        } else if bytes.len() > 3
                            && bytes[2] >= b'0'
                            && bytes[2] <= b'9'
                            && bytes[3] == b'$'
                        {
                            let v = 10 * v + (bytes[2] - b'0') as c_int;
                            nthis = v - 1;
                            fmt.replace_range(1..4, "");
                        }
                    }
                }

                if let Some(star_pos) = fmt.find('*') {
                    let starc = &fmt[star_pos..];
                    let mut nstar: c_int = -1;
                    let starc_bytes = starc.as_bytes();

                    if starc_bytes.len() > 3 && starc_bytes[1] >= b'1' && starc_bytes[1] <= b'9' {
                        let v = (starc_bytes[1] - b'0') as c_int;
                        if starc_bytes[2] == b'$' {
                            nstar = v - 1;
                            fmt.replace_range(star_pos + 1..star_pos + 3, "");
                        } else if starc_bytes.len() > 3
                            && starc_bytes[2] >= b'0'
                            && starc_bytes[2] <= b'9'
                            && starc_bytes[3] == b'$'
                        {
                            let v = 10 * v + (starc_bytes[2] - b'0') as c_int;
                            nstar = v - 1;
                            fmt.replace_range(star_pos + 1..star_pos + 4, "");
                        }
                    }

                    if nstar < 0 {
                        nstar = cnt;
                        cnt += 1;
                    }

                    if fmt[star_pos + 1..].contains('*') {
                        Rf_error(
                            b"at most one asterisk '*' is supported in each conversion specification\0"
                                .as_ptr() as *const _,
                        );
                        return res;
                    }

                    if (nstar as usize) < MAXNARGS {
                        let star_char = Rf_mkChar(c"*".as_ptr());
                        SET_STRING_ELT(res, nstar as R_xlen_t, star_char);
                        maxlen = if (nstar as usize + 1) > maxlen {
                            nstar as usize + 1
                        } else {
                            maxlen
                        };
                    }
                }

                if !fmt.is_empty() && fmt.as_bytes().last() != Some(&b'%') {
                    if nthis < 0 {
                        nthis = cnt;
                        cnt += 1;
                    }
                    if (nthis as usize) < MAXNARGS {
                        let c_str = std::ffi::CString::new(fmt).unwrap_or_default();
                        let char_sxp = Rf_mkChar(c_str.as_ptr());
                        SET_STRING_ELT(res, nthis as R_xlen_t, char_sxp);
                        maxlen = if (nthis as usize + 1) > maxlen {
                            nthis as usize + 1
                        } else {
                            maxlen
                        };
                    }
                }

                cur += chunk;
            } else {
                let mut chunk: usize = 0;
                for j in cur..n {
                    if format_bytes[j] == b'%' {
                        chunk = j - cur;
                        break;
                    }
                }
                if chunk == 0 {
                    chunk = n - cur;
                }
                cur += chunk;
            }
        }

        let _ = maxlen;
        res
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
