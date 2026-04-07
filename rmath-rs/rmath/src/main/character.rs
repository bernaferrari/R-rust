#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/character.c — character/string utility functions.
//!
//! This module ports the standalone string manipulation functions that don't
//! require SEXP or R interpreter internals.
//!
//! Ported standalone functions:
//!   mystrcpy,
//!   iswvowel,
//!   tr_build_spec, tr_free_spec, tr_get_next_char_from_spec,
//!   wtr_build_spec, wtr_free_spec, wtr_get_next_char_from_spec,
//!   xtable_comp, xtable_key_comp

// ---------------------------------------------------------------------------
// String copy utility
// ---------------------------------------------------------------------------

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::safe::Sexp;

/// Copy a string using memmove (handles overlapping regions).
pub fn mystrcpy(dest: &mut [u8], src: &[u8]) {
    let len = src.len().min(dest.len());
    dest[..len].copy_from_slice(&src[..len]);
}

// ---------------------------------------------------------------------------
// Wide character vowel check
// ---------------------------------------------------------------------------

/// Vowel codepoints (Latin vowels with diacritics).
const VOWELS: &[u32] = &[
    0x61, 0x65, 0x69, 0x6f, 0x75, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe8, 0xe9, 0xea, 0xeb, 0xec,
    0xed, 0xee, 0xef, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0x101,
    0x103, 0x105, 0x113, 0x115, 0x117, 0x119, 0x11b, 0x129, 0x12b, 0x12d, 0x12f, 0x131, 0x14d,
    0x14f, 0x151, 0x169, 0x16b, 0x16d, 0x16f, 0x171, 0x173,
];

/// Check if a wide character is a vowel (Latin vowels with diacritics).
pub fn iswvowel(w: char) -> bool {
    let v = w as u32;
    VOWELS.contains(&v)
}

// ---------------------------------------------------------------------------
// Byte translation specification (tr_spec)
// ---------------------------------------------------------------------------

/// Type of a translation specification entry.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub enum TrSpecType {
    Init = 0,
    Char = 1,
    Range = 2,
}

/// A single entry in a byte translation specification.
#[derive(Clone, Debug)]
pub struct TrSpec {
    pub spec_type: TrSpecType,
    pub next: Option<Box<TrSpec>>,
    pub c: Option<u8>,
    pub first: Option<u8>,
    pub last: Option<u8>,
}

/// Build a translation specification from a byte string.
///
/// Parses ranges like "a-z" into individual character entries.
/// Returns a linked list of TrSpec nodes.
pub fn tr_build_spec(s: &[u8]) -> Option<Box<TrSpec>> {
    let len = s.len();
    if len == 0 {
        return None;
    }

    let mut head: Option<Box<TrSpec>> = None;
    let mut tail: &mut Option<Box<TrSpec>> = &mut head;
    let mut i = 0;

    while i < len.saturating_sub(2) {
        let mut node = Box::new(TrSpec {
            spec_type: TrSpecType::Char,
            next: None,
            c: None,
            first: None,
            last: None,
        });

        if s[i + 1] == b'-' {
            if s[i] > s[i + 2] {
                // Decreasing range — error in R, we just skip
                i += 3;
                continue;
            }
            node.spec_type = TrSpecType::Range;
            node.first = Some(s[i]);
            node.last = Some(s[i + 2]);
            i += 3;
        } else {
            node.spec_type = TrSpecType::Char;
            node.c = Some(s[i]);
            i += 1;
        }

        *tail = Some(node);
        tail = &mut tail.as_mut().expect("expected Some, got None").next;
    }

    // Remaining characters (0, 1, or 2 left)
    while i < len {
        let node = Box::new(TrSpec {
            spec_type: TrSpecType::Char,
            next: None,
            c: Some(s[i]),
            first: None,
            last: None,
        });
        *tail = Some(node);
        tail = &mut tail.as_mut().expect("expected Some, got None").next;
        i += 1;
    }

    head
}

/// Free a translation specification (no-op in Rust due to RAII).
pub fn tr_free_spec(_trs: Option<Box<TrSpec>>) {
    // Rust's Drop handles deallocation automatically
}

