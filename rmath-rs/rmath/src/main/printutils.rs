#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/printutils.c
//!
//! Printing utilities: encoding R values into printable string representations,
//! and R's Rprintf/REprintf output routines.
//!
//! Fully ported standalone functions:
//!   R_Decode2Long, EncodeLogical, EncodeInteger, EncodeReal0, EncodeReal,
//!   EncodeRealDrop0, EncodeReal2, EncodeComplex, EncodeRaw, Rstrwid,
//!   IndexWidth
//!
//! Fully ported SEXP-dependent functions:
//!   EncodeEnvironment, EncodeExtptr, StringFromReal, Rstrlen,
//!   EncodeString, EncodeElement, EncodeElement0, EncodeChar,
//!   Rprintf, Rvprintf, REvprintf, REvprintf_internal,
//!   Rcons_vprintf, VectorIndex

use std::ffi::{CStr, CString};
use std::io::Write as IoWrite;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::LazyLock;
use std::sync::Mutex;

use crate::sexp::accessors::{
    CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, RAW, REAL, STRING_ELT, TYPEOF,
};
use crate::sexp::constructors::Rf_mkChar;
use crate::sexp::ffi::{
    NA_INTEGER, R_NA_BIT_PATTERN, R_size_t, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE,
};
use crate::sexp::globals::R_NilValue;

use crate::main::format::{formatComplex, formatInteger, formatLogical, formatReal, formatString};

// ---------------------------------------------------------------------------
// R_print global state
//
// These mirror the R_print structure used by the encode functions for NA
// string representation and width.
// ---------------------------------------------------------------------------

/// Mirrors R's `R_print` structure from Print.h (fields used by printutils.c).
#[derive(Clone, Copy, Debug)]
pub struct RPrint {
    pub na_string: *const c_char,
    pub na_string_noquote: *const c_char,
    pub na_width: c_int,
    pub na_width_noquote: c_int,
    pub gap: c_int,
}

impl Default for RPrint {
    fn default() -> Self {
        RPrint {
            na_string: ptr::null(),
            na_string_noquote: ptr::null(),
            na_width: 2,
            na_width_noquote: 2,
            gap: 1,
        }
    }
}

/// Thread-local print configuration.
static mut R_PRINT: RPrint = RPrint {
    na_string: ptr::null(),
    na_string_noquote: ptr::null(),
    na_width: 2,
    na_width_noquote: 2,
    gap: 1,
};

/// Return the current R_print configuration.
pub unsafe fn get_R_print() -> &'static RPrint {
    unsafe { &*std::ptr::addr_of!(R_PRINT) }
}

/// Set the R_print configuration.
pub unsafe fn set_R_print(rp: RPrint) {
    unsafe {
        std::ptr::addr_of_mut!(R_PRINT).write(rp);
    }
}

/// Helper: return the NA string from R_print, falling back to "NA".
unsafe fn na_string_str() -> &'static str {
    unsafe {
        let rp = std::ptr::addr_of_mut!(R_PRINT);
        let na = (*rp).na_string;
        if !na.is_null() {
            let p = CHAR(na as SEXP);
            if !p.is_null() {
                let s = CStr::from_ptr(p);
                if let Ok(s) = s.to_str() {
                    return s;
                }
            }
        }
        "NA"
    }
}

