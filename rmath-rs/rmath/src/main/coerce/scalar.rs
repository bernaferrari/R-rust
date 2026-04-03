#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Scalar conversion functions: LogicalFromInteger, IntegerFromReal, etc.

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, Rbyte, Rcomplex, SEXP};

use super::helpers::{R_NaString, WARN_IMAG, WARN_INT_NA, WARN_NA};
use super::{ISNAN, NA_REAL, R_IsNA};

// ---------------------------------------------------------------------------
// strtod wrapper (C lib)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double;
}

// ---------------------------------------------------------------------------
// LogicalFrom* conversions
// ---------------------------------------------------------------------------

/// Convert integer to logical.
///
/// Returns `NA_LOGICAL` if `x` is `NA_INTEGER`, otherwise 1 if non-zero, 0 if zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LogicalFromInteger(x: c_int, _warn: *mut c_int) -> c_int {
    if x == NA_INTEGER {
        NA_LOGICAL
    } else if x != 0 {
        1
    } else {
        0
    }
}

/// Convert real to logical.
///
/// Returns `NA_LOGICAL` if `x` is NaN, otherwise 1 if non-zero, 0 if zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LogicalFromReal(x: c_double, _warn: *mut c_int) -> c_int {
    if ISNAN(x) {
        NA_LOGICAL
    } else if x != 0.0 {
        1
    } else {
        0
    }
}

/// Convert complex to logical.
///
/// Returns `NA_LOGICAL` if either part is NaN, otherwise 1 if non-zero, 0 if zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LogicalFromComplex(x: Rcomplex, _warn: *mut c_int) -> c_int {
    if ISNAN(x.r) || ISNAN(x.i) {
        NA_LOGICAL
    } else if x.r != 0.0 || x.i != 0.0 {
        1
    } else {
        0
    }
}

/// Convert string (CHARSXP) to logical.
///
/// Returns 1 for "TRUE"/"T" (case-insensitive), 0 for "FALSE"/"F",
/// NA_LOGICAL for NA_STRING or unrecognized strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LogicalFromString(x: SEXP, _warn: *mut c_int) -> c_int {
    unsafe {
        if x.is_null() || x == R_NaString() {
            return NA_LOGICAL;
        }
        let s = CHAR(x);
        if s.is_null() {
            return NA_LOGICAL;
        }
        let bytes = CStr::from_ptr(s).to_bytes();
        let str = std::str::from_utf8_unchecked(bytes).trim();

        match str.to_uppercase().as_str() {
            "TRUE" | "T" => 1,
            "FALSE" | "F" => 0,
            _ => NA_LOGICAL,
        }
    }
}

// ---------------------------------------------------------------------------
// IntegerFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to integer.
///
/// Returns `NA_INTEGER` if `x` is `NA_LOGICAL`, otherwise passes through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IntegerFromLogical(x: c_int, _warn: *mut c_int) -> c_int {
    if x == NA_LOGICAL { NA_INTEGER } else { x }
}

/// Convert real to integer.
///
/// Returns `NA_INTEGER` if `x` is NaN or outside `INT_MIN..INT_MAX` range.
/// Sets `WARN_INT_NA` flag in `warn` on overflow.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IntegerFromReal(x: c_double, warn: *mut c_int) -> c_int {
    unsafe {
        if ISNAN(x) {
            NA_INTEGER
        } else if x >= (c_int::MAX as f64) + 1.0 || x <= c_int::MIN as f64 {
            if !warn.is_null() {
                *warn |= WARN_INT_NA;
            }
            NA_INTEGER
        } else {
            x as c_int
        }
    }
}

/// Convert complex to integer.
///
/// Returns `NA_INTEGER` if real part is NaN or out of range.
/// Sets `WARN_IMAG` if imaginary part is non-zero.
/// Sets `WARN_INT_NA` on overflow.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IntegerFromComplex(x: Rcomplex, warn: *mut c_int) -> c_int {
    unsafe {
        if ISNAN(x.r) || ISNAN(x.i) {
            NA_INTEGER
        } else if x.r > (c_int::MAX as f64) + 1.0 || x.r <= c_int::MIN as f64 {
            if !warn.is_null() {
                *warn |= WARN_INT_NA;
            }
            NA_INTEGER
        } else {
            if x.i != 0.0 && !warn.is_null() {
                *warn |= WARN_IMAG;
            }
            x.r as c_int
        }
    }
}

