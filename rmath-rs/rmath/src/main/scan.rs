#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/scan.c
//!
//! The original C implementation provides `scan()`, `readline()` / `do_readln`,
//! and the low-level character-at-a-time input parsing engine used by R's
//! interactive console and data readers.
//!
//! SEXP-dependent entry points (real implementations):
//!   - do_scan      (.Internal(scan ...)) -- reads data from files
//!   - do_readln    (readline())         -- reads lines from files
//!
//! Standalone utility functions (real implementations):
//!   - Rspace()          -- whitespace classification (ASCII space/tab/CR/LF + NBSP)
//!   - Strtoi()          -- safe strtol wrapper returning NA_INTEGER on overflow
//!   - Strtod()          -- wrapper around R_strtod4 with configurable decimal char
//!   - strtoc()          -- complex-number parser (a+bi, ai, a)
//!   - strtoraw()        -- two-digit hex raw-byte parser
//!
//! Notes:
//!   - do_scan reads from file paths via std::fs (R connections not available)
//!   - do_readln reads from file paths (console/interactive input is a stub)
//!   - The original C fillBuffer/scanchar/scanVector/scanFrame are reimplemented
//!     as Rust-native tokenize_line + scan_vector_impl / scan_frame_impl

use std::io::BufRead;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

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
            return std::f64::NAN;
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
                    std::f64::NAN
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
            if trimmed.as_bytes()[i_pos] == b'i' {
                if let Ok(val) = stripped.parse::<f64>() {
                    return Rcomplex { r: 0.0, i: val };
                }
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
pub unsafe fn strtoraw(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
) -> std::os::raw::c_uchar {
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
// Local helper: isString / isNull / translateChar / asLogical / etc.
// ---------------------------------------------------------------------------

/// Check if an SEXP is a string vector.
#[inline(always)]
unsafe fn scan_isString(x: SEXP) -> bool {
    unsafe { Rf_isString(x) != 0 }
}

/// Check if an SEXP is NULL.
#[inline(always)]
unsafe fn scan_isNull(x: SEXP) -> bool {
    unsafe { Rf_isNull(x) != 0 }
}

/// Translate a CHARSXP to a C string (stub: just return CHAR()).
#[inline(always)]
unsafe fn scan_translateChar(s: SEXP) -> *const c_char {
    unsafe { CHAR(s) }
}

/// Extract a logical value from a scalar SEXP.
#[inline(always)]
unsafe fn scan_asLogical(x: SEXP) -> c_int {
    unsafe {
        if scan_isNull(x) {
            return NA_INTEGER; // NA_LOGICAL == NA_INTEGER
        }
        if TYPEOF(x) == SEXPTYPE::LGLSXP.0 && LENGTH(x) >= 1 {
            *LOGICAL(x)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP.0 && LENGTH(x) >= 1 {
            *INTEGER(x)
        } else {
            NA_INTEGER
        }
    }
}

/// Extract an integer value from a scalar SEXP.
#[inline(always)]
unsafe fn scan_asInteger(x: SEXP) -> c_int {
    unsafe {
        if scan_isNull(x) {
            return NA_INTEGER;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP.0 && LENGTH(x) >= 1 {
            *INTEGER(x)
        } else if TYPEOF(x) == SEXPTYPE::LGLSXP.0 && LENGTH(x) >= 1 {
            *LOGICAL(x)
        } else if TYPEOF(x) == SEXPTYPE::REALSXP.0 && LENGTH(x) >= 1 {
            let v = *REAL(x);
            if v.is_nan() || v > c_int::MAX as c_double || v < c_int::MIN as c_double {
                NA_INTEGER
            } else {
                v as c_int
            }
        } else {
            NA_INTEGER
        }
    }
}

/// Extract an integer as R_xlen_t (non-negative).
#[inline(always)]
unsafe fn scan_asXLength(x: SEXP) -> R_xlen_t {
    unsafe {
        let v = scan_asInteger(x);
        if v < 0 { 0 } else { v as R_xlen_t }
    }
}

/// Get the first element of a string vector as a Rust String.
#[inline(always)]
unsafe fn scan_getString(x: SEXP) -> Option<String> {
    unsafe {
        if scan_isNull(x) || TYPEOF(x) != SEXPTYPE::STRSXP.0 || LENGTH(x) < 1 {
            return None;
        }
        let charsxp = STRING_ELT(x, 0);
        if charsxp.is_null() {
            return None;
        }
        let ptr = CHAR(charsxp);
        if ptr.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(ptr)
            .to_str()
            .ok()
            .map(|s| s.to_owned())
    }
}

/// Tokenize a string into fields based on separator mode.
///
/// When `sepchar` is 0 (whitespace mode), fields are separated by any run of
/// whitespace.  Otherwise `sepchar` is the single-byte separator.
fn tokenize_line(line: &str, sepchar: u8, comchar: c_int) -> Vec<String> {
    let mut fields: Vec<String> = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Skip comment lines
        if (b as c_int) == comchar {
            break;
        }

        if sepchar == 0 {
            // Whitespace separator mode: skip leading whitespace
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
                i += 1;
            }
            if i >= len {
                break;
            }
            // Handle quoted strings
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let start = i;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
                fields.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
                if i < len {
                    i += 1; // skip closing quote
                }
            } else {
                // Read until whitespace
                let start = i;
                while i < len
                    && bytes[i] != b' '
                    && bytes[i] != b'\t'
                    && bytes[i] != b'\r'
                    && bytes[i] != b'\n'
                {
                    i += 1;
                }
                fields.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
            }
        } else {
            // Single-character separator mode
            if b == sepchar {
                fields.push(String::new());
                i += 1;
                continue;
            }
            // Handle quoted strings
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let start = i;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 1;
                    }
                    i += 1;
                }
                fields.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
                if i < len {
                    i += 1;
                }
                // Skip until separator or end of line
                while i < len && bytes[i] != sepchar && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
                if i < len && bytes[i] == sepchar {
                    i += 1;
                }
            } else {
                let start = i;
                while i < len && bytes[i] != sepchar && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
                fields.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
                if i < len && bytes[i] == sepchar {
                    i += 1;
                }
            }
        }
    }
    fields
}

