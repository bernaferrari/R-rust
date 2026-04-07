#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/scan.c
//!
//! The original C implementation provides `scan()`, `readline()` / `do_readln`,
//! and the low-level character-at-a-time input parsing engine used by R's
//! interactive console and data readers.
//!
//! Heavily SEXP-dependent entry points:
//!   - do_scan      (.Internal(scan ...))
//!   - do_readln    (readline())
//!
//! Extractable standalone utilities:
//!   - Rspace()          -- whitespace classification (ASCII space/tab/CR/LF + NBSP)
//!   - Strtoi()          -- safe strtol wrapper returning NA_INTEGER on overflow
//!   - Strtod()          -- wrapper around R_strtod4 with configurable decimal char
//!   - strtoc()          -- complex-number parser (a+bi, ai, a)
//!   - strtoraw()        -- two-digit hex raw-byte parser
//!
//! Remaining functions (fillBuffer, scanchar, scanVector, scanFrame,
//! extractItem, etc.) depend on R connections, SEXP vectors, and R's
//! memory allocator and are provided as stubs only.

use std::os::raw::{c_char, c_int};

use crate::sexp::ffi::{NA_INTEGER, Rcomplex, SEXP};

// ---------------------------------------------------------------------------
// Constants from scan.c
// ---------------------------------------------------------------------------

/// Initial allocation size for the scan vector.
pub const SCAN_BLOCKSIZE: usize = 1000;

/// Size of the console prompt buffer.
pub const CONSOLE_PROMPT_SIZE: usize = 256;

/// Sentinel value meaning "no comment character".
pub const NO_COMCHAR: c_int = 100000;

// ---------------------------------------------------------------------------
// Standalone utility: Rspace
// ---------------------------------------------------------------------------