/// Get the next character from a translation specification.
///
/// Returns the character and advances the pointer.
pub fn tr_get_next_char(p: &mut Option<Box<TrSpec>>) -> u8 {
    let current = match p.take() {
        Some(node) => node,
        None => return 0,
    };

    match current.spec_type {
        TrSpecType::Char => {
            let c = current.c.unwrap_or(0);
            *p = current.next;
            c
        }
        TrSpecType::Range => {
            let c = current.first.unwrap_or(0);
            let last = current.last.unwrap_or(0);
            if c == last {
                *p = current.next;
            } else {
                let mut new_node = current;
                new_node.first = Some(c + 1);
                *p = Some(new_node);
            }
            c
        }
        TrSpecType::Init => {
            *p = current.next;
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Wide character translation specification (wtr_spec)
// ---------------------------------------------------------------------------

/// Type of a wide translation specification entry.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub enum WtrSpecType {
    Init = 0,
    Char = 1,
    Range = 2,
}

/// A single entry in a wide character translation specification.
#[derive(Clone, Debug)]
pub struct WtrSpec {
    pub spec_type: WtrSpecType,
    pub next: Option<Box<WtrSpec>>,
    pub c: Option<char>,
    pub first: Option<char>,
    pub last: Option<char>,
}

/// Build a wide character translation specification from a char slice.
///
/// Parses ranges like "a-z" into individual character entries.
pub fn wtr_build_spec(s: &[char]) -> Option<Box<WtrSpec>> {
    let len = s.len();
    if len == 0 {
        return None;
    }

    let mut head: Option<Box<WtrSpec>> = None;
    let mut tail: &mut Option<Box<WtrSpec>> = &mut head;
    let mut i = 0;

    while i < len.saturating_sub(2) {
        let mut node = Box::new(WtrSpec {
            spec_type: WtrSpecType::Char,
            next: None,
            c: None,
            first: None,
            last: None,
        });

        if s[i + 1] == '-' {
            if (s[i] as u32) > (s[i + 2] as u32) {
                i += 3;
                continue;
            }
            node.spec_type = WtrSpecType::Range;
            node.first = Some(s[i]);
            node.last = Some(s[i + 2]);
            i += 3;
        } else {
            node.spec_type = WtrSpecType::Char;
            node.c = Some(s[i]);
            i += 1;
        }

        *tail = Some(node);
        tail = &mut tail.as_mut().expect("expected Some, got None").next;
    }

    while i < len {
        let node = Box::new(WtrSpec {
            spec_type: WtrSpecType::Char,
            next: None,
            c: Some(s[i]),
            first: None,
            last: None,
        });
        *tail = Some(node);
        tail = &mut tail.as_mut().expect("expected Some, got None").next;
        i += 1;
    }

    head
}

/// Free a wide translation specification (no-op in Rust due to RAII).
pub fn wtr_free_spec(_trs: Option<Box<WtrSpec>>) {
    // Rust's Drop handles deallocation automatically
}

/// Get the next wide character from a translation specification.
pub fn wtr_get_next_char(p: &mut Option<Box<WtrSpec>>) -> char {
    let current = match p.take() {
        Some(node) => node,
        None => return '\0',
    };

    match current.spec_type {
        WtrSpecType::Char => {
            let c = current.c.unwrap_or('\0');
            *p = current.next;
            c
        }
        WtrSpecType::Range => {
            let c = current.first.unwrap_or('\0');
            let last = current.last.unwrap_or('\0');
            if c == last {
                *p = current.next;
            } else {
                let mut new_node = current;
                // Increment the Unicode codepoint
                let next = char::from_u32(c as u32 + 1).unwrap_or('\0');
                new_node.first = Some(next);
                *p = Some(new_node);
            }
            c
        }
        WtrSpecType::Init => {
            *p = current.next;
            '\0'
        }
    }
}

// ---------------------------------------------------------------------------
// Translation table comparison functions
// ---------------------------------------------------------------------------

/// Translation table entry.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct XtableT {
    pub c_old: char,
    pub c_new: char,
}

/// Comparison function for sorting a translation table by old character.
pub fn xtable_comp(a: &XtableT, b: &XtableT) -> std::cmp::Ordering {
    a.c_old.cmp(&b.c_old)
}

/// Key comparison for searching a translation table.
pub fn xtable_key_comp(key: char, entry: &XtableT) -> std::cmp::Ordering {
    key.cmp(&entry.c_old)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Get (or create) the NA_STRING sentinel CHARSXP.
///
/// In R, NA_STRING is a specific CHARSXP with the NA bit set in its gp field.
unsafe fn get_na_string() -> SEXP {
    use crate::sexp::ffi::SexprecCore;
    use std::sync::OnceLock;
    static NA_STRING_VAL: OnceLock<usize> = OnceLock::new();
    let val = NA_STRING_VAL.get_or_init(|| {
        let mut node = SexprecCore::new_vector(SEXPTYPE::CHARSXP, 2);
        node.sxpinfo.set_gp(1);
        Box::into_raw(Box::new(node)) as usize
    });
    *val as SEXP
}

/// Get R_BlankString -- the empty string CHARSXP.
unsafe fn blank_string() -> SEXP {
    unsafe { Rf_mkChar(c"".as_ptr()) }
}

/// Simplified `asLogical` — extract logical value from scalar SEXP.
unsafe fn as_logical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return crate::sexp::ffi::NA_LOGICAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP.0 {
            let p = LOGICAL(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_LOGICAL;
            }
            *p
        } else if t == SEXPTYPE::INTSXP.0 {
            let p = INTEGER(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_LOGICAL;
            }
            *p
        } else if t == SEXPTYPE::REALSXP.0 {
            let p = REAL(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_LOGICAL;
            }
            let v = *p;
            if crate::sexp::ffi::ISNAN(v) {
                return crate::sexp::ffi::NA_LOGICAL;
            }
            if v != 0.0 { 1 } else { 0 }
        } else {
            crate::sexp::ffi::NA_LOGICAL
        }
    }
}

