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
///
/// Returns `Rcomplex { r: NA_REAL, i: NA_REAL }` if `x` is R's NA (specific bit pattern).
/// For other values (including non-NA NaN), passes through with `i = 0.0`.
pub unsafe fn ComplexFromReal(x: c_double, _warn: *mut c_int) -> Rcomplex {
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
pub unsafe fn ComplexFromStringC(s: *const c_char, warn: *mut c_int) -> Rcomplex {
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
                _ => {} // intentionally unhandled: non-sign character in exponent parsing
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
            let imag_body = if let Some(stripped) = imag_str.strip_suffix('i') {
                stripped
            } else {
                imag_str
            };

            if let (Some(r), Some(i)) = (parse_double_str(real_str), parse_double_str(imag_body)) {
                return Rcomplex { r, i: sign * i };
            }
        } else if let Some(body) = str.strip_suffix('i') {
            // Pure imaginary: "3i"
            if let Some(i) = parse_double_str(body) {
                return Rcomplex { r: 0.0, i };
            }
        } else {
            // Pure real
            if let Some(r) = parse_double_str(str) {
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
pub unsafe fn ComplexFromString(x: SEXP, warn: *mut c_int) -> Rcomplex {
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
    } else if x.fract() == 0.0 {
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

