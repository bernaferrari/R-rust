#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of standalone functions from R's src/main/util.c
//!
//! This file contains pure algorithmic/utility functions that do NOT depend
//! on SEXP, R_alloc, PROTECT, or other R internals. Functions that depend
//! on R types are provided as stubs returning null/zero.

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;
use std::{mem::MaybeUninit, slice};

use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{R_ClassSymbol, R_DimSymbol, getAttrib};
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL value (from R_ext/Arith.h)
pub const R_NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// R's NaN value
pub const R_NaN: c_double = f64::NAN;

/// R's positive infinity
pub const R_PosInf: c_double = f64::INFINITY;

/// R's negative infinity
pub const R_NegInf: c_double = f64::NEG_INFINITY;

/// Rboolean type matching C definition
pub type Rboolean = c_int;

pub const TRUE: Rboolean = 1;
pub const FALSE: Rboolean = 0;

fn r_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

/// R_wchar_t: unsigned 32-bit for UCS-4
pub type R_wchar_t = u32;

/// UTF-16 surrogate helpers
const LOW_SURROGATE_START: u32 = 0xDC00;

// ---------------------------------------------------------------------------
// Minimal libc helpers (avoiding the libc crate)
// ---------------------------------------------------------------------------

#[inline]
fn libc_isspace(c: c_char) -> bool {
    let byte = c as u8;
    byte == b' '
        || byte == b'\t'
        || byte == b'\n'
        || byte == b'\r'
        || byte == b'\x0b'
        || byte == b'\x0c'
}

#[inline]
fn to_lower(c: u8) -> u8 {
    if c >= b'A' && c <= b'Z' { c + 32 } else { c }
}

// ---------------------------------------------------------------------------
// String utility: strIsASCII
// ---------------------------------------------------------------------------

/// Check whether a string consists solely of ASCII characters.
///
/// Port of R's `strIsASCII` from util.c.
pub unsafe fn Rf_strIsASCII(s: *const c_char) -> Rboolean {
    unsafe {
        if s.is_null() {
            return TRUE;
        }
        let mut p = s;
        while *p != 0 {
            let byte = *p as u8;
            if byte > 0x7F {
                return FALSE;
            }
            p = p.add(1);
        }
        TRUE
    }
}

// ---------------------------------------------------------------------------
// UTF-8 table: number of additional bytes per leading byte
// ---------------------------------------------------------------------------

/// Table from R's util.c: number of additional bytes needed for a UTF-8
/// sequence given its leading byte (low 6 bits).
static UTF8_TABLE4: [u8; 64] = [
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5,
];

/// Return the number of bytes in a UTF-8 character given its leading byte.
///
/// Port of R's `utf8clen` from util.c.
/// This allows through 8-bit chars 10xxxxxx, which are invalid.
pub fn utf8clen(c: c_char) -> c_int {
    let byte = c as u8;
    if (byte & 0xC0) != 0xC0 {
        return 1;
    }
    1 + UTF8_TABLE4[(byte & 0x3F) as usize] as c_int
}

// ---------------------------------------------------------------------------
// UTF-8 <-> UCS conversion helpers
// ---------------------------------------------------------------------------

/// Convert a UTF-16 surrogate pair to a UCS-4 codepoint.
///
/// Port of R's static `utf16toucs` from util.c.
#[inline]
fn utf16toucs(high: u32, low: u32) -> R_wchar_t {
    0x10000 + ((high & 0x3FF) << 10) + (low & 0x3FF)
}

/// Return the low UTF-16 surrogate from a 4-byte UTF-8 sequence.
///
/// Port of R's static `utf8toutf16low` from util.c.
/// Assumes all validation has been done already.
#[inline]
fn utf8toutf16low(bytes: &[u8]) -> u32 {
    LOW_SURROGATE_START | ((u32::from(bytes[2]) & 0x0f) << 6) | (u32::from(bytes[3]) & 0x3f)
}

/// Convert UTF-8 (high surrogate wchar + string pointer) to UCS-32.
///
/// Port of R's `utf8toucs32` from util.c.
///
/// # Safety
/// `s` must be readable for at least four bytes.
pub unsafe fn Rf_utf8toucs32(high: u32, s: *const c_char) -> R_wchar_t {
    // SAFETY: caller guarantees four readable bytes at `s`.
    let bytes = unsafe { slice::from_raw_parts(s.cast::<u8>(), 4) };
    utf16toucs(high, utf8toutf16low(bytes))
}

// ---------------------------------------------------------------------------
// utf8toucs: convert a single UTF-8 character to wchar_t
// ---------------------------------------------------------------------------

const INVALID_UTF8: usize = usize::MAX;
const INCOMPLETE_UTF8: usize = usize::MAX - 1;

/// Decode one UTF-8 scalar from a bounded prefix.
///
/// Returns `(value, bytes consumed)`, or `Err(INVALID_UTF8)` / `Err(INCOMPLETE_UTF8)`.
/// Keeps GNU's 32-bit wchar arithmetic, including the legacy 5- and 6-byte
/// masks/shifts and the no-trail-validation rule for those sequences.
fn decode_wide_prefix(input: &[u8]) -> Result<(u32, usize), usize> {
    let Some(&lead) = input.first() else {
        return Ok((0, 0));
    };
    if lead < 0x80 {
        return Ok((u32::from(lead), 1));
    }
    if lead < 0xc0 {
        return Err(INVALID_UTF8);
    }
    let n = utf8clen(lead as c_char) as usize;
    let prefix = input.get(..n).ok_or(INCOMPLETE_UTF8)?;
    let trail = &prefix[1..];
    if n <= 4 && trail.iter().any(|byte| byte & 0xc0 != 0x80) {
        return Err(INVALID_UTF8);
    }
    let b = |i: usize| u32::from(prefix[i] & 0x3f);
    let lead = u32::from(lead);
    let value = match n {
        2 => ((lead & 0x1f) << 6) | b(1),
        3 => ((lead & 0x0f) << 12) | (b(1) << 6) | b(2),
        4 => ((lead & 0x0f) << 18) | (b(1) << 12) | (b(2) << 6) | b(3),
        5 => ((lead & 0x0f) << 24) | (b(1) << 12) | (b(2) << 12) | (b(3) << 6) | b(4),
        6 => {
            ((lead & 0x0f) << 30) | (b(1) << 24) | (b(2) << 18) | (b(3) << 12) | (b(4) << 6) | b(5)
        }
        _ => return Err(INVALID_UTF8),
    };
    if n == 3 && ((0xd800..=0xdfff).contains(&value) || matches!(value, 0xfffe | 0xffff)) {
        return Err(INVALID_UTF8);
    }
    Ok((value, n))
}

/// Convert a single UTF-8 character to a wide character.
///
/// Returns the number of bytes consumed from `s`, or (size_t)-1 on invalid,
/// or (size_t)-2 if the string is too short.
///
/// If `wc` is null, the result is discarded but the byte count is returned.
///
/// Port of R's `utf8toucs` from util.c.
///
/// # Safety
/// `s` must be a readable NUL-terminated string. If `wc` is non-null it must
/// be writable for one `u32` (possibly uninitialized) and not overlap `s`.
pub unsafe fn utf8toucs(wc: *mut u32, s: *const c_char) -> usize {
    // Read at most one encoded character, without scanning the entire string.
    let mut prefix = [0; 6];
    let len = unsafe {
        // SAFETY: `s` is a readable NUL-terminated string; the loop stops at
        // the first NUL or after `bound` bytes.
        let first = s.cast::<u8>().read();
        let bound = if first < 0xc0 {
            1
        } else {
            utf8clen(first as c_char) as usize
        };
        let mut len = 0;
        for (i, slot) in prefix[..bound].iter_mut().enumerate() {
            let byte = s.cast::<u8>().add(i).read();
            if byte == 0 {
                break;
            }
            *slot = byte;
            len += 1;
        }
        len
    };
    match decode_wide_prefix(&prefix[..len]) {
        Ok((value, n)) => {
            if !wc.is_null() {
                // SAFETY: `wc` is a writable non-overlapping output slot.
                unsafe { wc.write(value) };
            }
            n
        }
        Err(code) => code,
    }
}

// ---------------------------------------------------------------------------
// utf8towcs / utf8towcs4: convert UTF-8 strings to wide strings
// ---------------------------------------------------------------------------

/// Shared decoder for `utf8towcs` (32-bit wide output, preserving the legacy
/// handling of surrogate-valued inputs) and `utf8towcs4` (UCS-4 output).
/// Writes at most `output.len()` units; a null output means count only.
fn decode_wide_string(
    mut input: &[u8],
    mut output: Option<&mut [MaybeUninit<u32>]>,
    ucs4: bool,
) -> usize {
    let limit = output.as_ref().map_or(usize::MAX, |out| out.len());
    let mut written = 0;
    while written < limit {
        let (mut value, consumed) = match decode_wide_prefix(input) {
            Ok(result) => result,
            Err(_) => {
                if ucs4 && let Some(out) = output.as_deref_mut() {
                    out[written].write(0);
                }
                eprintln!(
                    "invalid input in {}",
                    if ucs4 { "utf8towcs4" } else { "utf8towcs" }
                );
                return 0;
            }
        };
        let low =
            (consumed >= 4 && (0xd800..=0xdbff).contains(&value)).then(|| utf8toutf16low(input));
        if ucs4 && let Some(low) = low {
            value = utf16toucs(value, low);
        }
        if let Some(out) = output.as_deref_mut() {
            out[written].write(value);
        }
        if consumed == 0 {
            break;
        }
        written += 1;
        if !ucs4
            && let Some(low) = low
            && written < limit
        {
            if let Some(out) = output.as_deref_mut() {
                out[written].write(low);
            }
            written += 1;
        }
        input = &input[consumed..];
    }
    written
}

/// Convert a UTF-8 string to a wide-character string.
///
/// If `wc` is null, only counts the resulting characters.
/// `n` is the maximum number of wide characters to write.
///
/// Returns the number of wide characters written (not including terminator).
///
/// Port of R's `utf8towcs` from util.c. Reading the input as a `CStr` may
/// inspect the full string even when conversion stops at capacity.
///
/// # Safety
/// `s` must be a readable NUL-terminated string. If `wc` is non-null it must
/// be writable for `n` `u32` values (possibly uninitialized) and must not
/// overlap `s`.
pub unsafe fn utf8towcs(wc: *mut u32, s: *const c_char, n: usize) -> usize {
    if !wc.is_null() && n == 0 {
        return 0;
    }
    // SAFETY: `s` is NUL-terminated and readable; `wc`, when non-null, is
    // writable for `n` possibly-uninitialized values and disjoint from `s`.
    let (input, output) = unsafe {
        (
            CStr::from_ptr(s).to_bytes(),
            (!wc.is_null()).then(|| slice::from_raw_parts_mut(wc.cast::<MaybeUninit<u32>>(), n)),
        )
    };
    decode_wide_string(input, output, false)
}