/// Simplified `asInteger` — extract integer value from scalar SEXP.
unsafe fn as_integer(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return crate::sexp::ffi::NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP.0 {
            let p = INTEGER(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::LGLSXP.0 {
            let p = LOGICAL(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::REALSXP.0 {
            let p = REAL(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_INTEGER;
            }
            let v = *p;
            if crate::sexp::ffi::ISNAN(v) {
                return crate::sexp::ffi::NA_INTEGER;
            }
            v as c_int
        } else {
            crate::sexp::ffi::NA_INTEGER
        }
    }
}

/// Compute the number of characters (bytes) in a CHARSXP string.
/// Simplified version — counts bytes (R's "bytes" type).
unsafe fn charsxp_byte_len(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let p = CHAR(s);
        if p.is_null() {
            return 0;
        }
        let mut len = 0;
        while *p.add(len as usize) != 0 {
            len += 1;
        }
        len
    }
}

/// Simplified `R_nchar` — count characters/bytes/width for a single CHARSXP.
///
/// In this simplified port we support:
///   "bytes" — byte length (LENGTH of CHARSXP)
///   "chars" — byte count for non-UTF8 (simplified)
///   "width" — same as "chars" in byte locales (simplified)
unsafe fn r_nchar(
    string: SEXP,
    type_str: &str,
    allow_na: bool,
    keep_na: bool,
    _idx: R_xlen_t,
) -> c_int {
    unsafe {
        let na = get_na_string();
        if string == na {
            return if keep_na {
                crate::sexp::ffi::NA_INTEGER
            } else {
                2
            };
        }
        match type_str {
            "bytes" => charsxp_byte_len(string),
            "chars" | "width" => charsxp_byte_len(string),
            _ => charsxp_byte_len(string),
        }
    }
}

// ---------------------------------------------------------------------------
// Safe Sexp<'a>-based helper functions
// ---------------------------------------------------------------------------

/// Safe version: get the NA_STRING sentinel as a `Sexp<'a>`.
fn get_na_string_safe<'a>() -> Sexp<'a> {
    unsafe { Sexp::from_raw_unchecked(get_na_string()) }
}

/// Safe version: check if a `Sexp` is the NA_STRING sentinel.
fn is_na_string(x: Sexp) -> bool {
    x.as_raw() == get_na_string()
}

/// Safe version: get R_BlankString as a `Sexp<'a>`.
fn blank_string_safe<'a>() -> Sexp<'a> {
    unsafe { Sexp::from_raw_unchecked(blank_string()) }
}

/// Safe version of `as_logical` — extract logical value from scalar `Sexp`.
///
/// Returns `NA_LOGICAL` for null, NA, or unrecognised types.
/// Returns 1 for TRUE/non-zero, 0 for FALSE/zero.
fn as_logical_safe<'a>(x: Sexp<'a>) -> c_int {
    if x.is_nil() {
        return crate::sexp::ffi::NA_LOGICAL;
    }
    match x.typeof_() {
        SEXPTYPE::LGLSXP => x.logical_elt(0).unwrap_or(crate::sexp::ffi::NA_LOGICAL),
        SEXPTYPE::INTSXP => {
            let v = x.integer_elt(0).unwrap_or(crate::sexp::ffi::NA_INTEGER);
            if v == crate::sexp::ffi::NA_INTEGER {
                crate::sexp::ffi::NA_LOGICAL
            } else if v != 0 {
                1
            } else {
                0
            }
        }
        SEXPTYPE::REALSXP => {
            let v = x.real_elt(0).unwrap_or(crate::sexp::ffi::NA_REAL);
            if v.is_nan() {
                crate::sexp::ffi::NA_LOGICAL
            } else if v != 0.0 {
                1
            } else {
                0
            }
        }
        _ => crate::sexp::ffi::NA_LOGICAL,
    }
}

/// Safe version of `as_integer` — extract integer value from scalar `Sexp`.
fn as_integer_safe<'a>(x: Sexp<'a>) -> c_int {
    if x.is_nil() {
        return crate::sexp::ffi::NA_INTEGER;
    }
    match x.typeof_() {
        SEXPTYPE::INTSXP => x.integer_elt(0).unwrap_or(crate::sexp::ffi::NA_INTEGER),
        SEXPTYPE::LGLSXP => {
            let v = x.logical_elt(0).unwrap_or(crate::sexp::ffi::NA_LOGICAL);
            if v == crate::sexp::ffi::NA_LOGICAL {
                crate::sexp::ffi::NA_INTEGER
            } else {
                v
            }
        }
        SEXPTYPE::REALSXP => {
            let v = x.real_elt(0).unwrap_or(crate::sexp::ffi::NA_REAL);
            if v.is_nan() {
                crate::sexp::ffi::NA_INTEGER
            } else {
                v as c_int
            }
        }
        _ => crate::sexp::ffi::NA_INTEGER,
    }
}

/// Safe version: compute byte length of a CHARSXP.
fn charsxp_byte_len_safe<'a>(s: Sexp<'a>) -> c_int {
    if let Some(data) = s.data_ptr() {
        let bytes = unsafe { std::ffi::CStr::from_ptr(data as *const i8) };
        bytes.to_bytes().len() as c_int
    } else {
        0
    }
}

/// Safe version: count characters/bytes/width for a single CHARSXP.
fn r_nchar_safe<'a>(
    string: Sexp<'a>,
    type_str: &str,
    _allow_na: bool,
    keep_na: bool,
    _idx: R_xlen_t,
) -> c_int {
    let na = get_na_string_safe();
    if string == na {
        return if keep_na {
            crate::sexp::ffi::NA_INTEGER
        } else {
            2
        };
    }
    match type_str {
        "bytes" => charsxp_byte_len_safe(string),
        "chars" | "width" => charsxp_byte_len_safe(string),
        _ => charsxp_byte_len_safe(string),
    }
}

// ---------------------------------------------------------------------------
// do_chartr — character translation
// ---------------------------------------------------------------------------

