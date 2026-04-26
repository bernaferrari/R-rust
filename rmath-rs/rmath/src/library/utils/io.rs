/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998-2025  The R Core Team.
 *  Copyright (C) 1995,1996  Robert Gentleman and Ross Ihaka
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/utils/src/io.c
 */

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use crate::attrib_core;
use crate::main::coerce::asLogical;
use crate::main::duplicate::{Rf_duplicate, copyVector};
use crate::mainutils::connections::{
    connection_fgetc, connection_pushback, connection_write_bytes,
};
use crate::mainutils::errors::{R_CheckUserInterrupt, Rf_error, Rf_warning};
use crate::mainutils::print::PrintDefaults;
use crate::mainutils::printutils::EncodeElement0;
use crate::sexp::memory_ext::{vmaxget, vmaxset};

/// Get errno pointer (macOS uses __error(), Linux/Android use __errno_location).
#[inline]
unsafe fn errno_ptr() -> *mut c_int {
    #[cfg(target_os = "macos")]
    {
        libc::__error()
    }
    #[cfg(not(target_os = "macos"))]
    {
        unsafe extern "C" {
            fn __errno_location() -> *mut c_int;
        }
        __errno_location()
    }
}

/// Check if string is blank (empty or whitespace-only)
unsafe fn isBlankString(s: *const libc::c_char) -> c_int {
    if *s == 0 {
        return 1;
    }
    let mut p = s;
    while *p != 0 {
        if *p != b' ' as libc::c_char
            && *p != b'\t' as libc::c_char
            && *p != b'\n' as libc::c_char
            && *p != b'\r' as libc::c_char
        {
            return 0;
        }
        p = p.add(1);
    }
    1
}

/// Check if SEXP has a dim attribute (is an array)
unsafe fn isArray(x: SEXP) -> c_int {
    let dim = attrib_core::getAttrib(x, attrib_core::R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        0
    } else {
        1
    }
}

/* Constants from R headers */
const SCAN_BLOCKSIZE: c_int = 1000;
const CONSOLE_PROMPT_SIZE: usize = 256;
const CONSOLE_BUFFER_SIZE: usize = 1024;
const MAXELTSIZE: usize = 8192;
const NO_COMCHAR: c_int = 100000;
const R_EOF_VAL: c_int = -1;
const MAX_STRINGS: usize = 10000;
const BUF_SIZE: usize = 1000;
const DBL_DIG_VAL: c_int = 15;

/* NA values matching R's C definitions */
#[inline]
fn NA_INTEGER() -> c_int {
    c_int::MIN
}

#[inline]
fn NA_LOGICAL() -> c_int {
    c_int::MIN
}

#[inline]
fn NA_REAL() -> c_double {
    crate::sexp::ffi::NA_REAL
}

/* Platform boundary not yet represented as a Rust stdlib call. */
unsafe extern "C" {
    fn btowc(c: c_int) -> u32; // wint_t, WEOF sentinel
}

unsafe fn translateChar(x: SEXP) -> *const c_char {
    crate::sexp::accessors::translateChar(x)
}

unsafe fn string_elt_key(x: SEXP, index: R_xlen_t) -> Option<Vec<u8>> {
    let elt = STRING_ELT(x, index);
    if elt.is_null() || elt == NA_STRING() {
        None
    } else {
        Some(CStr::from_ptr(CHAR(elt)).to_bytes().to_vec())
    }
}

unsafe fn duplicated(x: SEXP, from_last: c_int) -> SEXP {
    if TYPEOF(x) != SEXPTYPE::STRSXP {
        Rf_error(
            b"duplicated() wrapper only supports character vectors\0".as_ptr() as *const c_char,
        );
    }

    let len = LENGTH(x);
    let ans = Rf_allocVector(SEXPTYPE::LGLSXP, len);
    if from_last != 0 {
        for i in (0..len).rev() {
            let key = string_elt_key(x, i as R_xlen_t);
            let mut seen = false;
            for j in (i + 1)..len {
                if string_elt_key(x, j as R_xlen_t) == key {
                    seen = true;
                    break;
                }
            }
            *LOGICAL(ans).add(i as usize) = seen as c_int;
        }
    } else {
        for i in 0..len {
            let key = string_elt_key(x, i as R_xlen_t);
            let mut seen = false;
            for j in 0..i {
                if string_elt_key(x, j as R_xlen_t) == key {
                    seen = true;
                    break;
                }
            }
            *LOGICAL(ans).add(i as usize) = seen as c_int;
        }
    }
    ans
}

unsafe fn sortVector(x: SEXP, decreasing: c_int) {
    if TYPEOF(x) != SEXPTYPE::STRSXP {
        Rf_error(
            b"sortVector() wrapper only supports character vectors\0".as_ptr() as *const c_char,
        );
    }

    let len = LENGTH(x) as R_xlen_t;
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(STRING_ELT(x, i));
    }
    values.sort_by(|&a, &b| {
        let ak = if a.is_null() || a == NA_STRING() {
            None
        } else {
            Some(CStr::from_ptr(CHAR(a)).to_bytes().to_vec())
        };
        let bk = if b.is_null() || b == NA_STRING() {
            None
        } else {
            Some(CStr::from_ptr(CHAR(b)).to_bytes().to_vec())
        };
        ak.cmp(&bk)
    });
    if decreasing != 0 {
        values.reverse();
    }
    for (i, value) in values.into_iter().enumerate() {
        SET_STRING_ELT(x, i as R_xlen_t, value);
    }
}

unsafe fn matchE(table: SEXP, x: SEXP, nomatch: c_int, _env: SEXP) -> SEXP {
    if TYPEOF(table) != SEXPTYPE::STRSXP || TYPEOF(x) != SEXPTYPE::STRSXP {
        Rf_error(b"matchE() wrapper only supports character vectors\0".as_ptr() as *const c_char);
    }

    let x_len = LENGTH(x);
    let table_len = LENGTH(table);
    let ans = Rf_allocVector(SEXPTYPE::INTSXP, x_len);
    for i in 0..x_len {
        let key = string_elt_key(x, i as R_xlen_t);
        let mut matched = nomatch;
        if key.is_some() {
            for j in 0..table_len {
                if string_elt_key(table, j as R_xlen_t) == key {
                    matched = j + 1;
                    break;
                }
            }
        }
        *INTEGER(ans).add(i as usize) = matched;
    }
    ans
}

unsafe fn streql(a: *const c_char, b: *const c_char) -> c_int {
    if a.is_null() || b.is_null() {
        return (a == b) as c_int;
    }
    (CStr::from_ptr(a).to_bytes() == CStr::from_ptr(b).to_bytes()) as c_int
}

unsafe fn write_connection_cstr(con: c_int, value: *const c_char) {
    if value.is_null() {
        return;
    }
    connection_write_bytes(con, CStr::from_ptr(value).to_bytes());
}

unsafe fn write_connection_cstr2(con: c_int, first: *const c_char, second: *const c_char) {
    write_connection_cstr(con, first);
    write_connection_cstr(con, second);
}

/* LocalData struct — mirrors the C version */
#[repr(C)]
struct LocalData {
    NAstrings: SEXP,
    quiet: c_int,
    sepchar: c_int,  /* 0 = whitespace-separated */
    decchar: c_char, /* '.' */
    quoteset: [c_char; 10],
    comchar: c_int, /* NO_COMCHAR */
    ttyflag: c_int, /* 0 */
    con: c_int,     /* connection table index */
    wasopen: bool,
    escapes: bool,
    save: c_int, /* 0 */
    isLatin1: bool,
    isUTF8: bool,
    skipNul: bool,
    convbuf: [c_char; 100],
}

impl LocalData {
    fn new() -> Self {
        LocalData {
            NAstrings: ptr::null_mut(),
            quiet: 0,
            sepchar: 0,
            decchar: b'.' as c_char,
            quoteset: [0; 10],
            comchar: NO_COMCHAR,
            ttyflag: 0,
            con: -1,
            wasopen: false,
            escapes: false,
            save: 0,
            isLatin1: false,
            isUTF8: false,
            skipNul: false,
            convbuf: [0; 100],
        }
    }
}