/// Convert string (CHARSXP) to integer.
///
/// Parses the string as a double, then converts to integer with overflow checking.
/// Returns NA_INTEGER for NA_STRING, blank strings, or unparseable strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IntegerFromString(x: SEXP, warn: *mut c_int) -> c_int {
    unsafe {
        if x.is_null() || x == R_NaString() {
            return NA_INTEGER;
        }
        let s = CHAR(x);
        if s.is_null() {
            return NA_INTEGER;
        }

        // Check for blank string
        let mut p = s;
        while *p != 0 {
            if *p != b' ' as c_char
                && *p != b'\t' as c_char
                && *p != b'\n' as c_char
                && *p != b'\r' as c_char
            {
                break;
            }
            p = p.add(1);
        }
        if *p == 0 {
            // Blank string
            return NA_INTEGER;
        }

        // Parse as double using strtod
        let mut endp: *mut c_char = ptr::null_mut();
        let xdouble = strtod(s, &mut endp);

        // Check that entire string was consumed
        let mut ep = endp;
        while *ep != 0 {
            if *ep != b' ' as c_char
                && *ep != b'\t' as c_char
                && *ep != b'\n' as c_char
                && *ep != b'\r' as c_char
            {
                if !warn.is_null() {
                    *warn |= WARN_NA;
                }
                return NA_INTEGER;
            }
            ep = ep.add(1);
        }

        // Convert double to integer with range checking (same as IntegerFromReal)
        if ISNAN(xdouble) {
            NA_INTEGER
        } else if xdouble >= (c_int::MAX as f64) + 1.0 || xdouble <= c_int::MIN as f64 {
            if !warn.is_null() {
                *warn |= WARN_INT_NA;
            }
            NA_INTEGER
        } else {
            xdouble as c_int
        }
    }
}

// ---------------------------------------------------------------------------
// RealFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to real.
///
/// Returns `NA_REAL` if `x` is `NA_LOGICAL`, otherwise passes through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RealFromLogical(x: c_int, _warn: *mut c_int) -> c_double {
    if x == NA_LOGICAL {
        NA_REAL
    } else {
        x as c_double
    }
}

/// Convert integer to real.
///
/// Returns `NA_REAL` if `x` is `NA_INTEGER`, otherwise passes through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RealFromInteger(x: c_int, _warn: *mut c_int) -> c_double {
    if x == NA_INTEGER {
        NA_REAL
    } else {
        x as c_double
    }
}

/// Convert complex to real.
///
/// Returns `NA_REAL` if either part is NaN.
/// Sets `WARN_IMAG` if imaginary part is non-zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RealFromComplex(x: Rcomplex, warn: *mut c_int) -> c_double {
    unsafe {
        if ISNAN(x.r) || ISNAN(x.i) {
            NA_REAL
        } else {
            if x.i != 0.0 && !warn.is_null() {
                *warn |= WARN_IMAG;
            }
            x.r
        }
    }
}

/// Convert string (CHARSXP) to real.
///
/// Parses the string as a double. Returns NA_REAL for NA_STRING,
/// blank strings, or unparseable strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RealFromString(x: SEXP, warn: *mut c_int) -> c_double {
    unsafe {
        if x.is_null() || x == R_NaString() {
            return NA_REAL;
        }
        let s = CHAR(x);
        if s.is_null() {
            return NA_REAL;
        }

        // Check for blank string
        let mut p = s;
        while *p != 0 {
            if *p != b' ' as c_char
                && *p != b'\t' as c_char
                && *p != b'\n' as c_char
                && *p != b'\r' as c_char
            {
                break;
            }
            p = p.add(1);
        }
        if *p == 0 {
            // Blank string
            return NA_REAL;
        }

        // Parse as double
        let mut endp: *mut c_char = ptr::null_mut();
        let xdouble = strtod(s, &mut endp);

        // Check that entire string was consumed
        let mut ep = endp;
        while *ep != 0 {
            if *ep != b' ' as c_char
                && *ep != b'\t' as c_char
                && *ep != b'\n' as c_char
                && *ep != b'\r' as c_char
            {
                if !warn.is_null() {
                    *warn |= WARN_NA;
                }
                return NA_REAL;
            }
            ep = ep.add(1);
        }

        xdouble
    }
}