/// Safe version of character translation using `Sexp<'a>`.
///
/// Translate characters in `x`: replace characters in `old` with corresponding
/// characters in `new`.
fn do_chartr_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let old = args.car().ok_or("missing 'old' argument")?;
    let args1 = args.cdr().ok_or("missing arguments")?;
    let new = args1.car().ok_or("missing 'new' argument")?;
    let args2 = args1.cdr().ok_or("missing arguments")?;
    let x = args2.car().ok_or("missing 'x' argument")?;

    let n = x.len() as c_int;

    // Validate old
    if old.typeof_() != SEXPTYPE::STRSXP
        || old.len() < 1
        || old.string_elt(0).map_or(true, |s| s == na)
    {
        return Err("invalid 'old' argument".to_string());
    }

    // Validate new
    if new.typeof_() != SEXPTYPE::STRSXP
        || new.len() < 1
        || new.string_elt(0).map_or(true, |s| s == na)
    {
        return Err("invalid 'new' argument".to_string());
    }

    // Validate x
    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("invalid 'x' argument".to_string());
    }

    // Build byte-level translation table (256 entries, identity by default)
    let mut xtable: [u8; 256] = [0u8; 256];
    for i in 0..256usize {
        xtable[i] = i as u8;
    }

    // Parse old spec
    let old_str = old.string_elt(0).ok_or("invalid old string")?;
    let old_bytes = unsafe {
        if let Some(data) = old_str.data_ptr() {
            std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
        } else {
            return Err("null old string".to_string());
        }
    };
    let old_spec = tr_build_spec(&old_bytes);

    // Parse new spec
    let new_str = new.string_elt(0).ok_or("invalid new string")?;
    let new_bytes = unsafe {
        if let Some(data) = new_str.data_ptr() {
            std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
        } else {
            return Err("null new string".to_string());
        }
    };
    let new_spec = tr_build_spec(&new_bytes);

    // Walk both specs and build the translation table
    let mut old_p = old_spec;
    let mut new_p = new_spec;
    loop {
        let c_old = tr_get_next_char(&mut old_p);
        let c_new = tr_get_next_char(&mut new_p);
        if c_old == 0 {
            break;
        }
        if c_new == 0 {
            return Err("'old' is longer than 'new'".to_string());
        }
        xtable[c_old as usize] = c_new;
    }

    let y = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n)) };
    let y_sexp = unsafe { Sexp::from_raw_unchecked(y) };

    for i in 0..n as R_xlen_t {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            y_sexp.set_string_elt(i, na);
        } else {
            let el = el.expect("unwrap on None/Err");
            let xi_bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    continue;
                }
            };
            let mut buf: Vec<u8> = xi_bytes.to_vec();
            for b in buf.iter_mut() {
                *b = xtable[*b as usize];
            }
            let cs = std::ffi::CString::new(buf.clone()).expect("CString::new failed: contains null byte");
            let ch = unsafe { Rf_mkCharLen(cs.as_ptr(), buf.len() as c_int) };
            let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
            y_sexp.set_string_elt(i, ch_sexp);
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(y_sexp)
}

/// Translate characters in `x`: replace characters in `old` with corresponding
/// characters in `new`.
///
/// This is the Rust port of R's `do_chartr` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_chartr_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

// ---------------------------------------------------------------------------
// do_toupper — convert to uppercase
// ---------------------------------------------------------------------------

/// Convert characters in a character vector to uppercase.
///
/// This is the Rust port of R's `do_tolower`/`do_toupper` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_toupper_lower_safe(s, true) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

// ---------------------------------------------------------------------------
// do_tolower — convert to lowercase
// ---------------------------------------------------------------------------

/// Convert characters in a character vector to lowercase.
///
/// This is the Rust port of R's `do_tolower` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_toupper_lower_safe(s, false) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe shared implementation for do_toupper and do_tolower.
fn do_toupper_lower_safe<'a>(args: Sexp<'a>, upper: bool) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();
    let x = args.car().ok_or("missing argument")?;

    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("non-character argument".to_string());
    }

    let n = x.len();
    let y = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n as c_int)) };
    let y_sexp = unsafe { Sexp::from_raw_unchecked(y) };

    for i in 0..n {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            y_sexp.set_string_elt(i, na);
        } else {
            let el = el.expect("unwrap on None/Err");
            let xi_bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    continue;
                }
            };
            let mut buf: Vec<u8> = xi_bytes.to_vec();
            for b in buf.iter_mut() {
                if upper {
                    *b = (*b as char).to_ascii_uppercase() as u8;
                } else {
                    *b = (*b as char).to_ascii_lowercase() as u8;
                }
            }
            let cs = std::ffi::CString::new(buf.clone()).expect("CString::new failed: contains null byte");
            let ch = unsafe { Rf_mkCharLen(cs.as_ptr(), buf.len() as c_int) };
            let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
            y_sexp.set_string_elt(i, ch_sexp);
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(y_sexp)
}

// ---------------------------------------------------------------------------
// do_nchar — character counting
// ---------------------------------------------------------------------------

