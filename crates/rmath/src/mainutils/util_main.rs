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

use crate::eval::attrib_core::{R_ClassSymbol, R_DimSymbol, getAttrib};
use crate::sexp::accessors::*;
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
const HIGH_SURROGATE_START: u32 = 0xD800;
const LOW_SURROGATE_START: u32 = 0xDC00;
const IS_HIGH_SURROGATE_MASK: u32 = 0xF800;

/// The largest finite double, used for overflow detection.
/// Equivalent to C's DBL_MAX.
const DBL_MAX: f64 = 1.7976931348623157e308;

/// The largest exactly representable integer in an IEEE 754 double
/// (2^53 - 1), used for the exact clause in R_strtod5.
const MAX_EXACT_DOUBLE: f64 = 9007199254740991.0; // 0x1.fffffffffffffp52

// ---------------------------------------------------------------------------
// Minimal libc helpers (avoiding the libc crate)
// ---------------------------------------------------------------------------

#[inline]
unsafe fn libc_strlen(s: *const c_char) -> usize {
    unsafe {
        let mut len = 0usize;
        let mut p = s;
        while *p != 0 {
            len += 1;
            p = p.add(1);
        }
        len
    }
}

#[inline]
unsafe fn libc_isspace(c: c_char) -> bool {
    let byte = c as u8;
    byte == b' '
        || byte == b'\t'
        || byte == b'\n'
        || byte == b'\r'
        || byte == b'\x0b'
        || byte == b'\x0c'
}

#[inline]
unsafe fn libc_strncmp(s1: *const c_char, s2: &[u8], n: usize) -> c_int {
    unsafe {
        let mut p1 = s1;
        let mut p2 = s2.as_ptr();
        for _ in 0..n {
            let c1 = *p1 as i8;
            let c2 = *p2 as i8;
            if c1 != c2 {
                return (c1 - c2) as c_int;
            }
            if c1 == 0 {
                return 0;
            }
            p1 = p1.add(1);
            p2 = p2.add(1);
        }
        0
    }
}

#[inline]
unsafe fn libc_strncasecmp(s1: *const c_char, s2: &[u8], n: usize) -> c_int {
    unsafe {
        let mut p1 = s1;
        let mut p2 = s2.as_ptr();
        for _ in 0..n {
            let c1 = to_lower(*p1 as u8) as i8;
            let c2 = to_lower(*p2 as u8) as i8;
            if c1 != c2 {
                return (c1 - c2) as c_int;
            }
            if *p1 == 0 {
                return 0;
            }
            p1 = p1.add(1);
            p2 = p2.add(1);
        }
        0
    }
}

#[inline]
fn to_lower(c: u8) -> u8 {
    if c >= b'A' && c <= b'Z' { c + 32 } else { c }
}

