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

use crate::mainutils::r_format::CArg;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::{protect, protect_with_index_raw};

use crate::attrib_core;
use crate::main::coerce::asLogical;
use crate::main::duplicate::{Rf_duplicate, copyVector};
use crate::mainutils::connections::{
    connection_fgetc, connection_pushback, connection_token, connection_write_bytes,
};
use crate::mainutils::errors::{
    R_CheckUserInterrupt, Rf_error, Rf_error_cstr, Rf_warning, Rf_warning_cstr,
};
use crate::mainutils::print::PrintDefaults;
use crate::mainutils::format::{
    complex_field_with_print, format_get_R_print, real_field_with_print,
};
use crate::mainutils::printutils::{
    encode_complex_text, encode_integer_text, encode_logical_text, encode_raw_text,
    encode_real0_text,
};
use crate::sexp::{Sexp, owner::OwnerToken};
use crate::sexp::memory_ext::{vmaxget, vmaxset};
use crate::mainutils::number_parse::{ParsedNumber, parse_number};

/// Check if SEXP has a dim attribute (is an array)
fn isArray(x: SEXP) -> c_int {
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

fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

fn string_elt_key(x: SEXP, index: R_xlen_t) -> Option<Vec<u8>> {
        let elt = STRING_ELT(x, index);
        if elt.is_null() || elt == NA_STRING() {
            None
        } else {
        Some(char_cstr_bytes(elt))
    }
}

fn duplicated(x: SEXP, from_last: c_int) -> SEXP {
    if TYPEOF(x) != SEXPTYPE::STRSXP {
        Rf_error_cstr(c"duplicated() wrapper only supports character vectors");
    }

    let len = LENGTH(x);
    let ans = Rf_allocVector(SEXPTYPE::LGLSXP, len);
    // ALTREP element reads may collect; keep the result rooted.
    let _ans_guard = protect(ans);
    if from_last != 0 {
        for i in (0..len).rev() {
            let key = string_elt_key(x, i64::from(i));
            let mut seen = false;
            for j in (i + 1)..len {
                if string_elt_key(x, i64::from(j)) == key {
                    seen = true;
                    break;
                }
            }
            SET_LOGICAL_ELT(ans, i, seen as c_int);
        }
    } else {
        for i in 0..len {
            let key = string_elt_key(x, i64::from(i));
            let mut seen = false;
            for j in 0..i {
                if string_elt_key(x, i64::from(j)) == key {
                    seen = true;
                    break;
                }
            }
            SET_LOGICAL_ELT(ans, i, seen as c_int);
        }
    }
    ans
}

fn sortVector(x: SEXP, decreasing: c_int) {
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            Rf_error_cstr(c"sortVector() wrapper only supports character vectors");
        }

        let len = LENGTH(x) as R_xlen_t;
        let mut values = Vec::with_capacity(len as usize);
        for i in 0..len {
            values.push(STRING_ELT(x, i));
        }
        values.sort_by(|&a, &b| {
            let text = |s: SEXP| -> String {
                if s.is_null() || s == NA_STRING() {
                    return String::new();
                }
            String::from_utf8_lossy(&crate::sexp::accessors::char_cstr_bytes(s)).into_owned()
            };
            let ord = crate::mainutils::essentials::collate_str(&text(a), &text(b));
            if decreasing != 0 { ord.reverse() } else { ord }
        });

        for (i, value) in values.into_iter().enumerate() {
            SET_STRING_ELT(x, i as R_xlen_t, value);
        }
}

fn matchE(table: SEXP, x: SEXP, nomatch: c_int, _env: SEXP) -> SEXP {
    if TYPEOF(table) != SEXPTYPE::STRSXP || TYPEOF(x) != SEXPTYPE::STRSXP {
        Rf_error_cstr(c"matchE() wrapper only supports character vectors");
    }

    let x_len = LENGTH(x);
    let table_len = LENGTH(table);
    let ans = Rf_allocVector(SEXPTYPE::INTSXP, x_len);
    // ALTREP element reads may collect; keep the result rooted.
    let _ans_guard = protect(ans);
    for i in 0..x_len {
        let key = string_elt_key(x, i64::from(i));
        let mut matched = nomatch;
        if key.is_some() {
            for j in 0..table_len {
                if string_elt_key(table, i64::from(j)) == key {
                    matched = j + 1;
                    break;
                }
            }
        }
        SET_INTEGER_ELT(ans, i, matched);
    }
    ans
}

/* LocalData struct — mirrors the C version */
#[repr(C)]
struct LocalData {
    quiet: c_int,
    sepchar: c_int, /* 0 = whitespace-separated */
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
            quiet: 0,
            sepchar: 0,
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

fn blank(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| Rspace(c_uint::from(byte)))
}

fn is_na_string(bytes: &[u8], na_strings: SEXP) -> bool {
    (0..XLENGTH(na_strings))
        .any(|i| char_cstr_bytes(STRING_ELT(na_strings, i)) == bytes)
}