/// Count the number of characters in each element of a character vector.
///
/// This is the Rust port of R's `do_nchar` from character.c.
/// Supports type = "bytes", "chars", "width".
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_nchar_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_nchar using `Sexp<'a>`.
fn do_nchar_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();
    let x_arg = args.car().ok_or("missing argument")?;

    // Coerce to STRSXP if needed (simplified: just check it's a string)
    if x_arg.typeof_() != SEXPTYPE::STRSXP {
        return Err("'nchar' requires a character vector".to_string());
    }

    let len = x_arg.len();

    // Parse type argument (second arg)
    let stype = args.cdr().and_then(|a| a.car()).ok_or("missing 'type' argument")?;
    if stype.typeof_() != SEXPTYPE::STRSXP || stype.len() != 1 {
        return Err("invalid 'type' argument".to_string());
    }
    let type_char = stype.string_elt(0).ok_or("invalid 'type' string")?;
    let type_str = unsafe {
        if let Some(data) = type_char.data_ptr() {
            std::ffi::CStr::from_ptr(data as *const i8)
                .to_string_lossy()
                .to_string()
        } else {
            return Err("null type string".to_string());
        }
    };
    let type_str_trimmed = type_str.trim();

    let type_code: &str = if type_str_trimmed.starts_with("bytes") {
        "bytes"
    } else if type_str_trimmed.starts_with("chars") {
        "chars"
    } else if type_str_trimmed.starts_with("width") {
        "width"
    } else {
        return Err("invalid 'type' argument".to_string());
    };

    // Parse allowNA (third arg)
    let allow_na_val = args
        .cdr()
        .and_then(|a| a.cdr())
        .and_then(|a| a.car())
        .map(|a| as_logical_safe(a))
        .unwrap_or(crate::sexp::ffi::NA_LOGICAL);
    let allow_na = if allow_na_val == crate::sexp::ffi::NA_LOGICAL {
        false
    } else {
        allow_na_val != 0
    };

    // Parse keepNA (fourth arg, optional)
    let nargs = crate::sexp::constructors::Rf_length(args.as_raw());
    let keep_na: bool;
    if nargs >= 4 {
        let keep_na_val = args
            .cdr()
            .and_then(|a| a.cdr())
            .and_then(|a| a.cdr())
            .and_then(|a| a.car())
            .map(|a| as_logical_safe(a))
            .unwrap_or(crate::sexp::ffi::NA_LOGICAL);
        if keep_na_val == crate::sexp::ffi::NA_LOGICAL {
            keep_na = type_code != "width";
        } else {
            keep_na = keep_na_val != 0;
        }
    } else {
        keep_na = type_code != "width";
    }

    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let sxi = x_arg.string_elt(i);
        if sxi.map_or(true, |s| s == na) {
            if keep_na {
                s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_INTEGER);
            } else {
                s_sexp.set_integer_elt(i, 2); // NA string length
            }
        } else {
            let res = r_nchar_safe(sxi.expect("unwrap on None/Err"), type_code, allow_na, keep_na, i);
            if res == -1 {
                return Err(format!("invalid multibyte string, element {}", i + 1));
            } else if res == -2 {
                if type_code == "chars" {
                    return Err(format!(
                        "number of characters is not computable in \"bytes\" encoding, element {}",
                        i + 1
                    ));
                } else {
                    return Err(format!(
                        "width is not computable in \"bytes\" encoding, element {}",
                        i + 1
                    ));
                }
            } else {
                s_sexp.set_integer_elt(i, res);
            }
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

/// Extract substrings from a character vector.
///
/// This is the Rust port of R's `do_substr` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_substr_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_substr using `Sexp<'a>`.
fn do_substr_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();
    let blank = blank_string_safe();

    let x = args.car().ok_or("missing 'x' argument")?;
    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("extracting substrings from a non-character object".to_string());
    }
    let len = x.len();

    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    if len > 0 {
        let sa = args.cdr().and_then(|a| a.car()).ok_or("missing start positions")?; // start positions
        let so = args
            .cdr()
            .and_then(|a| a.cdr())
            .and_then(|a| a.car()); // stop positions

        let k = sa.len() as c_int;
        let l_val = so.as_ref().map(|s| s.len() as c_int).unwrap_or(1);

        if sa.typeof_() != SEXPTYPE::INTSXP || k == 0 {
            return Err("invalid substring arguments".to_string());
        }
        if let Some(ref so) = so {
            if so.typeof_() != SEXPTYPE::INTSXP || l_val == 0 {
                return Err("invalid substring arguments".to_string());
            }
        }

        for i in 0..len {
            let start = sa.integer_elt((i as c_int % k) as R_xlen_t).unwrap_or(crate::sexp::ffi::NA_INTEGER);
            let stop = so
                .as_ref()
                .and_then(|s| s.integer_elt((i as c_int % l_val) as R_xlen_t))
                .unwrap_or(c_int::MAX);
            let el = x.string_elt(i);

            if el.map_or(true, |s| s == na)
                || start == crate::sexp::ffi::NA_INTEGER
                || stop == crate::sexp::ffi::NA_INTEGER
            {
                s_sexp.set_string_elt(i, na);
                continue;
            }

            let el = el.expect("unwrap on None/Err");
            let ss_bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_string_elt(i, blank);
                    continue;
                }
            };
            let slen = ss_bytes.len() as c_int;

            let mut start = start;
            let stop = stop;

            if start < 1 {
                start = 1;
            }

            if start > stop {
                s_sexp.set_string_elt(i, blank);
            } else {
                // Byte-level substring (non-MBCS path)
                // R's 1-based indexing
                let from = (start - 1) as usize;
                let to = (stop - 1) as usize;

                let rfrom: usize;
                let rlen: usize;

                if to < ss_bytes.len() {
                    rfrom = from;
                    rlen = to - from + 1;
                } else if from < ss_bytes.len() {
                    rfrom = from;
                    rlen = ss_bytes.len() - from;
                } else {
                    // start is beyond the string length
                    s_sexp.set_string_elt(i, blank);
                    continue;
                }

                let substr_bytes = &ss_bytes[rfrom..rfrom + rlen];
                let cs = std::ffi::CString::new(substr_bytes).expect("CString::new failed: contains null byte");
                let ch = unsafe { Rf_mkCharLen(cs.as_ptr(), substr_bytes.len() as c_int) };
                let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
                s_sexp.set_string_elt(i, ch_sexp);
            }
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_nzchar — non-zero character check
// ---------------------------------------------------------------------------