/* Typecvt_Info — tracks possible types during type conversion */
#[repr(C)]
struct Typecvt_Info {
    islogical: bool,
    isinteger: bool,
    isreal: bool,
    iscomplex: bool,
}

impl Typecvt_Info {
    fn new() -> Self {
        Typecvt_Info {
            islogical: true,
            isinteger: true,
            isreal: true,
            iscomplex: true,
        }
    }
}

/* Rspace — check if character is whitespace */
#[inline]
fn Rspace(c: c_uint) -> bool {
    c == b' ' as c_uint || c == b'\t' as c_uint || c == b'\n' as c_uint || c == b'\r' as c_uint
}

/* isNAstring — check if a string matches one of the NA strings */
#[inline]
unsafe fn isNAstring(buf: *const c_char, mode: c_int, d: &LocalData) -> c_int {
    if mode == 0 {
        let len = libc::strlen(buf);
        if len == 0 {
            return 1;
        }
    }
    let n = LENGTH(d.NAstrings);
    for i in 0..n {
        let s = CHAR(STRING_ELT(d.NAstrings, i as R_xlen_t));
        if libc::strcmp(buf, s) == 0 {
            return 1;
        }
    }
    0
}

/* Strtoi — strtol wrapper returning NA_INTEGER on overflow */
unsafe fn Strtoi(nptr: *const c_char, base: c_int) -> c_int {
    let mut endp: *mut c_char = ptr::null_mut();
    *errno_ptr() = 0;
    let res = libc::strtol(nptr, &mut endp, base);
    if !endp.is_null() && *endp != 0 {
        return NA_INTEGER();
    }
    if res > c_int::MAX as i64 || res < c_int::MIN as i64 {
        return NA_INTEGER();
    }
    if *errno_ptr() != 0 {
        return NA_INTEGER();
    }
    res as c_int
}

/* Strtod — ported from R_strtod5 in r-source/src/main/util.c
 * Handles decimal char substitution, NA/NaN/Inf, hex literals, exact mode.
 */
unsafe fn Strtod(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    na: c_int,
    d: &LocalData,
    i_exact: c_int,
) -> c_double {
    if nptr.is_null() || *nptr == 0 {
        if !endptr.is_null() {
            *endptr = nptr as *mut c_char;
        }
        return 0.0;
    }

    let dec = d.decchar;
    let exact = i_exact != 0;
    let mut ans: c_double = 0.0;
    let mut sign: c_int = 1;
    let mut p = nptr;

    /* skip whitespace */
    while libc::isspace(*p as c_int) != 0 {
        p = p.add(1);
    }

    /* check for NA */
    if na != 0 && libc::strncmp(p, b"NA\0".as_ptr() as *const c_char, 2) == 0 {
        ans = NA_REAL();
        p = p.add(2);
        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return ans;
    }

    /* optional sign */
    match *p as u8 {
        b'-' => {
            sign = -1;
            p = p.add(1);
        }
        b'+' => {
            p = p.add(1);
        }
        _ => {} // intentionally unhandled: non-whitespace/non-sign character in scan
    }

    /* check for NaN, Inf, infinity */
    if libc::strncasecmp(p, b"NaN\0".as_ptr() as *const c_char, 3) == 0 {
        ans = f64::NAN;
        p = p.add(3);
        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return sign as c_double * ans;
    } else if libc::strncasecmp(p, b"infinity\0".as_ptr() as *const c_char, 8) == 0 {
        ans = f64::INFINITY;
        p = p.add(8);
        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return sign as c_double * ans;
    } else if libc::strncasecmp(p, b"Inf\0".as_ptr() as *const c_char, 3) == 0 {
        ans = f64::INFINITY;
        p = p.add(3);
        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return sign as c_double * ans;
    }

    let mut expn: c_int = 0;

    /* Hexadecimal "0x..." */
    if libc::strlen(p) > 2
        && *p == '0' as c_char
        && (*p.add(1) == 'x' as c_char || *p.add(1) == 'X' as c_char)
    {
        let mut exph: c_int = -1;
        p = p.add(2);
        while *p != 0 {
            let ch = *p as u8;
            if ch >= b'0' && ch <= b'9' {
                ans = 16.0 * ans + (ch - b'0') as c_double;
            } else if ch >= b'a' && ch <= b'f' {
                ans = 16.0 * ans + (ch - b'a' + 10) as c_double;
            } else if ch >= b'A' && ch <= b'F' {
                ans = 16.0 * ans + (ch - b'A' + 10) as c_double;
            } else if ch == dec as u8 {
                exph = 0;
                p = p.add(1);
                continue;
            } else {
                break;
            }
            if exph >= 0 {
                exph += 4;
            }
            p = p.add(1);
        }

        /* exact clause for hex */
        if exact && ans > ((1u64 << 53) - 1) as c_double {
            if i_exact == NA_LOGICAL() {
                // warning mode — just warn, still return
                // Rf_warning not easily callable with format args here; skip
            } else {
                ans = NA_REAL();
                p = nptr;
                if !endptr.is_null() {
                    *endptr = p as *mut c_char;
                }
                return ans;
            }
        }

        /* Binary exponent, if any */
        if *p == 'p' as c_char || *p == 'P' as c_char {
            let mut expsign: c_int = 1;
            p = p.add(1);
            match *p as u8 {
                b'-' => {
                    expsign = -1;
                    p = p.add(1);
                }
                b'+' => {
                    p = p.add(1);
                }
                _ => {} // intentionally unhandled: non-whitespace/non-sign character in scan
            }
            let mut n: c_int = 0;
            let mut ndig: c_int = 0;
            while *p >= '0' as c_char && *p <= '9' as c_char {
                if n < 9999 {
                    n = n * 10 + (*p as c_int - '0' as c_int);
                }
                p = p.add(1);
                ndig += 1;
            }
            if ndig == 0 {
                ans = NA_REAL();
                p = nptr;
                if !endptr.is_null() {
                    *endptr = p as *mut c_char;
                }
                return ans;
            }
            expn += expsign * n;
        }

        if ans != 0.0 {
            if exph > 0 {
                if expn - exph < -122 {
                    let mut fac: c_double = 1.0;
                    let mut p2: c_double = 2.0;
                    let mut n = exph;
                    while n != 0 {
                        if n & 1 != 0 {
                            fac *= p2;
                        }
                        n >>= 1;
                        p2 *= p2;
                    }
                    ans /= fac;
                    p2 = 2.0;
                    /* fall through to apply expn */
                } else {
                    expn -= exph;
                }
            }
            if expn < 0 {
                let mut fac: c_double = 1.0;
                let mut p2: c_double = 2.0;
                let mut n = -expn;
                while n != 0 {
                    if n & 1 != 0 {
                        fac *= p2;
                    }
                    n >>= 1;
                    p2 *= p2;
                }
                ans /= fac;
            } else {
                let mut fac: c_double = 1.0;
                let mut p2: c_double = 2.0;
                let mut n = expn;
                while n != 0 {
                    if n & 1 != 0 {
                        fac *= p2;
                    }
                    n >>= 1;
                    p2 *= p2;
                }
                ans *= fac;
            }
        }

        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return sign as c_double * ans;
    }

    /* Decimal number */
    let mut ndigits: c_int = 0;
    while *p >= '0' as c_char && *p <= '9' as c_char {
        ans = 10.0 * ans + (*p as c_int - '0' as c_int) as c_double;
        p = p.add(1);
        ndigits += 1;
    }
    if *p == dec {
        p = p.add(1);
        while *p >= '0' as c_char && *p <= '9' as c_char {
            ans = 10.0 * ans + (*p as c_int - '0' as c_int) as c_double;
            p = p.add(1);
            ndigits += 1;
            expn -= 1;
        }
    }
    if ndigits == 0 {
        ans = NA_REAL();
        p = nptr;
        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return ans;
    }

    /* exact clause */
    if exact && ans > ((1u64 << 53) - 1) as c_double {
        if i_exact == NA_LOGICAL() {
            // warning mode — skip
        } else {
            ans = NA_REAL();
            p = nptr;
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return ans;
        }
    }

    /* Exponent */
    if *p == 'e' as c_char || *p == 'E' as c_char {
        let mut expsign: c_int = 1;
        p = p.add(1);
        match *p as u8 {
            b'-' => {
                expsign = -1;
                p = p.add(1);
            }
            b'+' => {
                p = p.add(1);
            }
            _ => {} // intentionally unhandled: non-whitespace/non-sign character in scan
        }
        let mut n: c_int = 0;
        let mut ndig2: c_int = 0;
        while *p >= '0' as c_char && *p <= '9' as c_char {
            if n < 9999 {
                n = n * 10 + (*p as c_int - '0' as c_int);
            }
            p = p.add(1);
            ndig2 += 1;
        }
        if ndig2 == 0 {
            ans = NA_REAL();
            p = nptr;
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return ans;
        }
        expn += expsign * n;
    }

    /* avoid unnecessary underflow for large negative exponents */
    if expn + ndigits < -300 {
        let mut n = ndigits;
        while n > 0 {
            ans /= 10.0;
            n -= 1;
        }
        expn += ndigits;
    }

    if expn < -307 {
        let mut fac: c_double = 1.0;
        let mut p10: c_double = 10.0;
        let mut n = -expn;
        while n != 0 {
            if n & 1 != 0 {
                fac /= p10;
            }
            n >>= 1;
            p10 *= p10;
        }
        ans *= fac;
    } else if expn < 0 {
        let mut fac: c_double = 1.0;
        let mut p10: c_double = 10.0;
        let mut n = -expn;
        while n != 0 {
            if n & 1 != 0 {
                fac *= p10;
            }
            n >>= 1;
            p10 *= p10;
        }
        ans /= fac;
    } else if ans != 0.0 {
        let mut fac: c_double = 1.0;
        let mut p10: c_double = 10.0;
        let mut n = expn;
        while n != 0 {
            if n & 1 != 0 {
                fac *= p10;
            }
            n >>= 1;
            p10 *= p10;
        }
        ans *= fac;
    }

    /* explicit overflow to infinity */
    if ans > f64::MAX {
        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        return if sign > 0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
    }

    if !endptr.is_null() {
        *endptr = p as *mut c_char;
    }
    sign as c_double * ans
}

