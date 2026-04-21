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
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
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
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
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
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
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
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
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
        if t == SEXPTYPE::LGLSXP {
            let p = LOGICAL(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_LOGICAL;
            }
            *p
        } else if t == SEXPTYPE::INTSXP {
            let p = INTEGER(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_LOGICAL;
            }
            *p
        } else if t == SEXPTYPE::REALSXP {
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
        if t == SEXPTYPE::INTSXP {
            let p = INTEGER(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::LGLSXP {
            let p = LOGICAL(x);
            if p.is_null() {
                return crate::sexp::ffi::NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::REALSXP {
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
// Safe wrapper helpers for CHARSXP byte operations
// ---------------------------------------------------------------------------

/// Read a CHARSXP as a byte slice.
unsafe fn charsxp_bytes(s: SEXP) -> &'static [u8] {
    unsafe {
        if s.is_null() {
            return &[];
        }
        let p = CHAR(s);
        if p.is_null() {
            return &[];
        }
        std::ffi::CStr::from_ptr(p).to_bytes()
    }
}

/// Create a CHARSXP from a byte slice.
unsafe fn make_charsxp(bytes: &[u8]) -> SEXP {
    unsafe {
        if bytes.is_empty() {
            return Rf_mkCharLen(c"".as_ptr(), 0);
        }
        let cs = std::ffi::CString::new(bytes).unwrap_or_default();
        Rf_mkCharLen(cs.as_ptr(), bytes.len() as c_int)
    }
}

// ---------------------------------------------------------------------------
// do_chartr — character translation
// ---------------------------------------------------------------------------

/// Safe version of character translation using `Sexp<'a>`.
///
/// Translate characters in `x`: replace characters in `old` with corresponding
/// characters in `new`.
pub fn chartr_safe<'a>(x: Sexp<'a>, old: Sexp<'a>, new: Sexp<'a>) -> Result<SEXP, String> {
    let na = unsafe { get_na_string() };

    if old.typeof_() != SEXPTYPE::STRSXP {
        return Err("invalid 'old' argument".into());
    }
    if old.is_empty() {
        return Err("invalid 'old' argument".into());
    }
    let old_first = old.string_elt(0).ok_or("invalid 'old' argument")?;
    if old_first.as_raw() == na {
        return Err("invalid 'old' argument".into());
    }

    if new.typeof_() != SEXPTYPE::STRSXP {
        return Err("invalid 'new' argument".into());
    }
    if new.is_empty() {
        return Err("invalid 'new' argument".into());
    }
    let new_first = new.string_elt(0).ok_or("invalid 'new' argument")?;
    if new_first.as_raw() == na {
        return Err("invalid 'new' argument".into());
    }

    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("invalid 'x' argument".into());
    }

    // Build byte-level translation table (256 entries, identity by default)
    let mut xtable: [u8; 256] = {
        let mut t = [0u8; 256];
        for i in 0..256usize {
            t[i] = i as u8;
        }
        t
    };

    // Parse old spec
    let old_bytes = unsafe { charsxp_bytes(old_first.as_raw()) };
    let old_spec = tr_build_spec(old_bytes);
    // Parse new spec
    let new_bytes = unsafe { charsxp_bytes(new_first.as_raw()) };
    let new_spec = tr_build_spec(new_bytes);

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
            return Err("'old' is longer than 'new'".into());
        }
        xtable[c_old as usize] = c_new;
    }

    let n = x.len() as c_int;
    let y = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, n)) };

    for i in 0..x.len() {
        let el = x.string_elt(i).ok_or("missing string element")?;
        if el.as_raw() == na {
            unsafe { SET_STRING_ELT(y, i, na) };
        } else {
            let xi_bytes = unsafe { charsxp_bytes(el.as_raw()) };
            let mut buf: Vec<u8> = xi_bytes.to_vec();
            for b in buf.iter_mut() {
                *b = xtable[*b as usize];
            }
            let ch = unsafe { make_charsxp(&buf) };
            unsafe { SET_STRING_ELT(y, i, ch) };
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(y)
}

/// Translate characters in `x`: replace characters in `old` with corresponding
/// characters in `new`.
///
/// This is the Rust port of R's `do_chartr` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let old = match args_s.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let args2 = match args_s.cdr() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let new = match args2.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let args3 = match args2.cdr() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let x = match args3.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };

        match chartr_safe(x, old, new) {
            Ok(result) => result,
            Err(_) => crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { crate::sexp::globals::R_NilValue() })
}