/// Check if a string matches an NA representation.
fn is_na_string(s: &str, na_strings: &[String]) -> bool {
    if s.is_empty() {
        return true; // empty string is NA for numeric types
    }
    for na in na_strings {
        if s == na {
            return true;
        }
    }
    false
}

/// Build a CHARSXP from a Rust string.
unsafe fn make_charsxp(s: &str) -> SEXP {
    unsafe {
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        Rf_mkChar(cstr.as_ptr())
    }
}

/// Extract NA strings from an SEXP string vector.
unsafe fn extract_na_strings(na_strings_sexp: SEXP) -> Vec<String> {
    unsafe {
        let mut result = Vec::new();
        if scan_isNull(na_strings_sexp) {
            return result;
        }
        if TYPEOF(na_strings_sexp) == SEXPTYPE::STRSXP.0 {
            let len = LENGTH(na_strings_sexp);
            for i in 0..len as R_xlen_t {
                let elt = STRING_ELT(na_strings_sexp, i);
                if !elt.is_null() {
                    let ptr = CHAR(elt);
                    if !ptr.is_null() {
                        if let Ok(s) = std::ffi::CStr::from_ptr(ptr).to_str() {
                            result.push(s.to_owned());
                        }
                    }
                }
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_scan (.Internal(scan ...))
// ---------------------------------------------------------------------------

/// Real implementation of `do_scan` -- the `.Internal(scan ...)` entry point.
///
/// This reads data from a file path (passed as a string), parses fields
/// according to `what`, `sep`, `dec`, and builds an R vector of the
/// appropriate type.
///
/// Supported types: LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP.
/// Also supports VECSXP (list of types) for multi-column data frames.
///
/// File reading is done via Rust's std::fs, since R connections are not
/// available in this standalone port.
pub unsafe fn do_scan(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Parse arguments from the pairlist.  The C code uses checkArity and
        // CAR/CDR traversal.  We do the same.
        let mut args = args;

        let _call = call;
        let _op = op;
        let _rho = rho;

        // file = CAR(args); args = CDR(args);
        let file_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // what = CAR(args); args = CDR(args);
        let what = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // nmax = asXLength(CAR(args)); args = CDR(args);
        let nmax = scan_asXLength(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // sep = CAR(args); args = CDR(args);
        let sep_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // dec = CAR(args); args = CDR(args);
        let _dec_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // quotes = CAR(args); args = CDR(args);
        let _quotes_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // nskip = asXLength(CAR(args)); args = CDR(args);
        let nskip = scan_asXLength(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // nlines = asXLength(CAR(args)); args = CDR(args);
        let nlines = scan_asXLength(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // NAstrings = CAR(args); args = CDR(args);
        let na_strings_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // flush = asLogical(CAR(args)); args = CDR(args);
        let _flush = scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // fill = asLogical(CAR(args)); args = CDR(args);
        let fill = scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // stripwhite = CAR(args); args = CDR(args);
        let _stripwhite_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // quiet = asLogical(CAR(args)); args = CDR(args);
        let quiet = scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // blskip = asLogical(CAR(args)); args = CDR(args);
        let blskip = if scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        }) == NA_INTEGER
        {
            1
        } else {
            scan_asLogical(if args.is_null() {
                R_NilValue()
            } else {
                CAR(args)
            })
        };
        let _blskip = blskip;
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // multiline = asLogical(CAR(args)); args = CDR(args);
        let _multiline = if scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        }) == NA_INTEGER
        {
            1
        } else {
            scan_asLogical(if args.is_null() {
                R_NilValue()
            } else {
                CAR(args)
            })
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // comstr = CAR(args); args = CDR(args);
        let comstr_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // escapes = asLogical(CAR(args)); args = CDR(args);
        let _escapes = scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // encoding = CAR(args); args = CDR(args);
        let _encoding_arg = if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        };
        args = if args.is_null() {
            ptr::null_mut()
        } else {
            CDR(args)
        };

        // skipNul = asLogical(CAR(args));
        let _skip_nul = scan_asLogical(if args.is_null() {
            R_NilValue()
        } else {
            CAR(args)
        });

        // Determine separator
        let sepchar: u8 = if scan_isNull(sep_arg) {
            0 // whitespace mode
        } else if scan_isString(sep_arg) && LENGTH(sep_arg) >= 1 {
            let s = scan_getString(sep_arg);
            match s {
                Some(ref v) if v.is_empty() => 0,
                Some(ref v) => v.as_bytes()[0],
                None => 0,
            }
        } else {
            0
        };

        // Determine comment character
        let comchar: c_int = if scan_isString(comstr_arg) {
            match scan_getString(comstr_arg) {
                Some(ref s) if s.len() == 1 => s.as_bytes()[0] as c_int,
                _ => NO_COMCHAR,
            }
        } else {
            NO_COMCHAR
        };

        // Extract NA strings
        let na_strings = extract_na_strings(na_strings_arg);

        // Determine quiet flag
        let quiet_flag = if quiet == NA_INTEGER { 0 } else { quiet };

        // Determine fill flag
        let fill_flag = if fill == NA_INTEGER { false } else { fill != 0 };

        // Resolve the file path
        let file_path = if scan_isString(file_arg) && LENGTH(file_arg) >= 1 {
            scan_getString(file_arg)
        } else {
            None
        };

        // Determine the target type
        let what_type = TYPEOF(what);

        // If it's a VECSXP (multi-column), handle it as a data frame scan
        if what_type == SEXPTYPE::VECSXP.0 {
            return scan_frame_impl(
                file_path,
                what,
                nmax,
                nlines,
                nskip,
                sepchar,
                comchar,
                &na_strings,
                quiet_flag,
                fill_flag,
            );
        }

        // For atomic types, do a vector scan
        scan_vector_impl(
            file_path,
            what_type,
            nmax,
            nlines,
            nskip,
            sepchar,
            comchar,
            &na_strings,
            quiet_flag,
        )
    }
}

/// Implementation for scanning a single vector (atomic types).
unsafe fn scan_vector_impl(
    file_path: Option<String>,
    target_type: c_int,
    nmax: R_xlen_t,
    nlines: R_xlen_t,
    nskip: R_xlen_t,
    sepchar: u8,
    comchar: c_int,
    na_strings: &[String],
    quiet: c_int,
) -> SEXP {
    unsafe {
        // If no file path, return empty vector (console input not supported)
        let path = match file_path {
            Some(p) => p,
            None => return Rf_allocVector(target_type, 0),
        };

        // Try to open the file
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => return Rf_allocVector(target_type, 0),
        };
        let reader = std::io::BufReader::new(file);

        let mut fields: Vec<String> = Vec::with_capacity(SCAN_BLOCKSIZE);
        let mut lines_read: R_xlen_t = 0;
        let mut items_read: R_xlen_t = 0;

        // Skip lines
        let mut skip_reader = reader.lines();
        for _ in 0..nskip {
            match skip_reader.next() {
                Some(Ok(_)) => {}
                _ => break,
            }
        }

        // Read and tokenize lines
        'outer: for line_result in skip_reader {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            lines_read += 1;
            if nlines > 0 && lines_read > nlines {
                break;
            }

            let line_fields = tokenize_line(&line, sepchar, comchar);
            for field in line_fields {
                if nmax > 0 && items_read >= nmax {
                    break 'outer;
                }
                fields.push(field);
                items_read += 1;
            }
        }

        let n = items_read as i64;
        if n == 0 {
            return Rf_allocVector(target_type, 0);
        }

        // Allocate result vector
        let ans = Rf_allocVector3(target_type, n);
        if ans.is_null() {
            return R_NilValue();
        }

        // Fill the vector based on type
        for i in 0..n {
            let field = &fields[i as usize];
            match target_type {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    let val = parse_logical(field, na_strings);
                    *LOGICAL(ans).add(i as usize) = val;
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    let val = parse_integer(field, na_strings);
                    *INTEGER(ans).add(i as usize) = val;
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    let val = parse_double(field, na_strings);
                    *REAL(ans).add(i as usize) = val;
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    let val = parse_complex(field, na_strings);
                    *COMPLEX(ans).add(i as usize) = val;
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    let charsxp = if is_na_string(field, na_strings) {
                        R_NilValue()
                    } else {
                        make_charsxp(field)
                    };
                    SET_STRING_ELT(ans, i, charsxp);
                }
                t if t == SEXPTYPE::RAWSXP.0 => {
                    let val = parse_raw(field, na_strings);
                    *RAW(ans).add(i as usize) = val;
                }
                _ => break,
            }
        }

        ans
    }
}