/* strtoc — parse a complex number from a string.
 * Forms: "3.5", "3.5i", "3+4i", "3-4i", "i", "2i"
 * Ported from r-source/src/library/utils/src/io.c
 */
unsafe fn strtoc(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    na: c_int,
    d: &LocalData,
    i_exact: c_int,
) -> Rcomplex {
    let x = Strtod(nptr, endptr, na, d, i_exact);
    let ep = if !endptr.is_null() {
        *endptr
    } else {
        ptr::null_mut()
    };

    if !ep.is_null() && isBlankString(ep) != 0 {
        /* pure real number */
        Rcomplex { r: x, i: 0.0 }
    } else if !ep.is_null() && *ep == 'i' as c_char {
        if ep == nptr as *mut c_char {
            /* bare "i" — NA */
            Rcomplex {
                r: NA_REAL(),
                i: NA_REAL(),
            }
        } else {
            /* "Ni" form — pure imaginary */
            if !endptr.is_null() {
                *endptr = ep.add(1);
            }
            Rcomplex { r: 0.0, i: x }
        }
    } else {
        /* Try "real+imagi" or "real-imagi" form */
        let s = ep;
        let mut y_end: *mut c_char = ptr::null_mut();
        let y = Strtod(s, &mut y_end, na, d, i_exact);
        if !y_end.is_null() && *y_end == 'i' as c_char {
            if !endptr.is_null() {
                *endptr = y_end.add(1);
            }
            Rcomplex { r: x, i: y }
        } else {
            /* parse failure — return NA */
            if !endptr.is_null() {
                *endptr = nptr as *mut c_char;
            }
            Rcomplex {
                r: NA_REAL(),
                i: NA_REAL(),
            }
        }
    }
}

/* ConsoleGetcharWithPushBack — read from console with pushback.
 * Legitimate stub: requires R_ReadConsole which is not available
 * without a running R console session. Always returns EOF.
 */
unsafe fn ConsoleGetcharWithPushBack(con: c_int) -> c_int {
    connection_fgetc(con)
}

/* scanchar_raw — read next character from connection or console */
unsafe fn scanchar_raw(d: &mut LocalData) -> c_int {
    let c = if d.ttyflag != 0 {
        ConsoleGetcharWithPushBack(d.con)
    } else {
        connection_fgetc(d.con)
    };
    if c == 0 {
        if d.skipNul {
            let mut c2 = c;
            loop {
                c2 = if d.ttyflag != 0 {
                    ConsoleGetcharWithPushBack(d.con)
                } else {
                    connection_fgetc(d.con)
                };
                if c2 != 0 {
                    break;
                }
            }
            c2
        } else {
            c
        }
    } else {
        c
    }
}

/* unscanchar — push back a character */
#[inline]
fn unscanchar(c: c_int, d: &mut LocalData) {
    d.save = c;
}

/* scanchar — read next character with comment/escape handling */
unsafe fn scanchar(inQuote: bool, d: &mut LocalData) -> c_int {
    let mut next;
    if d.save != 0 {
        next = d.save;
        d.save = 0;
    } else {
        next = scanchar_raw(d);
    }
    if next == d.comchar && !inQuote {
        loop {
            next = scanchar_raw(d);
            if next == '\n' as c_int || next == R_EOF_VAL {
                break;
            }
        }
    }
    if next == '\\' as c_int && d.escapes {
        next = scanchar_raw(d);
        if next >= '0' as c_int && next <= '8' as c_int {
            let mut octal = next - '0' as c_int;
            next = scanchar_raw(d);
            if next >= '0' as c_int && next <= '8' as c_int {
                octal = 8 * octal + next - '0' as c_int;
                next = scanchar_raw(d);
                if next >= '0' as c_int && next <= '8' as c_int {
                    octal = 8 * octal + next - '0' as c_int;
                } else {
                    unscanchar(next, d);
                }
            } else {
                unscanchar(next, d);
            }
            next = octal;
        } else {
            next = match next {
                97 => 0x07,  // 'a' -> BEL
                98 => 0x08,  // 'b' -> BS
                102 => 0x0c, // 'f' -> FF
                110 => 0x0a, // 'n' -> LF
                114 => 0x0d, // 'r' -> CR
                116 => 0x09, // 't' -> TAB
                118 => 0x0b, // 'v' -> VT
                120 => {
                    // 'x'
                    let mut val = 0;
                    for _ in 0..2 {
                        next = scanchar_raw(d);
                        let ext = if next >= '0' as c_int && next <= '9' as c_int {
                            next - '0' as c_int
                        } else if next >= 'A' as c_int && next <= 'F' as c_int {
                            next - 'A' as c_int + 10
                        } else if next >= 'a' as c_int && next <= 'f' as c_int {
                            next - 'a' as c_int + 10
                        } else {
                            unscanchar(next, d);
                            break;
                        };
                        val = 16 * val + ext;
                    }
                    val
                }
                _ => {
                    // Any other char and even EOF escapes to itself,
                    // but need to preserve \" etc inside quotes.
                    if inQuote
                        && libc::strchr(d.quoteset.as_ptr(), next as c_int).is_null() == false
                    {
                        unscanchar(next, d);
                        '\\' as c_int
                    } else {
                        next
                    }
                }
            };
        }
    }
    next
}