// ---------------------------------------------------------------------------
// ComplexFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to complex.
///
/// Returns `Rcomplex { r: NA_REAL, i: 0.0 }` if `x` is `NA_LOGICAL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ComplexFromLogical(x: c_int, _warn: *mut c_int) -> Rcomplex {
    if x == NA_LOGICAL {
        Rcomplex { r: NA_REAL, i: 0.0 }
    } else {
        Rcomplex {
            r: x as f64,
            i: 0.0,
        }
    }
}

/// Convert integer to complex.
///
/// Returns `Rcomplex { r: NA_REAL, i: 0.0 }` if `x` is `NA_INTEGER`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ComplexFromInteger(x: c_int, _warn: *mut c_int) -> Rcomplex {
    if x == NA_INTEGER {
        Rcomplex { r: NA_REAL, i: 0.0 }
    } else {
        Rcomplex {
            r: x as f64,
            i: 0.0,
        }
    }
}

/// Convert real to complex.
///
/// Returns `Rcomplex { r: NA_REAL, i: NA_REAL }` if `x` is R's NA (specific bit pattern).
/// For other values (including non-NA NaN), passes through with `i = 0.0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ComplexFromReal(x: c_double, _warn: *mut c_int) -> Rcomplex {
    if R_IsNA(x) {
        Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        }
    } else {
        Rcomplex { r: x, i: 0.0 }
    }
}

/// Convert a C string to complex.
///
/// Parses strings like "3", "2i", "3+2i", "3-2i".
/// Returns `Rcomplex { r: NA_REAL, i: NA_REAL }` for invalid input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ComplexFromStringC(s: *const c_char, warn: *mut c_int) -> Rcomplex {
    unsafe {
        if s.is_null() {
            return Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            };
        }
        let bytes = CStr::from_ptr(s).to_bytes();
        let str = std::str::from_utf8_unchecked(bytes).trim();

        if str.is_empty() {
            return Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            };
        }

        // Try "a+bi" or "a-bi" format
        let mut split_pos: Option<usize> = None;
        for (i, ch) in str.char_indices() {
            match ch {
                '+' | '-' if i > 0 => {
                    split_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }

        if let Some(pos) = split_pos {
            let real_str = &str[..pos];
            let sign: f64 = if str.as_bytes()[pos] == b'-' {
                -1.0
            } else {
                1.0
            };
            let imag_str = &str[pos + 1..];

            // Imaginary part should end with 'i'
            let imag_body = if imag_str.ends_with('i') {
                &imag_str[..imag_str.len() - 1]
            } else {
                imag_str
            };

            if let (Ok(r), Ok(i)) = (real_str.parse::<f64>(), imag_body.parse::<f64>()) {
                return Rcomplex { r, i: sign * i };
            }
        } else if str.ends_with('i') {
            // Pure imaginary: "3i"
            let body = &str[..str.len() - 1];
            if let Ok(i) = body.parse::<f64>() {
                return Rcomplex { r: 0.0, i };
            }
        } else {
            // Pure real
            if let Ok(r) = str.parse::<f64>() {
                return Rcomplex { r, i: 0.0 };
            }
        }

        if !warn.is_null() {
            *warn |= WARN_NA;
        }
        Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        }
    }
}