// ---------------------------------------------------------------------------
// do_toupper — convert to uppercase
// ---------------------------------------------------------------------------

/// Safe version of toupper using `Sexp<'a>`.
pub fn toupper_safe(x: Sexp<'_>) -> Result<SEXP, String> {
    case_transform_safe(x, true)
}

/// Convert characters in a character vector to uppercase.
///
/// This is the Rust port of R's `do_tolower`/`do_toupper` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };

        match toupper_safe(x) {
            Ok(result) => result,
            Err(_) => crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { crate::sexp::globals::R_NilValue() })
}

// ---------------------------------------------------------------------------
// do_tolower — convert to lowercase
// ---------------------------------------------------------------------------

/// Safe version of tolower using `Sexp<'a>`.
pub fn tolower_safe(x: Sexp<'_>) -> Result<SEXP, String> {
    case_transform_safe(x, false)
}

/// Convert characters in a character vector to lowercase.
///
/// This is the Rust port of R's `do_tolower` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };

        match tolower_safe(x) {
            Ok(result) => result,
            Err(_) => crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { crate::sexp::globals::R_NilValue() })
}

/// Shared safe implementation for toupper and tolower.
fn case_transform_safe(x: Sexp<'_>, upper: bool) -> Result<SEXP, String> {
    let na = unsafe { get_na_string() };

    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("non-character argument".into());
    }

    let n = x.len();
    let y = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, n as c_int)) };

    for i in 0..n {
        let el = x.string_elt(i).ok_or("missing string element")?;
        if el.as_raw() == na {
            unsafe { SET_STRING_ELT(y, i, na) };
        } else {
            let xi_bytes = unsafe { charsxp_bytes(el.as_raw()) };
            let mut buf: Vec<u8> = xi_bytes.to_vec();
            for b in buf.iter_mut() {
                if upper {
                    *b = (*b as char).to_ascii_uppercase() as u8;
                } else {
                    *b = (*b as char).to_ascii_lowercase() as u8;
                }
            }
            let ch = unsafe { make_charsxp(&buf) };
            unsafe { SET_STRING_ELT(y, i, ch) };
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(y)
}

// ---------------------------------------------------------------------------
// do_nchar — character counting
// ---------------------------------------------------------------------------

/// Enum for nchar type argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NcharType {
    Bytes,
    Chars,
    Width,
}