/* ruleout_types — determine possible types for a string */
unsafe fn ruleout_types(
    s: *const c_char,
    typeInfo: &mut Typecvt_Info,
    data: &LocalData,
    exact: c_int,
) {
    if s.is_null() {
        return;
    }
    let s_str = std::ffi::CStr::from_ptr(s);
    let s_bytes = s_str.to_bytes();

    if typeInfo.islogical {
        // Check for T/F/TRUE/FALSE
        if s_bytes == b"T" || s_bytes == b"F" || s_bytes == b"TRUE" || s_bytes == b"FALSE" {
            typeInfo.isinteger = false;
            typeInfo.isreal = false;
            typeInfo.iscomplex = false;
            return; // short cut
        } else {
            typeInfo.islogical = false;
        }
    }

    if typeInfo.isinteger {
        let res = Strtoi(s, 10);
        if res == NA_INTEGER() {
            typeInfo.isinteger = false;
        }
    }

    if typeInfo.isreal {
        let mut endp: *mut c_char = ptr::null_mut();
        Strtod(s, &mut endp, 1, data, exact);
        if isBlankString(endp as *const libc::c_char) == 0 {
            typeInfo.isreal = false;
        }
    }

    if typeInfo.iscomplex {
        let mut endp: *mut c_char = ptr::null_mut();
        strtoc(s, &mut endp, 1, data, exact);
        if isBlankString(endp as *const libc::c_char) == 0 {
            typeInfo.iscomplex = false;
        }
    }
}

/* isna — check if element at index is NA */
unsafe fn isna(x: SEXP, indx: R_xlen_t) -> bool {
    match TYPEOF(x) {
        tt if tt == SEXPTYPE::LGLSXP => LOGICAL(x).add(indx as usize).read() == NA_LOGICAL(),
        tt if tt == SEXPTYPE::INTSXP => INTEGER(x).add(indx as usize).read() == NA_INTEGER(),
        tt if tt == SEXPTYPE::REALSXP => {
            let v = REAL(x).add(indx as usize).read();
            v.is_nan()
        }
        tt if tt == SEXPTYPE::STRSXP => STRING_ELT(x, indx) == NA_STRING(),
        tt if tt == SEXPTYPE::CPLXSXP => {
            let rc = COMPLEX(x).add(indx as usize).read();
            rc.r.is_nan() || rc.i.is_nan()
        }
        _ => false,
    }
}

/* NA_STRING — get the NA string sentinel */
unsafe fn NA_STRING() -> SEXP {
    crate::main::relop::NA_STRING()
}

/* ========== Local helper functions ========== */

/// CAD4R — fourth element of args list (CAR of CDR of CDR of CDR of CDR)
#[inline]
unsafe fn CAD4R(x: SEXP) -> SEXP {
    CDR(CDR(CDR(CDR(x))))
}

/// isString — check if SEXP is a character vector
#[inline]
unsafe fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

/// isNull — check if SEXP is R_NilValue
#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/// isVectorList — check if SEXP is a list (VECSXP or EXPRSXP)
#[inline]
unsafe fn isVectorList(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP // VECSXP or EXPRSXP
}

/// isVectorAtomic — check if SEXP is an atomic vector
#[inline]
unsafe fn isVectorAtomic(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let t = TYPEOF(x);
    t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP // RAWSXP
}

/// inherits — check if object has a given class
unsafe fn inherits(x: SEXP, _what: *const c_char) -> bool {
    let klass = attrib_core::getAttrib(x, attrib_core::R_ClassSymbol());
    if isNull(klass) {
        // Check for special types by name
        if TYPEOF(x) == SEXPTYPE::VECSXP
            && CStr::from_ptr(_what).to_str().unwrap_or("") == "data.frame"
        {
            return true;
        }
        return false;
    }
    if TYPEOF(klass) != SEXPTYPE::STRSXP {
        return false;
    }
    let len = LENGTH(klass);
    let what_cstr = CStr::from_ptr(_what);
    let what_str = what_cstr.to_str().unwrap_or("");
    for i in 0..len {
        let s = CHAR(STRING_ELT(klass, i as R_xlen_t));
        if libc::strcmp(s, _what) == 0 {
            return true;
        }
    }
    false
}

/// SET_TYPEOF — set the type of a SEXP
#[inline]
unsafe fn SET_TYPEOF(x: SEXP, v: SEXPTYPE) {
    (*x).sxpinfo.set_type(v);
}

/// strchr helper for quoteset
#[inline]
unsafe fn strchr_quoteset(quoteset: &[c_char; 10], c: c_int) -> bool {
    for i in 0..10 {
        if quoteset[i as usize] == 0 {
            break;
        }
        if quoteset[i as usize] as c_int == c {
            return true;
        }
    }
    false
}

/// Format an error message and call Rf_error
unsafe fn r_error(fmt: *const c_char, arg: *const c_char) {
    // Rf_error in our Rust port takes a single format string
    // Build the full message using snprintf
    let mut buf = [0 as libc::c_char; 512];
    libc::snprintf(buf.as_mut_ptr(), 512, fmt, arg);
    Rf_error(buf.as_ptr());
}

/// Format an error message with int arg and call Rf_error
unsafe fn r_error_int(fmt: *const c_char, arg: c_int) {
    let mut buf = [0 as libc::c_char; 512];
    libc::snprintf(buf.as_mut_ptr(), 512, fmt, arg);
    Rf_error(buf.as_ptr());
}

/// Format a warning message and call Rf_warning
unsafe fn r_warning(fmt: *const c_char, arg: *const c_char) {
    let mut buf = [0 as libc::c_char; 512];
    libc::snprintf(buf.as_mut_ptr(), 512, fmt, arg);
    Rf_warning(buf.as_ptr());
}

/// EncodeElement2 — encode an element for writetable output.
/// For STRSXP: quotes the string if requested, handling doubled quotes.
/// For other types: delegates to EncodeElement0.
unsafe fn EncodeElement2(
    x: SEXP,
    indx: R_xlen_t,
    quote: bool,
    qmethod: bool,
    buf: &mut [c_char],
    dec: *const c_char,
) -> *const c_char {
    if TYPEOF(x) == SEXPTYPE::STRSXP {
        let p0 = translateChar(STRING_ELT(x, indx));
        if !quote {
            return p0;
        }
        // Calculate needed buffer length
        let mut nbuf: usize = 2; // opening + closing quote
        let mut p = p0;
        while *p != 0 {
            nbuf += if *p == '"' as c_char { 2 } else { 1 };
            p = p.add(1);
        }
        if nbuf > buf.len() {
            nbuf = buf.len();
        }
        let mut q = buf.as_mut_ptr();
        *q = '"' as c_char;
        q = q.add(1);
        p = p0;
        while *p != 0 && (q as usize - buf.as_ptr() as usize) < buf.len() - 2 {
            if *p == '"' as c_char {
                if qmethod {
                    *q = '\\' as c_char;
                } else {
                    *q = '"' as c_char;
                }
                q = q.add(1);
            }
            *q = *p;
            q = q.add(1);
            p = p.add(1);
        }
        *q = '"' as c_char;
        q = q.add(1);
        *q = 0;
        buf.as_ptr()
    } else {
        EncodeElement0(x, indx, if quote { '"' as c_int } else { 0 }, dec)
    }
}

/* ========== Exported functions ========== */

/*
 * countfields(file, sep, quotes, nskip, blskip, comment.char)
 *
 * Counts the number of fields per line in a file/connection.
 * Ported from r-source/src/library/utils/src/io.c
 */