/// Convert a UTF-8 string to a UCS-4 (R_wchar_t) string.
///
/// Port of R's `utf8towcs4` from util.c. Reading the input as a `CStr` may
/// inspect the full string even when conversion stops at capacity.
///
/// # Safety
/// `s` must be a readable NUL-terminated string. If `wc` is non-null it must
/// be writable for `n` `u32` values (possibly uninitialized) and must not
/// overlap `s`.
pub unsafe fn utf8towcs4(wc: *mut R_wchar_t, s: *const c_char, n: usize) -> usize {
    if !wc.is_null() && n == 0 {
        return 0;
    }
    // SAFETY: `s` is NUL-terminated and readable; `wc`, when non-null, is
    // writable for `n` possibly-uninitialized values and disjoint from `s`.
    let (input, output) = unsafe {
        (
            CStr::from_ptr(s).to_bytes(),
            (!wc.is_null()).then(|| slice::from_raw_parts_mut(wc.cast::<MaybeUninit<u32>>(), n)),
        )
    };
    decode_wide_string(input, output, true)
}

// ---------------------------------------------------------------------------
// Rwcrtomb32: convert a single UCS-4 codepoint to UTF-8
// ---------------------------------------------------------------------------

/// Table from pcre.c used in UTF-8 encoding.
static UTF8_TABLE1: [u32; 6] = [0x7f, 0x7ff, 0xffff, 0x1fffff, 0x3ffffff, 0x7fffffff];
static UTF8_TABLE2: [u8; 6] = [0, 0xc0, 0xe0, 0xf0, 0xf8, 0xfc];

/// Encode one wide scalar. Clears the first output byte so failure paths are
/// well-defined, then writes the encoding only when it fits with room for a
/// terminator. Returns the encoded byte count (no terminator), or 0.
fn encode_wide_scalar(
    mut output: Option<&mut [MaybeUninit<u8>]>,
    mut value: u32,
    capacity: usize,
) -> usize {
    if capacity == 0 {
        return 0;
    }
    if let Some(out) = output.as_deref_mut() {
        out[0].write(0);
    }
    if value == 0 {
        return 0;
    }
    let Some(last) = UTF8_TABLE1.iter().position(|max| value <= *max) else {
        return 0;
    };
    if last >= capacity - 1 {
        return 0;
    }
    if let Some(out) = output {
        for slot in out[1..last + 1].iter_mut().rev() {
            slot.write(0x80 | (value & 0x3f) as u8);
            value >>= 6;
        }
        out[0].write(UTF8_TABLE2[last] | value as u8);
    }
    last + 1
}

/// Convert a single UCS-4 codepoint to UTF-8.
///
/// Writes the UTF-8 encoding into `s` (no terminator is appended; the first
/// byte is cleared on failure). Returns the number of bytes encoded, or 0.
/// A null `s` counts respecting `n`.
///
/// Port of R's static `Rwcrtomb32` from util.c.
///
/// # Safety
/// If `s` is non-null it must be writable for `n` bytes (possibly
/// uninitialized).
pub unsafe fn Rwcrtomb32(s: *mut c_char, cvalue: R_wchar_t, n: usize) -> usize {
    // SAFETY: `s`, when non-null with n > 0, is writable for n bytes.
    let output = unsafe {
        (!s.is_null() && n > 0).then(|| slice::from_raw_parts_mut(s.cast::<MaybeUninit<u8>>(), n))
    };
    encode_wide_scalar(output, cvalue, n)
}

// ---------------------------------------------------------------------------
// wcstoutf8 / wcs4toutf8: convert wide strings to UTF-8
// ---------------------------------------------------------------------------

/// Encode a NUL-terminated wide stream, appending a terminator while there
/// is room. Returns the number of bytes including the terminator.
fn encode_wide_string(
    input: impl Iterator<Item = u32>,
    mut output: Option<&mut [MaybeUninit<u8>]>,
    capacity: usize,
    combine_surrogates: bool,
) -> usize {
    let mut input = input.peekable();
    let mut written = 0;
    while let Some(mut value) = input.next() {
        if combine_surrogates
            && (0xd800..=0xdbff).contains(&value)
            && let Some(&low) = input.peek()
            && (0xdc00..=0xdfff).contains(&low)
        {
            value = utf16toucs(value, low);
            input.next();
        } else if combine_surrogates && (0xd800..=0xdfff).contains(&value) {
            eprintln!("unpaired surrogate Unicode point {value:x}");
        }
        let tail = output.as_deref_mut().map(|out| &mut out[written..]);
        let used = encode_wide_scalar(tail, value, capacity - written);
        if used == 0 {
            break;
        }
        written += used;
    }
    if let Some(out) = output {
        out[written].write(0);
    }
    written + 1
}

/// Shared raw adapter for `wcstoutf8` and `wcs4toutf8`.
///
/// # Safety
/// `wc` must be a readable NUL-terminated `u32` string. If `s` is non-null it
/// must be writable for `n` bytes (possibly uninitialized) and must not
/// overlap `wc`.
unsafe fn wide_to_utf8(s: *mut c_char, mut wc: *const u32, n: usize, combine: bool) -> usize {
    if n == 0 {
        return 0;
    }
    // A lazy iterator keeps capacity-limited calls from scanning the full
    // wide input; peeking after a checked high surrogate may read the
    // terminator, which is in bounds.
    let input = std::iter::from_fn(move || {
        // SAFETY: each successful step advances to the next unit within the
        // NUL-terminated input, and a zero is never advanced past.
        unsafe {
            let value = wc.read();
            if value == 0 {
                None
            } else {
                wc = wc.add(1);
                Some(value)
            }
        }
    });
    // SAFETY: `s`, when non-null, is writable for n bytes and disjoint.
    let output = unsafe {
        (!s.is_null()).then(|| slice::from_raw_parts_mut(s.cast::<MaybeUninit<u8>>(), n))
    };
    encode_wide_string(input, output, n, combine)
}

/// Convert a wide-character string to UTF-8.
///
/// `s` can be a buffer of size `n` or null. If `n` is 0 or `s` is null,
/// nothing is written. Returns the number of chars including the terminating
/// null. If the buffer is not big enough, the result is truncated but
/// still null-terminated.
///
/// Port of R's `wcstoutf8` from util.c.
///
/// # Safety
/// `wc` must be a readable NUL-terminated `u32` string. If `s` is non-null it
/// must be writable for `n` bytes (possibly uninitialized) and must not
/// overlap `wc`.
pub unsafe fn wcstoutf8(s: *mut c_char, wc: *const u32, n: usize) -> usize {
    // SAFETY: forwarded contract; `wc` is NUL-terminated and `s` disjoint.
    unsafe { wide_to_utf8(s, wc, n, true) }
}

/// Convert a UCS-4 (R_wchar_t) string to UTF-8.
///
/// Port of R's `wcs4toutf8` from util.c.
///
/// # Safety
/// `wc` must be a readable NUL-terminated `u32` string. If `s` is non-null it
/// must be writable for `n` bytes (possibly uninitialized) and must not
/// overlap `wc`.
pub unsafe fn wcs4toutf8(s: *mut c_char, wc: *const R_wchar_t, n: usize) -> usize {
    // SAFETY: forwarded contract; `wc` is NUL-terminated and `s` disjoint.
    unsafe { wide_to_utf8(s, wc, n, false) }
}

// ---------------------------------------------------------------------------
// StringTrue / StringFalse: check if a string is a recognized true/false name
// ---------------------------------------------------------------------------

/// Table of recognized "true" names.
static TRUENAMES: [&str; 5] = ["T", "True", "TRUE", "true", ""];

/// Table of recognized "false" names.
static FALSENAMES: [&str; 5] = ["F", "False", "FALSE", "false", ""];

/// Check if a string matches one of R's recognized "true" names.
///
/// Port of R's `StringTrue` from util.c.
pub unsafe fn StringTrue(name: *const c_char) -> Rboolean {
    unsafe {
        if name.is_null() {
            return FALSE;
        }
        let s = CStr::from_ptr(name).to_str().unwrap_or("");
        for tname in TRUENAMES.iter() {
            if s == *tname {
                return TRUE;
            }
        }
        FALSE
    }
}

/// Check if a string matches one of R's recognized "false" names.
///
/// Port of R's `StringFalse` from util.c.
pub unsafe fn StringFalse(name: *const c_char) -> Rboolean {
    unsafe {
        if name.is_null() {
            return FALSE;
        }
        let s = CStr::from_ptr(name).to_str().unwrap_or("");
        for fname in FALSENAMES.iter() {
            if s == *fname {
                return TRUE;
            }
        }
        FALSE
    }
}

// ---------------------------------------------------------------------------
// Adobe Symbol encoding tables and conversion
// ---------------------------------------------------------------------------