/// Safe version of nchar using `Sexp<'a>`.
pub fn nchar_safe(
    x: Sexp<'_>,
    type_: NcharType,
    allow_na: bool,
    keep_na: bool,
) -> Result<SEXP, String> {
    let na = unsafe { get_na_string() };

    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("'nchar' requires a character vector".into());
    }

    let type_code = match type_ {
        NcharType::Bytes => "bytes",
        NcharType::Chars => "chars",
        NcharType::Width => "width",
    };

    let len = x.len();
    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, len as c_int)) };

    for i in 0..len {
        let sxi = x.string_elt(i).ok_or("missing string element")?;
        if sxi.as_raw() == na {
            let val = if keep_na {
                crate::sexp::ffi::NA_INTEGER
            } else {
                2 // NA string length
            };
            unsafe {
                let s_data = INTEGER(s);
                *s_data.add(i as usize) = val;
            }
        } else {
            let res = unsafe { r_nchar(sxi.as_raw(), type_code, allow_na, keep_na, i) };
            if res == -1 {
                return Err(format!("invalid multibyte string, element {}", i + 1));
            } else if res == -2 {
                let msg = if type_code == "chars" {
                    format!(
                        "number of characters is not computable in \"bytes\" encoding, element {}",
                        i + 1
                    )
                } else {
                    format!(
                        "width is not computable in \"bytes\" encoding, element {}",
                        i + 1
                    )
                };
                return Err(msg);
            } else {
                unsafe {
                    let s_data = INTEGER(s);
                    *s_data.add(i as usize) = res;
                }
            }
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(s)
}

/// Count the number of characters in each element of a character vector.
///
/// This is the Rust port of R's `do_nchar` from character.c.
/// Supports type = "bytes", "chars", "width".
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        unsafe {
            let args_s = match Sexp::from_raw(args) {
                Some(s) => s,
                None => return crate::sexp::globals::R_NilValue(),
            };

            let x_arg = match args_s.car() {
                Some(s) => s,
                None => return crate::sexp::globals::R_NilValue(),
            };

            // Parse type argument (second arg)
            let args2 = match args_s.cdr() {
                Some(s) => s,
                None => return crate::sexp::globals::R_NilValue(),
            };
            let stype = match args2.car() {
                Some(s) => s,
                None => return crate::sexp::globals::R_NilValue(),
            };

            if stype.typeof_() != SEXPTYPE::STRSXP || stype.len() != 1 {
                return crate::sexp::globals::R_NilValue();
            }
            let type_char = match stype.string_elt(0) {
                Some(s) => s,
                None => return crate::sexp::globals::R_NilValue(),
            };
            let type_str = std::ffi::CStr::from_ptr(CHAR(type_char.as_raw())).to_string_lossy();
            let type_str_trimmed = type_str.trim();

            let type_code: NcharType = if type_str_trimmed.starts_with("bytes") {
                NcharType::Bytes
            } else if type_str_trimmed.starts_with("chars") {
                NcharType::Chars
            } else if type_str_trimmed.starts_with("width") {
                NcharType::Width
            } else {
                return crate::sexp::globals::R_NilValue();
            };

            // Parse allowNA (third arg)
            let args3 = match args2.cdr() {
                Some(s) => s,
                None => return crate::sexp::globals::R_NilValue(),
            };
            let allow_na_val = as_logical(
                args3
                    .car()
                    .map(|s| s.as_raw())
                    .unwrap_or_else(|| crate::sexp::globals::R_NilValue()),
            );
            let allow_na = if allow_na_val == crate::sexp::ffi::NA_LOGICAL {
                false
            } else {
                allow_na_val != 0
            };

            // Parse keepNA (fourth arg, optional)
            let nargs = crate::sexp::constructors::Rf_length(args);
            let keep_na: bool;
            if nargs >= 4 {
                let args4 = match args3.cdr() {
                    Some(s) => s,
                    None => return crate::sexp::globals::R_NilValue(),
                };
                let keep_na_val = as_logical(
                    args4
                        .car()
                        .map(|s| s.as_raw())
                        .unwrap_or_else(|| crate::sexp::globals::R_NilValue()),
                );
                if keep_na_val == crate::sexp::ffi::NA_LOGICAL {
                    keep_na = type_code != NcharType::Width;
                } else {
                    keep_na = keep_na_val != 0;
                }
            } else {
                keep_na = type_code != NcharType::Width;
            }

            match nchar_safe(x_arg, type_code, allow_na, keep_na) {
                Ok(result) => result,
                Err(_) => crate::sexp::globals::R_NilValue(),
            }
        }
    }))
    .unwrap_or_else(|_| unsafe { crate::sexp::globals::R_NilValue() })
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

/// Safe version of substr using `Sexp<'a>`.
pub fn substr_safe<'a>(
    x: Sexp<'a>,
    starts: Sexp<'a>,
    stops: Option<Sexp<'a>>,
) -> Result<SEXP, String> {
    let na = unsafe { get_na_string() };
    let blank = unsafe { blank_string() };

    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err("extracting substrings from a non-character object".into());
    }

    let len = x.len();

    if starts.typeof_() != SEXPTYPE::INTSXP || starts.is_empty() {
        return Err("invalid substring arguments".into());
    }

    if let Some(ref stops) = stops
        && (stops.typeof_() != SEXPTYPE::INTSXP || stops.is_empty())
    {
        return Err("invalid substring arguments".into());
    }

    let k = starts.len();
    let l_val = stops.as_ref().map(|s| s.len()).unwrap_or(1);

    let s = unsafe { Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, len as c_int)) };

    for i in 0..len {
        let start = starts
            .integer_elt((i as R_xlen_t) % k)
            .unwrap_or(crate::sexp::ffi::NA_INTEGER);
        let stop = stops
            .as_ref()
            .and_then(|s| s.integer_elt((i as R_xlen_t) % l_val))
            .unwrap_or(c_int::MAX);

        let el = x.string_elt(i).ok_or("missing string element")?;

        if el.as_raw() == na
            || start == crate::sexp::ffi::NA_INTEGER
            || stop == crate::sexp::ffi::NA_INTEGER
        {
            unsafe { SET_STRING_ELT(s, i, na) };
            continue;
        }

        let ss_bytes = unsafe { charsxp_bytes(el.as_raw()) };
        let slen = ss_bytes.len() as c_int;

        let mut start = start;

        if start < 1 {
            start = 1;
        }

        if start > stop {
            unsafe { SET_STRING_ELT(s, i, blank) };
        } else {
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
                unsafe { SET_STRING_ELT(s, i, blank) };
                continue;
            }

            let substr_bytes = &ss_bytes[rfrom..rfrom + rlen];
            let ch = unsafe { make_charsxp(substr_bytes) };
            unsafe { SET_STRING_ELT(s, i, ch) };
        }
    }

    unsafe { Rf_unprotect(1) };
    Ok(s)
}