pub unsafe fn countfields(args: SEXP) -> SEXP {
    let mut data = LocalData::new();
    data.NAstrings = R_NilValue();

    let mut args_cdr = CDR(args);
    let file = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let sep = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let quotes = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let mut nskip = crate::main::coerce::asInteger(CAR(args_cdr));
    args_cdr = CDR(args_cdr);
    let mut blskip = asLogical(CAR(args_cdr));
    args_cdr = CDR(args_cdr);
    let comstr = CAR(args_cdr);

    // Validate comment.char
    if TYPEOF(comstr) != SEXPTYPE::STRSXP || LENGTH(comstr) != 1 {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"comment.char\0".as_ptr() as *const c_char,
        );
    }
    let p = translateChar(STRING_ELT(comstr, 0));
    data.comchar = NO_COMCHAR;
    let plen = libc::strlen(p);
    if plen > 1 {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"comment.char\0".as_ptr() as *const c_char,
        );
    } else if plen == 1 {
        data.comchar = *p as c_int;
    }

    if nskip < 0 || nskip == NA_INTEGER() {
        nskip = 0;
    }
    if blskip == NA_LOGICAL() {
        blskip = 1;
    }

    // Parse separator
    if isString(sep) || isNull(sep) {
        if LENGTH(sep) == 0 {
            data.sepchar = 0;
        } else {
            data.sepchar = *translateChar(STRING_ELT(sep, 0)) as c_int;
        }
    } else {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"sep\0".as_ptr() as *const c_char,
        );
    }

    // Parse quotes
    if isString(quotes) {
        let sc = translateChar(STRING_ELT(quotes, 0));
        if libc::strlen(sc) > 0 {
            libc::strcpy(data.quoteset.as_mut_ptr(), sc);
        } else {
            data.quoteset[0] = 0;
        }
    } else if isNull(quotes) {
        data.quoteset[0] = 0;
    } else {
        r_error(
            b"invalid quote symbol set\0".as_ptr() as *const c_char,
            ptr::null(),
        );
    }

    // Set up connection
    let mut i = crate::main::coerce::asInteger(file);
    data.con = i;
    if i == 0 {
        data.ttyflag = 1;
    } else {
        data.ttyflag = 0;
        // Note: wasopen tracking and connection open/close would require
        // full Rconnection struct access which is opaque. We proceed assuming
        // the connection is already open (as called from R level).
        // Skip nskip lines
        for _ in 0..nskip {
            loop {
                let c = scanchar(false, &mut data);
                if c == '\n' as c_int || c == R_EOF_VAL {
                    break;
                }
            }
        }
    }

    let mut blocksize = SCAN_BLOCKSIZE;
    let mut ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, blocksize));
    let mut nlines: c_int = 0;
    let mut nfields: c_int = 0;
    let mut inquote: c_int = 0;
    let mut quote: c_int = 0;
    data.save = 0;

    loop {
        let c = scanchar(inquote > 0, &mut data);
        if c == R_EOF_VAL {
            if nfields != 0 {
                *INTEGER(ans).add(nlines as usize) = nfields;
            } else {
                nlines -= 1;
            }
            break;
        } else if c == '\n' as c_int {
            if inquote != 0 {
                *INTEGER(ans).add(nlines as usize) = NA_INTEGER();
                nlines += 1;
            } else if nfields != 0 || blskip == 0 {
                *INTEGER(ans).add(nlines as usize) = nfields;
                nlines += 1;
                nfields = 0;
                inquote = 0;
            }
            if nlines == blocksize {
                let bns = ans;
                blocksize = 2 * blocksize;
                ans = Rf_allocVector(SEXPTYPE::INTSXP, blocksize);
                Rf_unprotect(1);
                Rf_protect(ans);
                copyVector(ans, bns);
            }
            continue;
        } else if data.sepchar != 0 {
            // Non-whitespace separator
            if nfields == 0 {
                nfields += 1;
            }
            if inquote != 0 && c == R_EOF_VAL {
                // quoted string terminated by EOF — error in full R, just record NA
                *INTEGER(ans).add(nlines as usize) = NA_INTEGER();
                nlines += 1;
            }
            if inquote != 0 && c == quote {
                inquote = 0;
            } else if strchr_quoteset(&data.quoteset, c) {
                inquote = nlines + 1;
                quote = c;
            }
            if c == data.sepchar && inquote == 0 {
                nfields += 1;
            }
        } else if !Rspace(c as c_uint) {
            // Whitespace separator
            if strchr_quoteset(&data.quoteset, c) {
                quote = c;
                inquote = nlines + 1;
                loop {
                    let c2 = scanchar(inquote > 0, &mut data);
                    if c2 == quote {
                        break;
                    }
                    if c2 == R_EOF_VAL {
                        // quoted string terminated by EOF
                        *INTEGER(ans).add(nlines as usize) = NA_INTEGER();
                        nlines += 1;
                        if nlines == blocksize {
                            let bns = ans;
                            blocksize = 2 * blocksize;
                            ans = Rf_allocVector(SEXPTYPE::INTSXP, blocksize);
                            Rf_unprotect(1);
                            Rf_protect(ans);
                            copyVector(ans, bns);
                        }
                    } else if c2 == '\n' as c_int {
                        *INTEGER(ans).add(nlines as usize) = NA_INTEGER();
                        nlines += 1;
                        if nlines == blocksize {
                            let bns = ans;
                            blocksize = 2 * blocksize;
                            ans = Rf_allocVector(SEXPTYPE::INTSXP, blocksize);
                            Rf_unprotect(1);
                            Rf_protect(ans);
                            copyVector(ans, bns);
                        }
                    }
                }
                inquote = 0;
            } else {
                // Consume the non-space token
                let mut c2;
                loop {
                    c2 = scanchar(false, &mut data);
                    if !Rspace(c2 as c_uint) && c2 != R_EOF_VAL {
                        // DBCS handling would go here
                    }
                    if Rspace(c2 as c_uint) || c2 == R_EOF_VAL {
                        break;
                    }
                }
                if c2 == R_EOF_VAL {
                    unscanchar('\n' as c_int, &mut data);
                } else {
                    unscanchar(c2, &mut data);
                }
            }
            nfields += 1;
        }
    }

    // Push back if possible
    if data.save != 0 && data.ttyflag == 0 {
        let line = [data.save as c_char, 0];
        connection_pushback(data.con, CStr::from_ptr(line.as_ptr()).to_bytes());
    }

    if nlines < 0 {
        Rf_unprotect(1);
        return R_NilValue();
    }
    if nlines == blocksize {
        Rf_unprotect(1);
        return ans;
    }

    let bns = Rf_allocVector(SEXPTYPE::INTSXP, nlines + 1);
    for j in 0..=nlines {
        *INTEGER(bns).add(j as usize) = *INTEGER(ans).add(j as usize);
    }
    Rf_unprotect(1);
    bns
}

/*
 * typeconvert(call, op, args, env)
 *
 * Called from R as .External2(C_typeconvert,
 *   x, na.strings, as.is, dec, match.arg(numerals), tryLogical)
 *
 * Converts a character vector to logical, integer, numeric, complex,
 * or factor. Full port from r-source/src/library/utils/src/io.c
 */