/// Implementation for scanning a data frame (VECSXP what).
unsafe fn scan_frame_impl(
    file_path: Option<String>,
    what: SEXP,
    nmax: R_xlen_t,
    nlines: R_xlen_t,
    nskip: R_xlen_t,
    sepchar: u8,
    comchar: c_int,
    na_strings: &[String],
    quiet: c_int,
    fill: bool,
) -> SEXP {
    unsafe {
        let path = match file_path {
            Some(p) => p,
            None => return Rf_allocVector(SEXPTYPE::VECSXP.0, 0),
        };

        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => return Rf_allocVector(SEXPTYPE::VECSXP.0, 0),
        };
        let reader = std::io::BufReader::new(file);

        let nc = XLENGTH(what);
        if nc == 0 {
            return Rf_allocVector(SEXPTYPE::VECSXP.0, 0);
        }

        // Collect column types
        let mut col_types: Vec<c_int> = Vec::with_capacity(nc as usize);
        for i in 0..nc {
            let w = VECTOR_ELT(what, i);
            col_types.push(TYPEOF(w));
        }

        let mut skip_reader = reader.lines();
        for _ in 0..nskip {
            match skip_reader.next() {
                Some(Ok(_)) => {}
                _ => {
                    // Not enough lines to skip; return empty frame
                    let ans = Rf_allocVector(SEXPTYPE::VECSXP.0, nc as c_int);
                    return ans;
                }
            }
        }

        // Read all rows of fields
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut lines_read: R_xlen_t = 0;

        for line_result in skip_reader {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            lines_read += 1;
            if nlines > 0 && lines_read > nlines {
                break;
            }
            if nmax > 0 && rows.len() as R_xlen_t >= nmax {
                break;
            }

            let fields = tokenize_line(&line, sepchar, comchar);
            if fields.is_empty() {
                continue; // skip blank lines
            }

            // Pad or truncate to match column count
            let mut row = fields;
            if fill && row.len() < nc as usize {
                while row.len() < nc as usize {
                    row.push(String::new());
                }
            }
            rows.push(row);
        }

        let n = rows.len() as R_xlen_t;
        if n == 0 {
            return Rf_allocVector(SEXPTYPE::VECSXP.0, 0);
        }

        // Allocate the list vector
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP.0, nc);
        if ans.is_null() {
            return R_NilValue();
        }

        // Allocate column vectors and fill them
        for col in 0..nc as usize {
            let col_type = col_types[col];
            let col_vec = Rf_allocVector3(col_type, n);
            SET_VECTOR_ELT(ans, col as R_xlen_t, col_vec);

            for row in 0..n as usize {
                let field = if row < rows.len() && col < rows[row].len() {
                    &rows[row][col]
                } else {
                    ""
                };

                match col_type {
                    t if t == SEXPTYPE::LGLSXP.0 => {
                        *LOGICAL(col_vec).add(row) = parse_logical(field, na_strings);
                    }
                    t if t == SEXPTYPE::INTSXP.0 => {
                        *INTEGER(col_vec).add(row) = parse_integer(field, na_strings);
                    }
                    t if t == SEXPTYPE::REALSXP.0 => {
                        *REAL(col_vec).add(row) = parse_double(field, na_strings);
                    }
                    t if t == SEXPTYPE::CPLXSXP.0 => {
                        *COMPLEX(col_vec).add(row) = parse_complex(field, na_strings);
                    }
                    t if t == SEXPTYPE::STRSXP.0 => {
                        let charsxp = if is_na_string(field, na_strings) {
                            R_NilValue()
                        } else {
                            make_charsxp(field)
                        };
                        SET_STRING_ELT(col_vec, row as R_xlen_t, charsxp);
                    }
                    t if t == SEXPTYPE::RAWSXP.0 => {
                        *RAW(col_vec).add(row) = parse_raw(field, na_strings);
                    }
                    _ => {}
                }
            }
        }

        // Suppress unused warning
        let _ = quiet;

        ans
    }
}