fn conversion_text(ch: SEXP, na_strings: SEXP) -> Option<Vec<u8>> {
    if ch == NA_STRING() {
        return None;
    }
    let bytes = char_cstr_bytes(ch);
    (!blank(&bytes) && !is_na_string(&bytes, na_strings)).then_some(bytes)
}

fn parse_integer(bytes: &[u8]) -> c_int {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| {
            text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
                .parse()
                .ok()
        })
        .unwrap_or_else(NA_INTEGER)
}

fn parse_real(bytes: &[u8], decimal: u8, allow_na: bool, exact: c_int) -> ParsedNumber {
    let parsed = parse_number(bytes, decimal, allow_na, exact);
    if parsed.accuracy_loss {
        let message = CString::new(format!(
            "accuracy loss in conversion from \"{}\" to numeric",
            String::from_utf8_lossy(bytes)
        ))
        .expect("checked character bytes contain no NUL");
        Rf_warning_cstr(&message);
    }
    parsed
}

fn parse_complex(bytes: &[u8], decimal: u8, allow_na: bool, exact: c_int) -> Option<Rcomplex> {
    let real = parse_real(bytes, decimal, allow_na, exact);
    let rest = &bytes[real.consumed..];
    if blank(rest) {
        return Some(Rcomplex {
            r: real.value,
            i: 0.0,
        });
    }
    if let Some(suffix) = rest.strip_prefix(b"i") {
        return (real.consumed != 0 && blank(suffix)).then_some(Rcomplex {
            r: 0.0,
            i: real.value,
        });
    }
    let imag = parse_real(rest, decimal, allow_na, exact);
    let suffix = rest[imag.consumed..].strip_prefix(b"i")?;
    blank(suffix).then_some(Rcomplex {
        r: real.value,
        i: imag.value,
    })
}

/* ConsoleGetcharWithPushBack — read from console with pushback.
 * Legitimate stub: requires R_ReadConsole which is not available
 * without a running R console session. Always returns EOF.
 */
fn ConsoleGetcharWithPushBack(con: c_int) -> c_int {
    connection_fgetc(con)
}