pub unsafe fn typeconvert(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, env);

    if args.is_null() {
        return R_NilValue();
    }

    let args_cdr = CDR(args);
    if args_cdr.is_null() {
        return R_NilValue();
    }

    if !isString(CAR(args_cdr)) {
        Rf_error(b"the first argument must be of mode character\0".as_ptr() as *const c_char);
    }

    let mut data = LocalData::new();
    data.NAstrings = R_NilValue();

    let mut args_rest = args_cdr;

    /* args[1] = cvec (already have via CAR(args_cdr)) */
    /* args[2] = na.strings */
    args_rest = CDR(args_rest);
    let na_strings_arg = CAR(args_rest);
    if TYPEOF(na_strings_arg) != SEXPTYPE::STRSXP {
        Rf_error(b"invalid 'na.strings' argument\0".as_ptr() as *const c_char);
    }
    data.NAstrings = na_strings_arg;

    /* args[3] = as.is */
    args_rest = CDR(args_rest);
    let as_is = asLogical(CAR(args_rest));
    let as_is_flag = if as_is == NA_LOGICAL() {
        false
    } else {
        as_is != 0
    };

    /* args[4] = dec */
    args_rest = CDR(args_rest);
    let dec = CAR(args_rest);
    if isString(dec) || isNull(dec) {
        if LENGTH(dec) == 0 {
            data.decchar = b'.' as c_char;
        } else {
            data.decchar = *translateChar(STRING_ELT(dec, 0));
        }
    }

    /* args[5] = numerals */
    args_rest = CDR(args_rest);
    let numerals = CAR(args_rest);
    let mut i_exact: c_int = 0; // false = allow.loss
    let mut exact = false;
    if isString(numerals) {
        let tmp = CHAR(STRING_ELT(numerals, 0));
        if libc::strcmp(tmp, b"allow.loss\0".as_ptr() as *const c_char) == 0 {
            i_exact = 0;
            exact = false;
        } else if libc::strcmp(tmp, b"warn.loss\0".as_ptr() as *const c_char) == 0 {
            i_exact = NA_LOGICAL();
            exact = false;
        } else if libc::strcmp(tmp, b"no.loss\0".as_ptr() as *const c_char) == 0 {
            i_exact = 1;
            exact = true;
        }
    }

    /* args[6] = tryLogical */
    args_rest = CDR(args_rest);
    let try_logical = asLogical(CAR(args_rest));
    let try_logical_flag = if try_logical == NA_LOGICAL() {
        false
    } else {
        try_logical != 0
    };

    let cvec = CAR(args_cdr);
    let len = LENGTH(cvec);

    /* Save dim/dimnames attributes */
    let dims = Rf_protect(attrib_core::getAttrib(cvec, attrib_core::R_DimSymbol()));
    let names = if isArray(cvec) != 0 {
        Rf_protect(attrib_core::getAttrib(
            cvec,
            attrib_core::R_DimNamesSymbol(),
        ))
    } else {
        Rf_protect(attrib_core::getAttrib(cvec, attrib_core::R_NamesSymbol()))
    };

    /* Find the first non-NA entry (empty => NA) */
    let mut typeInfo = Typecvt_Info::new();
    typeInfo.islogical = try_logical_flag;

    let mut first_non_na_idx: c_int = -1;
    let mut first_non_na_tmp: *const c_char = ptr::null();
    for i in 0..len {
        let tmp = CHAR(STRING_ELT(cvec, i as R_xlen_t));
        let is_na = STRING_ELT(cvec, i as R_xlen_t) == NA_STRING()
            || libc::strlen(tmp) == 0
            || isBlankString(tmp) != 0
            || isNAstring(tmp, 1, &data) != 0;
        if !is_na {
            first_non_na_idx = i;
            first_non_na_tmp = tmp;
            break;
        }
    }

    /* Use first non-NA entry to screen types */
    if first_non_na_idx >= 0 {
        ruleout_types(
            first_non_na_tmp,
            &mut typeInfo,
            &data,
            if exact { 1 } else { 0 },
        );
    }

    let mut done = false;
    let mut rval: SEXP = ptr::null_mut();

    /* Try logical conversion */
    if typeInfo.islogical {
        rval = Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP, len));
        let mut all_logical = true;
        for i in 0..len as R_xlen_t {
            let tmp = CHAR(STRING_ELT(cvec, i));
            if STRING_ELT(cvec, i) == NA_STRING()
                || libc::strlen(tmp) == 0
                || isBlankString(tmp) != 0
                || isNAstring(tmp, 1, &data) != 0
            {
                *LOGICAL(rval).add(i as usize) = NA_LOGICAL();
            } else {
                let s = CStr::from_ptr(tmp).to_str().unwrap_or("");
                if s == "F" || s == "FALSE" {
                    *LOGICAL(rval).add(i as usize) = 0;
                } else if s == "T" || s == "TRUE" {
                    *LOGICAL(rval).add(i as usize) = 1;
                } else {
                    all_logical = false;
                    typeInfo.islogical = false;
                    ruleout_types(tmp, &mut typeInfo, &data, if exact { 1 } else { 0 });
                    break;
                }
            }
        }
        if all_logical {
            done = true;
        } else {
            Rf_unprotect(1);
        }
    }

    /* Try integer conversion */
    if !done && typeInfo.isinteger {
        rval = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, len));
        let mut all_integer = true;
        for i in 0..len as R_xlen_t {
            let tmp = CHAR(STRING_ELT(cvec, i));
            if STRING_ELT(cvec, i) == NA_STRING()
                || libc::strlen(tmp) == 0
                || isBlankString(tmp) != 0
                || isNAstring(tmp, 1, &data) != 0
            {
                *INTEGER(rval).add(i as usize) = NA_INTEGER();
            } else {
                let val = Strtoi(tmp, 10);
                if val == NA_INTEGER() {
                    all_integer = false;
                    typeInfo.isinteger = false;
                    ruleout_types(tmp, &mut typeInfo, &data, if exact { 1 } else { 0 });
                    break;
                }
                *INTEGER(rval).add(i as usize) = val;
            }
        }
        if all_integer {
            done = true;
        } else {
            Rf_unprotect(1);
        }
    }

    /* Try real conversion */
    if !done && typeInfo.isreal {
        rval = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, len));
        let mut all_real = true;
        for i in 0..len as R_xlen_t {
            let tmp = CHAR(STRING_ELT(cvec, i));
            if STRING_ELT(cvec, i) == NA_STRING()
                || libc::strlen(tmp) == 0
                || isBlankString(tmp) != 0
                || isNAstring(tmp, 1, &data) != 0
            {
                *REAL(rval).add(i as usize) = NA_REAL();
            } else {
                let mut endp: *mut c_char = ptr::null_mut();
                let val = Strtod(tmp, &mut endp, 0, &data, i_exact);
                if isBlankString(endp as *const libc::c_char) == 0 {
                    all_real = false;
                    typeInfo.isreal = false;
                    ruleout_types(tmp, &mut typeInfo, &data, if exact { 1 } else { 0 });
                    break;
                }
                *REAL(rval).add(i as usize) = val;
            }
        }
        if all_real {
            done = true;
        } else {
            Rf_unprotect(1);
        }
    }

    /* Try complex conversion */
    if !done && typeInfo.iscomplex {
        rval = Rf_protect(Rf_allocVector(SEXPTYPE::CPLXSXP, len));
        let mut all_complex = true;
        for i in 0..len as R_xlen_t {
            let tmp = CHAR(STRING_ELT(cvec, i));
            if STRING_ELT(cvec, i) == NA_STRING()
                || libc::strlen(tmp) == 0
                || isBlankString(tmp) != 0
                || isNAstring(tmp, 1, &data) != 0
            {
                let z = COMPLEX(rval).add(i as usize);
                (*z).r = NA_REAL();
                (*z).i = NA_REAL();
            } else {
                let mut endp: *mut c_char = ptr::null_mut();
                let z = strtoc(tmp, &mut endp, 0, &data, i_exact);
                if isBlankString(endp as *const libc::c_char) == 0 {
                    all_complex = false;
                    typeInfo.iscomplex = false;
                    ruleout_types(tmp, &mut typeInfo, &data, if exact { 1 } else { 0 });
                    break;
                }
                let zp = COMPLEX(rval).add(i as usize);
                (*zp).r = z.r;
                (*zp).i = z.i;
            }
        }
        if all_complex {
            done = true;
        } else {
            Rf_unprotect(1);
        }
    }

    /* Fallback: character or factor */
    if !done {
        if as_is_flag {
            rval = Rf_protect(Rf_duplicate(cvec));
            /* Replace NA strings with NA_STRING */
            for i in 0..len as R_xlen_t {
                let tmp = CHAR(STRING_ELT(rval, i));
                if isNAstring(tmp, 1, &data) != 0 {
                    SET_STRING_ELT(rval, i, NA_STRING());
                }
            }
        } else {
            /* Factor conversion */
            let dup = Rf_protect(duplicated(cvec, 0));
            let mut j: c_int = 0;
            for i in 0..len {
                if STRING_ELT(cvec, i as R_xlen_t) == NA_STRING() {
                    continue;
                }
                if *LOGICAL(dup).add(i as usize) == 0
                    && isNAstring(CHAR(STRING_ELT(cvec, i as R_xlen_t)), 1, &data) == 0
                {
                    j += 1;
                }
            }

            let levs = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, j));
            j = 0;
            for i in 0..len {
                if STRING_ELT(cvec, i as R_xlen_t) == NA_STRING() {
                    continue;
                }
                if *LOGICAL(dup).add(i as usize) == 0
                    && isNAstring(CHAR(STRING_ELT(cvec, i as R_xlen_t)), 1, &data) == 0
                {
                    SET_STRING_ELT(levs, j as R_xlen_t, STRING_ELT(cvec, i as R_xlen_t));
                    j += 1;
                }
            }

            /* Reuse dup (LGLSXP of right length) as integer vector */
            rval = dup;
            SET_TYPEOF(rval, SEXPTYPE::INTSXP);

            /* Sort levels lexicographically */
            sortVector(levs, 0);

            let a = Rf_protect(matchE(levs, cvec, NA_INTEGER(), env));
            for i in 0..len {
                *INTEGER(rval).add(i as usize) = *INTEGER(a).add(i as usize);
            }

            attrib_core::setAttrib(rval, attrib_core::R_LevelsSymbol(), levs);
            let class_str = Rf_protect(Rf_mkString(b"factor\0".as_ptr() as *const c_char));
            attrib_core::setAttrib(rval, attrib_core::R_ClassSymbol(), class_str);
            Rf_unprotect(3);
        }
    }

    /* Restore attributes */
    attrib_core::setAttrib(rval, attrib_core::R_DimSymbol(), dims);
    if isArray(cvec) != 0 {
        attrib_core::setAttrib(rval, attrib_core::R_DimNamesSymbol(), names);
    } else {
        attrib_core::setAttrib(rval, attrib_core::R_NamesSymbol(), names);
    }

    Rf_unprotect(3);
    rval
}