/// Classify a character as whitespace in R's sense.
///
/// Recognises ASCII space, tab, CR, LF.  When `known_to_be_latin1` is true,
/// 0xa0 (NBSP in Latin-1) is also treated as whitespace.
///
/// In the original C code this is `R_INLINE` and also handles the Win32
/// non-MBCS locale case.  This port exposes it as a regular function.
pub unsafe fn Rspace(c: std::os::raw::c_uint) -> bool {
    if c == b' ' as std::os::raw::c_uint
        || c == b'\t' as std::os::raw::c_uint
        || c == b'\n' as std::os::raw::c_uint
        || c == b'\r' as std::os::raw::c_uint
    {
        return true;
    }
    // 0xa0 = NBSP in Latin-1 (original: known_to_be_latin1 check)
    if c == 0xa0 {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Standalone utility: Strtoi
// ---------------------------------------------------------------------------

/// Parse a string as a signed integer in the given base.
///
/// Like `strtol` but for `int` rather than `long`.  Returns `NA_INTEGER`
/// on overflow, trailing garbage, or ERANGE.
///
/// Note: this implementation uses Rust's `from_str_radix` which returns
/// `i32` directly, so 64-bit overflow cannot occur.  We still check for
/// trailing characters to match R semantics.
pub unsafe fn Strtoi(nptr: *const c_char, base: c_int) -> c_int {
    unsafe {
        if nptr.is_null() {
            return NA_INTEGER;
        }
        let s = match std::ffi::CStr::from_ptr(nptr).to_str() {
            Ok(s) => s,
            Err(_) => return NA_INTEGER,
        };
        // Trim leading whitespace (R's strtol does this).
        let trimmed = s.trim_start();
        if trimmed.is_empty() {
            return NA_INTEGER;
        }
        let radix = if base == 0 {
            if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                16
            } else if trimmed.starts_with('0') && trimmed.len() > 1 {
                8
            } else {
                10
            }
        } else if base >= 2 && base <= 36 {
            base as u32
        } else {
            return NA_INTEGER;
        };
        let to_parse = if radix == 16 && (trimmed.starts_with("0x") || trimmed.starts_with("0X")) {
            &trimmed[2..]
        } else {
            trimmed
        };
        match i32::from_str_radix(to_parse, radix) {
            Ok(v) => {
                // Check that the entire string was consumed (modulo trailing whitespace).
                // R's strtol stops at the first non-matching char; we must reject
                // strings with non-digit trailing content.
                // For simplicity we accept trailing whitespace like C strtol.
                let rest = to_parse.trim_start_matches(|ch: char| {
                    ch.is_ascii_digit()
                        || (radix > 10
                            && ch.is_ascii_alphabetic()
                            && (ch.to_ascii_lowercase() as u32 - 'a' as u32) < (radix - 10) as u32)
                        || ch == '+'
                        || ch == '-'
                });
                // Allow trailing whitespace (strtol behaviour).
                if rest.trim_start().is_empty() {
                    v
                } else {
                    NA_INTEGER
                }
            }
            Err(_) => NA_INTEGER,
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: Strtod
// ---------------------------------------------------------------------------

/// Parse a string as a double, using the given decimal character.
///
/// This is a simplified port of R's `Strtod` / `R_strtod4`.  The full
/// implementation would handle the `decchar` override (e.g. comma as
/// decimal separator) and NaN/Inf variants.
pub unsafe fn Strtod(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    decchar: c_char,
    treat_as_na: bool,
) -> f64 {
    unsafe {
        if nptr.is_null() {
            return f64::NAN;
        }
        let s = match std::ffi::CStr::from_ptr(nptr).to_str() {
            Ok(s) => s,
            Err(_) => {
                if !endptr.is_null() {
                    *endptr = nptr as *mut c_char;
                }
                return 0.0;
            }
        };

        // Replace the decimal character if it differs from '.'.
        let normalized = if decchar != b'.' as c_char && !s.is_empty() {
            s.replace(std::char::from_u32(decchar as u32).unwrap_or('.'), ".")
        } else {
            s.to_owned()
        };

        let trimmed = normalized.trim_start();
        match trimmed.parse::<f64>() {
            Ok(v) => {
                // Compute how many leading bytes were consumed (whitespace + digits).
                let consumed_leading = s.len() - trimmed.len();
                let digit_len = trimmed.trim_end().len();
                if !endptr.is_null() {
                    *endptr = nptr.add(consumed_leading + digit_len) as *mut c_char;
                }
                v
            }
            Err(_) => {
                if treat_as_na {
                    f64::NAN
                } else {
                    if !endptr.is_null() {
                        *endptr = nptr as *mut c_char;
                    }
                    0.0
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: strtoc
// ---------------------------------------------------------------------------

/// Parse a string as a complex number (a+bi, ai, a, or bi).
///
/// Returns the parsed `Rcomplex`.  If the string cannot be parsed, both
/// real and imaginary parts are set to 0.
pub unsafe fn strtoc(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    treat_as_na: bool,
    decchar: c_char,
) -> Rcomplex {
    unsafe {
        if nptr.is_null() {
            return Rcomplex { r: 0.0, i: 0.0 };
        }
        let s = match std::ffi::CStr::from_ptr(nptr).to_str() {
            Ok(s) => s,
            Err(_) => return Rcomplex { r: 0.0, i: 0.0 },
        };
        let trimmed = s.trim_start();

        // Handle pure imaginary: "3i" or "-3i"
        let stripped = trimmed.trim_end_matches('i').trim_end();
        if stripped != trimmed && !stripped.ends_with('e') && !stripped.ends_with('E') {
            // Make sure 'i' is at the very end and not part of exponent notation.
            let i_pos = trimmed.trim_end().len() - 1;
            if trimmed.as_bytes()[i_pos] == b'i'
                && let Ok(val) = stripped.parse::<f64>()
            {
                return Rcomplex { r: 0.0, i: val };
            }
        }

        // Handle "a+bi" or "a-bi"
        if let Some(plus_pos) = find_complex_separator(trimmed) {
            let real_str = trimmed[..plus_pos].trim_end();
            let imag_str = &trimmed[plus_pos + 1..].trim_start();
            let imag_start = if imag_str.starts_with('+') || imag_str.starts_with('-') {
                1
            } else {
                0
            };
            let sign: f64 = if imag_str.starts_with('-') { -1.0 } else { 1.0 };
            let imag_body = &imag_str[imag_start..];
            let imag_stripped = imag_body.trim_end_matches('i').trim_end();
            if let (Ok(r), Ok(i)) = (real_str.parse::<f64>(), imag_stripped.parse::<f64>()) {
                return Rcomplex { r, i: sign * i };
            }
        }

        // Try plain real number.
        if let Ok(x) = trimmed.parse::<f64>() {
            return Rcomplex { r: x, i: 0.0 };
        }

        Rcomplex { r: 0.0, i: 0.0 }
    }
}

/// Find the '+' or '-' separator in a complex literal like "3+4i".
/// Returns the index of the separator, or None.
fn find_complex_separator(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    // Skip leading sign.
    let start = if !bytes.is_empty() && (bytes[0] == b'+' || bytes[0] == b'-') {
        1
    } else {
        0
    };
    for i in start..bytes.len() {
        if bytes[i] == b'+' || bytes[i] == b'-' {
            // Make sure it's not in an exponent (e.g., 1e+10).
            if i > 0 && (bytes[i - 1] == b'e' || bytes[i - 1] == b'E') {
                continue;
            }
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Standalone utility: strtoraw
// ---------------------------------------------------------------------------

/// Parse a two-digit hexadecimal string as a raw byte.
///
/// Skips leading whitespace, then reads exactly two hex digits.
/// Returns 0 on failure.
pub unsafe fn strtoraw(nptr: *const c_char, endptr: *mut *mut c_char) -> std::os::raw::c_uchar {
    unsafe {
        if nptr.is_null() {
            return 0;
        }
        let s = match std::ffi::CStr::from_ptr(nptr).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let mut p = s.trim_start();
        let mut val: u8 = 0;
        let mut consumed = s.len() - p.len();
        for _ in 0..2 {
            if p.is_empty() {
                break;
            }
            let c = p.as_bytes()[0];
            let digit = if c >= b'0' && c <= b'9' {
                c - b'0'
            } else if c >= b'A' && c <= b'F' {
                c - b'A' + 10
            } else if c >= b'a' && c <= b'f' {
                c - b'a' + 10
            } else {
                break;
            };
            val = val.wrapping_mul(16).wrapping_add(digit);
            p = &p[1..];
            consumed += 1;
        }
        if !endptr.is_null() {
            *endptr = nptr.add(consumed) as *mut c_char;
        }
        val
    }
}

// ---------------------------------------------------------------------------
// Stub: do_scan (.Internal(scan ...))
// ---------------------------------------------------------------------------

/// Stub for `do_scan` -- the `.Internal(scan ...)` entry point.
///
/// In the full R implementation this reads data from a connection, parsing
/// fields according to `what`, `sep`, `dec`, quote/comment rules, etc.
/// It depends on R connections, SEXP allocation, and many internal helpers.
pub unsafe fn do_scan(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    std::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Stub: do_readln (readline())
// ---------------------------------------------------------------------------

/// Stub for `do_readln` -- the `readline()` entry point.
///
/// In the full R implementation this reads a line from the interactive
/// console, stripping leading and trailing whitespace.  It depends on
/// `R_ReadConsole`, `R_Interactive`, and SEXP string creation.
pub unsafe fn do_readln(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    std::ptr::null_mut()
}