/// Conversion table from Adobe Symbol to Unicode (with PUA).
/// Index into this table using (byte - 32).
static S2U: [u32; 224] = [
    0x0020_u32, 0x0021, 0x2200, 0x0023, 0x2203, 0x0025, 0x0026, 0x220D, 0x0028, 0x0029, 0x2217,
    0x002B, 0x002C, 0x2212, 0x002E, 0x002F, 0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036,
    0x0037, 0x0038, 0x0039, 0x003A, 0x003B, 0x003C, 0x003D, 0x003E, 0x003F, 0x2245, 0x0391, 0x0392,
    0x03A7, 0x0394, 0x0395, 0x03A6, 0x0393, 0x0397, 0x0399, 0x03D1, 0x039A, 0x039B, 0x039C, 0x039D,
    0x039F, 0x03A0, 0x0398, 0x03A1, 0x03A3, 0x03A4, 0x03A5, 0x03C2, 0x03A9, 0x039E, 0x03A8, 0x0396,
    0x005B, 0x2234, 0x005D, 0x22A5, 0x005F, 0xF8E5, 0x03B1, 0x03B2, 0x03C7, 0x03B4, 0x03B5, 0x03C6,
    0x03B3, 0x03B7, 0x03B9, 0x03D5, 0x03BA, 0x03BB, 0x03BC, 0x03BD, 0x03BF, 0x03C0, 0x03B8, 0x03C1,
    0x03C3, 0x03C4, 0x03C5, 0x03D6, 0x03C9, 0x03BE, 0x03C8, 0x03B6, 0x007B, 0x007C, 0x007D, 0x223C,
    0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020,
    0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020,
    0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x20AC, 0x03D2, 0x2032,
    0x2264, 0x2044, 0x221E, 0x0192, 0x2663, 0x2666, 0x2665, 0x2660, 0x2194, 0x2190, 0x2191, 0x2192,
    0x2193, 0x00B0, 0x00B1, 0x2033, 0x2265, 0x00D7, 0x221D, 0x2202, 0x2022, 0x00F7, 0x2260, 0x2261,
    0x2248, 0x2026, 0xF8E6, 0xF8E7, 0x21B5, 0x2135, 0x2111, 0x211C, 0x2118, 0x2297, 0x2295, 0x2205,
    0x2229, 0x222A, 0x2283, 0x2287, 0x2284, 0x2282, 0x2286, 0x2208, 0x2209, 0x2220, 0x2207, 0xF6DA,
    0xF6D9, 0xF6DB, 0x220F, 0x221A, 0x22C5, 0x00AC, 0x2227, 0x2228, 0x21D4, 0x21D0, 0x21D1, 0x21D2,
    0x21D3, 0x25CA, 0x2329, 0xF8E8, 0xF8E9, 0xF8EA, 0x2211, 0xF8EB, 0xF8EC, 0xF8ED, 0xF8EE, 0xF8EF,
    0xF8F0, 0xF8F1, 0xF8F2, 0xF8F3, 0xF8F4, 0x0020, 0x232A, 0x222B, 0x2320, 0xF8F5, 0x2321, 0xF8F6,
    0xF8F7, 0xF8F8, 0xF8F9, 0xF8FA, 0xF8FB, 0xF8FC, 0xF8FD, 0xF8FE, 0x0020,
];

/// Conversion table from Adobe Symbol to Unicode (without PUA).
static S2UNICODE: [u32; 224] = [
    0x0020_u32, 0x0021, 0x2200, 0x0023, 0x2203, 0x0025, 0x0026, 0x220D, 0x0028, 0x0029, 0x2217,
    0x002B, 0x002C, 0x2212, 0x002E, 0x002F, 0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036,
    0x0037, 0x0038, 0x0039, 0x003A, 0x003B, 0x003C, 0x003D, 0x003E, 0x003F, 0x2245, 0x0391, 0x0392,
    0x03A7, 0x0394, 0x0395, 0x03A6, 0x0393, 0x0397, 0x0399, 0x03D1, 0x039A, 0x039B, 0x039C, 0x039D,
    0x039F, 0x03A0, 0x0398, 0x03A1, 0x03A3, 0x03A4, 0x03A5, 0x03C2, 0x03A9, 0x039E, 0x03A8, 0x0396,
    0x005B, 0x2234, 0x005D, 0x22A5, 0x005F, 0x23AF, 0x03B1, 0x03B2, 0x03C7, 0x03B4, 0x03B5, 0x03C6,
    0x03B3, 0x03B7, 0x03B9, 0x03D5, 0x03BA, 0x03BB, 0x03BC, 0x03BD, 0x03BF, 0x03C0, 0x03B8, 0x03C1,
    0x03C3, 0x03C4, 0x03C5, 0x03D6, 0x03C9, 0x03BE, 0x03C8, 0x03B6, 0x007B, 0x007C, 0x007D, 0x223C,
    0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020,
    0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020,
    0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x0020, 0x20AC, 0x03D2, 0x2032,
    0x2264, 0x2044, 0x221E, 0x0192, 0x2663, 0x2666, 0x2665, 0x2660, 0x2194, 0x2190, 0x2191, 0x2192,
    0x2193, 0x00B0, 0x00B1, 0x2033, 0x2265, 0x00D7, 0x221D, 0x2202, 0x2022, 0x00F7, 0x2260, 0x2261,
    0x2248, 0x2026, 0x23D0, 0x23AF, 0x21B5, 0x2135, 0x2111, 0x211C, 0x2118, 0x2297, 0x2295, 0x2205,
    0x2229, 0x222A, 0x2283, 0x2287, 0x2284, 0x2282, 0x2286, 0x2208, 0x2209, 0x2220, 0x2207, 0x00AE,
    0x00A9, 0x2122, 0x220F, 0x221A, 0x22C5, 0x00AC, 0x2227, 0x2228, 0x21D4, 0x21D0, 0x21D1, 0x21D2,
    0x21D3, 0x25CA, 0x2329, 0x00AE, 0x00A9, 0x2122, 0x2211, 0x239B, 0x239C, 0x239D, 0x23A1, 0x23A2,
    0x23A3, 0x23A7, 0x23A8, 0x23A9, 0x23AA, 0x0020, 0x232A, 0x222B, 0x2320, 0x23AE, 0x2321, 0x239E,
    0x239F, 0x23A0, 0x23A4, 0x23A5, 0x23A6, 0x23AB, 0x23AC, 0x23AD, 0x0020,
];

/// Convert a string in Adobe Symbol encoding to UTF-8.
///
/// `work` is the output buffer of size `nwork`.
/// `c0` is the input string (Adobe Symbol encoded, single-byte).
/// `usePUA` controls whether to use the Private Usage Area mapping.
///
/// Returns `work`.
///
/// Port of R's `Rf_AdobeSymbol2utf8` from util.c.
///
/// When both pointers are non-null, a nonempty output buffer is
/// NUL-terminated. Nothing is written if either pointer is null or `nwork`
/// is zero.
///
/// # Safety
/// For non-null pointers with `nwork > 0`, `c0` must be a readable
/// NUL-terminated string and `work` must be writable for `nwork` bytes
/// (possibly uninitialized) without overlapping `c0`.
pub unsafe fn Rf_AdobeSymbol2utf8(
    work: *mut c_char,
    c0: *const c_char,
    nwork: usize,
    usePUA: Rboolean,
) -> *mut c_char {
    if work.is_null() || c0.is_null() || nwork == 0 {
        return work;
    }
    // SAFETY: the non-null input and writable output regions are disjoint.
    let (input, output) = unsafe {
        (
            CStr::from_ptr(c0).to_bytes(),
            slice::from_raw_parts_mut(work.cast::<MaybeUninit<u8>>(), nwork),
        )
    };
    adobe_symbol_to_utf8(input, output, usePUA != 0);
    work
}

fn adobe_symbol_to_utf8(input: &[u8], output: &mut [MaybeUninit<u8>], use_pua: bool) {
    let Some(capacity) = output.len().checked_sub(1) else {
        return;
    };
    let table = if use_pua { &S2U } else { &S2UNICODE };
    let mut written = 0;
    for &byte in input {
        let codepoint = if byte < 32 {
            u32::from(b' ')
        } else {
            table[usize::from(byte - 32)]
        };
        let ch = char::from_u32(codepoint).expect("Adobe Symbol mapping is a Unicode scalar");
        let mut encoded = [0; 4];
        let bytes = ch.encode_utf8(&mut encoded).as_bytes();
        if bytes.len() > capacity - written {
            break;
        }
        for (slot, &byte) in output[written..written + bytes.len()].iter_mut().zip(bytes) {
            slot.write(byte);
        }
        written += bytes.len();
        // Match GNU R's early stop after a complete character.
        if output.len() - written < 6 {
            break;
        }
    }
    output[written].write(0);
}