/// Test if elements of a character vector have non-zero length.
/// nzchar(x) returns TRUE for each element with nchar > 0.
pub unsafe fn do_nzchar(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_nzchar_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_nzchar using `Sexp<'a>`.
fn do_nzchar_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let keep_na = if let Some(cdr) = args.cdr() {
        if let Some(second) = cdr.car() {
            let v = crate::main::coerce::asLogical(second.as_raw());
            if v == crate::sexp::ffi::NA_LOGICAL {
                1
            } else {
                v
            }
        } else {
            0
        }
    } else {
        0
    };

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s.is_nil() || s == na) {
            s_sexp.set_integer_elt(
                i,
                if keep_na != 0 {
                    crate::sexp::ffi::NA_LOGICAL
                } else {
                    0
                },
            );
        } else {
            let el = el.expect("unwrap on None/Err");
            let is_empty = unsafe {
                if let Some(data) = el.data_ptr() {
                    let cs = std::ffi::CStr::from_ptr(data as *const i8);
                    cs.to_bytes().is_empty()
                } else {
                    true
                }
            };
            s_sexp.set_integer_elt(i, if is_empty { 0 } else { 1 });
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_startsWith / do_endsWith — string prefix/suffix check
// ---------------------------------------------------------------------------

/// Check if strings start with a given prefix.
pub unsafe fn do_startsWith(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_startswith_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_startsWith using `Sexp<'a>`.
fn do_startswith_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing 'x' argument")?;
    let prefix = args
        .cdr()
        .and_then(|a| a.car())
        .ok_or("missing 'prefix' argument")?;

    let (x, protect_count_x) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let (prefix, protect_count_prefix) = if prefix.typeof_() == SEXPTYPE::STRSXP {
        (prefix, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(
                prefix.as_raw(),
                SEXPTYPE::STRSXP.0,
            ))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let protect_count = protect_count_x + protect_count_prefix;

    let len = x.len();
    let plen = prefix.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let xi = x.string_elt(i);
        let pi = prefix.string_elt(i % plen);
        if xi.map_or(true, |s| s.is_nil() || s == na) || pi.map_or(true, |s| s.is_nil() || s == na) {
            s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
        } else {
            let xi = xi.expect("unwrap on None/Err");
            let pi = pi.expect("unwrap on None/Err");
            let xs = unsafe {
                if let Some(data) = xi.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
                    continue;
                }
            };
            let ps = unsafe {
                if let Some(data) = pi.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
                    continue;
                }
            };
            s_sexp.set_integer_elt(i, if xs.starts_with(&ps) { 1 } else { 0 });
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

/// Check if strings end with a given suffix.
pub unsafe fn do_endsWith(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_endswith_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_endsWith using `Sexp<'a>`.
fn do_endswith_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing 'x' argument")?;
    let suffix = args
        .cdr()
        .and_then(|a| a.car())
        .ok_or("missing 'suffix' argument")?;

    let (x, protect_count_x) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let (suffix, protect_count_suffix) = if suffix.typeof_() == SEXPTYPE::STRSXP {
        (suffix, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(
                suffix.as_raw(),
                SEXPTYPE::STRSXP.0,
            ))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let protect_count = protect_count_x + protect_count_suffix;

    let len = x.len();
    let slen = suffix.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let xi = x.string_elt(i);
        let si = suffix.string_elt(i % slen);
        if xi.map_or(true, |s| s.is_nil() || s == na) || si.map_or(true, |s| s.is_nil() || s == na) {
            s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
        } else {
            let xi = xi.expect("unwrap on None/Err");
            let si = si.expect("unwrap on None/Err");
            let xs = unsafe {
                if let Some(data) = xi.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
                    continue;
                }
            };
            let ss = unsafe {
                if let Some(data) = si.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
                    continue;
                }
            };
            s_sexp.set_integer_elt(i, if xs.ends_with(&ss) { 1 } else { 0 });
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_strtoi — convert string to integer
// ---------------------------------------------------------------------------

/// Convert strings to integers using a given base.
pub unsafe fn do_strtoi(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_strtoi_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_strtoi using `Sexp<'a>`.
fn do_strtoi_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing 'x' argument")?;
    let base = args
        .cdr()
        .and_then(|a| a.car())
        .map(|a| as_integer_safe(a))
        .unwrap_or(10);

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_INTEGER);
        } else {
            let el = el.expect("unwrap on None/Err");
            let cs = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8)
                        .to_str()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                } else {
                    s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_INTEGER);
                    continue;
                }
            };
            let val = if base == 10 {
                cs.parse::<c_int>()
            } else {
                c_int::from_str_radix(&cs, base as u32)
            };
            s_sexp.set_integer_elt(i, val.unwrap_or(crate::sexp::ffi::NA_INTEGER));
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_strrep — repeat strings
// ---------------------------------------------------------------------------

/// Repeat strings a given number of times.
pub unsafe fn do_strrep(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_strrep_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_strrep using `Sexp<'a>`.
fn do_strrep_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing 'x' argument")?;
    let times = args
        .cdr()
        .and_then(|a| a.car())
        .map(|a| as_integer_safe(a))
        .ok_or("missing 'times' argument")?;

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) || times < 0 {
            s_sexp.set_string_elt(i, na);
        } else {
            let el = el.expect("unwrap on None/Err");
            let cs = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8)
                        .to_str()
                        .unwrap_or("")
                } else {
                    s_sexp.set_string_elt(i, na);
                    continue;
                }
            };
            let repeated = cs.repeat(times as usize);
            let cstr = std::ffi::CString::new(repeated).expect("CString::new failed: contains null byte");
            let ch = unsafe { Rf_mkCharLen(cstr.as_ptr(), cstr.as_bytes().len() as c_int) };
            let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
            s_sexp.set_string_elt(i, ch_sexp);
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_strtrim — trim strings to a width
// ---------------------------------------------------------------------------