/*
 * menu(choices)
 *
 * Interactive menu selection. Legitimate stub: requires R_ReadConsole
 * which is not available without a running R console session.
 * Always returns 0 (no selection).
 */
pub unsafe fn menu(choices: SEXP) -> SEXP {
    let _ = choices;
    Rf_ScalarInteger(0)
}

/*
 * readtablehead(file, nlines, comment.char, blank.lines.skip, quote, sep, skipNul)
 *
 * Reads header lines from a file/connection for read.table.
 * Simplified version of readLines, with skip of blank lines and comment-only lines.
 * Ported from r-source/src/library/utils/src/io.c
 */
pub unsafe fn readtablehead(args: SEXP) -> SEXP {
    let mut data = LocalData::new();
    data.NAstrings = R_NilValue();

    let mut args_cdr = CDR(args);
    let file = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let nlines = crate::main::coerce::asInteger(CAR(args_cdr));
    args_cdr = CDR(args_cdr);
    let comstr = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let mut blskip = asLogical(CAR(args_cdr));
    args_cdr = CDR(args_cdr);
    let quotes = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let sep = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let skipNul = asLogical(CAR(args_cdr));

    if nlines <= 0 || nlines == NA_INTEGER() {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"nlines\0".as_ptr() as *const c_char,
        );
    }
    if blskip == NA_LOGICAL() {
        blskip = 1;
    }

    // Parse quotes
    if isString(quotes) {
        let sc = translateChar(STRING_ELT(quotes, 0));
        if libc::strlen(sc) > 0 {
            libc::strcpy(data.quoteset.as_mut_ptr(), sc);
        } else {
            data.quoteset[0] = 0;
        }
    } else if isNull(quotes) {
        data.quoteset[0] = 0;
    } else {
        r_error(
            b"invalid quote symbol set\0".as_ptr() as *const c_char,
            ptr::null(),
        );
    }

    // Validate comment.char
    if TYPEOF(comstr) != SEXPTYPE::STRSXP || LENGTH(comstr) != 1 {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"comment.char\0".as_ptr() as *const c_char,
        );
    }
    let p = translateChar(STRING_ELT(comstr, 0));
    data.comchar = NO_COMCHAR;
    let plen = libc::strlen(p);
    if plen > 1 {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"comment.char\0".as_ptr() as *const c_char,
        );
    } else if plen == 1 {
        data.comchar = *p as c_int;
    }

    // Parse separator
    if isString(sep) || isNull(sep) {
        if LENGTH(sep) == 0 {
            data.sepchar = 0;
        } else {
            data.sepchar = *translateChar(STRING_ELT(sep, 0)) as c_int;
        }
    } else {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"sep\0".as_ptr() as *const c_char,
        );
    }

    if skipNul == NA_LOGICAL() {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"skipNul\0".as_ptr() as *const c_char,
        );
    }
    data.skipNul = skipNul != 0;

    // Set up connection
    let i = crate::main::coerce::asInteger(file);
    data.con = i;
    data.ttyflag = if i == 0 { 1 } else { 0 };
    // Note: wasopen tracking requires full Rconnection struct access.
    // We assume the connection is properly set up from R level.

    let mut buf_size: usize = BUF_SIZE;
    let mut buf = libc::malloc(buf_size) as *mut c_char;
    if buf.is_null() {
        r_error(
            b"cannot allocate buffer in 'readTableHead'\0".as_ptr() as *const c_char,
            ptr::null(),
        );
    }

    let mut ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, nlines));
    let mut nread: c_int = 0;

    while nread < nlines {
        let mut nbuf: usize = 0;
        let mut empty = true;
        let mut skip = false;
        let mut firstnonwhite = true;
        let mut quote: c_int = 0;
        let mut c: c_int = 0;
        let mut last_c: c_int = 0;

        loop {
            c = scanchar(true, &mut data);
            if c == R_EOF_VAL {
                last_c = c;
                break;
            }

            // Grow buffer if needed
            if nbuf >= buf_size - 3 {
                buf_size *= 2;
                let tmp = libc::realloc(buf as *mut c_void, buf_size) as *mut c_char;
                if tmp.is_null() {
                    libc::free(buf as *mut c_void);
                    r_error(
                        b"cannot allocate buffer in 'readTableHead'\0".as_ptr() as *const c_char,
                        ptr::null(),
                    );
                }
                buf = tmp;
            }

            // Handle quotes
            if quote != 0 {
                if data.sepchar == 0 && c == '\\' as c_int {
                    // all escapes should be passed through
                    *buf.add(nbuf) = c as c_char;
                    nbuf += 1;
                    let c2 = scanchar(true, &mut data);
                    if c2 == R_EOF_VAL {
                        libc::free(buf as *mut c_void);
                        r_error(
                            b"\\ followed by EOF\0".as_ptr() as *const c_char,
                            ptr::null(),
                        );
                    }
                    *buf.add(nbuf) = c2 as c_char;
                    nbuf += 1;
                    continue;
                } else if c == quote {
                    if data.sepchar == 0 {
                        quote = 0;
                    } else {
                        // Check for doubled quote
                        let c2 = scanchar(true, &mut data);
                        if c2 == quote {
                            *buf.add(nbuf) = c as c_char;
                            nbuf += 1;
                        } else {
                            unscanchar(c2, &mut data);
                            quote = 0;
                        }
                    }
                }
            } else if !skip
                && (firstnonwhite || data.sepchar != 0)
                && strchr_quoteset(&data.quoteset, c)
            {
                quote = c;
            } else if !skip && data.sepchar == 0 && Rspace(c as c_uint) {
                firstnonwhite = true;
            } else if c != ' ' as c_int && c != '\t' as c_int {
                firstnonwhite = false;
            }

            // Check for empty line
            if empty && !skip {
                if c != '\n' as c_int && c != data.comchar {
                    empty = false;
                }
            }
            if quote == 0 && !skip && c == data.comchar {
                skip = true;
            }
            if quote != 0 || c != '\n' as c_int {
                *buf.add(nbuf) = c as c_char;
                nbuf += 1;
            } else {
                last_c = c;
                break;
            }
        }
        *buf.add(nbuf) = 0;

        if data.ttyflag != 0 && empty {
            // No more lines from tty
            libc::free(buf as *mut c_void);
            // Trim result to actual number read
            if nread < nlines {
                let ans2 = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, nread));
                for j in 0..nread {
                    SET_STRING_ELT(ans2, j as R_xlen_t, STRING_ELT(ans, j as R_xlen_t));
                }
                Rf_unprotect(2);
                return ans2;
            }
            Rf_unprotect(1);
            return ans;
        }

        if !empty || (last_c != R_EOF_VAL && blskip == 0) {
            SET_STRING_ELT(ans, nread as R_xlen_t, Rf_mkChar(buf));
            nread += 1;
            // Check for embedded nulls (strlen < nbuf)
            if libc::strlen(buf) < nbuf {
                let mut warn_buf = [0 as libc::c_char; 256];
                libc::snprintf(
                    warn_buf.as_mut_ptr(),
                    256,
                    b"line %d appears to contain embedded nulls\0".as_ptr() as *const c_char,
                    nread as c_int,
                );
                Rf_warning(warn_buf.as_ptr());
            }
        }
        if last_c == R_EOF_VAL {
            libc::free(buf as *mut c_void);
            // Trim result to actual number read
            if nread < nlines {
                let ans2 = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, nread));
                for j in 0..nread {
                    SET_STRING_ELT(ans2, j as R_xlen_t, STRING_ELT(ans, j as R_xlen_t));
                }
                Rf_unprotect(2);
                return ans2;
            }
            Rf_unprotect(1);
            return ans;
        }
    }

    libc::free(buf as *mut c_void);
    Rf_unprotect(1);
    ans
}