/// Helper: return the no-quote NA string from R_print, falling back to "NA".
unsafe fn na_string_noquote_str() -> &'static str {
    unsafe {
        let rp = std::ptr::addr_of_mut!(R_PRINT);
        let na = (*rp).na_string_noquote;
        if !na.is_null() {
            let p = CHAR(na as SEXP);
            if !p.is_null() {
                let s = CStr::from_ptr(p);
                if let Ok(s) = s.to_str() {
                    return s;
                }
            }
        }
        "NA"
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const NB: usize = 1000;
const BUFSIZE: usize = 8192;

/// R's NA_LOGICAL sentinel value.
pub const NA_LOGICAL: c_int = c_int::MIN;

// ---------------------------------------------------------------------------
// Standalone utility: R_Decode2Long
// ---------------------------------------------------------------------------

/// Decode a string with an optional size suffix (G/M/K/k) to a `R_size_t`.
///
/// Suffixes:
///   G -> * 1,073,741,824  (Giga, 2^30)
///   M -> * 1,048,576      (Mega, 2^20)
///   K -> * 1,024           (binary kilo)
///   k -> * 1,000           (decimal kilo)
///
/// Returns the decoded value.  `ierr` is set to:
///   0  = success (no suffix, or suffix applied)
///   1  = overflow with M suffix
///   2  = overflow with K suffix
///   3  = overflow with k suffix
///   4  = overflow with G suffix
///  -1  = unrecognized suffix
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Decode2Long(p: *mut c_char, ierr: *mut c_int) -> R_size_t {
    unsafe {
        let bytes = CStr::from_ptr(p).to_bytes();
        let s = std::str::from_utf8_unchecked(bytes);

        // Parse leading integer
        let mut end = 0usize;
        let mut v: i64 = 0;
        for (i, ch) in s.as_bytes().iter().enumerate() {
            if ch.is_ascii_digit() {
                v = v * 10 + (*ch - b'0') as i64;
                end = i + 1;
            } else {
                break;
            }
        }
        *ierr = 0;

        if end >= s.len() {
            return v as R_size_t;
        }

        let suffix = s.as_bytes()[end] as char;
        let R_SIZE_T_MAX: u64 = R_size_t::MAX as u64;

        match suffix {
            'G' => {
                let giga: i64 = 1_073_741_824;
                if (giga as f64 * v as f64) > R_SIZE_T_MAX as f64 {
                    *ierr = 4;
                    return v as R_size_t;
                }
                (giga * v) as R_size_t
            }
            'M' => {
                let mega: i64 = 1_048_576;
                if (mega as f64 * v as f64) > R_SIZE_T_MAX as f64 {
                    *ierr = 1;
                    return v as R_size_t;
                }
                (mega * v) as R_size_t
            }
            'K' => {
                let kibi: i64 = 1024;
                if (kibi as f64 * v as f64) > R_SIZE_T_MAX as f64 {
                    *ierr = 2;
                    return v as R_size_t;
                }
                (kibi * v) as R_size_t
            }
            'k' => {
                let kilo: i64 = 1000;
                if (kilo as f64 * v as f64) > R_SIZE_T_MAX as f64 {
                    *ierr = 3;
                    return v as R_size_t;
                }
                (kilo * v) as R_size_t
            }
            _ => {
                *ierr = -1;
                v as R_size_t
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeLogical
// ---------------------------------------------------------------------------

/// Encode a logical value for printing.
///
/// Returns a static string.  `x` is the logical value (NA_LOGICAL for NA),
/// `w` is the minimum field width.
pub unsafe fn EncodeLogical(x: c_int, w: c_int) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; NB]>> = LazyLock::new(|| Mutex::new([0u8; NB]));

        let na = na_string_str();

        // Fast path: exact-width matches
        if x == NA_LOGICAL {
            if w == R_PRINT.na_width {
                return na.as_ptr() as *const c_char;
            }
        } else if x != 0 {
            if w == 4 {
                return b"TRUE\0".as_ptr() as *const c_char;
            }
        } else {
            if w == 5 {
                return b"FALSE\0".as_ptr() as *const c_char;
            }
        }

        let val = if x == NA_LOGICAL {
            na
        } else if x != 0 {
            "TRUE"
        } else {
            "FALSE"
        };

        let width = w as usize;
        let mw = if width < NB - 1 { width } else { NB - 1 };

        let mut buf = BUF.lock().unwrap();
        // Right-justify into buffer
        let val_bytes = val.as_bytes();
        let val_len = val_bytes.len().min(mw);
        buf.fill(b' ');
        buf[mw..].copy_from_slice(&[0u8; 1]); // zero the tail
        let start = mw - val_len;
        buf[start..mw].copy_from_slice(&val_bytes[..val_len]);
        buf[mw] = 0; // null-terminate

        buf.as_ptr() as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeInteger
// ---------------------------------------------------------------------------

/// Encode an integer value for printing.
///
/// Returns a static string.  `x` is the integer value (NA_INTEGER for NA),
/// `w` is the minimum field width.
pub unsafe fn EncodeInteger(x: c_int, w: c_int) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; NB]>> = LazyLock::new(|| Mutex::new([0u8; NB]));

        let mut buf = BUF.lock().unwrap();

        if x == NA_INTEGER {
            let na = na_string_str();
            let width = w as usize;
            let mw = if width < NB - 1 { width } else { NB - 1 };
            let val_len = na.len().min(mw);
            let start = mw - val_len;
            buf[..mw].fill(b' ');
            buf[start..mw].copy_from_slice(&na.as_bytes()[..val_len]);
            buf[mw] = 0;
        } else {
            let s = format!("{}", x);
            let width = w as usize;
            let mw = if width < NB - 1 { width } else { NB - 1 };
            let val_len = s.len().min(mw);
            let start = mw - val_len;
            buf[..mw].fill(b' ');
            buf[start..mw].copy_from_slice(&s.as_bytes()[..val_len]);
            buf[mw] = 0;
        }

        buf.as_ptr() as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Helper: format_number_fixed
// ---------------------------------------------------------------------------

/// Helper: format a float with fixed-point notation and given precision.
/// Avoids Rust format! precision syntax issues with named args.
fn format_number_fixed(x: f64, prec: usize) -> String {
    match prec {
        0 => format!("{}", x as i64),
        1 => format!("{:.1}", x),
        2 => format!("{:.2}", x),
        3 => format!("{:.3}", x),
        4 => format!("{:.4}", x),
        5 => format!("{:.5}", x),
        6 => format!("{:.6}", x),
        7 => format!("{:.7}", x),
        8 => format!("{:.8}", x),
        9 => format!("{:.9}", x),
        10 => format!("{:.10}", x),
        11 => format!("{:.11}", x),
        12 => format!("{:.12}", x),
        13 => format!("{:.13}", x),
        14 => format!("{:.14}", x),
        15 => format!("{:.15}", x),
        _ => format!("{:.15}", x),
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeReal0
// ---------------------------------------------------------------------------

/// Encode a real (double) value for printing.
///
/// `x` is the value, `w` is the total width, `d` is the number of digits
/// after the decimal, `e` is non-zero to use scientific notation.
/// `dec` is the decimal separator string (typically ".").
///
/// Returns a pointer to a static buffer.
pub unsafe fn EncodeReal0(
    x: f64,
    w: c_int,
    d: c_int,
    e: c_int,
    dec: *const c_char,
) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; 2 * NB]>> = LazyLock::new(|| Mutex::new([0u8; 2 * NB]));

        let mut buf = BUF.lock().unwrap();
        let dec_str = if dec.is_null() {
            "."
        } else {
            match CStr::from_ptr(dec).to_str() {
                Ok(s) => s,
                Err(_) => ".",
            }
        };

        // IEEE: normalize signed zero
        let x = if x == 0.0 { 0.0 } else { x };

        let na = na_string_str();
        let mw = if (w as usize) < NB - 1 {
            w as usize
        } else {
            NB - 1
        };

        let formatted = if !x.is_finite() {
            if x.is_nan() {
                // Check for NA: R uses a specific bit pattern
                let bits = x.to_bits();
                if (bits & 0x0007fffffffffffffu64) == 0x0001954u64 && (bits >> 52) == 0x7ffu64 {
                    format!("{:width$}", na, width = mw)
                } else {
                    format!("{:width$}", "NaN", width = mw)
                }
            } else if x > 0.0 {
                format!("{:width$}", "Inf", width = mw)
            } else {
                format!("{:width$}", "-Inf", width = mw)
            }
        } else if e != 0 {
            if d != 0 {
                format!("{:+.prec$e}", x, prec = d as usize)
            } else {
                format!("{:.prec$e}", x, prec = d as usize)
            }
        } else {
            format_number_fixed(x, d as usize)
        };

        // Copy into buf, replacing "." with dec if needed
        let out = if dec_str != "." {
            let mut idx = 0usize;
            for ch in formatted.chars() {
                if ch == '.' {
                    for dc in dec_str.chars() {
                        if idx < 2 * NB - 1 {
                            buf[idx] = dc as u8;
                            idx += 1;
                        }
                    }
                } else {
                    if idx < 2 * NB - 1 {
                        buf[idx] = ch as u8;
                        idx += 1;
                    }
                }
            }
            buf[idx] = 0;
            buf.as_ptr()
        } else {
            let bytes = formatted.as_bytes();
            let len = bytes.len().min(NB - 1);
            buf[..len].copy_from_slice(&bytes[..len]);
            buf[len] = 0;
            buf.as_ptr()
        };

        out as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeReal
// ---------------------------------------------------------------------------

/// Encode a real value for printing (single-char decimal separator variant).
pub unsafe fn EncodeReal(
    x: f64,
    w: c_int,
    d: c_int,
    e: c_int,
    cdec: c_char,
) -> *const c_char {
    unsafe {
        let dec_buf = [cdec as u8, 0u8];
        EncodeReal0(x, w, d, e, dec_buf.as_ptr() as *const c_char)
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeRealDrop0
// ---------------------------------------------------------------------------

/// Encode a real value, dropping trailing zeros after the decimal point.
///
/// Otherwise behaves identically to `EncodeReal0`.
pub unsafe fn EncodeRealDrop0(
    x: f64,
    w: c_int,
    d: c_int,
    e: c_int,
    dec: *const c_char,
) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; 2 * NB]>> = LazyLock::new(|| Mutex::new([0u8; 2 * NB]));

        let mut buf = BUF.lock().unwrap();
        let dec_str = if dec.is_null() {
            "."
        } else {
            match CStr::from_ptr(dec).to_str() {
                Ok(s) => s,
                Err(_) => ".",
            }
        };

        // IEEE: normalize signed zero
        let x = if x == 0.0 { 0.0 } else { x };

        let na = na_string_str();
        let mw = if (w as usize) < NB - 1 {
            w as usize
        } else {
            NB - 1
        };

        let formatted = if !x.is_finite() {
            if x.is_nan() {
                let bits = x.to_bits();
                if (bits & 0x0007fffffffffffffu64) == 0x0001954u64 && (bits >> 52) == 0x7ffu64 {
                    format!("{:width$}", na, width = mw)
                } else {
                    format!("{:width$}", "NaN", width = mw)
                }
            } else if x > 0.0 {
                format!("{:width$}", "Inf", width = mw)
            } else {
                format!("{:width$}", "-Inf", width = mw)
            }
        } else if e != 0 {
            if d != 0 {
                format!("{:+.prec$e}", x, prec = d as usize)
            } else {
                format!("{:.prec$e}", x, prec = d as usize)
            }
        } else {
            format_number_fixed(x, d as usize)
        };

        // Drop trailing zeros after decimal point
        let mut trimmed: Vec<u8> = formatted.bytes().collect();
        if let Some(dot_pos) = trimmed.iter().position(|&b| b == b'.') {
            // Find the last non-zero digit after the dot
            let mut last_nonzero = dot_pos + 1;
            for (i, &ch) in trimmed.iter().enumerate().skip(dot_pos + 1) {
                if ch != b'0' {
                    last_nonzero = i + 1;
                }
            }
            if last_nonzero == dot_pos + 1 {
                // All digits after dot are zero; remove them and the dot
                trimmed.truncate(dot_pos);
            } else if last_nonzero < trimmed.len() {
                trimmed.truncate(last_nonzero);
            }
        }

        // Replace "." with dec if needed
        let out = if dec_str != "." {
            let mut idx = 0usize;
            for &byte in &trimmed {
                if byte == b'.' {
                    for dc in dec_str.bytes() {
                        if idx < 2 * NB - 1 {
                            buf[idx] = dc;
                            idx += 1;
                        }
                    }
                } else {
                    if idx < 2 * NB - 1 {
                        buf[idx] = byte;
                        idx += 1;
                    }
                }
            }
            buf[idx] = 0;
            buf.as_ptr()
        } else {
            let len = trimmed.len().min(NB - 1);
            buf[..len].copy_from_slice(&trimmed[..len]);
            buf[len] = 0;
            buf.as_ptr()
        };

        out as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeReal2
// ---------------------------------------------------------------------------

/// Encode a real value for printing, always using the `#` flag in %f format.
///
/// This is used in specific contexts where the `#` flag ensures a decimal
/// point is always present.
pub unsafe fn EncodeReal2(x: f64, w: c_int, d: c_int, e: c_int) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; NB]>> = LazyLock::new(|| Mutex::new([0u8; NB]));

        let mut buf = BUF.lock().unwrap();

        // IEEE: normalize signed zero
        let x = if x == 0.0 { 0.0 } else { x };

        let na = na_string_str();
        let mw = if (w as usize) < NB - 1 {
            w as usize
        } else {
            NB - 1
        };

        let formatted = if !x.is_finite() {
            if x.is_nan() {
                let bits = x.to_bits();
                if (bits & 0x0007fffffffffffffu64) == 0x0001954u64 && (bits >> 52) == 0x7ffu64 {
                    format!("{:width$}", na, width = mw)
                } else {
                    format!("{:width$}", "NaN", width = mw)
                }
            } else if x > 0.0 {
                format!("{:width$}", "Inf", width = mw)
            } else {
                format!("{:width$}", "-Inf", width = mw)
            }
        } else if e != 0 {
            if d != 0 {
                format!("{:+.prec$e}", x, prec = d as usize)
            } else {
                format!("{:.prec$e}", x, prec = d as usize)
            }
        } else {
            format_number_fixed(x, d as usize)
        };

        let bytes = formatted.as_bytes();
        let len = bytes.len().min(NB - 1);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0;

        buf.as_ptr() as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeComplex
// ---------------------------------------------------------------------------

/// Encode a complex value for printing.
///
/// `wr`, `dr`, `er` are width, digits, scientific flag for the real part.
/// `wi`, `di`, `ei` are width, digits, scientific flag for the imaginary part.
/// `dec` is the decimal separator string.
pub unsafe fn EncodeComplex(
    x: Rcomplex,
    wr: c_int,
    dr: c_int,
    er: c_int,
    wi: c_int,
    di: c_int,
    ei: c_int,
    dec: *const c_char,
) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; NB + 3]>> = LazyLock::new(|| Mutex::new([0u8; NB + 3]));

        let mut buf = BUF.lock().unwrap();

        let dec_str = if dec.is_null() {
            "."
        } else {
            match CStr::from_ptr(dec).to_str() {
                Ok(s) => s,
                Err(_) => ".",
            }
        };

        // Normalize signed zeros
        let r = if x.r == 0.0 { 0.0 } else { x.r };
        let mut i = if x.i == 0.0 { 0.0 } else { x.i };

        let dec_cstr = CString::new(dec_str).unwrap();
        let dec_ptr = dec_cstr.as_ptr();

        let na = na_string_str();
        let na_bits: u64 = 0x7ff0000000001954;
        let is_na = |v: f64| v.to_bits() == na_bits;

        let result = if is_na(r) || is_na(i) {
            format!("{}", na)
        } else {
            let re_str = format!("{}", r);
            let flag_neg_im = i < 0.0;
            if flag_neg_im {
                i = -i;
            }
            let im_str = format!("{}", i);
            let im_str = if im_str == "0" {
                // Effectively zero imaginary part, don't show sign
                format!("{}+0i", re_str)
            } else {
                format!(
                    "{}{}{}i",
                    re_str,
                    if flag_neg_im { "-" } else { "+" },
                    im_str
                )
            };
            im_str
        };

        let bytes = result.as_bytes();
        let len = bytes.len().min(NB + 2);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0;

        buf.as_ptr() as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: EncodeRaw
// ---------------------------------------------------------------------------

/// Encode a raw byte as a two-digit hex string with optional prefix.
pub unsafe fn EncodeRaw(x: Rbyte, prefix: *const c_char) -> *const c_char {
    unsafe {
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static BUF: LazyLock<Mutex<[u8; 10]>> = LazyLock::new(|| Mutex::new([0u8; 10]));

        let mut buf = BUF.lock().unwrap();

        let prefix_str = if prefix.is_null() {
            ""
        } else {
            match CStr::from_ptr(prefix).to_str() {
                Ok(s) => s,
                Err(_) => "",
            }
        };

        let s = format!("{}{:02x}", prefix_str, x);
        let bytes = s.as_bytes();
        let len = bytes.len().min(9);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0;

        buf.as_ptr() as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: Rstrwid
// ---------------------------------------------------------------------------

/// Compute the display width of a string in its escaped form.
///
/// `str` is the input string, `slen` is its byte length,
/// `ienc` is the encoding (0=CE_NATIVE, 1=CE_UTF8, 2=CE_BYTES),
/// `quote` is the quote character (0 for none).
///
/// This counts the number of columns needed when the string is printed
/// with escape sequences (e.g., `\n` counts as 2 columns).
pub unsafe fn Rstrwid(
    str: *const c_char,
    slen: c_int,
    ienc: c_int,
    quote: c_int,
) -> c_int {
    unsafe {
        if str.is_null() || slen <= 0 {
            return 0;
        }

        let bytes = std::slice::from_raw_parts(str as *const u8, slen as usize);
        let quote_char = quote as u8;

        // CE_BYTES = 2
        if ienc == 2 {
            let mut len = 0i32;
            for &k in bytes {
                if k >= 0x20 && k < 0x80 {
                    len += 1;
                } else {
                    len += 4; // \xHH
                }
            }
            return len;
        }

        // For CE_NATIVE and CE_UTF8, we do a simplified ASCII-centric version
        // that handles the common cases. Full MBCS/wchar support would require
        // the locale infrastructure from rlocale.
        let mut len = 0i32;
        for &k in bytes {
            if k < 0x80 {
                // ASCII
                if k >= 0x20 && k != 0x7f {
                    // Printable ASCII
                    match k {
                        b'\\' => len += 2,
                        b'\'' | b'"' | b'`' => {
                            if quote_char != 0 && k == quote_char {
                                len += 2;
                            } else {
                                len += 1;
                            }
                        }
                        _ => len += 1,
                    }
                } else {
                    // Control characters
                    match k {
                        0x07 | 0x08 | 0x0C | b'\n' | b'\r' | b'\t' | 0x0B | 0x00 => len += 2,
                        _ => len += 4, // octal \OOO
                    }
                }
            } else {
                // Non-ASCII byte: assume width 1 for simplicity in CE_NATIVE mode.
                // A full implementation would use wcwidth/mbrtowc.
                len += 1;
            }
        }

        len
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: IndexWidth
// ---------------------------------------------------------------------------

/// Compute the display width needed for an index label.
///
/// Returns the number of decimal digits in `n`.
/// Note: `IndexWidth` is also defined in format.rs (c_int variant).
/// This version uses R_xlen_t for wider range.
pub unsafe fn IndexWidth_xlen(n: R_xlen_t) -> c_int {
    if n <= 0 {
        return 1;
    }
    (n as f64).log10().floor() as c_int + 1
}

// ---------------------------------------------------------------------------
// SEXP-dependent functions
// ---------------------------------------------------------------------------

/// Encode an environment SEXP for display.
pub unsafe fn EncodeEnvironment(_x: SEXP) -> *const c_char {
    static BUF: LazyLock<Mutex<[u8; 1000]>> = LazyLock::new(|| Mutex::new([0u8; 1000]));
    let mut buf = BUF.lock().unwrap();
    let s = "<environment: 0x0>";
    let bytes = s.as_bytes();
    buf[..bytes.len()].copy_from_slice(bytes);
    buf[bytes.len()] = 0;
    buf.as_ptr() as *const c_char
}

/// Encode an external pointer SEXP for display.
pub unsafe fn EncodeExtptr(_x: SEXP) -> *const c_char {
    static BUF: LazyLock<Mutex<[u8; 1000]>> = LazyLock::new(|| Mutex::new([0u8; 1000]));
    let mut buf = BUF.lock().unwrap();
    let s = "<pointer: 0x0>";
    let bytes = s.as_bytes();
    buf[..bytes.len()].copy_from_slice(bytes);
    buf[bytes.len()] = 0;
    buf.as_ptr() as *const c_char
}

/// Create a CHARSXP from a formatted real value.
///
/// Uses `formatReal` to determine optimal formatting, then creates a CHARSXP
/// via `Rf_mkChar`. Returns R_NilValue (NA_STRING equivalent) for NA values.
pub unsafe fn StringFromReal(x: f64, _warn: *mut c_int) -> SEXP {
    unsafe {
        // Check for NA
        if x.to_bits() == R_NA_BIT_PATTERN {
            // Return R_NilValue as a stand-in for NA_STRING when no arena is initialized
            return R_NilValue();
        }

        // Use formatReal to determine optimal w, d, e
        let mut w: c_int = 0;
        let mut d: c_int = 0;
        let mut e: c_int = 0;
        formatReal(&x, 1, &mut w, &mut d, &mut e, 0);

        // IEEE: normalize signed zero
        let x = if x == 0.0 { 0.0 } else { x };

        // Use EncodeRealDrop0 for the formatted string
        let s = EncodeRealDrop0(x, w, d, e, b".\0".as_ptr() as *const c_char);
        Rf_mkChar(s)
    }
}

/// Compute the escaped display width of a CHARSXP.
///
/// Delegates to `Rstrwid` with the CHARSXP's character data and length.
pub unsafe fn Rstrlen(s: SEXP, quote: c_int) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let p = CHAR(s);
        if p.is_null() {
            return 0;
        }
        let len = LENGTH(s);
        Rstrwid(p, len, 0, quote) // CE_NATIVE = 0
    }
}

/// Print-adjustment enum, matching R's Rprt_adj.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rprt_adj {
    none = 0,
    left = 1,
    right = 2,
    centre = 3,
}

/// Encode a CHARSXP for printing with escaping and quoting.
///
/// Handles ASCII escaping (backslash, quotes, control chars -> \n etc.),
/// padding/justification, and quoting. Returns a pointer to an internal
/// thread-local buffer.
pub unsafe fn EncodeString(
    s: SEXP,
    w: c_int,
    quote: c_int,
    justify: Rprt_adj,
) -> *const c_char {
    unsafe {
        static BUFFER: LazyLock<Mutex<Vec<u8>>> =
            LazyLock::new(|| Mutex::new(Vec::with_capacity(BUFSIZE)));

        let mut buffer = BUFFER.lock().unwrap();
        buffer.clear();

        if s.is_null() {
            buffer.push(0);
            return buffer.as_ptr() as *const c_char;
        }

        // Get the character data
        let p = CHAR(s);
        if p.is_null() {
            buffer.push(0);
            return buffer.as_ptr() as *const c_char;
        }

        let bytes = CStr::from_ptr(p).to_bytes();
        let cnt = bytes.len();

        // Compute display width (escaped width)
        let i = Rstrwid(p, cnt as c_int, 0, quote);

        let quote_char = quote as u8;

        // Compute padding
        let mut b = w - i - if quote != 0 { 2 } else { 0 };
        if justify == Rprt_adj::none {
            b = 0;
        }

        // Left/centre padding
        if b > 0 && justify != Rprt_adj::left {
            let b0 = if justify == Rprt_adj::centre {
                b / 2
            } else {
                b
            };
            for _ in 0..b0 {
                buffer.push(b' ');
            }
            b -= b0;
        }

        // Opening quote
        if quote != 0 {
            buffer.push(quote_char);
        }

        // Encode each byte with escaping (ASCII path, matching R's non-MBCS path)
        for &k in bytes {
            if k < 0x80 {
                // ASCII
                if k != b'\t' && (k >= 0x20 && k < 0x7f) {
                    match k {
                        b'\\' => {
                            buffer.push(b'\\');
                            buffer.push(b'\\');
                        }
                        b'\'' | b'"' | b'`' => {
                            if quote != 0 && k == quote_char {
                                buffer.push(b'\\');
                            }
                            buffer.push(k);
                        }
                        _ => buffer.push(k),
                    }
                } else {
                    // Control characters / non-printable ASCII
                    match k {
                        0x07 => {
                            buffer.push(b'\\');
                            buffer.push(b'a');
                        }
                        0x08 => {
                            buffer.push(b'\\');
                            buffer.push(b'b');
                        }
                        0x0C => {
                            buffer.push(b'\\');
                            buffer.push(b'f');
                        }
                        b'\n' => {
                            buffer.push(b'\\');
                            buffer.push(b'n');
                        }
                        b'\r' => {
                            buffer.push(b'\\');
                            buffer.push(b'r');
                        }
                        b'\t' => {
                            buffer.push(b'\\');
                            buffer.push(b't');
                        }
                        0x0B => {
                            buffer.push(b'\\');
                            buffer.push(b'v');
                        }
                        0x00 => {
                            buffer.push(b'\\');
                            buffer.push(b'0');
                        }
                        _ => {
                            // Octal encoding: \OOO
                            buffer.push(b'\\');
                            buffer.push(b'0' + (k >> 6));
                            buffer.push(b'0' + ((k >> 3) & 7));
                            buffer.push(b'0' + (k & 7));
                        }
                    }
                }
            } else {
                // High byte: pass through (non-MBCS simplified path)
                if k >= 0x20 {
                    buffer.push(k);
                } else {
                    // Octal encoding
                    buffer.push(b'\\');
                    buffer.push(b'0' + (k >> 6));
                    buffer.push(b'0' + ((k >> 3) & 7));
                    buffer.push(b'0' + (k & 7));
                }
            }
        }

        // Closing quote
        if quote != 0 {
            buffer.push(quote_char);
        }

        // Right/centre padding
        if b > 0 && justify != Rprt_adj::right {
            for _ in 0..b {
                buffer.push(b' ');
            }
        }

        buffer.push(0); // null-terminate
        buffer.as_ptr() as *const c_char
    }
}

/// Encode a single element of an R vector for printing.
///
/// Dispatches on TYPEOF(x) to the appropriate encode function.
/// The `cdec` parameter is the decimal separator character.
pub unsafe fn EncodeElement(
    x: SEXP,
    indx: c_int,
    quote: c_int,
    cdec: c_char,
) -> *const c_char {
    unsafe {
        let dec_buf = [cdec as u8, 0u8];
        EncodeElement0(
            x,
            indx as R_xlen_t,
            quote,
            dec_buf.as_ptr() as *const c_char,
        )
    }
}

/// Encode a single element of an R vector for printing (R_xlen_t index).
///
/// Dispatches on TYPEOF(x) to the appropriate encode function.
/// Uses `formatReal`/`formatLogical`/`formatInteger`/`formatComplex`/`formatString`
/// to determine optimal widths, then calls the corresponding Encode function.
pub unsafe fn EncodeElement0(
    x: SEXP,
    indx: R_xlen_t,
    quote: c_int,
    dec: *const c_char,
) -> *const c_char {
    unsafe {
        let sexptype = TYPEOF(x);

        match SEXPTYPE(sexptype) {
            SEXPTYPE::LGLSXP => {
                let log_data = LOGICAL(x);
                let val = *log_data.add(indx as usize);
                let mut w: c_int = 0;
                formatLogical(log_data.add(indx as usize), 1, &mut w);
                EncodeLogical(val, w)
            }
            SEXPTYPE::INTSXP => {
                let int_data = INTEGER(x);
                let val = *int_data.add(indx as usize);
                let mut w: c_int = 0;
                formatInteger(int_data.add(indx as usize), 1, &mut w);
                EncodeInteger(val, w)
            }
            SEXPTYPE::REALSXP => {
                let real_data = REAL(x);
                let val = *real_data.add(indx as usize);
                let mut w: c_int = 0;
                let mut d: c_int = 0;
                let mut e: c_int = 0;
                formatReal(real_data.add(indx as usize), 1, &mut w, &mut d, &mut e, 0);
                EncodeReal0(val, w, d, e, dec)
            }
            SEXPTYPE::STRSXP => {
                let elt = STRING_ELT(x, indx);
                let mut w: c_int = 0;
                formatString(&elt, 1, &mut w, quote);
                EncodeString(elt, w, quote, Rprt_adj::left)
            }
            SEXPTYPE::CPLXSXP => {
                let cpx_data = COMPLEX(x);
                let val = *cpx_data.add(indx as usize);
                let mut wr: c_int = 0;
                let mut dr: c_int = 0;
                let mut er: c_int = 0;
                let mut wi: c_int = 0;
                let mut di: c_int = 0;
                let mut ei: c_int = 0;
                formatComplex(
                    cpx_data.add(indx as usize),
                    1,
                    &mut wr,
                    &mut dr,
                    &mut er,
                    &mut wi,
                    &mut di,
                    &mut ei,
                    0,
                );
                EncodeComplex(val, wr, dr, er, wi, di, ei, dec)
            }
            SEXPTYPE::RAWSXP => {
                let raw_data = RAW(x);
                let val = *raw_data.add(indx as usize);
                EncodeRaw(val, b"\0".as_ptr() as *const c_char)
            }
            _ => {
                static EMPTY: [u8; 1] = [0];
                EMPTY.as_ptr() as *const c_char
            }
        }
    }
}

/// Encode a CHARSXP for display in error messages.
///
/// Simple wrapper around `EncodeString` with width=0, quote=0, left-justified.
/// Note: the returned pointer points to an internal buffer that is overwritten
/// by subsequent calls to EncodeChar/EncodeString.
pub unsafe fn EncodeChar(x: SEXP) -> *const c_char {
    unsafe { EncodeString(x, 0, 0, Rprt_adj::left) }
}

// ---------------------------------------------------------------------------
// Printing functions
// ---------------------------------------------------------------------------

/// Rprintf: formatted output to R's standard output.
///
/// In R, this routes through the connection infrastructure. Our simplified
/// implementation writes the format string directly to stdout (without
/// processing variadic arguments, since Rust cannot represent C varargs).
/// For format-string-only calls (no % args), this produces correct output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rprintf(format: *const c_char, _args: *mut c_void) {
    unsafe {
        if format.is_null() {
            return;
        }
        let s = CStr::from_ptr(format);
        if let Ok(text) = s.to_str() {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            let _ = handle.write_all(text.as_bytes());
        }
    }
}

/// Rvprintf: varargs variant of Rprintf.
///
/// Simplified implementation: writes the format string directly to stdout.
pub unsafe fn Rvprintf(format: *const c_char, _arg: *mut c_void) {
    unsafe {
        if format.is_null() {
            return;
        }
        let s = CStr::from_ptr(format);
        if let Ok(text) = s.to_str() {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            let _ = handle.write_all(text.as_bytes());
        }
    }
}

// REprintf is defined in special/mlutils.rs

/// REvprintf: varargs variant of REprintf (to stderr).
///
/// Simplified implementation: writes the format string directly to stderr.
pub unsafe fn REvprintf(format: *const c_char, _arg: *mut c_void) {
    unsafe {
        if format.is_null() {
            return;
        }
        let s = CStr::from_ptr(format);
        if let Ok(text) = s.to_str() {
            let stderr = std::io::stderr();
            let mut handle = stderr.lock();
            let _ = handle.write_all(text.as_bytes());
        }
    }
}

/// Internal implementation of REvprintf.
///
/// Returns the number of characters written (length of the format string).
/// Simplified: does not process variadic arguments.
pub unsafe fn REvprintf_internal(format: *const c_char, _arg: *mut c_void) -> c_int {
    unsafe {
        if format.is_null() {
            return 0;
        }
        let s = CStr::from_ptr(format);
        if let Ok(text) = s.to_str() {
            let stderr = std::io::stderr();
            let mut handle = stderr.lock();
            let _ = handle.write_all(text.as_bytes());
            text.len() as c_int
        } else {
            0
        }
    }
}

/// Console vprintf implementation.
///
/// Writes the format string to stdout. Returns the number of characters written.
/// Simplified: does not process variadic arguments.
pub unsafe fn Rcons_vprintf(format: *const c_char, _arg: *mut c_void) -> c_int {
    unsafe {
        if format.is_null() {
            return 0;
        }
        let s = CStr::from_ptr(format);
        if let Ok(text) = s.to_str() {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            let _ = handle.write_all(text.as_bytes());
            text.len() as c_int
        } else {
            0
        }
    }
}

/// Print a vector index label.
///
/// Prints `[i]` with left-padding to width `w`.
pub unsafe fn VectorIndex(i: R_xlen_t, w: c_int) {
    unsafe {
        let iw = IndexWidth_xlen(i);
        let total_label_width = iw + 2; // "[i]" = 2 brackets + digits
        let pad = w - total_label_width;
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        if pad > 0 {
            let _ = handle.write_all(&vec![b' '; pad as usize]);
        }
        let label = format!("[{}]", i);
        let _ = handle.write_all(label.as_bytes());
    }
}