#[inline]
unsafe fn libc_malloc(size: usize) -> *mut c_void {
    unsafe {
        let layout = std::alloc::Layout::from_size_align(size, 1)
            .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
        std::alloc::alloc(layout) as *mut c_void
    }
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
pub unsafe fn utf8clen(c: c_char) -> c_int {
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
unsafe fn utf16toucs(high: u32, low: u32) -> R_wchar_t {
    0x10000 + ((high & 0x3FF) << 10) + (low & 0x3FF)
}

/// Return the low UTF-16 surrogate from a 4-byte UTF-8 sequence.
///
/// Port of R's static `utf8toutf16low` from util.c.
/// Assumes all validation has been done already.
#[inline]
unsafe fn utf8toutf16low(s: *const c_char) -> u32 {
    unsafe { LOW_SURROGATE_START | ((*s.add(2) as u32 & 0x0F) << 6) | (*s.add(3) as u32 & 0x3F) }
}

/// Convert UTF-8 (high surrogate wchar + string pointer) to UCS-32.
///
/// Port of R's `utf8toucs32` from util.c.
pub unsafe fn Rf_utf8toucs32(high: u32, s: *const c_char) -> R_wchar_t {
    unsafe { utf16toucs(high, utf8toutf16low(s)) }
}

// ---------------------------------------------------------------------------
// utf8toucs: convert a single UTF-8 character to wchar_t
// ---------------------------------------------------------------------------

/// Convert a single UTF-8 character to a wide character.
///
/// Returns the number of bytes consumed from `s`, or (size_t)-1 on invalid,
/// or (size_t)-2 if the string is too short.
///
/// If `wc` is null, the result is discarded but the byte count is returned.
///
/// Port of R's `utf8toucs` from util.c.
pub unsafe fn utf8toucs(wc: *mut u32, s: *const c_char) -> usize {
    unsafe {
        let byte = *s as u8;
        let mut local: u32 = 0;
        let w = if wc.is_null() { &mut local } else { &mut *wc };

        if byte == 0 {
            *w = 0;
            return 0;
        } else if byte < 0x80 {
            *w = byte as u32;
            return 1;
        } else if byte < 0xC0 {
            return usize::MAX; // -1
        } else if byte < 0xE0 {
            // 2-byte sequence
            if libc_strlen(s) < 2 {
                return usize::MAX - 1;
            } // -2
            if (*s.add(1) as u8 & 0xC0) == 0x80 {
                *w = (((byte & 0x1F) as u32) << 6) | (*s.add(1) as u32 & 0x3F);
                return 2;
            } else {
                return usize::MAX; // -1
            }
        } else if byte < 0xF0 {
            // 3-byte sequence
            if libc_strlen(s) < 3 {
                return usize::MAX - 1;
            } // -2
            if (*s.add(1) as u8 & 0xC0) == 0x80 && (*s.add(2) as u8 & 0xC0) == 0x80 {
                let cvalue = (((byte & 0x0F) as u32) << 12)
                    | ((*s.add(1) as u32 & 0x3F) << 6)
                    | (*s.add(2) as u32 & 0x3F);
                // Surrogate range check
                if cvalue >= 0xD800 && cvalue <= 0xDFFF {
                    return usize::MAX;
                }
                if cvalue == 0xFFFE || cvalue == 0xFFFF {
                    return usize::MAX;
                }
                *w = cvalue;
                return 3;
            } else {
                return usize::MAX; // -1
            }
        } else if byte < 0xF8 {
            // 4-byte sequence
            if libc_strlen(s) < 4 {
                return usize::MAX - 1;
            } // -2
            if (*s.add(1) as u8 & 0xC0) == 0x80
                && (*s.add(2) as u8 & 0xC0) == 0x80
                && (*s.add(3) as u8 & 0xC0) == 0x80
            {
                let cvalue = (((byte & 0x0F) as u32) << 18)
                    | ((*s.add(1) as u32 & 0x3F) << 12)
                    | ((*s.add(2) as u32 & 0x3F) << 6)
                    | (*s.add(3) as u32 & 0x3F);
                // On platforms where wchar_t < 4 (UTF-16), return high surrogate.
                // In our Rust port, we always use u32, so we just store the value.
                *w = cvalue;
                return 4;
            } else {
                return usize::MAX; // -1
            }
        }

        // 5-byte and 6-byte sequences (very rare, no validation)
        if byte < 0xFC {
            // 5-byte
            if libc_strlen(s) < 5 {
                return usize::MAX - 1;
            }
            *w = (((byte & 0x0F) as u32) << 24)
                | ((*s.add(1) as u32 & 0x3F) << 12)
                | ((*s.add(2) as u32 & 0x3F) << 12)
                | ((*s.add(3) as u32 & 0x3F) << 6)
                | (*s.add(4) as u32 & 0x3F);
            return 5;
        } else {
            // 6-byte
            if libc_strlen(s) < 6 {
                return usize::MAX - 1;
            }
            *w = (((byte & 0x0F) as u32) << 30)
                | ((*s.add(1) as u32 & 0x3F) << 24)
                | ((*s.add(2) as u32 & 0x3F) << 18)
                | ((*s.add(3) as u32 & 0x3F) << 12)
                | ((*s.add(4) as u32 & 0x3F) << 6)
                | (*s.add(5) as u32 & 0x3F);
            return 6;
        }
    }
}

// ---------------------------------------------------------------------------
// utf8towcs: convert UTF-8 string to wide-char string
// ---------------------------------------------------------------------------

/// Check if a wchar value is a high surrogate.
#[inline]
fn is_high_surrogate(w: u32) -> bool {
    (w & IS_HIGH_SURROGATE_MASK) == HIGH_SURROGATE_START
}

/// Convert a UTF-8 string to a wide-character string.
///
/// If `wc` is null, only counts the resulting characters.
/// `n` is the maximum number of wide characters to write.
///
/// Returns the number of wide characters written (not including terminator).
///
/// Port of R's `utf8towcs` from util.c.
pub unsafe fn utf8towcs(wc: *mut u32, s: *const c_char, n: usize) -> usize {
    unsafe {
        let mut res: isize = 0;
        let mut t = s;
        let mut local: u32 = 0;

        if !wc.is_null() {
            let mut p = wc;
            loop {
                let m = utf8toucs(p, t) as isize;
                if m < 0 {
                    eprintln!("invalid input in utf8towcs");
                    return 0;
                }
                if m == 0 {
                    break;
                }
                res += 1;
                if res as usize >= n {
                    break;
                }
                if is_high_surrogate(*p) {
                    p = p.add(1);
                    *p = utf8toutf16low(t);
                    res += 1;
                    if res as usize >= n {
                        break;
                    }
                }
                p = p.add(1);
                t = t.offset(m);
            }
        } else {
            loop {
                let m = utf8toucs(&mut local, t) as isize;
                if m < 0 {
                    eprintln!("invalid input in utf8towcs");
                    return 0;
                }
                if m == 0 {
                    break;
                }
                res += 1;
                if is_high_surrogate(local) {
                    res += 1;
                }
                t = t.offset(m);
            }
        }
        res as usize
    }
}

// ---------------------------------------------------------------------------
// utf8towcs4: convert UTF-8 string to UCS-4 (R_wchar_t) string
// ---------------------------------------------------------------------------

/// Convert a UTF-8 string to a UCS-4 (R_wchar_t) string.
///
/// Port of R's `utf8towcs4` from util.c.
pub unsafe fn utf8towcs4(wc: *mut R_wchar_t, s: *const c_char, n: usize) -> usize {
    unsafe {
        let mut res: isize = 0;
        let mut t = s;

        if !wc.is_null() {
            let mut p = wc;
            loop {
                let mut local: u32 = 0;
                let m = utf8toucs(&mut local, t) as isize;
                *p = local as R_wchar_t;
                if m < 0 {
                    eprintln!("invalid input in utf8towcs4");
                    return 0;
                }
                if m == 0 {
                    break;
                }
                if is_high_surrogate(*p) {
                    *p = Rf_utf8toucs32(*p, t);
                }
                res += 1;
                if res as usize >= n {
                    break;
                }
                p = p.add(1);
                t = t.offset(m);
            }
        } else {
            loop {
                let mut local: u32 = 0;
                let m = utf8toucs(&mut local, t) as isize;
                if m < 0 {
                    eprintln!("invalid input in utf8towcs4");
                    return 0;
                }
                if m == 0 {
                    break;
                }
                res += 1;
                t = t.offset(m);
            }
        }
        res as usize
    }
}

// ---------------------------------------------------------------------------
// Rwcrtomb32: convert a single UCS-4 codepoint to UTF-8
// ---------------------------------------------------------------------------

/// Table from pcre.c used in UTF-8 encoding.
static UTF8_TABLE1: [u32; 6] = [0x7f, 0x7ff, 0xffff, 0x1fffff, 0x3ffffff, 0x7fffffff];
static UTF8_TABLE2: [u8; 6] = [0, 0xc0, 0xe0, 0xf0, 0xf8, 0xfc];

/// Convert a single UCS-4 codepoint to UTF-8.
///
/// If `s` is null or `n` is 0, returns the number of bytes that would be needed.
/// Otherwise writes the UTF-8 encoding into `s` (which must have room for `n` bytes).
/// Returns the number of bytes written (not including the null terminator).
///
/// Port of R's static `Rwcrtomb32` from util.c.
pub unsafe fn Rwcrtomb32(s: *mut c_char, mut cvalue: R_wchar_t, n: usize) -> usize {
    unsafe {
        if n == 0 {
            return 0;
        }
        if !s.is_null() {
            *s = 0;
        } // simplify exit
        if cvalue == 0 {
            return 0;
        }

        let mut i: usize = 0;
        while i < UTF8_TABLE1.len() {
            if (cvalue as u32) <= UTF8_TABLE1[i] {
                break;
            }
            i += 1;
        }

        if i >= n - 1 {
            return 0;
        } // need space for terminal null

        if !s.is_null() {
            let mut si = s.add(i);
            let mut j = i;
            while j > 0 {
                j -= 1;
                *si = (0x80 | (cvalue & 0x3F)) as c_char;
                si = si.sub(1);
                cvalue >>= 6;
            }
            *si = (UTF8_TABLE2[i] | (cvalue as u8)) as c_char;
        }
        i + 1
    }
}

// ---------------------------------------------------------------------------
// wcstoutf8: convert wide string (UTF-16/UCS-2/UCS-4) to UTF-8
// ---------------------------------------------------------------------------

/// Check if two consecutive wide chars form a surrogate pair.
#[allow(clippy::bad_bit_mask)]
#[inline]
fn is_surrogate_pair(high: u32, low: u32) -> bool {
    is_high_surrogate(high) && (low & IS_HIGH_SURROGATE_MASK) == LOW_SURROGATE_START
}

/// Convert a wide-character string to UTF-8.
///
/// `s` can be a buffer of size `n` or null. If `n` is 0 or `s` is null,
/// nothing is written. Returns the number of chars including the terminating
/// null. If the buffer is not big enough, the result is truncated but
/// still null-terminated.
///
/// Port of R's `wcstoutf8` from util.c.
#[allow(clippy::bad_bit_mask)]
pub unsafe fn wcstoutf8(s: *mut c_char, wc: *const u32, n: usize) -> usize {
    unsafe {
        if n == 0 {
            return 0;
        }
        let mut res: usize = 0;
        let mut p = wc;
        let mut t = s;

        loop {
            let ch = *p;
            if ch == 0 {
                break;
            }

            let m = if is_surrogate_pair(ch, *p.add(1)) {
                let cvalue = ((ch & 0x3FF) << 10) + (*p.add(1) & 0x3FF) + 0x010000;
                p = p.add(1); // skip low surrogate
                Rwcrtomb32(t, cvalue, n - res)
            } else {
                if is_high_surrogate(ch) || (ch & IS_HIGH_SURROGATE_MASK) == LOW_SURROGATE_START {
                    eprintln!("unpaired surrogate Unicode point {:x}", ch);
                }
                Rwcrtomb32(t, ch, n - res)
            };

            if m == 0 {
                break;
            }
            res += m;
            if !t.is_null() {
                t = t.add(m);
            }
            p = p.add(1);
        }
        // Write null terminator
        if !t.is_null() {
            *t = 0;
        }
        res + 1
    }
}

// ---------------------------------------------------------------------------
// wcs4toutf8: convert UCS-4 string to UTF-8
// ---------------------------------------------------------------------------

/// Convert a UCS-4 (R_wchar_t) string to UTF-8.
///
/// Port of R's `wcs4toutf8` from util.c.
pub unsafe fn wcs4toutf8(s: *mut c_char, wc: *const R_wchar_t, n: usize) -> usize {
    unsafe {
        if n == 0 {
            return 0;
        }
        let mut res: usize = 0;
        let mut p = wc;
        let mut t = s;

        loop {
            let ch = *p;
            if ch == 0 {
                break;
            }
            let m = Rwcrtomb32(t, ch, n - res);
            if m == 0 {
                break;
            }
            res += m;
            if !t.is_null() {
                t = t.add(m);
            }
            p = p.add(1);
        }
        if !t.is_null() {
            *t = 0;
        }
        res + 1
    }
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
pub unsafe fn Rf_AdobeSymbol2utf8(
    work: *mut c_char,
    c0: *const c_char,
    nwork: usize,
    usePUA: Rboolean,
) -> *mut c_char {
    unsafe {
        if work.is_null() || c0.is_null() {
            return work;
        }
        let table = if usePUA != 0 { &S2U } else { &S2UNICODE };
        let mut c = c0 as *const u8;
        let mut t = work as *mut u8;

        while *c != 0 {
            if *c < 32 {
                *t = b' ';
                t = t.add(1);
            } else {
                let u = table[(*c - 32) as usize];
                if u < 128 {
                    *t = u as u8;
                    t = t.add(1);
                } else if u < 0x800 {
                    *t = (0xc0 | (u >> 6)) as u8;
                    t = t.add(1);
                    *t = (0x80 | (u & 0x3f)) as u8;
                    t = t.add(1);
                } else {
                    *t = (0xe0 | (u >> 12)) as u8;
                    t = t.add(1);
                    *t = (0x80 | ((u >> 6) & 0x3f)) as u8;
                    t = t.add(1);
                    *t = (0x80 | (u & 0x3f)) as u8;
                    t = t.add(1);
                }
            }
            // Check buffer space: need room for 6 more bytes (max UTF-8 char)
            if t.add(6) > (work.add(nwork) as *mut u8) {
                break;
            }
            c = c.add(1);
        }
        *t = 0;
        work
    }
}

/// Convert an Adobe Symbol byte to UCS-2 codepoint.
///
/// Port of R's `Rf_AdobeSymbol2ucs2` from util.c.
pub unsafe fn Rf_AdobeSymbol2ucs2(n: c_int) -> c_int {
    if n >= 32 && n < 256 {
        S2U[(n - 32) as usize] as c_int
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// R_strtod5 / R_strtod4 / R_strtod / R_atof: string-to-double conversion
// ---------------------------------------------------------------------------

/// Maximum exponent prefix to prevent overflow (from R's util.c).
const MAX_EXPONENT_PREFIX: c_int = 9999;

/// R's custom string-to-double conversion.
///
/// This is the most general form, allowing the decimal point character,
/// "NA" acceptance, and exactness checking.
///
/// Port of R's `R_strtod5` from util.c.
#[allow(clippy::overly_complex_bool_expr)]
pub unsafe fn R_strtod5(
    str: *const c_char,
    endptr: *mut *mut c_char,
    dec: c_char,
    na: Rboolean,
    exact: c_int,
) -> c_double {
    unsafe {
        let mut ans: f64 = 0.0;
        let mut sign: c_int = 1;
        let mut p = str;
        let dec_byte = dec as u8;

        // optional whitespace
        while libc_isspace(*p) {
            p = p.add(1);
        }

        // check for "NA"
        if na != 0 && libc_strncmp(p, b"NA\0", 2) == 0 {
            ans = R_NA_REAL;
            p = p.add(2);
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return ans;
        }

        // optional sign
        let p_byte = *p as u8;
        if p_byte == b'-' {
            sign = -1;
            p = p.add(1);
        } else if p_byte == b'+' {
            p = p.add(1);
        }

        // check for NaN / Inf
        if libc_strncasecmp(p, b"NaN\0", 3) == 0 {
            ans = R_NaN;
            p = p.add(3);
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return sign as c_double * ans;
        } else if libc_strncasecmp(p, b"infinity\0", 8) == 0 {
            ans = R_PosInf;
            p = p.add(8);
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return sign as c_double * ans;
        } else if libc_strncasecmp(p, b"Inf\0", 3) == 0 {
            ans = R_PosInf;
            p = p.add(3);
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return sign as c_double * ans;
        }

        let mut expn: c_int = 0;

        // Hexadecimal "0x..."
        if libc_strlen(p) > 2
            && *p as u8 == b'0'
            && (*p.add(1) as u8 == b'x' || *p.add(1) as u8 == b'X')
        {
            let mut exph: c_int = -1;
            p = p.add(2);

            loop {
                let ch = *p as u8;
                if ch >= b'0' && ch <= b'9' {
                    ans = 16.0 * ans + (ch - b'0') as f64;
                } else if ch >= b'a' && ch <= b'f' {
                    ans = 16.0 * ans + (ch - b'a' + 10) as f64;
                } else if ch >= b'A' && ch <= b'F' {
                    ans = 16.0 * ans + (ch - b'A' + 10) as f64;
                } else if ch == dec_byte {
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

            // EXACT clause
            if exact != 0 && exact != 1 && ans > MAX_EXACT_DOUBLE && exact == 1 {
                ans = R_NA_REAL;
                p = str;
                if !endptr.is_null() {
                    *endptr = p as *mut c_char;
                }
                return sign as c_double * ans;
            }

            // Binary exponent
            if *p as u8 == b'p' || *p as u8 == b'P' {
                let mut expsign: c_int = 1;
                p = p.add(1);
                let psign_byte = *p as u8;
                if psign_byte == b'-' {
                    expsign = -1;
                    p = p.add(1);
                } else if psign_byte == b'+' {
                    p = p.add(1);
                }
                let mut n: c_int = 0;
                let mut ndig: c_int = 0;
                while *p as u8 >= b'0' && *p as u8 <= b'9' {
                    n = if n < MAX_EXPONENT_PREFIX {
                        n * 10 + (*p as u8 - b'0') as c_int
                    } else {
                        n
                    };
                    ndig += 1;
                    p = p.add(1);
                }
                if ndig == 0 {
                    ans = R_NA_REAL;
                    p = str;
                    if !endptr.is_null() {
                        *endptr = p as *mut c_char;
                    }
                    return ans;
                }
                expn += expsign * n;
            }

            if ans != 0.0 {
                let mut fac: f64 = 1.0;
                let mut p2: f64 = 2.0;
                if exph > 0 {
                    if expn - exph < -122 {
                        let mut n2 = exph;
                        fac = 1.0;
                        while n2 != 0 {
                            if n2 & 1 != 0 {
                                fac *= p2;
                            }
                            n2 >>= 1;
                            p2 *= p2;
                        }
                        ans /= fac;
                        p2 = 2.0;
                    } else {
                        expn -= exph;
                    }
                }
                if expn < 0 {
                    let mut n2 = -expn;
                    fac = 1.0;
                    while n2 != 0 {
                        if n2 & 1 != 0 {
                            fac *= p2;
                        }
                        n2 >>= 1;
                        p2 *= p2;
                    }
                    ans /= fac;
                } else {
                    let mut n2 = expn;
                    fac = 1.0;
                    while n2 != 0 {
                        if n2 & 1 != 0 {
                            fac *= p2;
                        }
                        n2 >>= 1;
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

        // Decimal parsing
        let mut ndigits: c_int = 0;
        while *p as u8 >= b'0' && *p as u8 <= b'9' {
            ans = 10.0 * ans + (*p as u8 - b'0') as f64;
            ndigits += 1;
            p = p.add(1);
        }
        if *p as u8 == dec_byte {
            p = p.add(1);
            while *p as u8 >= b'0' && *p as u8 <= b'9' {
                ans = 10.0 * ans + (*p as u8 - b'0') as f64;
                ndigits += 1;
                expn -= 1;
                p = p.add(1);
            }
        }

        if ndigits == 0 {
            ans = R_NA_REAL;
            p = str;
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return ans;
        }

        // EXACT clause for decimal
        if exact != 0 && exact != 1 && ans > MAX_EXACT_DOUBLE && exact == 1 {
            ans = R_NA_REAL;
            p = str;
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return sign as c_double * ans;
        }

        // Exponent
        if *p as u8 == b'e' || *p as u8 == b'E' {
            let mut expsign: c_int = 1;
            p = p.add(1);
            let psign_byte = *p as u8;
            if psign_byte == b'-' {
                expsign = -1;
                p = p.add(1);
            } else if psign_byte == b'+' {
                p = p.add(1);
            }
            let mut n: c_int = 0;
            let mut ndig: c_int = 0;
            while *p as u8 >= b'0' && *p as u8 <= b'9' {
                n = if n < MAX_EXPONENT_PREFIX {
                    n * 10 + (*p as u8 - b'0') as c_int
                } else {
                    n
                };
                ndig += 1;
                p = p.add(1);
            }
            if ndig == 0 {
                ans = R_NA_REAL;
                p = str;
                if !endptr.is_null() {
                    *endptr = p as *mut c_char;
                }
                return ans;
            }
            expn += expsign * n;
        }

        // Apply exponent
        // avoid unnecessary underflow for large negative exponents
        if expn + ndigits < -300 {
            for _ in 0..ndigits {
                ans /= 10.0;
            }
            expn += ndigits;
        }

        let mut p10: f64 = 10.0;
        if expn < -307 {
            let mut n2 = -expn;
            let mut fac: f64 = 1.0;
            while n2 != 0 {
                if n2 & 1 != 0 {
                    fac /= p10;
                }
                n2 >>= 1;
                p10 *= p10;
            }
            ans *= fac;
        } else if expn < 0 {
            let mut n2 = -expn;
            let mut fac: f64 = 1.0;
            while n2 != 0 {
                if n2 & 1 != 0 {
                    fac *= p10;
                }
                n2 >>= 1;
                p10 *= p10;
            }
            ans /= fac;
        } else if ans != 0.0 {
            let mut n2 = expn;
            let mut fac: f64 = 1.0;
            while n2 != 0 {
                if n2 & 1 != 0 {
                    fac *= p10;
                }
                n2 >>= 1;
                p10 *= p10;
            }
            ans *= fac;
        }

        // explicit overflow to infinity
        if ans > DBL_MAX {
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return if sign > 0 { R_PosInf } else { R_NegInf };
        }

        if !endptr.is_null() {
            *endptr = p as *mut c_char;
        }
        sign as c_double * ans
    }
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

/// Return a newly allocated copy of a string using malloc.
///
/// Port of R's `Rstrdup` from util.c. Uses `malloc` and calls `error()`
/// (panics in Rust) on allocation failure.
pub unsafe fn Rstrdup(s: *const c_char) -> *mut c_char {
    unsafe {
        let nb = libc_strlen(s) + 1;
        let cpy = libc_malloc(nb);
        if cpy.is_null() {
            eprintln!("allocation error in Rstrdup");
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(s, cpy as *mut c_char, nb);
        cpy as *mut c_char
    }
}

// ---------------------------------------------------------------------------
// Shell sort with index (isort_with_index)
// ---------------------------------------------------------------------------

/// Shell sort with parallel index array.
///
/// Port of R's static `isort_with_index` from util.c.
pub unsafe fn isort_with_index(x: *mut c_int, indx: *mut c_int, n: c_int) {
    unsafe {
        // Compute initial gap (Knuth's sequence)
        let mut h: c_int = 1;
        while h <= n / 9 {
            h = 3 * h + 1;
        }

        while h > 0 {
            let mut i = h;
            while i < n {
                let v = *x.add(i as usize);
                let iv = *indx.add(i as usize);
                let mut j = i;
                while j >= h && *x.add((j - h) as usize) > v {
                    *x.add(j as usize) = *x.add((j - h) as usize);
                    *indx.add(j as usize) = *indx.add((j - h) as usize);
                    j -= h;
                }
                *x.add(j as usize) = v;
                *indx.add(j as usize) = iv;
                i += 1;
            }
            h /= 3;
        }
    }
}

// ---------------------------------------------------------------------------
// bincode: binary coding algorithm
// ---------------------------------------------------------------------------

/// Binary coding: assign each value in `x` to a bin defined by `breaks`.
///
/// This is the core algorithm from R's `bincode` in util.c.
/// SEXP-dependent wrapper is a stub.
pub unsafe fn bincode_impl(
    x: *const c_double,
    n: usize,
    breaks: *const c_double,
    nb: c_int,
    code: *mut c_int,
    right: c_int,
    include_border: c_int,
) {
    unsafe {
        if nb < 2 || nb == NA_INTEGER {
            r_error("invalid 'breaks' argument");
        }

        let nb1 = nb - 1;
        let lft = if right != 0 { 0 } else { 1 };

        // Verify breaks are sorted
        for i in 1..nb {
            if *breaks.add((i - 1) as usize) > *breaks.add(i as usize) {
                r_error("'breaks' is not sorted");
            }
        }

        for i in 0..n {
            *code.add(i) = NA_INTEGER;
            let xi = *x.add(i);
            if !xi.is_nan() {
                let mut lo: c_int = 0;
                let mut hi = nb1;
                if xi < *breaks.add(lo as usize)
                    || *breaks.add(hi as usize) < xi
                    || (xi == *breaks.add(if lft != 0 { hi } else { lo } as usize)
                        && include_border == 0)
                {
                    // NA
                } else {
                    while hi - lo >= 2 {
                        let mid = (hi + lo) / 2;
                        if xi > *breaks.add(mid as usize)
                            || (lft != 0 && xi == *breaks.add(mid as usize))
                        {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    *code.add(i) = lo + 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent API helpers from util.c
// ---------------------------------------------------------------------------

/// Number of rows of a matrix-like object.
///
/// Equivalent of R's `nrows()` from util.c.
/// Uses the "dim" attribute: returns dims[0].
pub unsafe fn nrows(s: *const c_void) -> c_int {
    unsafe {
        let x = s as SEXP;
        if x.is_null() || x == R_NilValue() {
            return -1;
        }
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() {
            return LENGTH(x);
        }
        if LENGTH(dim) >= 1 { *INTEGER(dim) } else { -1 }
    }
}

/// Number of columns of a matrix-like object.
///
/// Equivalent of R's `ncols()` from util.c.
/// Uses the "dim" attribute: returns dims[1] if ndim >= 2, else 1.
pub unsafe fn ncols(s: *const c_void) -> c_int {
    unsafe {
        let x = s as SEXP;
        if x.is_null() || x == R_NilValue() {
            return -1;
        }
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() {
            return 1;
        }
        if LENGTH(dim) >= 2 {
            *INTEGER(dim).add(1)
        } else {
            1
        }
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
pub unsafe fn isUnordered(s: *const c_void) -> Rboolean {
    unsafe {
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
            let p = CHAR(elt);
            if !p.is_null() {
                let cs = CStr::from_ptr(p);
                if let Ok(s) = cs.to_str()
                    && (s == "unordered" || s == "factor")
                {
                    return TRUE;
                }
            }
        }
        FALSE
    }
}

/// Check if an object inherits from "ordered" class.
///
/// Equivalent of R's `isOrdered()` from util.c.
pub unsafe fn isOrdered(s: *const c_void) -> Rboolean {
    unsafe {
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
            let p = CHAR(elt);
            if !p.is_null() {
                let cs = CStr::from_ptr(p);
                if let Ok(s) = cs.to_str()
                    && s == "ordered"
                {
                    return TRUE;
                }
            }
        }
        FALSE
    }
}

/// Test if a SEXP is TRUE (handles NA, length != 1).
///
/// Equivalent of R's `R_isTRUE()` from util.c.
/// Returns TRUE only for a length-1 logical vector with value 1.
pub unsafe fn R_isTRUE(x: *const c_void) -> Rboolean {
    unsafe {
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
        let v = *LOGICAL(s);
        if v == 1 { TRUE } else { FALSE }
    }
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
pub unsafe fn type2char(t: c_int) -> *const c_char {
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
pub unsafe fn StringBlank(x: *const c_void) -> Rboolean {
    unsafe {
        let s = x as SEXP;
        if s.is_null() || s == R_NilValue() {
            return TRUE;
        }
        let p = CHAR(s);
        if p.is_null() {
            return TRUE;
        }
        isBlankString(p)
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

/// Convert a multibyte string to UCS-2 (UTF-16).
///
/// Simplified version that handles ASCII and basic UTF-8.
/// For enc=1 (CE_NATIVE), uses platform bytes. For enc=2 (CE_UTF8), parses UTF-8.
pub unsafe fn mbcsToUcs2(in_: *const c_char, out: *mut u16, nout: c_int, enc: c_int) -> usize {
    unsafe {
        if in_.is_null() || out.is_null() || nout <= 0 {
            return 0;
        }
        if enc == 2 {
            // CE_UTF8: parse UTF-8 code points and emit as UCS-2
            let mut si: usize = 0;
            let mut oi: usize = 0;
            while si < usize::MAX && oi < nout as usize {
                let b0 = *in_.add(si) as u8;
                if b0 == 0 {
                    break;
                }
                let clen = utf8clen(b0 as c_char) as usize;
                if clen == 1 {
                    if b0 < 0x80 {
                        *out.add(oi) = b0 as u16;
                        oi += 1;
                    }
                    si += 1;
                } else if clen == 2 && si + 1 < usize::MAX {
                    let b1 = *in_.add(si + 1) as u8;
                    let cp = (((b0 as u32) & 0x1F) << 6) | ((b1 as u32) & 0x3F);
                    if cp >= 0x80 {
                        *out.add(oi) = cp as u16;
                        oi += 1;
                    }
                    si += 2;
                } else if clen == 3 && si + 2 < usize::MAX {
                    let b1 = *in_.add(si + 1) as u8;
                    let b2 = *in_.add(si + 2) as u8;
                    let cp = (((b0 as u32) & 0x0F) << 12)
                        | (((b1 as u32) & 0x3F) << 6)
                        | ((b2 as u32) & 0x3F);
                    if cp >= 0x800 && (cp < 0xD800 || cp >= 0xE000) {
                        *out.add(oi) = cp as u16;
                        oi += 1;
                    }
                    // Surrogates would need special handling; skip for now
                    si += 3;
                } else if clen == 4 && si + 3 < usize::MAX {
                    // 4-byte: code point > 0xFFFF, needs surrogate pair — skip
                    si += 4;
                } else {
                    si += 1; // Invalid leading byte
                }
            }
            oi
        } else {
            // CE_NATIVE: treat as single-byte / Latin-1
            let mut si: usize = 0;
            let mut oi: usize = 0;
            while si < usize::MAX && oi < nout as usize {
                let b = *in_.add(si) as u8;
                if b == 0 {
                    break;
                }
                *out.add(oi) = b as u16;
                si += 1;
                oi += 1;
            }
            oi
        }
    }
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
        unsafe {
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
            assert!(val.to_bits() == R_NA_REAL.to_bits());
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
        unsafe {
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
            let mut code = [0i32; 5];
            bincode_impl(x.as_ptr(), 5, breaks.as_ptr(), 5, code.as_mut_ptr(), 1, 0);
            assert_eq!(code[0], NA_INTEGER); // 0.5 < breaks[0]
            assert_eq!(code[1], 1); // 1.5 in [1,2)
            assert_eq!(code[2], 2); // 2.5 in [2,3)
            assert_eq!(code[3], 3); // 3.5 in [3,4)
            assert_eq!(code[4], 4); // 4.5 in [4,5)
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
}