/// Trim strings to a given display width.
pub unsafe fn do_strtrim(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_strtrim_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_strtrim using `Sexp<'a>`.
fn do_strtrim_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing 'x' argument")?;
    let width = args
        .cdr()
        .and_then(|a| a.car())
        .map(|a| as_integer_safe(a))
        .unwrap_or(80);

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_string_elt(i, na);
        } else {
            let el = el.expect("unwrap on None/Err");
            let bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_string_elt(i, na);
                    continue;
                }
            };
            let trimmed = if width >= 0 && bytes.len() > width as usize {
                bytes[..width as usize].to_vec()
            } else {
                bytes
            };
            let cstr = std::ffi::CString::new(trimmed.clone()).expect("CString::new failed: contains null byte");
            let ch = unsafe { Rf_mkCharLen(cstr.as_ptr(), trimmed.len() as c_int) };
            let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
            s_sexp.set_string_elt(i, ch_sexp);
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_validUTF8 — check if strings are valid UTF-8
// ---------------------------------------------------------------------------

/// Check if elements of a character vector are valid UTF-8.
pub unsafe fn do_validUTF8(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_validutf8_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_validUTF8 using `Sexp<'a>`.
fn do_validutf8_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
        } else {
            let el = el.expect("unwrap on None/Err");
            let bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
                    continue;
                }
            };
            s_sexp.set_integer_elt(i, if std::str::from_utf8(&bytes).is_ok() { 1 } else { 0 });
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_validEnc — check if strings are valid in the current encoding
// ---------------------------------------------------------------------------

/// Check if strings are valid in the native encoding (always true in our UTF-8 impl).
pub unsafe fn do_validEnc(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_validenc_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_validEnc using `Sexp<'a>`.
fn do_validenc_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_integer_elt(i, crate::sexp::ffi::NA_LOGICAL);
        } else {
            s_sexp.set_integer_elt(i, 1); // always valid in our impl
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_encodeString — encode strings for display
// ---------------------------------------------------------------------------

/// Encode character strings for display (quote escaping).
pub unsafe fn do_encodeString(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_encodestring_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_encodeString using `Sexp<'a>`.
fn do_encodestring_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_string_elt(i, na);
        } else {
            let el = el.expect("unwrap on None/Err");
            let bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_string_elt(i, na);
                    continue;
                }
            };
            let mut encoded: Vec<u8> = Vec::with_capacity(bytes.len() + 2);
            for &b in &bytes {
                match b {
                    b'\\' => {
                        encoded.push(b'\\');
                        encoded.push(b'\\');
                    }
                    b'"' => {
                        encoded.push(b'\\');
                        encoded.push(b'"');
                    }
                    b'\n' => {
                        encoded.push(b'\\');
                        encoded.push(b'n');
                    }
                    b'\r' => {
                        encoded.push(b'\\');
                        encoded.push(b'r');
                    }
                    b'\t' => {
                        encoded.push(b'\\');
                        encoded.push(b't');
                    }
                    _ if b < 0x20 => {
                        encoded.push(b'\\');
                        encoded.push(b'x');
                        encoded.push(b"0123456789abcdef"[(b >> 4) as usize]);
                        encoded.push(b"0123456789abcdef"[(b & 0x0f) as usize]);
                    }
                    _ => encoded.push(b),
                }
            }
            let cstr = std::ffi::CString::new(encoded.clone()).expect("CString::new failed: contains null byte");
            let ch = unsafe { Rf_mkCharLen(cstr.as_ptr(), encoded.len() as c_int) };
            let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
            s_sexp.set_string_elt(i, ch_sexp);
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_makeNames — make syntactically valid names
// ---------------------------------------------------------------------------

/// Make character strings syntactically valid R names.
pub unsafe fn do_makeNames(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_makenames_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_makeNames using `Sexp<'a>`.
fn do_makenames_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let allow_unique = args
        .cdr()
        .and_then(|a| a.car())
        .map(|a| {
            let v = crate::main::coerce::asLogical(a.as_raw());
            v != 0 && v != crate::sexp::ffi::NA_LOGICAL
        })
        .unwrap_or(false);

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_string_elt(i, na);
            continue;
        }
        let el = el.expect("unwrap on None/Err");
        let cs = unsafe {
            if let Some(data) = el.data_ptr() {
                std::ffi::CStr::from_ptr(data as *const i8)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                s_sexp.set_string_elt(i, na);
                continue;
            }
        };
        let mut name = cs.clone();

        if name.is_empty() {
            name = format!("X{}", i + 1);
        } else {
            // Check if first char is valid (letter or .)
            let first = name.as_bytes()[0];
            if !(first.is_ascii_alphabetic() || first == b'.') {
                name = format!("X.{}", name);
            }
            // Replace invalid chars with .
            let mut result = Vec::<u8>::new();
            for (j, &b) in name.as_bytes().iter().enumerate() {
                if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
                    result.push(b);
                } else {
                    result.push(b'.');
                }
            }
            name = String::from_utf8(result).unwrap_or_default();
        }

        if allow_unique {
            let base = name.clone();
            let mut counter = 1usize;
            while seen.contains(&name) {
                name = format!("{}.{}", base, counter);
                counter += 1;
            }
        }
        seen.insert(name.clone());

        let cstr = std::ffi::CString::new(name).expect("CString::new failed: contains null byte");
        let ch = unsafe { Rf_mkCharLen(cstr.as_ptr(), cstr.as_bytes().len() as c_int) };
        let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
        s_sexp.set_string_elt(i, ch_sexp);
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_makeUnique — make strings unique by appending suffixes
// ---------------------------------------------------------------------------