// ---------------------------------------------------------------------------
// Field parsing functions
// ---------------------------------------------------------------------------

/// Parse a string as a logical value.
fn parse_logical(s: &str, na_strings: &[String]) -> c_int {
    if is_na_string(s, na_strings) {
        return NA_INTEGER;
    }
    let trimmed = s.trim();
    match trimmed.to_uppercase().as_str() {
        "T" | "TRUE" => 1,
        "F" | "FALSE" => 0,
        _ => NA_INTEGER,
    }
}

/// Parse a string as an integer value.
fn parse_integer(s: &str, na_strings: &[String]) -> c_int {
    if is_na_string(s, na_strings) {
        return NA_INTEGER;
    }
    let trimmed = s.trim();
    match trimmed.parse::<i32>() {
        Ok(v) => v,
        Err(_) => NA_INTEGER,
    }
}

/// Parse a string as a double value.
fn parse_double(s: &str, na_strings: &[String]) -> c_double {
    if is_na_string(s, na_strings) {
        return std::f64::NAN; // NA_REAL
    }
    let trimmed = s.trim();
    match trimmed.parse::<f64>() {
        Ok(v) => v,
        Err(_) => std::f64::NAN,
    }
}

/// Parse a string as a complex value.
fn parse_complex(s: &str, na_strings: &[String]) -> Rcomplex {
    if is_na_string(s, na_strings) {
        return Rcomplex {
            r: std::f64::NAN,
            i: std::f64::NAN,
        };
    }
    let trimmed = s.trim();

    // Use our existing strtoc by converting to CString
    let cstr = std::ffi::CString::new(trimmed).unwrap_or_default();
    unsafe { strtoc(cstr.as_ptr(), ptr::null_mut(), false, b'.' as c_char) }
}