/// Convert string (CHARSXP/STRSXP element) to complex.
///
/// Faithfully ports R's ComplexFromString from coerce.c which uses R_strtod.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ComplexFromString(x: SEXP, warn: *mut c_int) -> Rcomplex {
    unsafe {
        let mut z = Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };

        if x.is_null() || x == R_NaString() {
            return z;
        }

        let xx = CHAR(x);
        if xx.is_null() {
            return z;
        }

        // Check for blank string
        let mut p = xx;
        while *p != 0 {
            if *p != b' ' as c_char
                && *p != b'\t' as c_char
                && *p != b'\n' as c_char
                && *p != b'\r' as c_char
            {
                break;
            }
            p = p.add(1);
        }
        if *p == 0 {
            // Blank string
            return z;
        }

        // Try parsing: "real" or "imaginary i" or "real+/-imaginary i"
        let mut endp: *mut c_char = ptr::null_mut();
        let xr = strtod(xx, &mut endp);

        // Check if rest is blank => pure real
        let mut ep = endp;
        while *ep != 0 {
            if *ep != b' ' as c_char
                && *ep != b'\t' as c_char
                && *ep != b'\n' as c_char
                && *ep != b'\r' as c_char
            {
                break;
            }
            ep = ep.add(1);
        }
        if *ep == 0 {
            z.r = xr;
            z.i = 0.0;
            return z;
        }

        // Check for pure imaginary: "3i"
        if *endp == b'i' as c_char {
            let mut ep2 = endp.add(1);
            while *ep2 != 0 {
                if *ep2 != b' ' as c_char
                    && *ep2 != b'\t' as c_char
                    && *ep2 != b'\n' as c_char
                    && *ep2 != b'\r' as c_char
                {
                    break;
                }
                ep2 = ep2.add(1);
            }
            if *ep2 == 0 {
                z.r = 0.0;
                z.i = xr;
                return z;
            }
        }

        // Check for "real+/-imaginary i"
        if *endp == b'+' as c_char || *endp == b'-' as c_char {
            let xi = strtod(endp, &mut endp);
            if *endp == b'i' as c_char {
                let mut ep3 = endp.add(1);
                while *ep3 != 0 {
                    if *ep3 != b' ' as c_char
                        && *ep3 != b'\t' as c_char
                        && *ep3 != b'\n' as c_char
                        && *ep3 != b'\r' as c_char
                    {
                        break;
                    }
                    ep3 = ep3.add(1);
                }
                if *ep3 == 0 {
                    z.r = xr;
                    z.i = xi;
                    return z;
                }
            }
        }

        if !warn.is_null() {
            *warn |= WARN_NA;
        }
        z
    }
}

// ---------------------------------------------------------------------------
// StringFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to string (CHARSXP).
///
/// Returns "FALSE" for 0, "TRUE" for 1, NA_STRING for NA_LOGICAL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn StringFromLogical(x: c_int) -> SEXP {
    unsafe {
        if x == NA_LOGICAL {
            return R_NaString();
        }
        if x != 0 {
            Rf_mkChar(c"TRUE".as_ptr())
        } else {
            Rf_mkChar(c"FALSE".as_ptr())
        }
    }
}

/// Convert integer to string (CHARSXP).
///
/// Returns NA_STRING for NA_INTEGER, otherwise the decimal representation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn StringFromInteger(x: c_int, _warn: *mut c_int) -> SEXP {
    unsafe {
        if x == NA_INTEGER {
            return R_NaString();
        }
        // Format integer as string
        let s = format!("{}", x);
        let cstr = std::ffi::CString::new(s).unwrap();
        Rf_mkChar(cstr.as_ptr())
    }
}

/// Convert real to string (CHARSXP).
///
/// Returns NA_STRING for R's NA. Uses maximal precision (DBL_DIG=15) for
/// other values, matching R's behavior.
///
/// Note: The `#[unsafe(no_mangle)]` FFI symbol is defined in printutils.rs.
/// This is the module-private implementation used by coerceToString().
pub(crate) unsafe fn StringFromReal_impl(x: c_double, _warn: *mut c_int) -> SEXP {
    unsafe {
        if R_IsNA(x) {
            return R_NaString();
        }
        // Use 17 significant digits for round-trip safety (matches R's DBL_DIG + 2)
        let s = format!("{:.17e}", x);
        let cstr = std::ffi::CString::new(s).unwrap();
        Rf_mkChar(cstr.as_ptr())
    }
}

/// Convert complex to string (CHARSXP).
///
/// Returns NA_STRING if either part is R's NA. Otherwise formats as "r+i" or "r-i".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn StringFromComplex(x: Rcomplex, _warn: *mut c_int) -> SEXP {
    unsafe {
        if R_IsNA(x.r) || R_IsNA(x.i) {
            return R_NaString();
        }
        let s = if x.i >= 0.0 {
            format!("{:.17e}+{:.17e}i", x.r, x.i)
        } else {
            format!("{:.17e}{:.17e}i", x.r, x.i)
        };
        let cstr = std::ffi::CString::new(s).unwrap();
        Rf_mkChar(cstr.as_ptr())
    }
}

/// Convert raw byte to string (CHARSXP).
///
/// Formats as two-digit hexadecimal, e.g. 255 -> "ff".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn StringFromRaw(x: Rbyte, _warn: *mut c_int) -> SEXP {
    unsafe {
        let s = format!("{:02x}", x);
        let cstr = std::ffi::CString::new(s).unwrap();
        Rf_mkChar(cstr.as_ptr())
    }
}