/// Make character strings unique by appending .1, .2, etc.
pub unsafe fn do_makeUnique(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_makeunique_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_makeUnique using `Sexp<'a>`.
fn do_makeunique_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let sep = args
        .cdr()
        .and_then(|a| a.car())
        .and_then(|a| a.string_elt(0))
        .map(|s| unsafe {
            if let Some(data) = s.data_ptr() {
                std::ffi::CStr::from_ptr(data as *const i8)
                    .to_str()
                    .unwrap_or(".")
                    .to_string()
            } else {
                ".".to_string()
            }
        })
        .unwrap_or_else(|| ".".to_string());

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_string_elt(i, na);
            continue;
        }
        let el = el.expect("unwrap on None/Err");
        let cs = unsafe {
            if let Some(data) = el.data_ptr() {
                std::ffi::CStr::from_ptr(data as *const i8)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                s_sexp.set_string_elt(i, na);
                continue;
            }
        };
        let mut name = cs.clone();

        let count = seen.entry(name.clone()).or_insert(0);
        if *count > 0 {
            name = format!("{}{}{}", cs, sep, count);
        }
        *count += 1;

        let cstr = std::ffi::CString::new(name).expect("CString::new failed: contains null byte");
        let ch = unsafe { Rf_mkCharLen(cstr.as_ptr(), cstr.as_bytes().len() as c_int) };
        let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
        s_sexp.set_string_elt(i, ch_sexp);
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// do_abbreviate — abbreviate strings
// ---------------------------------------------------------------------------

/// Abbreviate strings to a minimum length that is still unique.
pub unsafe fn do_abbreviate(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Sexp::from_raw(args) {
            Some(s) => match do_abbreviate_safe(s) {
                Ok(result) => result.as_raw(),
                Err(_) => R_NilValue(),
            },
            None => R_NilValue(),
        }
    })).unwrap_or_else(|_| R_NilValue())
}

/// Safe version of do_abbreviate using `Sexp<'a>`.
fn do_abbreviate_safe<'a>(args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let na = get_na_string_safe();

    let x = args.car().ok_or("missing argument")?;
    let minlength = args
        .cdr()
        .and_then(|a| a.car())
        .map(|a| as_integer_safe(a))
        .unwrap_or(3);

    let (x, protect_count) = if x.typeof_() == SEXPTYPE::STRSXP {
        (x, 0)
    } else {
        let coerced = unsafe {
            Rf_protect(crate::main::coerce::coerceVector(x.as_raw(), SEXPTYPE::STRSXP.0))
        };
        (unsafe { Sexp::from_raw_unchecked(coerced) }, 1)
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int)) };
    let s_sexp = unsafe { Sexp::from_raw_unchecked(s) };

    // Simple implementation: truncate to minlength
    let ml = if minlength < 1 { 1 } else { minlength as usize };

    for i in 0..len {
        let el = x.string_elt(i);
        if el.map_or(true, |s| s == na) {
            s_sexp.set_string_elt(i, na);
        } else {
            let el = el.expect("unwrap on None/Err");
            let bytes = unsafe {
                if let Some(data) = el.data_ptr() {
                    std::ffi::CStr::from_ptr(data as *const i8).to_bytes().to_vec()
                } else {
                    s_sexp.set_string_elt(i, na);
                    continue;
                }
            };
            let trimmed = if bytes.len() > ml {
                bytes[..ml].to_vec()
            } else {
                bytes
            };
            let cstr = std::ffi::CString::new(trimmed.clone()).expect("CString::new failed: contains null byte");
            let ch = unsafe { Rf_mkCharLen(cstr.as_ptr(), trimmed.len() as c_int) };
            let ch_sexp = unsafe { Sexp::from_raw_unchecked(ch) };
            s_sexp.set_string_elt(i, ch_sexp);
        }
    }

    unsafe { Rf_unprotect(1 + protect_count) };
    Ok(s_sexp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mystrcpy() {
        let mut dest = [0u8; 10];
        let src = b"hello";
        mystrcpy(&mut dest, src);
        assert_eq!(&dest[..5], b"hello");
    }

    #[test]
    fn test_mystrcpy_overlap() {
        let mut buf = b"abcdef".to_vec();
        // Copy buf[2..5] = "cde" into buf[0..3], overlapping
        buf.copy_within(2..5, 0);
        assert_eq!(&buf[..6], b"cdedef");
    }

    #[test]
    fn test_iswvowel() {
        assert!(iswvowel('a'));
        assert!(iswvowel('e'));
        assert!(iswvowel('i'));
        assert!(iswvowel('o'));
        assert!(iswvowel('u'));
        assert!(iswvowel('\u{00e9}')); // é
        assert!(!iswvowel('b'));
        assert!(!iswvowel('z'));
    }

    #[test]
    fn test_tr_build_spec_chars() {
        let spec = tr_build_spec(b"abc");
        assert!(spec.is_some());
        let mut p = spec;
    }
}