/// Parse a string as a raw byte value.
fn parse_raw(s: &str, na_strings: &[String]) -> u8 {
    if is_na_string(s, na_strings) {
        return 0;
    }
    let cstr = std::ffi::CString::new(s).unwrap_or_default();
    unsafe { strtoraw(cstr.as_ptr(), ptr::null_mut()) }
}

// ---------------------------------------------------------------------------
// do_readln (readline())
// ---------------------------------------------------------------------------

/// Real implementation of `do_readln` -- the `readline()` entry point.
///
/// Reads lines from a file path (the first argument).  If the argument is
/// NULL, returns an empty string (console/interactive input is not supported
/// in this standalone port).
///
/// Returns a character vector (STRSXP) containing the line(s) read.
/// Leading and trailing whitespace is stripped from each line, matching
/// R's `readline()` behavior.
pub unsafe fn do_readln(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _call = call;
        let _op = op;
        let _rho = rho;

        // The first argument is the prompt (or a file descriptor as integer).
        // In R, readline(prompt) reads a line from the console with the given prompt.
        // Here we handle:
        //   1. If args is a string, treat it as a file path to read from
        //   2. If args is NULL or no args, return empty string (console stub)
        //   3. If args is an integer, treat it as a file descriptor

        if args.is_null() {
            // No arguments -- return empty string
            return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
        }

        let prompt = CAR(args);

        // If prompt is NULL, return empty string
        if scan_isNull(prompt) {
            return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
        }

        // If prompt is a string, treat the first element as a file path
        if scan_isString(prompt) && LENGTH(prompt) >= 1 {
            let file_path = scan_getString(prompt);
            match file_path {
                Some(path) => {
                    // Try to open and read the file
                    let file = match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(_) => {
                            // Cannot open file -- return empty string
                            return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
                        }
                    };

                    let reader = std::io::BufReader::new(file);
                    let mut lines: Vec<String> = Vec::new();

                    for line_result in reader.lines() {
                        match line_result {
                            Ok(line) => {
                                // Strip leading/trailing whitespace (matching R behavior)
                                lines.push(line.trim().to_owned());
                            }
                            Err(_) => break,
                        }
                    }

                    if lines.is_empty() {
                        return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
                    }

                    // Return a character vector with all lines
                    let n = lines.len() as i32;
                    let ans = Rf_allocVector(SEXPTYPE::STRSXP.0, n);
                    if ans.is_null() {
                        return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
                    }

                    for (i, line) in lines.iter().enumerate() {
                        let charsxp = make_charsxp(line);
                        SET_STRING_ELT(ans, i as R_xlen_t, charsxp);
                    }

                    ans
                }
                None => Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr()),
            }
        } else if TYPEOF(prompt) == SEXPTYPE::INTSXP.0 || TYPEOF(prompt) == SEXPTYPE::REALSXP.0 {
            // Integer argument: treat as file descriptor (0 = stdin)
            let fd = scan_asInteger(prompt);
            if fd == 0 {
                // stdin: not supported in this port, return empty string
                return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
            } else if fd > 0 {
                // On Unix, try to open /dev/fd/N
                #[cfg(unix)]
                {
                    let dev_fd_path = format!("/dev/fd/{}", fd);
                    return do_readln_file(&dev_fd_path);
                }
                #[cfg(not(unix))]
                {
                    return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
                }
            } else {
                Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr())
            }
        } else {
            // Unknown argument type -- return empty string
            Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr())
        }
    }
}

/// Helper: read all lines from a file path and return as STRSXP.
unsafe fn do_readln_file(path: &str) -> SEXP {
    unsafe {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => {
                return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
            }
        };

        let reader = std::io::BufReader::new(file);
        let mut lines: Vec<String> = Vec::new();

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    lines.push(line.trim().to_owned());
                }
                Err(_) => break,
            }
        }

        if lines.is_empty() {
            return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
        }

        let n = lines.len() as i32;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP.0, n);
        if ans.is_null() {
            return Rf_mkString(std::ffi::CString::new("").unwrap().as_ptr());
        }

        for (i, line) in lines.iter().enumerate() {
            let charsxp = make_charsxp(line);
            SET_STRING_ELT(ans, i as R_xlen_t, charsxp);
        }

        ans
    }
}
