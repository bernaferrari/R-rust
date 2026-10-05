use super::*;

// ---------------------------------------------------------------------------
// LogicalFrom* conversions
// ---------------------------------------------------------------------------

/// Convert integer to logical.
///
/// Returns `NA_LOGICAL` if `x` is `NA_INTEGER`, otherwise 1 if non-zero, 0 if zero.
pub unsafe fn LogicalFromInteger(x: c_int, _warn: *mut c_int) -> c_int {
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
pub unsafe fn LogicalFromReal(x: c_double, _warn: *mut c_int) -> c_int {
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
pub unsafe fn LogicalFromComplex(x: Rcomplex, _warn: *mut c_int) -> c_int {
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
pub unsafe fn LogicalFromString(x: SEXP, _warn: *mut c_int) -> c_int {
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
pub unsafe fn IntegerFromLogical(x: c_int, _warn: *mut c_int) -> c_int {
    if x == NA_LOGICAL { NA_INTEGER } else { x }
}

/// Convert real to integer.
///
/// Returns `NA_INTEGER` if `x` is NaN or outside `INT_MIN..INT_MAX` range.
/// Sets `WARN_INT_NA` flag in `warn` on overflow.
pub unsafe fn IntegerFromReal(x: c_double, warn: *mut c_int) -> c_int {
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
pub unsafe fn IntegerFromComplex(x: Rcomplex, warn: *mut c_int) -> c_int {
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
pub unsafe fn IntegerFromString(x: SEXP, warn: *mut c_int) -> c_int {
    unsafe {
        if x.is_null() || x == R_NaString() {
            return NA_INTEGER;
        }
        let text = CHAR(x);
        if text.is_null() {
            return NA_INTEGER;
        }
        let (value, flags) = super::text_number::integer(CStr::from_ptr(text).to_bytes());
        if !warn.is_null() {
            *warn |= flags;
        }
        value
    }
}

// ---------------------------------------------------------------------------
// RealFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to real.
///
/// Returns `NA_REAL` if `x` is `NA_LOGICAL`, otherwise passes through.
pub unsafe fn RealFromLogical(x: c_int, _warn: *mut c_int) -> c_double {
    if x == NA_LOGICAL {
        NA_REAL
    } else {
        x as c_double
    }
}

/// Convert integer to real.
///
/// Returns `NA_REAL` if `x` is `NA_INTEGER`, otherwise passes through.
pub unsafe fn RealFromInteger(x: c_int, _warn: *mut c_int) -> c_double {
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
pub unsafe fn RealFromComplex(x: Rcomplex, warn: *mut c_int) -> c_double {
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
pub unsafe fn RealFromString(x: SEXP, warn: *mut c_int) -> c_double {
    unsafe {
        if x.is_null() || x == R_NaString() {
            return NA_REAL;
        }
        let text = CHAR(x);
        if text.is_null() {
            return NA_REAL;
        }
        let (value, flags) = super::text_number::real(CStr::from_ptr(text).to_bytes());
        if !warn.is_null() {
            *warn |= flags;
        }
        value
    }
}

// ---------------------------------------------------------------------------
// ComplexFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to complex.
///
/// Returns `Rcomplex { r: NA_REAL, i: 0.0 }` if `x` is `NA_LOGICAL`.
pub unsafe fn ComplexFromLogical(x: c_int, _warn: *mut c_int) -> Rcomplex {
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
pub unsafe fn ComplexFromInteger(x: c_int, _warn: *mut c_int) -> Rcomplex {
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
/// GNU `as.complex` keeps the imaginary part 0, including for `NA_real_`.
pub unsafe fn ComplexFromReal(x: c_double, _warn: *mut c_int) -> Rcomplex {
    if R_IsNA(x) {
        Rcomplex { r: NA_REAL, i: 0.0 }
    } else {
        Rcomplex { r: x, i: 0.0 }
    }
}

/// Convert a C string to complex.
///
/// Parses strings like "3", "2i", "3+2i", "3-2i".
/// Returns `Rcomplex { r: NA_REAL, i: NA_REAL }` for invalid input.
pub unsafe fn ComplexFromStringC(s: *const c_char, warn: *mut c_int) -> Rcomplex {
    unsafe {
        if s.is_null() {
            return Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            };
        }
        let (value, flags) = super::text_number::complex(CStr::from_ptr(s).to_bytes());
        if !warn.is_null() {
            *warn |= flags;
        }
        value
    }
}

/// Convert string (CHARSXP/STRSXP element) to complex.
///
/// Faithfully ports R's ComplexFromString from coerce.c which uses R_strtod.
pub unsafe fn ComplexFromString(x: SEXP, warn: *mut c_int) -> Rcomplex {
    unsafe {
        let missing = Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };
        if x.is_null() || x == R_NaString() {
            return missing;
        }
        let text = CHAR(x);
        if text.is_null() {
            return missing;
        }
        let (value, flags) = super::text_number::complex(CStr::from_ptr(text).to_bytes());
        if !warn.is_null() {
            *warn |= flags;
        }
        value
    }
}

// ---------------------------------------------------------------------------
// StringFrom* conversions
// ---------------------------------------------------------------------------

/// Convert logical to string (CHARSXP).
///
/// Returns "FALSE" for 0, "TRUE" for 1, NA_STRING for NA_LOGICAL.
pub unsafe fn StringFromLogical(x: c_int) -> SEXP {
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
pub unsafe fn StringFromInteger(x: c_int, _warn: *mut c_int) -> SEXP {
    unsafe {
        if x == NA_INTEGER {
            return R_NaString();
        }
        // Format integer as string
        let s = format!("{}", x);
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        Rf_mkChar(cstr.as_ptr())
    }
}

pub fn string_from_real_for_complex(x: c_double) -> String {
    if R_IsNA(x) {
        "NA".to_string()
    } else if R_IsNaN(x) {
        "NaN".to_string()
    } else if x.is_infinite() {
        if x.is_sign_negative() {
            "-Inf".to_string()
        } else {
            "Inf".to_string()
        }
    } else {
        let fixed = if x.fract() == 0.0 {
            format!("{x:.0}")
        } else {
            let mut s = format!("{x:.15}");
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
            s
        };
        let ax = x.abs();
        let sci = if ax == 0.0 {
            fixed.clone()
        } else {
            let exp = ax.log10().floor() as i32;
            let mant = ax / 10f64.powi(exp);
            let mant_s = if (mant - mant.round()).abs() < 1e-10 {
                format!("{:.0}", mant.round())
            } else {
                let mut s = format!("{mant:.6}");
                while s.ends_with('0') {
                    s.pop();
                }
                if s.ends_with('.') {
                    s.pop();
                }
                s
            };
            format!("{mant_s}e{:+03}", exp)
        };
        if sci.len() < fixed.len() { sci } else { fixed }
    }
}

/// Convert complex to string (CHARSXP).
///
/// Returns NA_STRING if either part is R's NA. Otherwise formats as "r+i" or "r-i".
pub unsafe fn StringFromComplex(x: Rcomplex, _warn: *mut c_int) -> SEXP {
    unsafe {
        if R_IsNA(x.r) || R_IsNA(x.i) {
            return R_NaString();
        }
        let real = string_from_real_for_complex(x.r);
        let imaginary = string_from_real_for_complex(x.i.abs());
        let s = if x.i.is_sign_negative() {
            format!("{real}-{imaginary}i")
        } else {
            format!("{real}+{imaginary}i")
        };
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        Rf_mkChar(cstr.as_ptr())
    }
}

/// Convert raw byte to string (CHARSXP).
///
/// Formats as two-digit hexadecimal, e.g. 255 -> "ff".
pub unsafe fn StringFromRaw(x: Rbyte, _warn: *mut c_int) -> SEXP {
    unsafe {
        let s = format!("{:02x}", x);
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        Rf_mkChar(cstr.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// RealFromReal (passthrough for coerceToReal from STRSXP via RealFromString)
// ---------------------------------------------------------------------------