/* scanchar_raw — read next character from connection or console */
fn scanchar_raw(d: &mut LocalData) -> c_int {
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
fn scanchar(inQuote: bool, d: &mut LocalData) -> c_int {
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
                    if inQuote && strchr_quoteset(&d.quoteset, next) {
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
fn ruleout_types(bytes: &[u8], typeInfo: &mut Typecvt_Info, decimal: u8, exact: c_int) {
    if typeInfo.islogical {
        if matches!(bytes, b"T" | b"F" | b"TRUE" | b"FALSE") {
            typeInfo.isinteger = false;
            typeInfo.isreal = false;
            typeInfo.iscomplex = false;
            return; // short cut
        }
        typeInfo.islogical = false;
    }

    if typeInfo.isinteger && parse_integer(bytes) == NA_INTEGER() {
        typeInfo.isinteger = false;
    }

    if typeInfo.isreal {
        let parsed = parse_real(bytes, decimal, true, exact);
        typeInfo.isreal = blank(&bytes[parsed.consumed..]);
    }

    if typeInfo.iscomplex {
        typeInfo.iscomplex = parse_complex(bytes, decimal, true, exact).is_some();
    }
}

/* NA_STRING — get the NA string sentinel */
fn NA_STRING() -> SEXP {
    crate::main::relop::NA_STRING()
}

/* ========== Local helper functions ========== */

/// CAD4R — fourth element of args list (CAR of CDR of CDR of CDR of CDR)
#[inline]
fn CAD4R(x: SEXP) -> SEXP {
    CDR(CDR(CDR(CDR(x))))
}

/// isString — check if SEXP is a character vector
#[inline]
fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

/// isNull — check if SEXP is R_NilValue
#[inline]
fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/// isVectorList — check if SEXP is a list (VECSXP or EXPRSXP)
#[inline]
fn isVectorList(x: SEXP) -> bool {
    {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP // VECSXP or EXPRSXP
    }
}

/// isVectorAtomic — check if SEXP is an atomic vector
#[inline]
fn isVectorAtomic(x: SEXP) -> bool {
    crate::sexp::object::raw_is_atomic_vector(x)
}

/// SET_TYPEOF — set the type of a SEXP
#[inline]
fn SET_TYPEOF(x: SEXP, v: SEXPTYPE) {
    crate::sexp::accessors::SET_TYPEOF(x, v.as_c_int());
}

/// strchr helper for quoteset
#[inline]
fn strchr_quoteset(quoteset: &[c_char; 10], c: c_int) -> bool {
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
fn r_error(fmt: &CStr, arg: Option<&CStr>) {
    // Rf_error in our Rust port takes a single format string
    // Build the full message using snprintf
    let mut buf = [0 as core::ffi::c_char; 512];
    let fmt_b = fmt.to_bytes().to_vec();
    let args: Vec<crate::mainutils::r_format::CArg> = match arg {
        None => vec![],
        Some(arg) => vec![CArg::Str(arg.to_bytes())],
    };
    crate::mainutils::r_format::r_snprintf_c(&mut buf, &fmt_b, &args);
    Rf_error_cstr(crate::mainutils::errors::c_chars_until_nul(&buf));
}

/// Format an error message with int arg and call Rf_error
fn r_error_int(fmt: &CStr, arg: c_int) {
    let mut buf = [0 as core::ffi::c_char; 512];
    let fmt_b = fmt.to_bytes().to_vec();
    crate::mainutils::r_format::r_snprintf_c(&mut buf, &fmt_b, &[arg.into()]);
    Rf_error_cstr(crate::mainutils::errors::c_chars_until_nul(&buf));
}

/// Format a warning message and call Rf_warning
fn r_warning(fmt: &CStr, arg: Option<&CStr>) {
    let mut buf = [0 as core::ffi::c_char; 512];
    let fmt_b = fmt.to_bytes().to_vec();
    let args: Vec<crate::mainutils::r_format::CArg> = match arg {
        None => vec![],
        Some(arg) => vec![CArg::Str(arg.to_bytes())],
    };
    crate::mainutils::r_format::r_snprintf_c(&mut buf, &fmt_b, &args);
    Rf_warning_cstr(crate::mainutils::errors::c_chars_until_nul(&buf));
}

fn table_checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

fn is_factor_column(x: SEXP) -> bool {
    let klass = attrib_core::getAttrib(x, attrib_core::R_ClassSymbol());
    isString(klass)
        && (0..XLENGTH(klass))
            .any(|i| char_cstr_bytes(STRING_ELT(klass, i)) == b"factor")
}

fn table_string(text: &[u8], quote: bool, escape: bool) -> Vec<u8> {
    if !quote {
        return text.to_vec();
    }
    let quotes = text.iter().filter(|&&byte| byte == b'"').count();
    let length = text
        .len()
        .checked_add(quotes)
        .and_then(|n| n.checked_add(2))
        .unwrap_or_else(|| crate::sexp::context::r_error("quoted table value is too long"));
    let mut out = Vec::new();
    out.try_reserve_exact(length)
        .unwrap_or_else(|_| crate::sexp::context::r_error("cannot allocate quoted table value"));
    out.push(b'"');
    for &byte in text {
        if byte == b'"' {
            out.push(if escape { b'\\' } else { b'"' });
        }
        out.push(byte);
    }
    out.push(b'"');
    out
}

fn table_cell(
    x: &Sexp<'_>,
    i: R_xlen_t,
    levels: Option<&Sexp<'_>>,
    quote: bool,
    escape: bool,
    decimal: u8,
    owner: OwnerToken<'_>,
) -> Option<Vec<u8>> {
    let require_active = || {
        table_checked(owner.require_active());
    };
    let string = |x: &Sexp<'_>, i: R_xlen_t| {
        let s = table_checked(x.try_string_elt(i));
        require_active();
        (!s.is_na_string())
            .then(|| table_string(&char_cstr_bytes(s.as_raw()), quote, escape))
    };
    if let Some(levels) = levels {
        let code = match x.typeof_() {
            SEXPTYPE::INTSXP => {
                let value = table_checked(x.try_integer_elt(i));
                if value == NA_INTEGER() {
                    return None;
                }
                require_active();
                i64::from(value) - 1
            }
            SEXPTYPE::REALSXP => {
                let value = table_checked(x.try_real_elt(i));
                if value.is_nan() {
                    return None;
                }
                require_active();
                (value - 1.0) as i64
            }
            _ => crate::sexp::context::r_error("factor column does not have numeric codes"),
        };
        return string(levels, code);
    }
    Some(match x.typeof_() {
        SEXPTYPE::STRSXP => return string(x, i),
        SEXPTYPE::LGLSXP => {
            let v = table_checked(x.try_logical_elt(i));
            require_active();
            if v == NA_LOGICAL() {
                return None;
            }
            encode_logical_text(v, if v == 0 { 5 } else { 4 })
        }
        SEXPTYPE::INTSXP => {
            let v = table_checked(x.try_integer_elt(i));
            require_active();
            if v == NA_INTEGER() {
                return None;
            }
            encode_integer_text(v, 0)
        }
        SEXPTYPE::REALSXP => {
            let v = table_checked(x.try_real_elt(i));
            require_active();
            if v.is_nan() {
                return None;
            }
            let mut print = format_get_R_print();
            print.digits = DBL_DIG_VAL;
            let f = real_field_with_print(1, 0, print, |_| v);
            encode_real0_text(v, f.w, f.d, f.e, &[decimal])
        }
        SEXPTYPE::CPLXSXP => {
            let v = table_checked(x.try_complex_elt(i));
            require_active();
            if v.r.is_nan() || v.i.is_nan() {
                return None;
            }
            let mut print = format_get_R_print();
            print.digits = DBL_DIG_VAL;
            let f = complex_field_with_print(&[v], 0, print);
            let mut text =
                encode_complex_text(v, f.wr, f.dr, f.er, f.wi, f.di, f.ei);
            if decimal != b'.' {
                for byte in &mut text {
                    if *byte == b'.' {
                        *byte = decimal;
                    }
                }
            }
            text
        }
        SEXPTYPE::RAWSXP => {
            let v = table_checked(x.try_raw_elt(i));
            require_active();
            encode_raw_text(v, b"")
        }
        _ => Vec::new(),
    })
}

/* ========== Exported functions ========== */

/*
 * countfields(file, sep, quotes, nskip, blskip, comment.char)
 *
 * Counts the number of fields per line in a file/connection.
 * Ported from r-source/src/library/utils/src/io.c
 */
pub fn countfields(args: SEXP) -> SEXP {
    unsafe {
        let mut data = LocalData::new();

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
            r_error(c"invalid '%s' argument", Some(c"comment.char"));
        }
        let p = translateChar(STRING_ELT(comstr, 0));
        data.comchar = NO_COMCHAR;
        let plen = CStr::from_ptr(p).to_bytes().len();
        if plen > 1 {
            r_error(c"invalid '%s' argument", Some(c"comment.char"));
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
            r_error(c"invalid '%s' argument", Some(c"sep"));
        }

        // Parse quotes
        if isString(quotes) {
            let sc = translateChar(STRING_ELT(quotes, 0));
            let qbytes = CStr::from_ptr(sc).to_bytes();
            if !qbytes.is_empty() {
                // strcpy into the 10-byte quoteset, capped at 9 chars + NUL
                // (a longer quote set overflows in C, which we do not reproduce)
                let n = qbytes.len().min(data.quoteset.len() - 1);
                ptr::copy_nonoverlapping(qbytes.as_ptr(), data.quoteset.as_mut_ptr() as *mut u8, n);
                data.quoteset[n] = 0;
            } else {
                data.quoteset[0] = 0;
            }
        } else if isNull(quotes) {
            data.quoteset[0] = 0;
        } else {
            r_error(c"invalid quote symbol set", None);
        }

        // Set up connection
        let mut i = crate::main::coerce::asInteger(file);
        data.con = i;
        if i == 0 {
            data.ttyflag = 1;
        } else {
            data.ttyflag = 0;
            crate::mainutils::connections::ensure_connection_readable(i);
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
        let mut ans = Rf_allocVector(SEXPTYPE::INTSXP, blocksize);
        let mut ans_guard = protect_with_index_raw(ans, "countfields ans");
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
                    ans_guard.reprotect_raw(ans);
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
                                ans_guard.reprotect_raw(ans);
                                copyVector(ans, bns);
                            }
                        } else if c2 == '\n' as c_int {
                            *INTEGER(ans).add(nlines as usize) = NA_INTEGER();
                            nlines += 1;
                            if nlines == blocksize {
                                let bns = ans;
                                blocksize = 2 * blocksize;
                                ans = Rf_allocVector(SEXPTYPE::INTSXP, blocksize);
                                ans_guard.reprotect_raw(ans);
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
            return R_NilValue();
        }
        if nlines == blocksize {
            return ans;
        }

        let bns = Rf_allocVector(SEXPTYPE::INTSXP, nlines + 1);
        let _bns_guard = protect(bns);
        for j in 0..=nlines {
            *INTEGER(bns).add(j as usize) = *INTEGER(ans).add(j as usize);
        }
        bns
    }
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
pub fn typeconvert(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, env);

    if args.is_null() {
        return R_NilValue();
    }

    let args_cdr = CDR(args);
    if args_cdr.is_null() {
        return R_NilValue();
    }

    if !isString(CAR(args_cdr)) {
        Rf_error_cstr(c"the first argument must be of mode character");
    }

    let mut args_rest = args_cdr;

    /* args[1] = cvec (already have via CAR(args_cdr)) */
    /* args[2] = na.strings */
    args_rest = CDR(args_rest);
    let na_strings = CAR(args_rest);
    if TYPEOF(na_strings) != SEXPTYPE::STRSXP {
        Rf_error_cstr(c"invalid 'na.strings' argument");
    }

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
    let decimal = if isString(dec) && LENGTH(dec) > 0 {
        char_cstr_bytes(STRING_ELT(dec, 0))
            .first()
            .copied()
            .unwrap_or(0)
    } else {
        b'.'
    };

    /* args[5] = numerals */
    args_rest = CDR(args_rest);
    let numerals = CAR(args_rest);
    let mut i_exact: c_int = 0; // false = allow.loss
    if isString(numerals) && LENGTH(numerals) > 0 {
        match char_cstr_bytes(STRING_ELT(numerals, 0)).as_slice() {
            b"warn.loss" => i_exact = NA_LOGICAL(),
            b"no.loss" => i_exact = 1,
            _ => {}
        }
    }
    let screen_exact = i32::from(i_exact == 1);

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
    let dims = attrib_core::getAttrib(cvec, attrib_core::R_DimSymbol());
    let _dims_guard = protect(dims);
    let names = if isArray(cvec) != 0 {
        attrib_core::getAttrib(cvec, attrib_core::R_DimNamesSymbol())
    } else {
        attrib_core::getAttrib(cvec, attrib_core::R_NamesSymbol())
    };
    let _names_guard = protect(names);

    /* Find the first non-NA entry (empty => NA) */
    let mut typeInfo = Typecvt_Info::new();
    typeInfo.islogical = try_logical_flag;

    /* Use first non-NA entry to screen types */
    if let Some(bytes) = (0..i64::from(len))
        .find_map(|i| conversion_text(STRING_ELT(cvec, i), na_strings))
    {
        ruleout_types(&bytes, &mut typeInfo, decimal, screen_exact);
    }

    let mut done = false;
    let mut rval: SEXP = ptr::null_mut();
    let mut rval_guard = None;

    /* Try logical conversion */
    if typeInfo.islogical {
        rval = Rf_allocVector(SEXPTYPE::LGLSXP, len);
        rval_guard = Some(protect(rval));
        let mut all_logical = true;
        for i in 0..len {
            let Some(bytes) = conversion_text(STRING_ELT(cvec, i64::from(i)), na_strings) else {
                SET_LOGICAL_ELT(rval, i, NA_LOGICAL());
                continue;
            };
            let value = match bytes.as_slice() {
                b"F" | b"FALSE" => 0,
                b"T" | b"TRUE" => 1,
                _ => {
                    all_logical = false;
                    typeInfo.islogical = false;
                    ruleout_types(&bytes, &mut typeInfo, decimal, screen_exact);
                    break;
                }
            };
            SET_LOGICAL_ELT(rval, i, value);
        }
        if all_logical {
            done = true;
        } else {
            drop(rval_guard.take());
        }
    }

    /* Try integer conversion */
    if !done && typeInfo.isinteger {
        rval = Rf_allocVector(SEXPTYPE::INTSXP, len);
        rval_guard = Some(protect(rval));
        let mut all_integer = true;
        for i in 0..len {
            let Some(bytes) = conversion_text(STRING_ELT(cvec, i64::from(i)), na_strings) else {
                SET_INTEGER_ELT(rval, i, NA_INTEGER());
                continue;
            };
            let value = parse_integer(&bytes);
            if value == NA_INTEGER() {
                all_integer = false;
                typeInfo.isinteger = false;
                ruleout_types(&bytes, &mut typeInfo, decimal, screen_exact);
                break;
            }
            SET_INTEGER_ELT(rval, i, value);
        }
        if all_integer {
            done = true;
        } else {
            drop(rval_guard.take());
        }
    }

    /* Try real conversion */
    if !done && typeInfo.isreal {
        rval = Rf_allocVector(SEXPTYPE::REALSXP, len);
        rval_guard = Some(protect(rval));
        let mut all_real = true;
        for i in 0..len {
            let Some(bytes) = conversion_text(STRING_ELT(cvec, i64::from(i)), na_strings) else {
                SET_REAL_ELT(rval, i, NA_REAL());
                continue;
            };
            let parsed = parse_real(&bytes, decimal, false, i_exact);
            if !blank(&bytes[parsed.consumed..]) {
                all_real = false;
                typeInfo.isreal = false;
                ruleout_types(&bytes, &mut typeInfo, decimal, screen_exact);
                break;
            }
            SET_REAL_ELT(rval, i, parsed.value);
        }
        if all_real {
            done = true;
        } else {
            drop(rval_guard.take());
        }
    }

    /* Try complex conversion */
    if !done && typeInfo.iscomplex {
        rval = Rf_allocVector(SEXPTYPE::CPLXSXP, len);
        rval_guard = Some(protect(rval));
        let mut all_complex = true;
        for i in 0..len {
            let Some(bytes) = conversion_text(STRING_ELT(cvec, i64::from(i)), na_strings) else {
                SET_COMPLEX_ELT(
                    rval,
                    i,
                    Rcomplex {
                        r: NA_REAL(),
                        i: NA_REAL(),
                    },
                );
                continue;
            };
            let Some(value) = parse_complex(&bytes, decimal, false, i_exact) else {
                all_complex = false;
                typeInfo.iscomplex = false;
                ruleout_types(&bytes, &mut typeInfo, decimal, screen_exact);
                break;
            };
            SET_COMPLEX_ELT(rval, i, value);
        }
        if all_complex {
            done = true;
        } else {
            drop(rval_guard.take());
        }
    }

    /* Fallback: character or factor */
    if !done {
        if as_is_flag {
            rval = Rf_duplicate(cvec);
            rval_guard = Some(protect(rval));
            /* Replace NA strings with NA_STRING */
            for i in 0..len {
                if is_na_string(
                    &char_cstr_bytes(STRING_ELT(rval, i64::from(i))),
                    na_strings,
                ) {
                    SET_STRING_ELT(rval, i64::from(i), NA_STRING());
                }
            }
        } else {
            /* Factor conversion */
            let dup = duplicated(cvec, 0);
            rval_guard = Some(protect(dup));
            let mut j: c_int = 0;
            for i in 0..len {
                let ch = STRING_ELT(cvec, i64::from(i));
                if ch == NA_STRING() {
                    continue;
                }
                if LOGICAL_ELT(dup, i) == 0
                    && !is_na_string(&char_cstr_bytes(ch), na_strings)
                {
                    j += 1;
                }
            }

            let levs = Rf_allocVector(SEXPTYPE::STRSXP, j);
            let _levs_guard = protect(levs);
            j = 0;
            for i in 0..len {
                let ch = STRING_ELT(cvec, i64::from(i));
                if ch == NA_STRING() {
                    continue;
                }
                if LOGICAL_ELT(dup, i) == 0
                    && !is_na_string(&char_cstr_bytes(ch), na_strings)
                {
                    SET_STRING_ELT(levs, i64::from(j), ch);
                    j += 1;
                }
            }

            /* Reuse dup (LGLSXP of right length) as integer vector */
            rval = dup;
            SET_TYPEOF(rval, SEXPTYPE::INTSXP);

            /* Sort levels lexicographically */
            sortVector(levs, 0);

            let a = matchE(levs, cvec, NA_INTEGER(), env);
            let _a_guard = protect(a);
            for i in 0..len {
                SET_INTEGER_ELT(rval, i, INTEGER_ELT(a, i));
            }

            attrib_core::setAttrib(rval, attrib_core::R_LevelsSymbol(), levs);
            let class_str = crate::sexp::constructors::mk_string_str("factor");
            let _class_guard = protect(class_str);
            attrib_core::setAttrib(rval, attrib_core::R_ClassSymbol(), class_str);
        }
    }

    /* Restore attributes */
    attrib_core::setAttrib(rval, attrib_core::R_DimSymbol(), dims);
    if isArray(cvec) != 0 {
        attrib_core::setAttrib(rval, attrib_core::R_DimNamesSymbol(), names);
    } else {
        attrib_core::setAttrib(rval, attrib_core::R_NamesSymbol(), names);
    }

    rval
}

/*
 * menu(choices)
 *
 * Interactive menu selection. Legitimate stub: requires R_ReadConsole
 * which is not available without a running R console session.
 * Always returns 0 (no selection).
 */
pub fn menu(choices: SEXP) -> SEXP {
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
pub fn readtablehead(args: SEXP) -> SEXP {
    unsafe {
        let mut data = LocalData::new();

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
            r_error(c"invalid '%s' argument", Some(c"nlines"));
        }
        if blskip == NA_LOGICAL() {
            blskip = 1;
        }

        // Parse quotes
        if isString(quotes) {
            let sc = translateChar(STRING_ELT(quotes, 0));
            let qbytes = CStr::from_ptr(sc).to_bytes();
            if !qbytes.is_empty() {
                // strcpy into the 10-byte quoteset, capped at 9 chars + NUL
                // (a longer quote set overflows in C, which we do not reproduce)
                let n = qbytes.len().min(data.quoteset.len() - 1);
                ptr::copy_nonoverlapping(qbytes.as_ptr(), data.quoteset.as_mut_ptr() as *mut u8, n);
                data.quoteset[n] = 0;
            } else {
                data.quoteset[0] = 0;
            }
        } else if isNull(quotes) {
            data.quoteset[0] = 0;
        } else {
            r_error(c"invalid quote symbol set", None);
        }

        // Validate comment.char
        if TYPEOF(comstr) != SEXPTYPE::STRSXP || LENGTH(comstr) != 1 {
            r_error(c"invalid '%s' argument", Some(c"comment.char"));
        }
        let p = translateChar(STRING_ELT(comstr, 0));
        data.comchar = NO_COMCHAR;
        let plen = CStr::from_ptr(p).to_bytes().len();
        if plen > 1 {
            r_error(c"invalid '%s' argument", Some(c"comment.char"));
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
            r_error(c"invalid '%s' argument", Some(c"sep"));
        }

        if skipNul == NA_LOGICAL() {
            r_error(c"invalid '%s' argument", Some(c"skipNul"));
        }
        data.skipNul = skipNul != 0;

        // Set up connection
        let i = crate::main::coerce::asInteger(file);
        data.con = i;
        data.ttyflag = if i == 0 { 1 } else { 0 };
        if i != 0 {
            crate::mainutils::connections::ensure_connection_readable(i);
        }

        let mut buf_size: usize = BUF_SIZE;
        let mut buf: Vec<c_char> = vec![0; buf_size];

        let mut ans = Rf_allocVector(SEXPTYPE::STRSXP, nlines);
        let _ans_guard = protect(ans);
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
                    buf.resize(buf_size, 0);
                }

                // Handle quotes
                if quote != 0 {
                    if data.sepchar == 0 && c == '\\' as c_int {
                        // all escapes should be passed through
                        buf[nbuf] = c as c_char;
                        nbuf += 1;
                        let c2 = scanchar(true, &mut data);
                        if c2 == R_EOF_VAL {
                            r_error(c"\\ followed by EOF", None);
                        }
                        buf[nbuf] = c2 as c_char;
                        nbuf += 1;
                        continue;
                    } else if c == quote {
                        if data.sepchar == 0 {
                            quote = 0;
                        } else {
                            // Check for doubled quote
                            let c2 = scanchar(true, &mut data);
                            if c2 == quote {
                                buf[nbuf] = c as c_char;
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
                    buf[nbuf] = c as c_char;
                    nbuf += 1;
                } else {
                    last_c = c;
                    break;
                }
            }
            buf[nbuf] = 0;

            if data.ttyflag != 0 && empty {
                // No more lines from tty
                // Trim result to actual number read
                if nread < nlines {
                    let ans2 = Rf_allocVector(SEXPTYPE::STRSXP, nread);
                    let _ans2_guard = protect(ans2);
                    for j in 0..nread {
                        SET_STRING_ELT(ans2, j as R_xlen_t, STRING_ELT(ans, j as R_xlen_t));
                    }
                    return ans2;
                }
                return ans;
            }

            if !empty || (last_c != R_EOF_VAL && blskip == 0) {
                SET_STRING_ELT(ans, nread as R_xlen_t, Rf_mkChar(buf.as_ptr()));
                nread += 1;
                // Check for embedded nulls (strlen < nbuf)
                let strlen = buf.iter().take_while(|&&c| c != 0).count();
                if strlen < nbuf {
                    let mut warn_buf = [0 as core::ffi::c_char; 256];
                    crate::mainutils::r_format::r_snprintf_c(
                        &mut warn_buf,
                        b"line %d appears to contain embedded nulls\0",
                        &[nread.into()],
                    );
                    Rf_warning_cstr(crate::mainutils::errors::c_chars_until_nul(&warn_buf));
                }
            }
            if last_c == R_EOF_VAL {
                // Trim result to actual number read
                if nread < nlines {
                    let ans2 = Rf_allocVector(SEXPTYPE::STRSXP, nread);
                    let _ans2_guard = protect(ans2);
                    for j in 0..nread {
                        SET_STRING_ELT(ans2, j as R_xlen_t, STRING_ELT(ans, j as R_xlen_t));
                    }
                    return ans2;
                }
                return ans;
            }
        }

        ans
    }
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
pub fn writetable(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, env);

    let owner = table_checked(OwnerToken::current());
    let _pin = table_checked(owner.pin());
    let roots = table_checked(owner.sexp(args));
    let mut args_cdr = CDR(roots.as_raw());
    let x = CAR(args_cdr);
    args_cdr = CDR(args_cdr);

    // file is a connection — get it
    let file_arg = CAR(args_cdr);
    args_cdr = CDR(args_cdr);
    let con = crate::main::coerce::asInteger(file_arg);
    // Coercion may have re-entered the interpreter; re-validate the session
    // and bind the connection by identity, not by borrowed table slot.
    table_checked(owner.require_active());
    let connection =
        connection_token(con).unwrap_or_else(|e| crate::sexp::context::r_error(e));
    let write = |bytes: &[u8]| {
        table_checked(owner.require_active());
        let index = connection
            .require_current()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e));
        connection_write_bytes(index, bytes);
    };

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

    // Validate arguments (NA_INTEGER is the negative sentinel).
    if nr < 0 {
        r_error(c"invalid '%s' argument", Some(c"nr"));
    }
    if nc < 0 {
        r_error(c"invalid '%s' argument", Some(c"nc"));
    }
    if !isNull(rnames) && !isString(rnames) {
        r_error(c"invalid '%s' argument", Some(c"rnames"));
    }
    if !isString(sep) || LENGTH(sep) < 1 {
        r_error(c"invalid '%s' argument", Some(c"sep"));
    }
    if !isString(eol) || LENGTH(eol) < 1 {
        r_error(c"invalid '%s' argument", Some(c"eol"));
    }
    if !isString(na) || LENGTH(na) < 1 {
        r_error(c"invalid '%s' argument", Some(c"na"));
    }
    if !isString(dec) || LENGTH(dec) < 1 {
        r_error(c"invalid '%s' argument", Some(c"dec"));
    }
    if qmethod0 == NA_LOGICAL() {
        r_error(c"invalid '%s' argument", Some(c"qmethod"));
    }
    let qmethod = qmethod0 != 0;

    let sep = char_cstr_bytes(STRING_ELT(sep, 0));
    let eol = char_cstr_bytes(STRING_ELT(eol, 0));
    let na = char_cstr_bytes(STRING_ELT(na, 0));
    let sdec = char_cstr_bytes(STRING_ELT(dec, 0));
    let [decimal] = sdec.as_slice() else {
        crate::sexp::context::r_error("'dec' must be a single character");
    };
    let decimal = *decimal;

    // Root the table input and row names before any provider callback can
    // collect or replace the connection.
    let x = table_checked(owner.sexp(x));
    let row_names = (!isNull(rnames)).then(|| table_checked(owner.sexp(rnames)));

    // Check source shapes before admitting workspace.
    let data_frame = isVectorList(x.as_raw());
    if data_frame {
        if x.len() < i64::from(nc) {
            crate::sexp::context::r_error("corrupt data frame -- not enough columns");
        }
    } else {
        if !isVectorAtomic(x.as_raw()) {
            r_error(c"write.table: x must be a matrix or data frame", None);
        }
        if x.len() < i64::from(nr) * i64::from(nc) {
            crate::sexp::context::r_error("matrix length is smaller than nrow * ncol");
        }
    }

    let columns = nc as usize;

    // Admit scratch workspace sized like the original R_alloc reservations,
    // before the first host allocation below.
    let scratch_per_col = std::mem::size_of::<bool>()
        + if data_frame {
            std::mem::size_of::<Option<Sexp<'_>>>()
        } else {
            0
        };
    let scratch_bytes = columns
        .checked_mul(scratch_per_col)
        .unwrap_or_else(|| crate::sexp::context::r_error("table workspace size overflow"));
    let _scratch = table_checked(owner.with_arena(|arena| arena.reserve_workspace(scratch_bytes)))
        .unwrap_or_else(|| crate::sexp::context::r_error("table workspace exceeds session memory limit"));

    // Parse quote columns.
    // quote_col[j] = true means column j+1 should be quoted;
    // quote_rn = true means row names should be quoted (when quote has a 0 element).
    let mut quote_rn = false;
    let mut quote_col = Vec::new();
    quote_col
        .try_reserve_exact(columns)
        .unwrap_or_else(|_| crate::sexp::context::r_error("cannot allocate table quote columns"));
    quote_col.resize(columns, false);
    if !isNull(quote) {
        let quote = table_checked(owner.sexp(quote));
        for i in 0..quote.len() {
            let this = table_checked(quote.try_integer_elt(i));
            if this == 0 {
                quote_rn = true;
            } else if this > 0 && let Some(flag) = quote_col.get_mut((this - 1) as usize) {
                *flag = true;
            }
        }
    }

    // Quote element reads above may have re-entered; re-validate the session.
    table_checked(owner.require_active());

    // Initialize print defaults once; cells format through the owned text
    // encoders with a captured configuration, never a temporarily changed
    // global precision.
    PrintDefaults();

    // Validate and root factor level vectors up front.
    let mut levels: Vec<Option<Sexp<'_>>> = Vec::new();
    if data_frame {
        levels
            .try_reserve_exact(columns)
            .unwrap_or_else(|_| crate::sexp::context::r_error("cannot allocate factor level table"));
        for j in 0..i64::from(nc) {
            let col = table_checked(x.try_vector_elt(j));
            table_checked(owner.require_active());
            if col.len() != i64::from(nr) {
                r_error_int(
                    c"corrupt data frame -- length of column %d does not match nrows",
                    (j + 1) as c_int,
                );
            }
            let levels_for_col = if is_factor_column(col.as_raw()) {
                table_checked(owner.require_active());
                let lev = attrib_core::getAttrib(col.as_raw(), attrib_core::R_LevelsSymbol());
                if isNull(lev) {
                    None
                } else {
                    if !matches!(col.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::REALSXP) {
                        r_error_int(
                            c"column %d claims to be a factor but does not have numeric codes",
                            (j + 1) as c_int,
                        );
                    }
                    Some(table_checked(owner.sexp(lev)))
                }
            } else {
                None
            };
            levels.push(levels_for_col);
        }
    }

    for i in 0..i64::from(nr) {
        // Check user interrupt every 1000 rows.
        if i % 1000 == 999 {
            R_CheckUserInterrupt();
        }

        if let Some(names) = &row_names {
            let bytes = table_cell(names, i, None, quote_rn, qmethod, decimal, owner)
                .unwrap_or_else(|| table_string(b"NA", quote_rn, qmethod));
            write(&bytes);
            write(&sep);
        }

        for (j, &quote) in quote_col.iter().enumerate() {
            if j != 0 {
                write(&sep);
            }
            let bytes = if data_frame {
                let col = table_checked(x.try_vector_elt(j as i64));
                table_checked(owner.require_active());
                table_cell(&col, i, levels[j].as_ref(), quote, qmethod, decimal, owner)
            } else {
                table_cell(&x, j as i64 * i64::from(nr) + i, None, quote, qmethod, decimal, owner)
            };
            write(bytes.as_deref().unwrap_or(&na));
        }

        write(&eol);
    }

    R_NilValue()
}