/// Convert an Adobe Symbol byte to UCS-2 codepoint.
///
/// Port of R's `Rf_AdobeSymbol2ucs2` from util.c.
pub fn Rf_AdobeSymbol2ucs2(n: c_int) -> c_int {
    if n >= 32 && n < 256 {
        S2U[(n - 32) as usize] as c_int
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// R_strtod5 / R_strtod4 / R_strtod / R_atof: string-to-double conversion
// ---------------------------------------------------------------------------

/// R's custom string-to-double conversion with GNU prefix/accuracy semantics.
///
/// # Safety
/// `str` must point to a valid NUL-terminated byte string. A non-null `endptr`
/// must be writable. The bounded parser does not invoke providers or native
/// conversion routines. Accuracy warnings may call back into the runtime.
pub unsafe fn R_strtod5(
    str: *const c_char,
    endptr: *mut *mut c_char,
    dec: c_char,
    na: Rboolean,
    exact: c_int,
) -> c_double {
    let (parsed, warning) = {
        // SAFETY: the translated C boundary requires a valid terminated string.
        // Complete every source read before a warning can reenter or detach it.
        let input = unsafe { CStr::from_ptr(str) }.to_bytes();
        let parsed = super::number_parse::parse_number(input, dec as u8, na != 0, exact);
        let warning = parsed.accuracy_loss.then(|| {
            std::ffi::CString::new(format!(
                "accuracy loss in conversion from \"{}\" to numeric",
                String::from_utf8_lossy(input)
            ))
            .expect("a terminated source contains no embedded NUL")
        });
        (parsed, warning)
    };
    // The original target's floating return path yields positive quiet NA or
    // its preferred positive NaN, including rejected negative tokens. Encode
    // that observed ABI result directly: Rust NaN arithmetic does not promise
    // a particular payload/sign, and must not determine compatibility here.
    let value = if parsed.value.is_nan() {
        let bits = if crate::sexp::ffi::R_IsNA(parsed.value) {
            R_NA_REAL.to_bits() | 0x0008_0000_0000_0000
        } else {
            0x7ff8_0000_0000_0000
        };
        f64::from_bits(bits)
    } else {
        parsed.value
    };
    if !endptr.is_null() {
        // SAFETY: consumed is bounded by the source bytes, and the destination
        // is writable by contract. Publish before any warning callback runs.
        unsafe { *endptr = str.add(parsed.consumed) as *mut c_char };
    }
    if let Some(message) = warning {
        // The message has independent storage throughout signaling. Neither
        // the source bytes nor their end-pointer destination are read again.
        crate::mainutils::errors::Rf_warning1(&message);
    }
    value
}

/// R's string-to-double with custom decimal point.
///
/// Port of R's `R_strtod4` from util.c.
pub unsafe fn R_strtod4(
    str: *const c_char,
    endptr: *mut *mut c_char,
    dec: c_char,
    na: Rboolean,
) -> c_double {
    unsafe { R_strtod5(str, endptr, dec, na, 0) }
}

/// R's string-to-double conversion.
///
/// Port of R's `R_strtod` from util.c.
pub unsafe fn R_strtod(str: *const c_char, endptr: *mut *mut c_char) -> c_double {
    unsafe { R_strtod5(str, endptr, b'.' as c_char, 0, 0) }
}

/// R's atof equivalent.
///
/// Port of R's `R_atof` from util.c.
pub unsafe fn R_atof(str: *const c_char) -> c_double {
    unsafe { R_strtod5(str, ptr::null_mut(), b'.' as c_char, 0, 0) }
}

// ---------------------------------------------------------------------------
// Rstrdup: malloc-based string duplication
// ---------------------------------------------------------------------------

/// Allocate a NUL-terminated copy using Rust's global allocator, or return
/// null if allocation fails.
///
/// Port of R's `Rstrdup` from util.c.
///
/// # Safety
/// `s` must be a readable NUL-terminated string. The result must be released
/// with the same allocator and the original size.
pub unsafe fn Rstrdup(s: *const c_char) -> *mut c_char {
    unsafe {
        let bytes = CStr::from_ptr(s).to_bytes_with_nul();
        let layout =
            std::alloc::Layout::array::<u8>(bytes.len()).expect("C string size is bounded");
        let copy = std::alloc::alloc(layout);
        if copy.is_null() {
            eprintln!("allocation error in Rstrdup");
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), copy, bytes.len());
        copy.cast()
    }
}

// ---------------------------------------------------------------------------
// Shell sort with index (isort_with_index)
// ---------------------------------------------------------------------------

/// Shell sort with parallel index array.
///
/// Port of R's static `isort_with_index` from util.c.
///
/// # Safety
/// For `n >= 2` both pointers describe separate initialized mutable buffers
/// of `n` elements.
pub unsafe fn isort_with_index(x: *mut c_int, indx: *mut c_int, n: c_int) {
    let Ok(n) = usize::try_from(n) else {
        return;
    };
    if n < 2 {
        return;
    }
    // SAFETY: for `n >= 2` the caller guarantees separate initialized mutable
    // buffers of `n` elements.
    let (x, indices) = unsafe {
        (
            std::slice::from_raw_parts_mut(x, n),
            std::slice::from_raw_parts_mut(indx, n),
        )
    };
    isort_with_index_slice(x, indices);
}

/// Shell sort two aligned buffers with Knuth's gap sequence.
fn isort_with_index_slice(x: &mut [c_int], indices: &mut [c_int]) {
    assert_eq!(x.len(), indices.len());
    let n = x.len();
    // Compute initial gap (Knuth's sequence)
    let mut gap = 1;
    while gap <= n / 9 {
        gap = 3 * gap + 1;
    }
    while gap > 0 {
        for i in gap..n {
            let value = x[i];
            let index = indices[i];
            let mut j = i;
            while j >= gap && x[j - gap] > value {
                x[j] = x[j - gap];
                indices[j] = indices[j - gap];
                j -= gap;
            }
            x[j] = value;
            indices[j] = index;
        }
        gap /= 3;
    }
}

// ---------------------------------------------------------------------------
// bincode: binary coding algorithm
// ---------------------------------------------------------------------------

/// Binary coding: assign each value in `x` to a bin defined by `breaks`.
///
/// Raw-buffer adapter for the binning algorithm from R's `bincode` in util.c.
///
/// # Safety
/// `x` describes `n` initialized doubles and `code` a writable out-region of
/// `n` ints that does not overlap the inputs; either may be null when
/// `n == 0`, and `code`'s cells may be uninitialized. When `nb >= 2`,
/// `breaks` describes `nb` initialized doubles.
pub unsafe fn bincode_impl(
    x: *const c_double,
    n: usize,
    breaks: *const c_double,
    nb: c_int,
    code: *mut c_int,
    right: c_int,
    include_border: c_int,
) {
    if nb < 2 {
        r_error("invalid 'breaks' argument");
    }
    // SAFETY: the caller guarantees `nb` initialized doubles.
    let breaks = unsafe { std::slice::from_raw_parts(breaks, nb as usize) };
    let x = if n == 0 {
        &[]
    } else {
        // SAFETY: the caller guarantees `n` initialized doubles.
        unsafe { std::slice::from_raw_parts(x, n) }
    };
    let codes = bincode_slice(x, breaks, right != 0, include_border != 0);
    if !codes.is_empty() {
        // SAFETY: the caller provides that many writable cells; `codes` is a
        // separate allocation.
        unsafe { ptr::copy_nonoverlapping(codes.as_ptr(), code, codes.len()) };
    }
}

/// Binary coding over slices: assign each value in `x` to a bin defined
/// by sorted `breaks`; NaN, out-of-range, and the open boundary when
/// `include_border` is false produce NA.
pub(crate) fn bincode_slice(
    x: &[f64],
    breaks: &[f64],
    right: bool,
    include_border: bool,
) -> Vec<c_int> {
    if breaks.len() < 2 {
        r_error("invalid 'breaks' argument");
    }
    if breaks.windows(2).any(|pair| pair[0] > pair[1]) {
        r_error("'breaks' is not sorted");
    }
    let last = breaks.len() - 1;
    x.iter()
        .map(|&value| {
            if value.is_nan()
                || value < breaks[0]
                || breaks[last] < value
                || (!include_border && value == breaks[if right { 0 } else { last }])
            {
                return NA_INTEGER;
            }
            let mut lo = 0;
            let mut hi = last;
            while hi - lo >= 2 {
                let mid = lo + (hi - lo) / 2;
                if value > breaks[mid] || (!right && value == breaks[mid]) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            c_int::try_from(lo + 1)
                .unwrap_or_else(|_| r_error("long vector 'breaks' is not supported"))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// SEXP-dependent API helpers from util.c
// ---------------------------------------------------------------------------

/// Number of rows of a matrix-like object.
///
/// Equivalent of R's `nrows()` from util.c.
/// Uses the "dim" attribute: returns dims[0].
pub fn nrows(s: *const c_void) -> c_int {
    let x = s as SEXP;
    if x.is_null() || x == R_NilValue() {
        return -1;
    }
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        return LENGTH(x);
    }
    if LENGTH(dim) >= 1 {
        crate::mainutils::essentials::integer_elt_checked(dim, 0)
    } else {
        -1
    }
}

/// Number of columns of a matrix-like object.
///
/// Equivalent of R's `ncols()` from util.c.
/// Uses the "dim" attribute: returns dims[1] if ndim >= 2, else 1.
pub fn ncols(s: *const c_void) -> c_int {
    let x = s as SEXP;
    if x.is_null() || x == R_NilValue() {
        return -1;
    }
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        return 1;
    }
    if LENGTH(dim) >= 2 {
        crate::mainutils::essentials::integer_elt_checked(dim, 1)
    } else {
        1
    }
}

/// Get the CHARSXP data pointer for a scalar string or symbol.
///
/// Equivalent of R's `asChar()` from util.c.
/// For SYMSXP, returns PRINTNAME(s). For CHARSXP, returns its data.
/// For STRSXP of length 1, returns STRING_ELT(x, 0).
pub unsafe fn asChar(x: *const c_void) -> *const c_void {
    unsafe {
        let s = x as SEXP;
        if s.is_null() || s == R_NilValue() {
            return ptr::null();
        }
        let t = TYPEOF(s);
        if t == SEXPTYPE::SYMSXP {
            CHAR(PRINTNAME(s)) as *const c_void
        } else if t == SEXPTYPE::CHARSXP {
            CHAR(s) as *const c_void
        } else if t == SEXPTYPE::STRSXP {
            let elt = STRING_ELT(s, 0);
            if elt.is_null() || elt == R_NilValue() {
                ptr::null()
            } else {
                CHAR(elt) as *const c_void
            }
        } else {
            ptr::null()
        }
    }
}

/// Check if an object inherits from "ordered" class.
///
/// Equivalent of R's `isUnordered()` from util.c.
pub fn isUnordered(s: *const c_void) -> Rboolean {
    let x = s as SEXP;
    if x.is_null() || x == R_NilValue() {
        return FALSE;
    }
    let klass = getAttrib(x, R_ClassSymbol());
    if klass.is_null() || klass == R_NilValue() {
        return FALSE;
    }
    let n = LENGTH(klass);
    for i in 0..n {
        let elt = STRING_ELT(klass, i as R_xlen_t);
        if elt.is_null() || elt == R_NilValue() {
            continue;
        }
        let bytes = crate::mainutils::printutils::charsxp_bytes(elt);
        {
            if let Ok(s) = std::str::from_utf8(&bytes)
                && (s == "unordered" || s == "factor")
            {
                return TRUE;
            }
        }
    }
    FALSE
}

/// Check if an object inherits from "ordered" class.
///
/// Equivalent of R's `isOrdered()` from util.c.
pub fn isOrdered(s: *const c_void) -> Rboolean {
    let x = s as SEXP;
    if x.is_null() || x == R_NilValue() {
        return FALSE;
    }
    let klass = getAttrib(x, R_ClassSymbol());
    if klass.is_null() || klass == R_NilValue() {
        return FALSE;
    }
    let n = LENGTH(klass);
    for i in 0..n {
        let elt = STRING_ELT(klass, i as R_xlen_t);
        if elt.is_null() || elt == R_NilValue() {
            continue;
        }
        let bytes = crate::mainutils::printutils::charsxp_bytes(elt);
        {
            if let Ok(s) = std::str::from_utf8(&bytes)
                && s == "ordered"
            {
                return TRUE;
            }
        }
    }
    FALSE
}

/// Test if a SEXP is TRUE (handles NA, length != 1).
///
/// Equivalent of R's `R_isTRUE()` from util.c.
/// Returns TRUE only for a length-1 logical vector with value 1.
pub fn R_isTRUE(x: *const c_void) -> Rboolean {
    let s = x as SEXP;
    if s.is_null() || s == R_NilValue() {
        return FALSE;
    }
    if TYPEOF(s) != SEXPTYPE::LGLSXP {
        return FALSE;
    }
    if LENGTH(s) != 1 {
        return FALSE;
    }
    let v = crate::mainutils::essentials::logical_elt_checked(s, 0);
    if v == 1 { TRUE } else { FALSE }
}

/// Convert a type name string to SEXPTYPE integer value.
///
/// Equivalent of R's `str2type()` from util.c.
pub unsafe fn str2type(s: *const c_char) -> c_int {
    unsafe {
        if s.is_null() {
            return -1;
        }
        let cs = CStr::from_ptr(s);
        let Ok(name) = cs.to_str() else { return -1 };
        match name {
            "logical" => SEXPTYPE::LGLSXP.into(),
            "integer" => SEXPTYPE::INTSXP.into(),
            "double" => SEXPTYPE::REALSXP.into(),
            "complex" => SEXPTYPE::CPLXSXP.into(),
            "character" => SEXPTYPE::STRSXP.into(),
            "raw" => SEXPTYPE::RAWSXP.into(),
            "list" => SEXPTYPE::VECSXP.into(),
            "expression" => SEXPTYPE::EXPRSXP.into(),
            "closure" | "function" => SEXPTYPE::CLOSXP.into(),
            "environment" => SEXPTYPE::ENVSXP.into(),
            _ => -1,
        }
    }
}

/// Convert a SEXPTYPE integer to its character name.
///
/// Equivalent of R's `type2char()` from util.c.
pub fn type2char(t: c_int) -> *const c_char {
    match t {
        0 => b"NULL\0".as_ptr() as *const c_char,
        1 => b"symbol\0".as_ptr() as *const c_char,
        2 => b"pairlist\0".as_ptr() as *const c_char,
        3 => b"closure\0".as_ptr() as *const c_char,
        4 => b"environment\0".as_ptr() as *const c_char,
        5 => b"promise\0".as_ptr() as *const c_char,
        6 => b"language\0".as_ptr() as *const c_char,
        7 => b"special\0".as_ptr() as *const c_char,
        8 => b"builtin\0".as_ptr() as *const c_char,
        9 => b"char\0".as_ptr() as *const c_char,
        10 => b"logical\0".as_ptr() as *const c_char,
        13 => b"integer\0".as_ptr() as *const c_char,
        14 => b"double\0".as_ptr() as *const c_char,
        15 => b"complex\0".as_ptr() as *const c_char,
        16 => b"character\0".as_ptr() as *const c_char,
        17 => b"...\0".as_ptr() as *const c_char,
        18 => b"any\0".as_ptr() as *const c_char,
        19 => b"list\0".as_ptr() as *const c_char,
        20 => b"expression\0".as_ptr() as *const c_char,
        21 => b"bytecode\0".as_ptr() as *const c_char,
        22 => b"externalptr\0".as_ptr() as *const c_char,
        23 => b"weakref\0".as_ptr() as *const c_char,
        24 => b"raw\0".as_ptr() as *const c_char,
        25 => b"S4\0".as_ptr() as *const c_char,
        _ => b"unknown\0".as_ptr() as *const c_char,
    }
}

/// `isBlankString` depends on mbcslocale global.
/// This simplified version only does ASCII whitespace checking.
pub unsafe fn isBlankString(s: *const c_char) -> Rboolean {
    unsafe {
        if s.is_null() {
            return TRUE;
        }
        let mut p = s;
        while *p != 0 {
            if !libc_isspace(*p) {
                return FALSE;
            }
            p = p.add(1);
        }
        TRUE
    }
}

/// Check if a CHARSXP contains only whitespace.
///
/// Equivalent of R's `StringBlank()` from util.c.
pub fn StringBlank(x: *const c_void) -> Rboolean {
    let s = x as SEXP;
    if s.is_null() || s == R_NilValue() {
        return TRUE;
    }
    let bytes = crate::mainutils::printutils::charsxp_bytes(s);
    if bytes.iter().all(|&b| libc_isspace(b as c_char)) {
        TRUE
    } else {
        FALSE
    }
}

/// Check if a string is valid in the current multibyte encoding.
///
/// Simplified: checks UTF-8 validity. In full R this checks against the locale encoding.
pub unsafe fn mbcsValid(str: *const c_char) -> Rboolean {
    unsafe { utf8Valid(str) }
}

/// Check if a byte string is valid UTF-8.
///
/// Equivalent of R's `utf8Valid()` from util.c.
pub unsafe fn utf8Valid(str: *const c_char) -> Rboolean {
    unsafe {
        if str.is_null() {
            return TRUE;
        }
        let mut p = str;
        while *p != 0 {
            let b = *p as u8;
            let clen = utf8clen(b as c_char) as usize;
            if clen == 1 {
                // ASCII or continuation byte at start = invalid
                if b >= 0x80 {
                    return FALSE;
                }
                p = p.add(1);
            } else {
                // Multi-byte: check we have enough continuation bytes
                for _ in 1..clen {
                    p = p.add(1);
                    if *p == 0 {
                        return FALSE;
                    }
                    let cb = *p as u8;
                    if cb < 0x80 || cb >= 0xC0 {
                        return FALSE;
                    }
                }
                p = p.add(1);
            }
        }
        TRUE
    }
}

/// Create a CHARSXP with encoding matching a reference string.
///
/// Equivalent of R's `markKnown()` from util.c.
pub unsafe fn markKnown(s: *const c_char, r#ref: *const c_void) -> *const c_void {
    unsafe {
        if s.is_null() {
            return ptr::null();
        }
        crate::sexp::constructors::Rf_mkChar(s) as *const c_void
    }
}

/// Convert a multibyte string to UCS-2 (UTF-16) code units.
///
/// Intentionally limited: `enc == CE_UTF8` decodes 1- to 3-byte UTF-8 scalar
/// values, skipping malformed chunks by their declared width and skipping
/// 4-byte (astral) characters entirely; other encodings are treated as
/// single-byte Latin-1/native. No terminator is written; returns the number
/// of `u16` units copied.
fn mbcs_to_ucs2(mut input: &[u8], output: &mut [MaybeUninit<u16>], utf8: bool) -> usize {
    if !utf8 {
        let mut written = 0;
        for (&byte, out) in input.iter().zip(output.iter_mut()) {
            out.write(u16::from(byte));
            written += 1;
        }
        return written;
    }
    let mut written = 0;
    while written < output.len() && !input.is_empty() {
        let width = utf8clen(input[0] as c_char) as usize;
        if width > 4 {
            input = &input[1..];
            continue;
        }
        let Some(prefix) = input.get(..width) else {
            break;
        };
        if width <= 3
            && let Ok(text) = std::str::from_utf8(prefix)
            && let Some(ch) = text.chars().next()
        {
            output[written].write(ch as u16);
            written += 1;
        }
        input = &input[width..];
    }
    written
}

/// Convert a multibyte string to UCS-2 (UTF-16).
///
/// Intentionally limited UTF-8/Latin-1 path (see `mbcs_to_ucs2`).
/// GNU enc=0 is native; enc=1 is UTF-8.
///
/// # Safety
/// `in_` must be a readable NUL-terminated string. `out` must be writable for
/// `nout` `u16` values (possibly uninitialized) when `nout > 0` and must not
/// overlap `in_`.
pub unsafe fn mbcsToUcs2(in_: *const c_char, out: *mut u16, nout: c_int, enc: c_int) -> usize {
    if in_.is_null() || out.is_null() || nout <= 0 {
        return 0;
    }
    // SAFETY: `in_` is a readable NUL-terminated string; `out` is writable
    // for `nout` possibly-uninitialized u16 values and disjoint.
    let (input, output) = unsafe {
        (
            CStr::from_ptr(in_).to_bytes(),
            slice::from_raw_parts_mut(out.cast::<MaybeUninit<u16>>(), nout as usize),
        )
    };
    mbcs_to_ucs2(input, output, enc == crate::sexp::encoding::CE_UTF8)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strIsASCII_all_ascii() {
        unsafe {
            let s = b"Hello, World!\0";
            assert_eq!(Rf_strIsASCII(s.as_ptr() as *const c_char), TRUE);
        }
    }

    #[test]
    fn test_strIsASCII_non_ascii() {
        unsafe {
            let s = b"caf\xc3\xa9\0"; // "cafe" with UTF-8 e-acute
            assert_eq!(Rf_strIsASCII(s.as_ptr() as *const c_char), FALSE);
        }
    }

    #[test]
    fn test_strIsASCII_null() {
        unsafe {
            assert_eq!(Rf_strIsASCII(ptr::null()), TRUE);
        }
    }

    #[test]
    fn test_utf8clen() {
        {
            assert_eq!(utf8clen('A' as c_char), 1);
            assert_eq!(utf8clen(0xC2u8 as c_char), 2); // 2-byte leading byte
            assert_eq!(utf8clen(0xE0u8 as c_char), 3); // 3-byte leading byte
            assert_eq!(utf8clen(0xF0u8 as c_char), 4); // 4-byte leading byte
            assert_eq!(utf8clen(0x80u8 as c_char), 1); // continuation byte -> 1
        }
    }

    #[test]
    fn test_StringTrue() {
        unsafe {
            assert_eq!(StringTrue(b"TRUE\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringTrue(b"True\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringTrue(b"T\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringTrue(b"true\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringTrue(b"FALSE\0".as_ptr() as *const c_char), FALSE);
            assert_eq!(StringTrue(b"yes\0".as_ptr() as *const c_char), FALSE);
        }
    }

    #[test]
    fn test_StringFalse() {
        unsafe {
            assert_eq!(StringFalse(b"FALSE\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringFalse(b"False\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringFalse(b"F\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringFalse(b"false\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(StringFalse(b"TRUE\0".as_ptr() as *const c_char), FALSE);
        }
    }

    #[test]
    fn gnu_number_parser_raw_na_and_nan_bits_match_original_abi() {
        // Independently obtained via original exported R_strtod5 at pinned
        // bac583951b728e97b9786804d3b4081f0fe18df5, not via our parser.
        // input hex | allow NA | exact | original bytes | offset | isNA | warning
        const ORIGINAL: &str = r#"|0|0|a20700000000f87f|0|TRUE|
|0|1|a20700000000f87f|0|TRUE|
|0|NA|a20700000000f87f|0|TRUE|
|1|0|a20700000000f87f|0|TRUE|
|1|1|a20700000000f87f|0|TRUE|
|1|NA|a20700000000f87f|0|TRUE|
20|0|0|a20700000000f87f|0|TRUE|
20|0|1|a20700000000f87f|0|TRUE|
20|0|NA|a20700000000f87f|0|TRUE|
20|1|0|a20700000000f87f|0|TRUE|
20|1|1|a20700000000f87f|0|TRUE|
20|1|NA|a20700000000f87f|0|TRUE|
2e|0|0|a20700000000f87f|0|TRUE|
2e|0|1|a20700000000f87f|0|TRUE|
2e|0|NA|a20700000000f87f|0|TRUE|
2e|1|0|a20700000000f87f|0|TRUE|
2e|1|1|a20700000000f87f|0|TRUE|
2e|1|NA|a20700000000f87f|0|TRUE|
2b2e|0|0|a20700000000f87f|0|TRUE|
2b2e|0|1|a20700000000f87f|0|TRUE|
2b2e|0|NA|a20700000000f87f|0|TRUE|
2b2e|1|0|a20700000000f87f|0|TRUE|
2b2e|1|1|a20700000000f87f|0|TRUE|
2b2e|1|NA|a20700000000f87f|0|TRUE|
2d2e|0|0|a20700000000f87f|0|TRUE|
2d2e|0|1|a20700000000f87f|0|TRUE|
2d2e|0|NA|a20700000000f87f|0|TRUE|
2d2e|1|0|a20700000000f87f|0|TRUE|
2d2e|1|1|a20700000000f87f|0|TRUE|
2d2e|1|NA|a20700000000f87f|0|TRUE|
2b31652b|0|0|a20700000000f87f|0|TRUE|
2b31652b|0|1|a20700000000f87f|0|TRUE|
2b31652b|0|NA|a20700000000f87f|0|TRUE|
2b31652b|1|0|a20700000000f87f|0|TRUE|
2b31652b|1|1|a20700000000f87f|0|TRUE|
2b31652b|1|NA|a20700000000f87f|0|TRUE|
2d31652b|0|0|a20700000000f87f|0|TRUE|
2d31652b|0|1|a20700000000f87f|0|TRUE|
2d31652b|0|NA|a20700000000f87f|0|TRUE|
2d31652b|1|0|a20700000000f87f|0|TRUE|
2d31652b|1|1|a20700000000f87f|0|TRUE|
2d31652b|1|NA|a20700000000f87f|0|TRUE|
39303037313939323534373430393933|0|0|0000000000004043|16|FALSE|
39303037313939323534373430393933|0|1|a20700000000f87f|0|TRUE|
39303037313939323534373430393933|0|NA|0000000000004043|16|FALSE|accuracy loss in conversion from "9007199254740993" to numeric
39303037313939323534373430393933|1|0|0000000000004043|16|FALSE|
39303037313939323534373430393933|1|1|a20700000000f87f|0|TRUE|
39303037313939323534373430393933|1|NA|0000000000004043|16|FALSE|accuracy loss in conversion from "9007199254740993" to numeric
2b39303037313939323534373430393933|0|0|0000000000004043|17|FALSE|
2b39303037313939323534373430393933|0|1|a20700000000f87f|0|TRUE|
2b39303037313939323534373430393933|0|NA|0000000000004043|17|FALSE|accuracy loss in conversion from "+9007199254740993" to numeric
2b39303037313939323534373430393933|1|0|0000000000004043|17|FALSE|
2b39303037313939323534373430393933|1|1|a20700000000f87f|0|TRUE|
2b39303037313939323534373430393933|1|NA|0000000000004043|17|FALSE|accuracy loss in conversion from "+9007199254740993" to numeric
2d39303037313939323534373430393933|0|0|00000000000040c3|17|FALSE|
2d39303037313939323534373430393933|0|1|a20700000000f87f|0|TRUE|
2d39303037313939323534373430393933|0|NA|00000000000040c3|17|FALSE|accuracy loss in conversion from "-9007199254740993" to numeric
2d39303037313939323534373430393933|1|0|00000000000040c3|17|FALSE|
2d39303037313939323534373430393933|1|1|a20700000000f87f|0|TRUE|
2d39303037313939323534373430393933|1|NA|00000000000040c3|17|FALSE|accuracy loss in conversion from "-9007199254740993" to numeric
30783230303030303030303030303031|0|0|0000000000004043|16|FALSE|
30783230303030303030303030303031|0|1|a20700000000f87f|0|TRUE|
30783230303030303030303030303031|0|NA|0000000000004043|16|FALSE|accuracy loss in conversion from "0x20000000000001" to numeric
30783230303030303030303030303031|1|0|0000000000004043|16|FALSE|
30783230303030303030303030303031|1|1|a20700000000f87f|0|TRUE|
30783230303030303030303030303031|1|NA|0000000000004043|16|FALSE|accuracy loss in conversion from "0x20000000000001" to numeric
2d30783230303030303030303030303031|0|0|00000000000040c3|17|FALSE|
2d30783230303030303030303030303031|0|1|a20700000000f87f|0|TRUE|
2d30783230303030303030303030303031|0|NA|00000000000040c3|17|FALSE|accuracy loss in conversion from "-0x20000000000001" to numeric
2d30783230303030303030303030303031|1|0|00000000000040c3|17|FALSE|
2d30783230303030303030303030303031|1|1|a20700000000f87f|0|TRUE|
2d30783230303030303030303030303031|1|NA|00000000000040c3|17|FALSE|accuracy loss in conversion from "-0x20000000000001" to numeric
4e41|0|0|a20700000000f87f|0|TRUE|
4e41|0|1|a20700000000f87f|0|TRUE|
4e41|0|NA|a20700000000f87f|0|TRUE|
4e41|1|0|a20700000000f87f|2|TRUE|
4e41|1|1|a20700000000f87f|2|TRUE|
4e41|1|NA|a20700000000f87f|2|TRUE|
2b4e41|0|0|a20700000000f87f|0|TRUE|
2b4e41|0|1|a20700000000f87f|0|TRUE|
2b4e41|0|NA|a20700000000f87f|0|TRUE|
2b4e41|1|0|a20700000000f87f|0|TRUE|
2b4e41|1|1|a20700000000f87f|0|TRUE|
2b4e41|1|NA|a20700000000f87f|0|TRUE|
2d4e41|0|0|a20700000000f87f|0|TRUE|
2d4e41|0|1|a20700000000f87f|0|TRUE|
2d4e41|0|NA|a20700000000f87f|0|TRUE|
2d4e41|1|0|a20700000000f87f|0|TRUE|
2d4e41|1|1|a20700000000f87f|0|TRUE|
2d4e41|1|NA|a20700000000f87f|0|TRUE|
204e614e|0|0|000000000000f87f|4|FALSE|
204e614e|0|1|000000000000f87f|4|FALSE|
204e614e|0|NA|000000000000f87f|4|FALSE|
204e614e|1|0|000000000000f87f|4|FALSE|
204e614e|1|1|000000000000f87f|4|FALSE|
204e614e|1|NA|000000000000f87f|4|FALSE|
2d4e614e|0|0|000000000000f87f|4|FALSE|
2d4e614e|0|1|000000000000f87f|4|FALSE|
2d4e614e|0|NA|000000000000f87f|4|FALSE|
2d4e614e|1|0|000000000000f87f|4|FALSE|
2d4e614e|1|1|000000000000f87f|4|FALSE|
2d4e614e|1|NA|000000000000f87f|4|FALSE|"#;
        let decode = |text: &str| {
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            (*instance).error_state.nwarnings = 1;
            let mut cases = 0;
            for row in ORIGINAL.lines() {
                let columns = row.split('|').collect::<Vec<_>>();
                let input = std::ffi::CString::new(decode(columns[0])).unwrap();
                let exact = if columns[2] == "NA" {
                    NA_INTEGER
                } else {
                    columns[2].parse::<c_int>().unwrap()
                };
                let expected = u64::from_le_bytes(decode(columns[3]).try_into().unwrap());
                let mut end = ptr::null_mut();
                (*instance).error_state.collect_warnings = 0;
                let value = R_strtod5(
                    input.as_ptr(),
                    &mut end,
                    b'.' as c_char,
                    columns[1].parse::<c_int>().unwrap(),
                    exact,
                );
                assert_eq!(value.to_bits(), expected, "original ABI row {row}");
                assert_eq!(
                    end,
                    input.as_ptr().add(columns[4].parse::<usize>().unwrap()) as *mut c_char,
                    "original ABI row {row}"
                );
                assert_eq!(crate::sexp::ffi::R_IsNA(value), columns[5] == "TRUE");
                if columns[6].is_empty() {
                    assert_eq!((*instance).error_state.collect_warnings, 0);
                } else {
                    assert_eq!((*instance).error_state.collect_warnings, 1);
                    let warnings = (*instance).error_state.warnings.owned().unwrap();
                    let names = warnings.try_attrib().unwrap().try_car().unwrap();
                    assert_eq!(
                        names.try_string_elt(0).unwrap().try_as_string().unwrap(),
                        columns[6]
                    );
                }
                cases += 1;
            }
            assert_eq!(cases, 102);
        });
    }

    #[test]
    fn gnu_number_parser_exact_rejects_decimal_and_hex_mantissas() {
        for text in ["9007199254740993", "0x20000000000001"] {
            let input = std::ffi::CString::new(text).unwrap();
            let mut end = std::ptr::null_mut();
            let value = unsafe { R_strtod5(input.as_ptr(), &mut end, b'.' as c_char, FALSE, TRUE) };
            assert!(
                crate::sexp::ffi::R_IsNA(value),
                "original exact=TRUE must reject {text}"
            );
            assert_eq!(
                end.cast_const(),
                input.as_ptr(),
                "original rejection consumes no bytes"
            );
        }
    }

    #[test]
    fn gnu_number_parser_wrappers_preserve_prefix_defaults_and_bounded_offsets() {
        for (input, value, offset) in [
            (c"  -12.5rest", -12.5, 7),
            (c"0x1.2.3tail", 18.1875, 7),
            (c"0x.", 0.0, 3),
            (c"0x1p-1074", 0.0, 9),
        ] {
            let mut end = ptr::null_mut();
            let actual = unsafe { R_strtod(input.as_ptr(), &mut end) };
            assert_eq!(actual, value);
            assert_eq!(end, unsafe { input.as_ptr().add(offset) } as *mut c_char);
            assert_eq!(unsafe { R_atof(input.as_ptr()) }, value);
        }
        let input = c" -1,25tail";
        let mut end = ptr::null_mut();
        assert_eq!(
            unsafe { R_strtod4(input.as_ptr(), &mut end, b',' as c_char, FALSE) },
            -1.25
        );
        assert_eq!(end, unsafe { input.as_ptr().add(6) } as *mut c_char);
    }

    #[test]
    fn gnu_number_parser_warning_owns_source_before_collecting_callback() {
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        let session = crate::sexp::RSession::new_for_gc_tests();
        let source = Rc::new(RefCell::new(Some(
            std::ffi::CString::new("9007199254740993").unwrap(),
        )));
        let pointer = source.borrow().as_ref().unwrap().as_ptr();
        let expected_end = unsafe { pointer.add(16) } as *mut c_char;
        let detached = source.clone();
        let collected = Rc::new(Cell::new(false));
        let observed = collected.clone();
        session.with_active_in(|instance| unsafe {
            (*instance).error_state.nwarnings = 1;
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                (*instance).memory_state.gc_force_gap = 0;
                // Remove the sole source allocation while the warning is
                // signaling. Any reread of its bytes after this point is invalid.
                drop(detached.borrow_mut().take());
                crate::sexp::gengc::full_gc_in(instance);
                observed.set(true);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let mut end = ptr::null_mut();
            let value = R_strtod5(pointer, &mut end, b'.' as c_char, FALSE, NA_INTEGER);
            assert!(
                collected.get(),
                "the actual warning must allocate and collect"
            );
            assert!(source.borrow().is_none());
            assert_eq!(value, 9_007_199_254_740_992.0);
            assert_eq!(end, expected_end);
            let warnings = (*instance).error_state.warnings.owned().unwrap();
            let names = warnings.try_attrib().unwrap().try_car().unwrap();
            assert_eq!(
                names.try_string_elt(0).unwrap().try_as_string().unwrap(),
                "accuracy loss in conversion from \"9007199254740993\" to numeric"
            );
            assert_eq!((*instance).error_state.collect_warnings, 1);
        });
    }

    #[test]
    fn test_R_strtod_basic() {
        unsafe {
            let mut endptr: *mut c_char = ptr::null_mut();
            let s = b"3.14\0";
            let val = R_strtod(s.as_ptr() as *const c_char, &mut endptr);
            assert!((val - 3.14).abs() < 1e-10);
        }
    }

    #[test]
    fn test_R_strtod_negative() {
        unsafe {
            let mut endptr: *mut c_char = ptr::null_mut();
            let s = b"-2.5e3\0";
            let val = R_strtod(s.as_ptr() as *const c_char, &mut endptr);
            assert!((val - (-2500.0)).abs() < 1e-5);
        }
    }

    #[test]
    fn test_R_atof() {
        unsafe {
            let s = b"42.5\0";
            let val = R_atof(s.as_ptr() as *const c_char);
            assert!((val - 42.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_R_strtod_nan_inf() {
        unsafe {
            let mut endptr: *mut c_char = ptr::null_mut();
            let s_nan = b"NaN\0";
            let val_nan = R_strtod(s_nan.as_ptr() as *const c_char, &mut endptr);
            assert!(val_nan.is_nan());

            let s_inf = b"Inf\0";
            let val_inf = R_strtod(s_inf.as_ptr() as *const c_char, &mut endptr);
            assert!(val_inf.is_infinite() && val_inf > 0.0);
        }
    }

    #[test]
    fn test_R_strtod5_with_na() {
        unsafe {
            let mut endptr: *mut c_char = ptr::null_mut();
            let s = b"NA\0";
            let val = R_strtod5(
                s.as_ptr() as *const c_char,
                &mut endptr,
                b'.' as c_char,
                TRUE,
                0,
            );
            // The original exported function returns quiet NA, not the
            // signaling in-memory NA constant (pinned direct ABI controls).
            assert_eq!(val.to_bits(), 0x7ff8_0000_0000_07a2);
            assert_eq!(endptr, s.as_ptr().add(2) as *mut c_char);
        }
    }

    #[test]
    fn test_isort_with_index() {
        unsafe {
            let mut x: Vec<c_int> = vec![5, 3, 1, 4, 2];
            let mut indx: Vec<c_int> = vec![1, 2, 3, 4, 5];
            isort_with_index(x.as_mut_ptr(), indx.as_mut_ptr(), 5);
            assert_eq!(x, vec![1, 2, 3, 4, 5]);
            assert_eq!(indx, vec![3, 5, 2, 4, 1]);
        }
    }

    #[test]
    fn test_AdobeSymbol2ucs2() {
        {
            assert_eq!(Rf_AdobeSymbol2ucs2(32), 0x0020); // space
            assert_eq!(Rf_AdobeSymbol2ucs2(33), 0x0021); // !
            assert_eq!(Rf_AdobeSymbol2ucs2(34), 0x2200); // forall
            assert_eq!(Rf_AdobeSymbol2ucs2(0), 0); // out of range
            assert_eq!(Rf_AdobeSymbol2ucs2(256), 0); // out of range
        }
    }

    #[test]
    fn test_isBlankString() {
        unsafe {
            assert_eq!(isBlankString(b"  \t\n\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(isBlankString(b"  a  \0".as_ptr() as *const c_char), FALSE);
            assert_eq!(isBlankString(b"\0".as_ptr() as *const c_char), TRUE);
        }
    }

    #[test]
    fn test_bincode_impl() {
        unsafe {
            let x = [0.5, 1.5, 2.5, 3.5, 4.5];
            let breaks = [1.0, 2.0, 3.0, 4.0, 5.0];
            let mut code = [std::mem::MaybeUninit::<i32>::uninit(); 5];
            bincode_impl(
                x.as_ptr(),
                5,
                breaks.as_ptr(),
                5,
                code.as_mut_ptr().cast(),
                1,
                0,
            );
            let code = code.map(|value| value.assume_init());
            assert_eq!(code[0], NA_INTEGER); // 0.5 < breaks[0]
            assert_eq!(code[1], 1); // 1.5 in [1,2)
            assert_eq!(code[2], 2); // 2.5 in [2,3)
            assert_eq!(code[3], 3); // 3.5 in [3,4)
            assert_eq!(code[4], 4); // 4.5 in [4,5)
        }
    }

    #[test]
    fn test_bincode_empty_input_accepts_null_buffers() {
        unsafe {
            bincode_impl(
                ptr::null(),
                0,
                [0.0, 1.0].as_ptr(),
                2,
                ptr::null_mut(),
                1,
                0,
            );
        }
    }

    #[test]
    fn test_bincode_rejects_too_few_breaks() {
        let err = std::panic::catch_unwind(|| unsafe {
            let x = [1.0];
            let breaks = [1.0];
            let mut code = [0i32; 1];
            bincode_impl(x.as_ptr(), 1, breaks.as_ptr(), 1, code.as_mut_ptr(), 1, 0);
        });
        assert!(err.is_err());
    }

    #[test]
    fn wide_conversion_utf8toucs_scalars_and_statuses() {
        let mut slot = MaybeUninit::<u32>::uninit();
        // ASCII scalar.
        assert_eq!(unsafe { utf8toucs(slot.as_mut_ptr(), c"a".as_ptr()) }, 1);
        assert_eq!(unsafe { slot.assume_init() }, u32::from(b'a'));
        // Supplementary scalar U+1F600.
        let mut slot = MaybeUninit::<u32>::uninit();
        assert_eq!(
            unsafe { utf8toucs(slot.as_mut_ptr(), c"\xF0\x9F\x98\x80".as_ptr()) },
            4
        );
        assert_eq!(unsafe { slot.assume_init() }, 0x1f600);
        // Empty input yields zero and writes a NUL unit.
        let mut slot = MaybeUninit::<u32>::uninit();
        assert_eq!(unsafe { utf8toucs(slot.as_mut_ptr(), c"".as_ptr()) }, 0);
        assert_eq!(unsafe { slot.assume_init() }, 0);
        // Legacy 5- and 6-byte widths keep the GNU masks/shifts.
        let mut slot = MaybeUninit::<u32>::uninit();
        assert_eq!(
            unsafe { utf8toucs(slot.as_mut_ptr(), c"\xF8\x88\x81\x80\x80".as_ptr()) },
            5
        );
        assert_eq!(unsafe { slot.assume_init() }, 0x0800_9000);
        let mut slot = MaybeUninit::<u32>::uninit();
        assert_eq!(
            unsafe { utf8toucs(slot.as_mut_ptr(), c"\xFC\x84\x80\x80\x80\x80".as_ptr(),) },
            6
        );
        assert_eq!(unsafe { slot.assume_init() }, 0x0400_0000);
    }

    #[test]
    fn wide_conversion_utf8toucs_errors_do_not_write() {
        // Truncated multibyte prefix: -2 sentinel, output untouched.
        let mut slot = MaybeUninit::<u32>::new(0xdead_beef);
        assert_eq!(
            unsafe { utf8toucs(slot.as_mut_ptr(), b"\xE2\x00".as_ptr().cast()) },
            usize::MAX - 1
        );
        assert_eq!(unsafe { slot.assume_init() }, 0xdead_beef);
        // Bad continuation byte: -1 sentinel, output untouched.
        let mut slot = MaybeUninit::<u32>::new(0xdead_beef);
        assert_eq!(
            unsafe { utf8toucs(slot.as_mut_ptr(), b"\xC2A\x00".as_ptr().cast()) },
            usize::MAX
        );
        assert_eq!(unsafe { slot.assume_init() }, 0xdead_beef);
        // Three-byte encoding of a surrogate: -1 sentinel, output untouched.
        let mut slot = MaybeUninit::<u32>::new(0xdead_beef);
        assert_eq!(
            unsafe { utf8toucs(slot.as_mut_ptr(), b"\xED\xA0\x80\x00".as_ptr().cast()) },
            usize::MAX
        );
        assert_eq!(unsafe { slot.assume_init() }, 0xdead_beef);
    }

    #[test]
    fn wide_conversion_utf8towcs_fills_and_terminates() {
        let mut out = [MaybeUninit::<u32>::uninit(); 3];
        let written =
            unsafe { utf8towcs(out.as_mut_ptr().cast::<u32>(), c"a".as_ptr(), out.len()) };
        assert_eq!(written, 1);
        assert_eq!(unsafe { out[0].assume_init() }, u32::from(b'a'));
        assert_eq!(unsafe { out[1].assume_init() }, 0);
    }

    #[test]
    fn wide_conversion_utf8towcs_capacity_edges() {
        // Count-only: null output counts the whole string.
        assert_eq!(unsafe { utf8towcs(ptr::null_mut(), c"ab".as_ptr(), 0) }, 2);
        // Non-null output with n = 0 returns early and writes nothing.
        let mut out = [MaybeUninit::<u32>::new(0xdead_beef); 2];
        assert_eq!(
            unsafe { utf8towcs(out.as_mut_ptr().cast::<u32>(), c"ab".as_ptr(), 0) },
            0
        );
        assert_eq!(unsafe { out[0].assume_init() }, 0xdead_beef);
        // Short capacity leaves the next slot untouched (no terminator).
        let mut out = [MaybeUninit::<u32>::new(0xdead_beef); 2];
        let written = unsafe { utf8towcs(out.as_mut_ptr().cast::<u32>(), c"ab".as_ptr(), 2) };
        assert_eq!(written, 2);
        assert_eq!(unsafe { out[0].assume_init() }, u32::from(b'a'));
        assert_eq!(unsafe { out[1].assume_init() }, u32::from(b'b'));
        // Exact capacity ends the scan before a malformed suffix is read.
        let mut out = [MaybeUninit::<u32>::new(0xdead_beef); 2];
        let written = unsafe {
            utf8towcs(
                out.as_mut_ptr().cast::<u32>(),
                b"ab\xFF\x00".as_ptr().cast(),
                2,
            )
        };
        assert_eq!(written, 2);
        assert_eq!(unsafe { out[0].assume_init() }, u32::from(b'a'));
        assert_eq!(unsafe { out[1].assume_init() }, u32::from(b'b'));
    }

    #[test]
    fn wide_conversion_utf8towcs_invalid_returns_zero() {
        let mut out = [MaybeUninit::<u32>::uninit(); 4];
        assert_eq!(
            unsafe {
                utf8towcs(
                    out.as_mut_ptr().cast::<u32>(),
                    b"a\x80\x00".as_ptr().cast(),
                    4,
                )
            },
            0
        );
        let mut out = [MaybeUninit::<u32>::uninit(); 4];
        assert_eq!(
            unsafe {
                utf8towcs4(
                    out.as_mut_ptr().cast::<u32>(),
                    b"a\x80\x00".as_ptr().cast(),
                    4,
                )
            },
            0
        );
        // utf8towcs4 keeps supplementary scalars as single UCS-4 units.
        let mut out = [MaybeUninit::<u32>::uninit(); 4];
        let written = unsafe {
            utf8towcs4(
                out.as_mut_ptr().cast::<u32>(),
                c"\xF0\x9F\x98\x80".as_ptr(),
                4,
            )
        };
        assert_eq!(written, 1);
        assert_eq!(unsafe { out[0].assume_init() }, 0x1f600);
    }

    #[test]
    fn wide_conversion_wcstoutf8_combines_surrogates() {
        let mut buf = [MaybeUninit::<u8>::uninit(); 8];
        let wide = [0xd83d_u32, 0xde00, 0];
        let n = unsafe { wcstoutf8(buf.as_mut_ptr().cast(), wide.as_ptr(), buf.len()) };
        assert_eq!(n, 5);
        let bytes = buf[..5]
            .iter()
            .map(|b| unsafe { b.assume_init() })
            .collect::<Vec<u8>>();
        assert_eq!(&bytes, b"\xF0\x9F\x98\x80\x00");
        // Count-only agrees when the same capacity is offered.
        assert_eq!(
            unsafe { wcstoutf8(ptr::null_mut(), wide.as_ptr(), buf.len()) },
            5
        );
        // Capacity 4 truncates the 4-byte encoding to a NUL per the guard.
        let mut buf = [MaybeUninit::<u8>::uninit(); 4];
        let n = unsafe { wcstoutf8(buf.as_mut_ptr().cast(), wide.as_ptr(), 4) };
        assert_eq!(n, 1);
        assert_eq!(unsafe { buf[0].assume_init() }, 0);
    }

    #[test]
    fn wide_conversion_wcs4toutf8_keeps_ucs4() {
        let mut buf = [MaybeUninit::<u8>::uninit(); 8];
        let wide = [0x1f600_u32, 0];
        let n = unsafe { wcs4toutf8(buf.as_mut_ptr().cast(), wide.as_ptr(), buf.len()) };
        assert_eq!(n, 5);
        let bytes = buf[..5]
            .iter()
            .map(|b| unsafe { b.assume_init() })
            .collect::<Vec<u8>>();
        assert_eq!(&bytes, b"\xF0\x9F\x98\x80\x00");
        assert_eq!(
            unsafe { wcs4toutf8(ptr::null_mut(), wide.as_ptr(), buf.len()) },
            5
        );
    }

    #[test]
    fn wide_conversion_rwcrtomb32_bounds() {
        // Supplementary scalar encodes as four bytes.
        let mut buf = [MaybeUninit::<u8>::uninit(); 8];
        assert_eq!(
            unsafe { Rwcrtomb32(buf.as_mut_ptr().cast(), 0x1f600, buf.len()) },
            4
        );
        let bytes = buf[..4]
            .iter()
            .map(|b| unsafe { b.assume_init() })
            .collect::<Vec<u8>>();
        assert_eq!(&bytes, b"\xF0\x9F\x98\x80");
        // Values above 0x7fffffff return zero and clear the first byte.
        let mut buf = [MaybeUninit::<u8>::new(b'x'); 2];
        assert_eq!(
            unsafe { Rwcrtomb32(buf.as_mut_ptr().cast(), 0x8000_0000, buf.len()) },
            0
        );
        assert_eq!(unsafe { buf[0].assume_init() }, 0);
        // n == 0 never indexes the output.
        let mut buf = [MaybeUninit::<u8>::new(b'x'); 1];
        assert_eq!(
            unsafe { Rwcrtomb32(buf.as_mut_ptr().cast(), u32::from(b'a'), 0) },
            0
        );
        assert_eq!(unsafe { buf[0].assume_init() }, b'x');
    }

    #[test]
    fn wide_conversion_strdup_copies_terminator() {
        let src = c"copy";
        let raw = unsafe { Rstrdup(src.as_ptr()) };
        assert!(!raw.is_null());
        unsafe {
            assert_eq!(CStr::from_ptr(raw), src);
            std::alloc::dealloc(
                raw.cast(),
                std::alloc::Layout::array::<u8>(src.to_bytes_with_nul().len()).unwrap(),
            );
        }
    }

    #[test]
    fn wide_conversion_mbcs_to_ucs2_truncated() {
        let utf8 = crate::sexp::encoding::CE_UTF8;
        // Exactly truncated 3-byte character: leading byte kept, rest skipped.
        let mut out = [MaybeUninit::<u16>::new(0xffff); 4];
        let n = unsafe {
            mbcsToUcs2(
                b"a\xE2\x82\x00".as_ptr().cast(),
                out.as_mut_ptr().cast::<u16>(),
                4,
                utf8,
            )
        };
        assert_eq!(n, 1);
        assert_eq!(unsafe { out[0].assume_init() }, u16::from(b'a'));
        assert_eq!(unsafe { out[1].assume_init() }, 0xffff);
        // Exactly truncated 4-byte character behaves the same.
        let mut out = [MaybeUninit::<u16>::new(0xffff); 4];
        let n = unsafe {
            mbcsToUcs2(
                b"a\xF0\x9F\x98\x00".as_ptr().cast(),
                out.as_mut_ptr().cast::<u16>(),
                4,
                utf8,
            )
        };
        assert_eq!(n, 1);
        assert_eq!(unsafe { out[0].assume_init() }, u16::from(b'a'));
        assert_eq!(unsafe { out[1].assume_init() }, 0xffff);
        // Non-UTF-8 input copies single bytes up to capacity.
        let mut out = [MaybeUninit::<u16>::new(0xffff); 4];
        let n = unsafe {
            mbcsToUcs2(
                b"abc\x00".as_ptr().cast(),
                out.as_mut_ptr().cast::<u16>(),
                4,
                0,
            )
        };
        assert_eq!(n, 3);
        assert_eq!(unsafe { out[2].assume_init() }, u16::from(b'c'));
        assert_eq!(unsafe { out[3].assume_init() }, 0xffff);
        // Unsupported 5-byte leads advance one byte; correctly encoded U+FFFF
        // (rejected by the wide decoder) is preserved here as legacy did.
        let input = c"\xF8a\xEF\xBF\xBF";
        let mut out = [0u16; 3];
        assert_eq!(
            unsafe { mbcsToUcs2(input.as_ptr(), out.as_mut_ptr(), 3, utf8) },
            2
        );
        assert_eq!(out, [u16::from(b'a'), 0xffff, 0]);
    }

    #[test]
    fn adobe_symbol_matches_gnu_reference_for_all_nonzero_bytes() {
        let input: Vec<u8> = (1..=255).chain(std::iter::once(0)).collect();
        // (usePUA, expected len, expected FNV-1a-64 of bytes before NUL),
        // verified against pinned GNU R 4.6.2.
        for (use_pua, expected_len, expected_hash) in [
            (0, 504, 0xc97c_922b_ab5e_a0d3_u64),
            (1, 508, 0xa60c_eb08_7e02_cfbd_u64),
        ] {
            let mut out = [MaybeUninit::<u8>::uninit(); 1024];
            let raw = unsafe {
                Rf_AdobeSymbol2utf8(
                    out.as_mut_ptr().cast(),
                    input.as_ptr().cast(),
                    out.len(),
                    use_pua,
                )
            };
            assert_eq!(raw, out.as_mut_ptr().cast());
            let bytes = unsafe { CStr::from_ptr(raw) }.to_bytes();
            assert_eq!(bytes.len(), expected_len);
            let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
            });
            assert_eq!(hash, expected_hash);
        }
    }

    #[test]
    fn adobe_symbol_short_buffers_never_write_partial_codepoints() {
        // '"' maps to forall (3 bytes), 'A' to alpha (2 bytes).
        let input = c"\"A";
        for n in 0..=9_usize {
            let mut out = [0xcc_u8; 9];
            let expected: &[u8] = match n {
                0 => &[],
                1..=3 => b"\x00",
                4..=8 => b"\xe2\x88\x80\x00",
                _ => b"\xe2\x88\x80\xce\x91\x00",
            };
            let raw = unsafe { Rf_AdobeSymbol2utf8(out.as_mut_ptr().cast(), input.as_ptr(), n, 0) };
            assert_eq!(raw, out.as_mut_ptr().cast());
            assert_eq!(&out[..expected.len()], expected);
            assert!(out[expected.len()..].iter().all(|&b| b == 0xcc));
        }
        // Miri-exact two-byte allocation: only the NUL fits inside.
        let mut short = [0xcc_u8; 2];
        let raw = unsafe { Rf_AdobeSymbol2utf8(short.as_mut_ptr().cast(), input.as_ptr(), 2, 0) };
        assert_eq!(raw, short.as_mut_ptr().cast());
        assert_eq!(short, [0, 0xcc]);
    }
}