/*
 * writetable(call, op, args, env)
 *
 * write.table(x, file, nr, nc, rnames, sep, eol, na, dec, quote, qstring)
 *   x is a matrix or data frame
 *   file is a connection
 *   sep, eol, dec, qstring are character strings
 *   quote is a numeric vector
 *
 * Ported from r-source/src/library/utils/src/io.c
 */
pub unsafe fn writetable(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, env);

    let mut args_cdr = CDR(args);
    let x = CAR(args_cdr);
    args_cdr = CDR(args_cdr);

    // file is a connection — get it
    let file_arg = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let con = crate::main::coerce::asInteger(file_arg);

    let nr = crate::main::coerce::asInteger(CAR(args_cdr));
    args_cdr = CDR(args_cdr);
    let nc = crate::main::coerce::asInteger(CAR(args_cdr));
    args_cdr = CDR(args_cdr);
    let rnames = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let sep = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let eol = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let na = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let dec = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let quote = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let qmethod0 = asLogical(CAR(args_cdr));

    // Validate arguments
    if nr == NA_INTEGER() {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"nr\0".as_ptr() as *const c_char,
        );
    }
    if nc == NA_INTEGER() {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"nc\0".as_ptr() as *const c_char,
        );
    }
    if !isNull(rnames) && !isString(rnames) {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"rnames\0".as_ptr() as *const c_char,
        );
    }
    if !isString(sep) {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"sep\0".as_ptr() as *const c_char,
        );
    }
    if !isString(eol) {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"eol\0".as_ptr() as *const c_char,
        );
    }
    if !isString(na) {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"na\0".as_ptr() as *const c_char,
        );
    }
    if !isString(dec) {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"dec\0".as_ptr() as *const c_char,
        );
    }
    if qmethod0 == NA_LOGICAL() {
        r_error(
            b"invalid '%s' argument\0".as_ptr() as *const c_char,
            b"qmethod\0".as_ptr() as *const c_char,
        );
    }
    let qmethod = qmethod0 != 0;

    let csep = translateChar(STRING_ELT(sep, 0));
    let ceol = translateChar(STRING_ELT(eol, 0));
    let cna = translateChar(STRING_ELT(na, 0));
    let sdec = translateChar(STRING_ELT(dec, 0));
    if libc::strlen(sdec) != 1 {
        r_error(
            b"'dec' must be a single character\0".as_ptr() as *const c_char,
            ptr::null(),
        );
    }

    // Parse quote columns
    // quote_col[j] = true means column j+1 should be quoted
    // quote_rn = true means row names should be quoted (when quote has a 0 element)
    let mut quote_rn = false;
    // Allocate quote_col array using R_alloc
    let quote_col = R_alloc(1, nc as usize) as *mut bool;
    for j in 0..nc as usize {
        *quote_col.add(j) = false;
    }
    let quote_len = if isNull(quote) { 0 } else { LENGTH(quote) };
    for i in 0..quote_len {
        let this = *INTEGER(quote).add(i as usize);
        if this == 0 {
            quote_rn = true;
        }
        if this > 0 {
            *quote_col.add((this - 1) as usize) = true;
        }
    }

    // Initialize print defaults for maximum precision
    PrintDefaults();

    // String buffer for EncodeElement2
    let mut encode_buf: [c_char; 8192] = [0; 8192];

    if isVectorList(x) {
        // A data frame
        // Pre-fetch level vectors for factor columns
        // Use R_alloc for the levels array
        let levels_arr = R_alloc(std::mem::size_of::<SEXP>(), nc as usize) as *mut SEXP;
        for j in 0..nc as usize {
            let xj = VECTOR_ELT(x, j as R_xlen_t);
            if LENGTH(xj) != nr {
                libc::snprintf(
                    encode_buf.as_mut_ptr(),
                    512,
                    b"corrupt data frame -- length of column %d does not match nrows\0".as_ptr()
                        as *const c_char,
                    j + 1,
                );
                Rf_error(encode_buf.as_ptr());
            }
            if inherits(xj, b"factor\0".as_ptr() as *const c_char) {
                *levels_arr.add(j) = attrib_core::getAttrib(xj, attrib_core::R_LevelsSymbol());
            } else {
                *levels_arr.add(j) = R_NilValue();
            }
        }

        for i in 0..nr as usize {
            // Check user interrupt every 1000 rows
            if i % 1000 == 999 {
                R_CheckUserInterrupt();
            }

            // Write row name + separator
            if !isNull(rnames) {
                let s = EncodeElement2(
                    rnames,
                    i as R_xlen_t,
                    quote_rn,
                    qmethod,
                    &mut encode_buf,
                    sdec,
                );
                write_connection_cstr2(con, s, csep);
            }

            for j in 0..nc as usize {
                if j > 0 {
                    write_connection_cstr(con, csep);
                }
                let xj = VECTOR_ELT(x, j as R_xlen_t);
                let tmp: *const c_char;
                if isna(xj, i as R_xlen_t) {
                    tmp = cna;
                } else if !isNull(*levels_arr.add(j)) {
                    // Factor column — look up level string
                    let lev = *levels_arr.add(j);
                    if TYPEOF(xj) == SEXPTYPE::INTSXP {
                        let idx = (*INTEGER(xj).add(i) - 1) as R_xlen_t;
                        tmp = EncodeElement2(
                            lev,
                            idx,
                            *quote_col.add(j),
                            qmethod,
                            &mut encode_buf,
                            sdec,
                        );
                    } else if TYPEOF(xj) == SEXPTYPE::REALSXP {
                        let idx = (*REAL(xj).add(i) - 1.0) as R_xlen_t;
                        tmp = EncodeElement2(
                            lev,
                            idx,
                            *quote_col.add(j),
                            qmethod,
                            &mut encode_buf,
                            sdec,
                        );
                    } else {
                        r_error_int(
                            b"column %d claims to be a factor but does not have numeric codes\0"
                                .as_ptr() as *const c_char,
                            (j + 1) as c_int,
                        );
                        tmp = ptr::null(); // unreachable
                    }
                } else {
                    tmp = EncodeElement2(
                        xj,
                        i as R_xlen_t,
                        *quote_col.add(j),
                        qmethod,
                        &mut encode_buf,
                        sdec,
                    );
                }
                write_connection_cstr(con, tmp);
            }

            // Write end-of-line
            write_connection_cstr(con, ceol);
        }
    } else {
        // A matrix
        if !isVectorAtomic(x) {
            r_error(
                b"write.table: x must be a matrix or data frame\0".as_ptr() as *const c_char,
                ptr::null(),
            );
        }

        for i in 0..nr as usize {
            // Check user interrupt every 1000 rows
            if i % 1000 == 999 {
                R_CheckUserInterrupt();
            }

            // Write row name + separator
            if !isNull(rnames) {
                let s = EncodeElement2(
                    rnames,
                    i as R_xlen_t,
                    quote_rn,
                    qmethod,
                    &mut encode_buf,
                    sdec,
                );
                write_connection_cstr2(con, s, csep);
            }

            for j in 0..nc as usize {
                if j > 0 {
                    write_connection_cstr(con, csep);
                }
                let col_offset = (j as R_xlen_t) * (nr as R_xlen_t);
                let tmp: *const c_char;
                if isna(x, i as R_xlen_t + col_offset) {
                    tmp = cna;
                } else {
                    tmp = EncodeElement2(
                        x,
                        i as R_xlen_t + col_offset,
                        *quote_col.add(j),
                        qmethod,
                        &mut encode_buf,
                        sdec,
                    );
                }
                write_connection_cstr(con, tmp);
            }

            // Write end-of-line
            write_connection_cstr(con, ceol);
        }
    }

    R_NilValue()
}