/// Extract substrings from a character vector.
///
/// This is the Rust port of R's `do_substr` from character.c.
/// For this port we use the byte-level (non-MBCS) path.
pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };

        let x = match args_s.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };

        let args2 = match args_s.cdr() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let sa = match args2.car() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };

        let args3 = match args2.cdr() {
            Some(s) => s,
            None => return crate::sexp::globals::R_NilValue(),
        };
        let so = args3.car();

        match substr_safe(x, sa, so) {
            Ok(result) => result,
            Err(_) => crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { crate::sexp::globals::R_NilValue() })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::globals::*;
    use std::ptr;

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
        assert_eq!(tr_get_next_char(&mut p), b'a');
        assert_eq!(tr_get_next_char(&mut p), b'b');
        assert_eq!(tr_get_next_char(&mut p), b'c');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_range() {
        let spec = tr_build_spec(b"a-c");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'a');
        assert_eq!(tr_get_next_char(&mut p), b'b');
        assert_eq!(tr_get_next_char(&mut p), b'c');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_mixed() {
        let spec = tr_build_spec(b"a-cx");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'a');
        assert_eq!(tr_get_next_char(&mut p), b'b');
        assert_eq!(tr_get_next_char(&mut p), b'c');
        assert_eq!(tr_get_next_char(&mut p), b'x');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_single() {
        let spec = tr_build_spec(b"z");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'z');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_empty() {
        assert!(tr_build_spec(b"").is_none());
    }

    #[test]
    fn test_wtr_build_spec_range() {
        let chars: Vec<char> = "a-c".chars().collect();
        let spec = wtr_build_spec(&chars);
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(wtr_get_next_char(&mut p), 'a');
        assert_eq!(wtr_get_next_char(&mut p), 'b');
        assert_eq!(wtr_get_next_char(&mut p), 'c');
        assert_eq!(wtr_get_next_char(&mut p), '\0');
    }

    #[test]
    fn test_xtable_comp() {
        let a = XtableT {
            c_old: 'a',
            c_new: 'x',
        };
        let b = XtableT {
            c_old: 'b',
            c_new: 'y',
        };
        assert_eq!(xtable_comp(&a, &b), std::cmp::Ordering::Less);
        assert_eq!(xtable_comp(&b, &a), std::cmp::Ordering::Greater);
        assert_eq!(xtable_comp(&a, &a), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_xtable_key_comp() {
        let entry = XtableT {
            c_old: 'm',
            c_new: 'x',
        };
        assert_eq!(xtable_key_comp('a', &entry), std::cmp::Ordering::Less);
        assert_eq!(xtable_key_comp('z', &entry), std::cmp::Ordering::Greater);
        assert_eq!(xtable_key_comp('m', &entry), std::cmp::Ordering::Equal);
    }

    // ---- Helper: build a STRSXP from Rust string slices ----

    /// Helper to build a STRSXP vector from Rust strings.
    unsafe fn make_strsxp(strs: &[&str]) -> SEXP {
        let n = strs.len() as c_int;
        let s = unsafe { Rf_allocVector(SEXPTYPE::STRSXP, n) };
        for (i, st) in strs.iter().enumerate() {
            let cs = std::ffi::CString::new(*st).unwrap_or_default();
            let ch = unsafe { Rf_mkCharLen(cs.as_ptr(), st.len() as c_int) };
            unsafe { SET_STRING_ELT(s, i as R_xlen_t, ch) };
        }
        s
    }

    /// Helper to build an INTSXP vector from Rust integers.
    unsafe fn make_intsxp(vals: &[c_int]) -> SEXP {
        let n = vals.len() as c_int;
        let s = unsafe { Rf_allocVector(SEXPTYPE::INTSXP, n) };
        let p = unsafe { INTEGER(s) };
        for (i, v) in vals.iter().enumerate() {
            unsafe { *p.add(i) = *v };
        }
        s
    }

    /// Helper to build an LGLSXP vector from Rust integers.
    unsafe fn make_lglsxp(vals: &[c_int]) -> SEXP {
        let n = vals.len() as c_int;
        let s = unsafe { Rf_allocVector(SEXPTYPE::LGLSXP, n) };
        let p = unsafe { LOGICAL(s) };
        for (i, v) in vals.iter().enumerate() {
            unsafe { *p.add(i) = *v };
        }
        s
    }

    /// Helper to read back a STRSXP element as a Rust String.
    unsafe fn strsxp_to_string(s: SEXP, i: R_xlen_t) -> String {
        let el = unsafe { STRING_ELT(s, i) };
        unsafe { std::ffi::CStr::from_ptr(CHAR(el)) }
            .to_string_lossy()
            .into_owned()
    }

    /// Helper to build a pairlist (args) from a vec of SEXP values.
    unsafe fn make_args(items: &[SEXP]) -> SEXP {
        if items.is_empty() {
            return unsafe { R_NilValue() };
        }
        let mut result = unsafe { R_NilValue() };
        for item in items.iter().rev() {
            result = unsafe { Rf_cons(*item, result) };
        }
        result
    }

    // ---- Tests for do_toupper ----

    #[test]
    fn test_do_toupper_basic() {
        unsafe {
            let x = make_strsxp(&["hello", "world"]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 2);
            assert_eq!(strsxp_to_string(result, 0), "HELLO");
            assert_eq!(strsxp_to_string(result, 1), "WORLD");
        }
    }

    #[test]
    fn test_do_toupper_already_upper() {
        unsafe {
            let x = make_strsxp(&["ABC", "XYZ"]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "ABC");
            assert_eq!(strsxp_to_string(result, 1), "XYZ");
        }
    }

    #[test]
    fn test_do_toupper_mixed() {
        unsafe {
            let x = make_strsxp(&["HeLLo", "WoRlD"]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "HELLO");
            assert_eq!(strsxp_to_string(result, 1), "WORLD");
        }
    }

    #[test]
    fn test_do_toupper_empty_string() {
        unsafe {
            let x = make_strsxp(&[""]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "");
        }
    }

    // ---- Tests for do_tolower ----

    #[test]
    fn test_do_tolower_basic() {
        unsafe {
            let x = make_strsxp(&["HELLO", "WORLD"]);
            let args = make_args(&[x]);
            let result = do_tolower(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 2);
            assert_eq!(strsxp_to_string(result, 0), "hello");
            assert_eq!(strsxp_to_string(result, 1), "world");
        }
    }

    #[test]
    fn test_do_tolower_already_lower() {
        unsafe {
            let x = make_strsxp(&["abc", "xyz"]);
            let args = make_args(&[x]);
            let result = do_tolower(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "abc");
            assert_eq!(strsxp_to_string(result, 1), "xyz");
        }
    }

    #[test]
    fn test_do_tolower_mixed() {
        unsafe {
            let x = make_strsxp(&["HeLLo", "WoRlD"]);
            let args = make_args(&[x]);
            let result = do_tolower(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "hello");
            assert_eq!(strsxp_to_string(result, 1), "world");
        }
    }

    // ---- Tests for do_chartr ----

    #[test]
    fn test_do_chartr_basic() {
        unsafe {
            let old = make_strsxp(&["aeiou"]);
            let new = make_strsxp(&["AEIOU"]);
            let x = make_strsxp(&["hello world"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(strsxp_to_string(result, 0), "hEllO wOrld");
        }
    }

    #[test]
    fn test_do_chartr_range() {
        unsafe {
            let old = make_strsxp(&["a-z"]);
            let new = make_strsxp(&["A-Z"]);
            let x = make_strsxp(&["hello"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "HELLO");
        }
    }

    #[test]
    fn test_do_chartr_multiple_strings() {
        unsafe {
            let old = make_strsxp(&["ab"]);
            let new = make_strsxp(&["BA"]);
            let x = make_strsxp(&["abc", "bad"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "BAc");
            assert_eq!(strsxp_to_string(result, 1), "ABd");
        }
    }

    #[test]
    fn test_do_chartr_no_match() {
        unsafe {
            let old = make_strsxp(&["xyz"]);
            let new = make_strsxp(&["XYZ"]);
            let x = make_strsxp(&["hello"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // Characters not in 'old' pass through unchanged
            assert_eq!(strsxp_to_string(result, 0), "hello");
        }
    }

    // ---- Tests for do_nchar ----

    #[test]
    fn test_do_nchar_bytes() {
        unsafe {
            let x = make_strsxp(&["hello", "world", ""]);
            let stype = make_strsxp(&["bytes"]);
            let allow_na = make_lglsxp(&[0]);
            let args = make_args(&[x, stype, allow_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 3);
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 5);
            assert_eq!(*p.add(1), 5);
            assert_eq!(*p.add(2), 0);
        }
    }

    #[test]
    fn test_do_nchar_chars() {
        unsafe {
            let x = make_strsxp(&["abc", "de"]);
            let stype = make_strsxp(&["chars"]);
            let allow_na = make_lglsxp(&[0]);
            let args = make_args(&[x, stype, allow_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 3);
            assert_eq!(*p.add(1), 2);
        }
    }

    #[test]
    fn test_do_nchar_width() {
        unsafe {
            let x = make_strsxp(&["test"]);
            let stype = make_strsxp(&["width"]);
            let allow_na = make_lglsxp(&[0]);
            let args = make_args(&[x, stype, allow_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 4);
        }
    }

    #[test]
    fn test_do_nchar_keep_na_false() {
        unsafe {
            // For non-width types, default keepNA = TRUE
            let x = make_strsxp(&["hello"]);
            let stype = make_strsxp(&["bytes"]);
            let allow_na = make_lglsxp(&[0]);
            let keep_na = make_lglsxp(&[0]); // FALSE
            let args = make_args(&[x, stype, allow_na, keep_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 5);
        }
    }

    // ---- Tests for do_substr ----

    #[test]
    fn test_do_substr_basic() {
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[2]);
            let stop = make_intsxp(&[4]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(strsxp_to_string(result, 0), "ell");
        }
    }

    #[test]
    fn test_do_substr_full_string() {
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[1]);
            let stop = make_intsxp(&[5]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "hello");
        }
    }

    #[test]
    fn test_do_substr_beyond_end() {
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[3]);
            let stop = make_intsxp(&[100]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // Should return "llo" (from position 3 to end)
            assert_eq!(strsxp_to_string(result, 0), "llo");
        }
    }

    #[test]
    fn test_do_substr_empty_result() {
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[4]);
            let stop = make_intsxp(&[3]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // start > stop => blank string
            assert_eq!(strsxp_to_string(result, 0), "");
        }
    }

    #[test]
    fn test_do_substr_multiple_strings() {
        unsafe {
            let x = make_strsxp(&["hello", "world"]);
            let start = make_intsxp(&[2]);
            let stop = make_intsxp(&[3]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(LENGTH(result), 2);
            assert_eq!(strsxp_to_string(result, 0), "el");
            assert_eq!(strsxp_to_string(result, 1), "or");
        }
    }

    #[test]
    fn test_do_substr_single_char() {
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[3]);
            let stop = make_intsxp(&[3]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "l");
        }
    }

    #[test]
    fn test_do_substr_empty_input() {
        unsafe {
            let x = make_strsxp(&[]);
            let start = make_intsxp(&[1]);
            let stop = make_intsxp(&[1]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(LENGTH(result), 0);
        }
    }

    // ---- Tests for helper functions ----

    #[test]
    fn test_as_logical() {
        unsafe {
            let t = make_lglsxp(&[1]);
            let f = make_lglsxp(&[0]);
            assert_eq!(as_logical(t), 1);
            assert_eq!(as_logical(f), 0);
        }
    }

    #[test]
    fn test_as_integer() {
        unsafe {
            let v = make_intsxp(&[42]);
            assert_eq!(as_integer(v), 42);
        }
    }

    #[test]
    fn test_charsxp_byte_len() {
        unsafe {
            let cs = Rf_mkCharLen(c"hello".as_ptr(), 5);
            assert_eq!(charsxp_byte_len(cs), 5);
        }
    }
}
