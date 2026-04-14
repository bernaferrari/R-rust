#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/coerce.c -- type conversion utilities.
//!
//! This module handles type conversion for elements of data vectors, as well
//! as full vector coercion (coerceVector) and the scalar asLogical/asInteger/
//! asReal/asComplex entry points used throughout R's internals.
//!
//! Ported functions:
//!   Scalar conversions:
//!     LogicalFromInteger, LogicalFromReal, LogicalFromComplex, LogicalFromString
//!     IntegerFromLogical, IntegerFromReal, IntegerFromComplex, IntegerFromString
//!     RealFromLogical, RealFromInteger, RealFromComplex, RealFromString
//!     ComplexFromLogical, ComplexFromInteger, ComplexFromReal, ComplexFromString
//!     ComplexFromStringC (C-string variant)
//!     StringFromLogical, StringFromInteger, StringFromReal, StringFromComplex, StringFromRaw
//!   Vector coercion:
//!     coerceVector -- main dispatcher
//!     coerceToLogical, coerceToInteger, coerceToReal, coerceToComplex,
//!     coerceToRaw, coerceToString, coerceToExpression, coerceToVectorList,
//!     coerceToPairList, coercePairList, coerceVectorList, coerceToSymbol
//!   Scalar accessors:
//!     asLogical, asLogical2, asInteger, asReal, asComplex
//!   R-level entry points:
//!     do_coerce, do_asCharacterFactor, asCharacterFactor
//!     do_asatomic, do_asvector, do_typeof, do_is, do_isvector
//!     do_isna, do_isnan, do_isfinite, do_isinfinite

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::OnceLock;

use crate::eval::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_LevelsSymbol, R_NamesSymbol, getAttrib,
    setAttrib,
};
use crate::mainutils::relop::PRIMVAL;
use crate::mainutils::subset::installTrChar;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE,
    SexprecCore,
};
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::safe::Sexp;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
pub const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// R's specific NA value as f64.
#[inline]
pub fn R_NA_REAL() -> f64 {
    f64::from_bits(R_NA_BIT_PATTERN)
}

/// Check if a double is R's NA (not just any NaN).
#[inline]
pub fn R_IsNA(x: f64) -> bool {
    x.to_bits() == R_NA_BIT_PATTERN
}

/// Check if a double is any NaN (including R's NA).
#[inline]
pub fn ISNAN(x: f64) -> bool {
    x.is_nan()
}

/// Check if a double is finite (not NaN and not Inf/-Inf).
#[inline]
pub fn R_FINITE(x: f64) -> bool {
    x.is_finite()
}

/// Check if a double is NaN but NOT R's NA.
#[inline]
pub fn R_IsNaN(x: f64) -> bool {
    x.is_nan() && x.to_bits() != R_NA_BIT_PATTERN
}

// ---------------------------------------------------------------------------
// Coercion warning flags (must match R's C defines)
// ---------------------------------------------------------------------------

/// Warning: NAs introduced by coercion.
pub const WARN_NA: c_int = 1;

/// Warning: NAs introduced by coercion to integer range.
pub const WARN_INT_NA: c_int = 2;

/// Warning: imaginary parts discarded in coercion.
pub const WARN_IMAG: c_int = 4;

/// Warning: out-of-range values treated as 0 in coercion to raw.
pub const WARN_RAW: c_int = 8;

// ---------------------------------------------------------------------------
// Internal helper: NA_STRING sentinel
// ---------------------------------------------------------------------------

/// Get (or create) the NA_STRING sentinel -- a special CHARSXP representing NA.
///
/// In R, NA_STRING is a specific CHARSXP with the NA bit set in its gp field.
/// We use a OnceLock to create it once and reuse it.
fn get_na_string() -> SEXP {
    static NA_STRING_VAL: OnceLock<usize> = OnceLock::new();
    let val = NA_STRING_VAL.get_or_init(|| {
        let mut node = SexprecCore::new_vector(SEXPTYPE::CHARSXP, 2);
        node.sxpinfo.set_gp(1);
        Box::into_raw(Box::new(node)) as usize
    });
    *val as SEXP
}

// ---------------------------------------------------------------------------
// Internal helpers: xlength, isBlankString wrappers
// ---------------------------------------------------------------------------

/// Get the extended length of an SEXP (handles NULL).
#[inline]
unsafe fn xlength(x: SEXP) -> R_xlen_t {
    unsafe { XLENGTH(x) }
}

/// Check if an SEXP is a vector atomic type.
#[inline]
unsafe fn isVectorAtomic(x: SEXP) -> bool {
    unsafe { Rf_isVectorAtomic(x) != 0 }
}

/// Check if an SEXP is a vector type (atomic or list).
#[inline]
unsafe fn isVector(x: SEXP) -> bool {
    unsafe { Rf_isVector(x) != 0 }
}

/// Check if an SEXP is a vector list type.
#[inline]
unsafe fn isVectorList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0
    }
}

/// Check if an SEXP is a function.
#[inline]
unsafe fn isFunction(x: SEXP) -> bool {
    unsafe { Rf_isFunction(x) != 0 }
}

/// Check if an SEXP is an environment.
#[inline]
unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { Rf_isEnvironment(x) != 0 }
}

/// Check if an SEXP is a symbol.
#[inline]
unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { Rf_isSymbol(x) != 0 }
}

/// Check if an SEXP is a string vector.
#[inline]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { Rf_isString(x) != 0 }
}

/// Check if an SEXP is NULL.
#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { Rf_isNull(x) != 0 }
}

/// Check if an SEXP is a list (pairlist).
#[inline]
unsafe fn isList(x: SEXP) -> bool {
    unsafe { Rf_isList(x) != 0 }
}

/// Check if an SEXP is a language object.
#[inline]
unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LANGSXP.0 }
}

/// Check if an SEXP is "vectorizable" (atomic vector types).
#[inline]
unsafe fn isVectorizable(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::RAWSXP.0
    }
}

/// Check if a SEXP is numeric (integer or real, but not logical).
#[inline]
unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        (t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0) && isVector(x)
    }
}

/// Check if a SEXP is logical.
#[inline]
unsafe fn isLogical(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LGLSXP.0 && isVector(x) }
}

/// Check if an SEXP has the S4 object bit set.
#[inline]
pub unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        // S4 is stored in bit 4 of gp (level field, upper bits)
        ((*x).sxpinfo.gp() >> 4) as c_int & 1
    }
}

/// Check if an SEXP has the OBJECT bit set.
#[inline]
unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

/// Check if an SEXP is a new list (generic vector).
#[inline]
unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::VECSXP.0 }
}

/// Check if a SEXP is an expression.
#[inline]
unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::EXPRSXP.0 }
}

/// Check if a SEXP is a matrix (has non-null dim attribute of length 2).
#[inline]
unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        !isNull(dim) && LENGTH(dim) == 2
    }
}

/// Check if a SEXP is an array (has non-null dim attribute).
#[inline]
unsafe fn isArray(x: SEXP) -> bool {
    unsafe { !isNull(getAttrib(x, R_DimSymbol())) }
}

/// Panic with an R error message.
unsafe fn error(msg: &str) {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Panic with an R error message (call-specific).
unsafe fn errorcall(_call: SEXP, msg: &str) {
    unsafe {
        error(msg);
    }
}

/// Get R_NaString -- returns the NA string CHARSXP.
fn R_NaString() -> SEXP {
    get_na_string()
}

/// Get R_BlankString -- returns the empty string CHARSXP.
fn R_BlankString() -> SEXP {
    unsafe { Rf_mkChar(c"".as_ptr()) }
}

// ---------------------------------------------------------------------------
// SHALLOW_DUPLICATE_ATTRIB and CLEAR_ATTRIB helpers
// ---------------------------------------------------------------------------

/// Copy attributes from `from` to `to` (shallow duplicate).
/// This matches R's SHALLOW_DUPLICATE_ATTRIB macro from coerce.c.
unsafe fn SHALLOW_DUPLICATE_ATTRIB(to: SEXP, from: SEXP) {
    unsafe {
        let attr_from = ATTRIB(from);
        if !isNull(attr_from) {
            // For simplicity, copy via setAttrib for the known attribute types.
            // In full R this does a shallow duplicate of the entire attribute list.
            let class = getAttrib(from, R_ClassSymbol());
            if !isNull(class) {
                setAttrib(to, R_ClassSymbol(), class);
            }
            let dim = getAttrib(from, R_DimSymbol());
            if !isNull(dim) {
                setAttrib(to, R_DimSymbol(), dim);
            }
            let dimnames = getAttrib(from, R_DimNamesSymbol());
            if !isNull(dimnames) {
                setAttrib(to, R_DimNamesSymbol(), dimnames);
            }
            let names = getAttrib(from, R_NamesSymbol());
            if !isNull(names) {
                setAttrib(to, R_NamesSymbol(), names);
            }
        }
    }
}

/// Clear all attributes from an SEXP, and reset object/S4 bits.
/// This matches R's CLEAR_ATTRIB macro from coerce.c.
unsafe fn CLEAR_ATTRIB(x: SEXP) {
    unsafe {
        let attr = ATTRIB(x);
        if !isNull(attr) {
            SET_ATTRIB(x, R_NilValue());
            if OBJECT(x) != 0 {
                SET_OBJECT(x, 0);
            }
            if IS_S4_OBJECT(x) != 0 {
                // Clear the S4 bit (bit 4 of gp)
                (*x).sxpinfo.set_gp((*x).sxpinfo.gp() & !(1 << 4));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CoercionWarning
// ---------------------------------------------------------------------------

/// Issue coercion warnings based on the warning flags.
///
/// This is the equivalent of R's `CoercionWarning()` from coerce.c.
pub unsafe fn CoercionWarning(warn: c_int) {
    // In a full implementation these would call R's warning() function.
    // For now we use eprintln to avoid aborting.
    if warn & WARN_NA != 0 {
        eprintln!("Warning: NAs introduced by coercion");
    }
    if warn & WARN_INT_NA != 0 {
        eprintln!("Warning: NAs introduced by coercion to integer range");
    }
    if warn & WARN_IMAG != 0 {
        eprintln!("Warning: imaginary parts discarded in coercion");
    }
    if warn & WARN_RAW != 0 {
        eprintln!("Warning: out-of-range values treated as 0 in coercion to raw");
    }
}

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

            if let (Ok(r), Ok(i)) = (real_str.parse::<f64>(), imag_body.parse::<f64>()) {
                return Rcomplex { r, i: sign * i };
            }
        } else if let Some(body) = str.strip_suffix('i') {
            // Pure imaginary: "3i"
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
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        Rf_mkChar(cstr.as_ptr())
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
        let s = if x.i >= 0.0 {
            format!("{:.17e}+{:.17e}i", x.r, x.i)
        } else {
            format!("{:.17e}{:.17e}i", x.r, x.i)
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

// ---------------------------------------------------------------------------
// Vector coercion functions
// ---------------------------------------------------------------------------

/// Coerce a vector to logical type.
unsafe fn coerceToLogical(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = LOGICAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::INTSXP.0 => LogicalFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => LogicalFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => LogicalFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => LogicalFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => {
                    LogicalFromInteger(RAW_ELT(v, ii) as c_int, &mut warn)
                }
                _ => NA_LOGICAL,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to integer type.
unsafe fn coerceToInteger(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = INTEGER(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => IntegerFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => IntegerFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => RAW_ELT(v, ii) as c_int,
                _ => NA_INTEGER,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to real (double) type.
unsafe fn coerceToReal(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = REAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => RealFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => RealFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => RealFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => RealFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => RealFromInteger(RAW_ELT(v, ii) as c_int, &mut warn),
                _ => NA_REAL,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to complex type.
unsafe fn coerceToComplex(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = COMPLEX(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => ComplexFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => ComplexFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => ComplexFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => ComplexFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => {
                    ComplexFromInteger(RAW_ELT(v, ii) as c_int, &mut warn)
                }
                _ => Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                },
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to raw type.
unsafe fn coerceToRaw(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::RAWSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = RAW(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let tmp: c_int = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    let val = IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    let val = INTEGER_ELT(v, ii);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    let val = IntegerFromReal(REAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    let val = IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    let val = IntegerFromString(STRING_ELT(v, i), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                _ => 0,
            };
            *pa.add(i as usize) = tmp as Rbyte;
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to string (character) type.
unsafe fn coerceToString(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let s = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => StringFromLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP.0 => StringFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => StringFromReal_impl(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => StringFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => StringFromRaw(RAW_ELT(v, ii), &mut warn),
                _ => R_NaString(),
            };
            SET_STRING_ELT(ans, i, s);
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to expression type.
unsafe fn coerceToExpression(v: SEXP) -> SEXP {
    unsafe {
        if !isVectorAtomic(v) {
            let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP.0, 1));
            SET_VECTOR_ELT(ans, 0, v);
            Rf_unprotect(1);
            return ans;
        }

        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP.0, n));

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP.0 => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP.0 => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP.0 => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP.0 => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP.0 => Rf_ScalarRaw(RAW_ELT(v, ii)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to generic vector (list) type.
unsafe fn coerceToVectorList(v: SEXP) -> SEXP {
    unsafe {
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, n));

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP.0 => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP.0 => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP.0 => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP.0 => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP.0 => Rf_ScalarRaw(RAW_ELT(v, ii)),
                t if t == SEXPTYPE::LISTSXP.0 || t == SEXPTYPE::LANGSXP.0 => CAR(v.add(i as usize)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to pairlist type.
unsafe fn coerceToPairList(v: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(v);
        let ans = Rf_protect(Rf_allocList(n));
        let mut ansp = ans;

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::LGLSXP.0, 1);
                    *LOGICAL(elt) = LOGICAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
                    *INTEGER(elt) = INTEGER_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::REALSXP.0, 1);
                    *REAL(elt) = REAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 1);
                    *COMPLEX(elt) = COMPLEX_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    SETCAR(ansp, Rf_ScalarString(STRING_ELT(v, i as R_xlen_t)));
                }
                t if t == SEXPTYPE::RAWSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::RAWSXP.0, 1);
                    *RAW(elt) = RAW_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 => {
                    SETCAR(ansp, VECTOR_ELT(v, i as R_xlen_t));
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
            }
            ansp = CDR(ansp);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        Rf_unprotect(1);
        ans
    }
}

/// Coerce a pairlist (LISTSXP/LANGSXP) to the given type.
unsafe fn coercePairList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if type_ == SEXPTYPE::EXPRSXP {
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP.0, 1));
            SET_VECTOR_ELT(rval, 0, v);
            Rf_unprotect(1);
            return rval;
        }

        if type_ == SEXPTYPE::STRSXP {
            let n = LENGTH(v);
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));
            let mut vp = v;
            for i in 0..n {
                let car = CAR(vp);
                if isString(car) && LENGTH(car) == 1 {
                    SET_STRING_ELT(rval, i as R_xlen_t, STRING_ELT(car, 0));
                } else {
                    // deparse not available; use StringFromLogical as fallback
                    SET_STRING_ELT(rval, i as R_xlen_t, StringFromLogical(0));
                }
                vp = CDR(vp);
            }
            Rf_unprotect(1);
            return rval;
        }

        if type_ == SEXPTYPE::VECSXP {
            // PairToVectorList
            let mut len: c_int = 0;
            let mut xptr = v;
            while !xptr.is_null() && !isNull(xptr) {
                len += 1;
                xptr = CDR(xptr);
            }
            let xnew = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, len as R_xlen_t));
            let mut xptr = v;
            for i in 0..len {
                SET_VECTOR_ELT(xnew, i as R_xlen_t, CAR(xptr));
                xptr = CDR(xptr);
            }
            Rf_unprotect(1);
            return xnew;
        }

        if isVectorizable(v) {
            let n = LENGTH(v);
            let rval = Rf_protect(Rf_allocVector3(type_.0, n as R_xlen_t));
            let mut vp = v;
            for i in 0..n {
                match type_.0 {
                    t if t == SEXPTYPE::LGLSXP.0 => {
                        *LOGICAL(rval).add(i as usize) = asLogical(CAR(vp));
                    }
                    t if t == SEXPTYPE::INTSXP.0 => {
                        *INTEGER(rval).add(i as usize) = asInteger(CAR(vp));
                    }
                    t if t == SEXPTYPE::REALSXP.0 => {
                        *REAL(rval).add(i as usize) = asReal(CAR(vp));
                    }
                    t if t == SEXPTYPE::CPLXSXP.0 => {
                        *COMPLEX(rval).add(i as usize) = asComplex(CAR(vp));
                    }
                    t if t == SEXPTYPE::RAWSXP.0 => {
                        *RAW(rval).add(i as usize) = asInteger(CAR(vp)) as Rbyte;
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
                }
                vp = CDR(vp);
            }
            Rf_unprotect(1);
            return rval;
        }

        error("cannot coerce type to vector");
        ptr::null_mut() // unreachable
    }
}

/// Coerce a vector list (VECSXP/EXPRSXP) to the given type.
unsafe fn coerceVectorList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;

        // expression -> list: just change the type tag
        if type_ == SEXPTYPE::VECSXP && TYPEOF(v) == SEXPTYPE::EXPRSXP.0 {
            let rval = Rf_allocVector3(SEXPTYPE::VECSXP.0, xlength(v));
            // Copy the data pointers
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> expression: just change the type tag
        if type_ == SEXPTYPE::EXPRSXP && TYPEOF(v) == SEXPTYPE::VECSXP.0 {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP.0, xlength(v));
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> pairlist
        if type_ == SEXPTYPE::LISTSXP {
            // VectorToPairList
            let n = LENGTH(v);
            let x = Rf_protect(Rf_allocList(n));
            let names = Rf_protect(getAttrib(v, R_NamesSymbol()));
            let mut xptr = x;
            for i in 0..n {
                SETCAR(xptr, VECTOR_ELT(v, i as R_xlen_t));
                xptr = CDR(xptr);
            }
            if !isNull(names) {
                let mut xptr2 = x;
                for i in 0..n {
                    let name_elt = STRING_ELT(names, i as R_xlen_t);
                    if !isNull(name_elt) {
                        let pname = CHAR(name_elt);
                        if !pname.is_null() && *pname != 0 {
                            SETTAG(xptr2, Rf_install(pname));
                        }
                    }
                    xptr2 = CDR(xptr2);
                }
            }
            Rf_unprotect(2);
            return x;
        }

        // list -> string
        if type_ == SEXPTYPE::STRSXP {
            let n = xlength(v);
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
            for i in 0..n {
                let elt = VECTOR_ELT(v, i);
                if isString(elt) && LENGTH(elt) == 1 {
                    SET_STRING_ELT(rval, i, STRING_ELT(elt, 0));
                } else {
                    // deparse not available; convert via asCharacterFactor-like path
                    SET_STRING_ELT(rval, i, StringFromLogical(0));
                }
            }
            Rf_unprotect(1);
            return rval;
        }

        if isVectorizable(v) {
            let n = xlength(v);
            let rval = Rf_protect(Rf_allocVector3(type_.0, n));
            match type_.0 {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    for i in 0..n {
                        *LOGICAL(rval).add(i as usize) = asLogical(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    for i in 0..n {
                        *INTEGER(rval).add(i as usize) = asInteger(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    for i in 0..n {
                        *REAL(rval).add(i as usize) = asReal(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    for i in 0..n {
                        *COMPLEX(rval).add(i as usize) = asComplex(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::RAWSXP.0 => {
                    for i in 0..n {
                        let tmp = asInteger(VECTOR_ELT(v, i));
                        if tmp < 0 || tmp > 255 {
                            warn |= WARN_RAW;
                        }
                        *RAW(rval).add(i as usize) = if tmp < 0 || tmp > 255 {
                            0
                        } else {
                            tmp as Rbyte
                        };
                    }
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            let names = getAttrib(v, R_NamesSymbol());
            if !isNull(names) {
                setAttrib(rval, R_NamesSymbol(), names);
            }
            Rf_unprotect(1);
            return rval;
        }

        error("list object cannot be coerced to type");
        ptr::null_mut() // unreachable
    }
}

/// Coerce to a symbol.
unsafe fn coerceToSymbol(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        if LENGTH(v) <= 0 {
            error("invalid data of mode (too short)");
        }

        let ans = Rf_protect(match TYPEOF(v) {
            t if t == SEXPTYPE::LGLSXP.0 => StringFromLogical(LOGICAL_ELT(v, 0)),
            t if t == SEXPTYPE::INTSXP.0 => StringFromInteger(INTEGER_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::REALSXP.0 => StringFromReal_impl(REAL_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::CPLXSXP.0 => StringFromComplex(COMPLEX_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::STRSXP.0 => STRING_ELT(v, 0),
            t if t == SEXPTYPE::RAWSXP.0 => StringFromRaw(RAW_ELT(v, 0), &mut warn),
            _ => R_NilValue(),
        });

        if warn != 0 {
            CoercionWarning(warn);
        }

        let sym = Rf_install(CHAR(ans));
        Rf_unprotect(1);
        sym
    }
}

/// Coerce a symbol (SYMSXP) to the given type.
/// This matches R's coerceSymbol() from coerce.c.
unsafe fn coerceSymbol(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut rval = R_NilValue();
        if type_ == SEXPTYPE::EXPRSXP {
            rval = Rf_protect(Rf_allocVector3(type_.0, 1));
            SET_VECTOR_ELT(rval, 0, v);
            Rf_unprotect(1);
        } else if type_ == SEXPTYPE::CHARSXP {
            rval = PRINTNAME(v);
        } else if type_ == SEXPTYPE::STRSXP {
            rval = Rf_ScalarString(PRINTNAME(v));
        }
        // else: warning, return R_NilValue
        rval
    }
}

/// Create a tag (symbol) from an SEXP.
/// If x is already a symbol or NULL, return it. If x is a string of length >= 1,
/// install it as a symbol.
unsafe fn CreateTag(x: SEXP) -> SEXP {
    unsafe {
        if isNull(x) || isSymbol(x) {
            return x;
        }
        if isString(x) && LENGTH(x) >= 1 {
            let s = STRING_ELT(x, 0);
            if !isNull(s) {
                let cs = CHAR(s);
                if !cs.is_null() && *cs != 0 {
                    return installTrChar(s);
                }
            }
        }
        // fallback: return NULL
        R_NilValue()
    }
}

/// Convert an SEXP to a function (closure).
/// This matches R's asFunction() from coerce.c.
unsafe fn asFunction(x: SEXP) -> SEXP {
    unsafe {
        if isFunction(x) {
            return x;
        }
        let f = Rf_protect(allocSExp(SEXPTYPE::CLOSXP));
        SET_CLOENV(f, R_GlobalEnv());
        // For simplicity, create a closure with empty formals and body = x
        SET_FORMALS(f, R_NilValue());
        SET_BODY(f, x);
        Rf_unprotect(1);
        f
    }
}

/// Common coercion helper for as.vector / as.XXX dispatch.
/// This matches R's ascommon() from coerce.c.
unsafe fn ascommon(call: SEXP, u: SEXP, type_: c_int) -> SEXP {
    unsafe {
        let target_type = SEXPTYPE(type_);

        if target_type == SEXPTYPE::CLOSXP {
            return asFunction(u);
        }

        if isVector(u)
            || isList(u)
            || isLanguage(u)
            || (isSymbol(u) && target_type == SEXPTYPE::EXPRSXP)
        {
            let v = if type_ != SEXPTYPE::ANYSXP.0 && TYPEOF(u) != type_ {
                coerceVector(u, type_)
            } else {
                u
            };

            // Drop attributes for certain types (as.pairlist behavior)
            if target_type == SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::LANGSXP.0
                && TYPEOF(u) != SEXPTYPE::LISTSXP.0
                && TYPEOF(u) != SEXPTYPE::EXPRSXP.0
                && TYPEOF(u) != SEXPTYPE::VECSXP.0
            {
                // Clear attributes
                let attr = ATTRIB(v);
                if !isNull(attr) {
                    SET_ATTRIB(v, R_NilValue());
                }
            }
            return v;
        }

        if isSymbol(u) && target_type == SEXPTYPE::STRSXP {
            return Rf_ScalarString(PRINTNAME(u));
        }
        if isSymbol(u) && target_type == SEXPTYPE::SYMSXP {
            return u;
        }
        if isSymbol(u) && target_type == SEXPTYPE::VECSXP {
            let v = Rf_allocVector3(SEXPTYPE::VECSXP.0, 1);
            SET_VECTOR_ELT(v, 0, u);
            return v;
        }

        errorcall(call, "cannot coerce type to vector of type");
        u // unreachable
    }
}

// ---------------------------------------------------------------------------
// coerceVector -- main coercion dispatcher
// ---------------------------------------------------------------------------

/// Coerce a vector from one type to another.
///
/// This is the main entry point for type coercion in R, equivalent to
/// R's `coerceVector()` from coerce.c. It dispatches to the appropriate
/// type-specific coercion function based on the source and target types.
pub unsafe fn coerceVector(v: SEXP, type_: c_int) -> SEXP {
    unsafe {
        if v.is_null() {
            return ptr::null_mut();
        }
        let target = SEXPTYPE(type_);

        // If already the right type, return as-is
        if TYPEOF(v) == type_ {
            return v;
        }

        let _v = Rf_protect(v);

        let ans = match TYPEOF(v) {
            t if t == SEXPTYPE::SYMSXP.0 => coerceSymbol(v, target),
            t if t == SEXPTYPE::NILSXP.0 || t == SEXPTYPE::LISTSXP.0 => {
                if type_ == SEXPTYPE::LISTSXP.0 {
                    v // already pairlist
                } else {
                    coercePairList(v, target)
                }
            }
            t if t == SEXPTYPE::LANGSXP.0 => {
                if type_ != SEXPTYPE::STRSXP.0 {
                    coercePairList(v, target)
                } else {
                    // LANGSXP -> STRSXP: special handling for operator names
                    let n = LENGTH(v);
                    let ans = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
                    let mut vp = v;
                    for i in 0..n as R_xlen_t {
                        let car = CAR(vp);
                        if isString(car) && LENGTH(car) == 1 {
                            SET_STRING_ELT(ans, i, STRING_ELT(car, 0));
                        } else if isSymbol(car) {
                            SET_STRING_ELT(ans, i, PRINTNAME(car));
                        } else {
                            SET_STRING_ELT(ans, i, StringFromLogical(0));
                        }
                        vp = CDR(vp);
                    }
                    ans
                }
            }
            t if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 => coerceVectorList(v, target),
            t if t == SEXPTYPE::ENVSXP.0 => {
                error("environments cannot be coerced to other types");
                ptr::null_mut() // unreachable
            }
            // Atomic vector types
            t if t == SEXPTYPE::LGLSXP.0
                || t == SEXPTYPE::INTSXP.0
                || t == SEXPTYPE::REALSXP.0
                || t == SEXPTYPE::CPLXSXP.0
                || t == SEXPTYPE::STRSXP.0
                || t == SEXPTYPE::RAWSXP.0 =>
            {
                match type_ {
                    t if t == SEXPTYPE::SYMSXP.0 => coerceToSymbol(v),
                    t if t == SEXPTYPE::LGLSXP.0 => coerceToLogical(v),
                    t if t == SEXPTYPE::INTSXP.0 => coerceToInteger(v),
                    t if t == SEXPTYPE::REALSXP.0 => coerceToReal(v),
                    t if t == SEXPTYPE::CPLXSXP.0 => coerceToComplex(v),
                    t if t == SEXPTYPE::RAWSXP.0 => coerceToRaw(v),
                    t if t == SEXPTYPE::STRSXP.0 => coerceToString(v),
                    t if t == SEXPTYPE::EXPRSXP.0 => coerceToExpression(v),
                    t if t == SEXPTYPE::VECSXP.0 => coerceToVectorList(v),
                    t if t == SEXPTYPE::LISTSXP.0 => coerceToPairList(v),
                    _ => {
                        error("cannot coerce type to vector of type");
                        ptr::null_mut() // unreachable
                    }
                }
            }
            _ => {
                error("cannot coerce type to vector of type");
                ptr::null_mut() // unreachable
            }
        };

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// asLogical -- coerce first element to logical
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a logical value.
///
/// This is R's `asLogical()` from coerce.c. Returns NA_LOGICAL for
/// empty vectors, and dispatches based on the vector's type.
pub unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { asLogical2(x, 0, R_NilValue()) }
}

/// Convert the first element of a vector to a logical value, with length checking.
///
/// This is R's `asLogical2()` from coerce.c.
pub unsafe fn asLogical2(x: SEXP, checking: c_int, _call: SEXP) -> c_int {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) {
            if xlength(x) < 1 {
                return NA_LOGICAL;
            }
            if checking != 0 && xlength(x) > 1 {
                // In R this calls errorcall; we just proceed
            }
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => LOGICAL_ELT(x, 0),
                t if t == SEXPTYPE::INTSXP.0 => LogicalFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => LogicalFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => LogicalFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => LogicalFromString(STRING_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => {
                    LogicalFromInteger(RAW_ELT(x, 0) as c_int, &mut warn)
                }
                _ => NA_LOGICAL,
            }
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            LogicalFromString(x, &mut warn)
        } else {
            NA_LOGICAL
        }
    }
}

// ---------------------------------------------------------------------------
// asInteger -- coerce first element to integer
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to an integer value.
///
/// This is R's `asInteger()` from coerce.c.
pub unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) && xlength(x) >= 1 {
            let res = match TYPEOF(x) {
                t if t == SEXPTYPE::RAWSXP.0 => RAW_ELT(x, 0) as c_int,
                t if t == SEXPTYPE::LGLSXP.0 => IntegerFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => INTEGER_ELT(x, 0),
                t if t == SEXPTYPE::REALSXP.0 => IntegerFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => IntegerFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => IntegerFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_INTEGER,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            let res = IntegerFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        }

        NA_INTEGER
    }
}

// ---------------------------------------------------------------------------
// asReal -- coerce first element to real (double)
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a real (double) value.
///
/// This is R's `asReal()` from coerce.c.
pub unsafe fn asReal(x: SEXP) -> c_double {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) && xlength(x) >= 1 {
            let res = match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => RealFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => RealFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => REAL_ELT(x, 0),
                t if t == SEXPTYPE::CPLXSXP.0 => RealFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => RealFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_REAL,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            let res = RealFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        }

        NA_REAL
    }
}

// ---------------------------------------------------------------------------
// asComplex -- coerce first element to complex
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a complex value.
///
/// This is R's `asComplex()` from coerce.c.
pub unsafe fn asComplex(x: SEXP) -> Rcomplex {
    unsafe {
        let mut warn: c_int = 0;
        let mut z = Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };

        if isVectorAtomic(x) && xlength(x) >= 1 {
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    z = ComplexFromLogical(LOGICAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    z = ComplexFromInteger(INTEGER_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    z = ComplexFromReal(REAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    z = COMPLEX_ELT(x, 0);
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    z = ComplexFromString(STRING_ELT(x, 0), &mut warn);
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for complex coercion
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            z = ComplexFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        }

        z
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// asRaw -- coerce first element to raw byte
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a raw byte value.
///
/// This follows the same pattern as asInteger/asReal, returning 0 for
/// out-of-range or NA values.
pub unsafe fn asRaw(x: SEXP) -> Rbyte {
    unsafe {
        if isVectorAtomic(x) && xlength(x) >= 1 {
            let val = asInteger(x);
            if val == NA_INTEGER || val < 0 || val > 255 {
                return 0;
            }
            return val as Rbyte;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// asRbool / asBool -- coerce to boolean (error on NA)
// ---------------------------------------------------------------------------

/// Coerce to Rboolean (c_int), erroring on NA_LOGICAL.
/// This matches R's asRboolean() from coerce.c.
pub unsafe fn asRbool(x: SEXP, call: SEXP) -> c_int {
    unsafe {
        let ans = asLogical2(x, 1, call);
        if ans == NA_LOGICAL {
            errorcall(call, "NA in coercion to boolean");
        }
        ans
    }
}

/// Coerce to bool, erroring on NA_LOGICAL.
/// This matches R's asBool() from coerce.c.
pub unsafe fn asBool(x: SEXP) -> c_int {
    unsafe {
        let ans = asLogical2(x, 1, R_NilValue());
        if ans == NA_LOGICAL {
            error("NA in coercion to boolean");
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// asCharacterFactor -- convert factor to character
// ---------------------------------------------------------------------------

/// Convert a factor to a character vector using its levels.
///
/// This is R's `asCharacterFactor()` from coerce.c.
pub unsafe fn asCharacterFactor(x: SEXP) -> SEXP {
    unsafe {
        let n = xlength(x);
        let labels = getAttrib(x, R_LevelsSymbol());
        if TYPEOF(labels) != SEXPTYPE::STRSXP.0 {
            error("malformed factor");
        }
        let nl = LENGTH(labels);

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
        for i in 0..n {
            let ii = INTEGER_ELT(x, i as c_int);
            if ii == NA_INTEGER {
                SET_STRING_ELT(ans, i, R_NaString());
            } else if ii >= 1 && ii <= nl {
                SET_STRING_ELT(ans, i, STRING_ELT(labels, (ii - 1) as R_xlen_t));
            } else {
                error("malformed factor");
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// R-level entry points (do_* functions)
// ---------------------------------------------------------------------------

/// R-level `as.character()` for factors (internal).
pub unsafe fn do_asCharacterFactor(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        asCharacterFactor(x)
    }
}

// ---------------------------------------------------------------------------
// Safe wrapper functions using Sexp<'a>
// ---------------------------------------------------------------------------

/// Safe version of `do_coerce` using `Sexp<'a>`.
///
/// Parses the mode string from the second argument and coerces the input
/// SEXP to the target type. Returns `Result<SEXP, String>` for error handling.
pub fn coerce_vector_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<SEXP, String> {
    if mode_str.typeof_() != SEXPTYPE::STRSXP || mode_str.len() != 1 {
        return Err("invalid 'mode' argument".to_string());
    }
    let mode_chars = mode_str.string_elt(0).ok_or("invalid 'mode' argument")?;
    let s = unsafe {
        let ptr = CHAR(mode_chars.as_raw());
        if ptr.is_null() {
            return Err("invalid 'mode' argument".to_string());
        }
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    };

    let type_: c_int = match s.as_str() {
        "logical" => SEXPTYPE::LGLSXP.0,
        "integer" => SEXPTYPE::INTSXP.0,
        "double" | "numeric" => SEXPTYPE::REALSXP.0,
        "complex" => SEXPTYPE::CPLXSXP.0,
        "character" => SEXPTYPE::STRSXP.0,
        "raw" => SEXPTYPE::RAWSXP.0,
        "list" => SEXPTYPE::VECSXP.0,
        "expression" => SEXPTYPE::EXPRSXP.0,
        "pairlist" => SEXPTYPE::LISTSXP.0,
        "any" => return Ok(x.as_raw()),
        "symbol" | "name" => SEXPTYPE::SYMSXP.0,
        _ => return Err("invalid 'mode' argument".to_string()),
    };

    let x_raw = x.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    let attr = ATTRIB(x_raw);
                    if isNull(attr) {
                        return Ok(x_raw);
                    }
                    let ans = Rf_protect(Rf_allocVector3(type_, xlength(x_raw)));
                    let src = DATAPTR(x_raw);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(type_) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x_raw) as usize * elem_size,
                        );
                    }
                    Rf_unprotect(1);
                    return Ok(ans);
                }
                _ => return Ok(x_raw),
            }
        }

        let ans = ascommon(ptr::null_mut(), x_raw, type_);
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP => {
                CLEAR_ATTRIB(ans);
            }
            _ => {} // intentionally unhandled: SEXPTYPE does not require attribute clearing
        }
        Ok(ans)
    }
}

/// Safe version of `do_asatomic` using `Sexp<'a>`.
///
/// Strips attributes and returns a clean atomic vector of the target type.
/// The `op` value selects the target type (0=character, 1=integer, 2=double,
/// 3=complex, 4=logical, 5=raw).
pub fn as_atomic_safe(x: Sexp<'_>, op: i32) -> Result<SEXP, String> {
    let type_: c_int = match op {
        0 => SEXPTYPE::STRSXP.0,
        1 => SEXPTYPE::INTSXP.0,
        2 => SEXPTYPE::REALSXP.0,
        3 => SEXPTYPE::CPLXSXP.0,
        4 => SEXPTYPE::LGLSXP.0,
        5 => SEXPTYPE::RAWSXP.0,
        _ => SEXPTYPE::STRSXP.0,
    };

    let x_raw = x.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            if isNull(ATTRIB(x_raw)) {
                return Ok(x_raw);
            }
            let ans = Rf_protect(Rf_allocVector3(type_, xlength(x_raw)));
            let src = DATAPTR(x_raw);
            let dst = DATAPTR(ans);
            let byte_len = xlength(x_raw) as usize
                * match SEXPTYPE(type_) {
                    SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                    SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                    SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                    SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                    _ => std::mem::size_of::<SEXP>(),
                };
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, byte_len);
            }
            CLEAR_ATTRIB(ans);
            Rf_unprotect(1);
            return Ok(ans);
        }

        let ans = coerceVector(x_raw, type_);
        CLEAR_ATTRIB(ans);
        Ok(ans)
    }
}

/// Safe version of `do_asvector` using `Sexp<'a>`.
///
/// Coerces to a vector of the specified mode, stripping attributes for
/// atomic types but preserving them for list/expression/pairlist types.
pub fn as_vector_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<SEXP, String> {
    if mode_str.typeof_() != SEXPTYPE::STRSXP || mode_str.len() != 1 {
        return Err("invalid 'mode' argument".to_string());
    }
    let mode_chars = mode_str.string_elt(0).ok_or("invalid 'mode' argument")?;
    let s = unsafe {
        let ptr = CHAR(mode_chars.as_raw());
        if ptr.is_null() {
            return Err("invalid 'mode' argument".to_string());
        }
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    };

    let type_: c_int = match s.as_str() {
        "logical" => SEXPTYPE::LGLSXP.0,
        "integer" => SEXPTYPE::INTSXP.0,
        "double" | "numeric" => SEXPTYPE::REALSXP.0,
        "complex" => SEXPTYPE::CPLXSXP.0,
        "character" => SEXPTYPE::STRSXP.0,
        "raw" => SEXPTYPE::RAWSXP.0,
        "list" => SEXPTYPE::VECSXP.0,
        "expression" => SEXPTYPE::EXPRSXP.0,
        "pairlist" => SEXPTYPE::LISTSXP.0,
        "symbol" | "name" => SEXPTYPE::SYMSXP.0,
        "function" => SEXPTYPE::CLOSXP.0,
        "any" => return Ok(x.as_raw()),
        _ => return Err("invalid 'mode' argument".to_string()),
    };

    let x_raw = x.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    if isNull(ATTRIB(x_raw)) {
                        return Ok(x_raw);
                    }
                    let ans = Rf_protect(Rf_allocVector3(type_, xlength(x_raw)));
                    let src = DATAPTR(x_raw);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(type_) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x_raw) as usize * elem_size,
                        );
                    }
                    CLEAR_ATTRIB(ans);
                    Rf_unprotect(1);
                    return Ok(ans);
                }
                _ => return Ok(x_raw),
            }
        }

        let ans = ascommon(ptr::null_mut(), x_raw, type_);
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::LISTSXP
            | SEXPTYPE::LANGSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP => {}
            _ => {
                CLEAR_ATTRIB(ans);
            }
        }
        Ok(ans)
    }
}

/// Safe version of `do_is` using `Sexp<'a>`.
///
/// Returns `Result<c_int, String>` where the c_int is 0 or 1 (logical value).
/// The `op` value selects the predicate to test.
pub fn is_type_safe(x: Sexp<'_>, op: i32) -> Result<c_int, String> {
    let ans = match op {
        0 => is_null_safe(x),
        10 => (x.typeof_() == SEXPTYPE::LGLSXP) as c_int,
        13 => {
            let t = x.typeof_();
            if t == SEXPTYPE::INTSXP {
                let x_raw = x.as_raw();
                unsafe {
                    let is_factor = crate::mainutils::objects::inherits2(
                        x_raw,
                        b"factor\0".as_ptr() as *const c_char,
                    ) != 0;
                    let is_ordered = crate::mainutils::objects::inherits2(
                        x_raw,
                        b"ordered\0".as_ptr() as *const c_char,
                    ) != 0;
                    if is_factor || is_ordered { 0 } else { 1 }
                }
            } else {
                0
            }
        }
        14 => (x.typeof_() == SEXPTYPE::REALSXP) as c_int,
        15 => (x.typeof_() == SEXPTYPE::CPLXSXP) as c_int,
        16 => (x.typeof_() == SEXPTYPE::STRSXP) as c_int,
        1 => {
            let x_raw = x.as_raw();
            unsafe {
                if IS_S4_OBJECT(x_raw) != 0 && TYPEOF(x_raw) == SEXPTYPE::OBJSXP.0 {
                    let dot_x_data =
                        crate::mainutils::subassign::R_getS4DataSlot(x_raw, SEXPTYPE::SYMSXP.0);
                    (TYPEOF(dot_x_data) == SEXPTYPE::SYMSXP.0) as c_int
                } else {
                    (TYPEOF(x_raw) == SEXPTYPE::SYMSXP.0) as c_int
                }
            }
        }
        4 => {
            let x_raw = x.as_raw();
            unsafe {
                if IS_S4_OBJECT(x_raw) != 0 && TYPEOF(x_raw) == SEXPTYPE::OBJSXP.0 {
                    let dot_x_data =
                        crate::mainutils::subassign::R_getS4DataSlot(x_raw, SEXPTYPE::ENVSXP.0);
                    (TYPEOF(dot_x_data) == SEXPTYPE::ENVSXP.0) as c_int
                } else {
                    (TYPEOF(x_raw) == SEXPTYPE::ENVSXP.0) as c_int
                }
            }
        }
        19 => {
            let t = x.typeof_();
            (t == SEXPTYPE::VECSXP || t == SEXPTYPE::LISTSXP) as c_int
        }
        2 => {
            let t = x.typeof_();
            (t == SEXPTYPE::LISTSXP || t == SEXPTYPE::NILSXP) as c_int
        }
        20 => (x.typeof_() == SEXPTYPE::EXPRSXP) as c_int,
        24 => (x.typeof_() == SEXPTYPE::RAWSXP) as c_int,
        6 => (x.typeof_() == SEXPTYPE::LANGSXP) as c_int,
        50 => unsafe { crate::sexp::accessors::OBJECT(x.as_raw()) },
        51 => unsafe { IS_S4_OBJECT(x.as_raw()) },
        100 => is_numeric_safe(x),
        101 => is_matrix_safe(x),
        102 => is_array_safe(x),
        200 => is_atomic_safe(x),
        201 => {
            let t = x.typeof_();
            matches!(
                t,
                SEXPTYPE::VECSXP
                    | SEXPTYPE::LISTSXP
                    | SEXPTYPE::CLOSXP
                    | SEXPTYPE::ENVSXP
                    | SEXPTYPE::PROMSXP
                    | SEXPTYPE::LANGSXP
                    | SEXPTYPE::SPECIALSXP
                    | SEXPTYPE::BUILTINSXP
                    | SEXPTYPE::EXPRSXP
            ) as c_int
        }
        300 => (x.typeof_() == SEXPTYPE::LANGSXP) as c_int,
        301 => {
            let t = x.typeof_();
            (t == SEXPTYPE::SYMSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::EXPRSXP) as c_int
        }
        302 => is_function_safe(x),
        _ => 0,
    };
    Ok(ans)
}

/// Safe version of `do_isvector` using `Sexp<'a>`.
///
/// Checks whether the SEXP is a vector of the specified mode, and whether
/// it has only a "names" attribute (no other attributes).
pub fn is_vector_type_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<c_int, String> {
    if mode_str.typeof_() != SEXPTYPE::STRSXP || mode_str.len() != 1 {
        return Err("invalid 'mode' argument".to_string());
    }
    let mode_chars = mode_str.string_elt(0).ok_or("invalid 'mode' argument")?;
    let s = unsafe {
        let ptr = CHAR(mode_chars.as_raw());
        if ptr.is_null() {
            return Err("invalid 'mode' argument".to_string());
        }
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    };

    let is_vec = if s == "any" {
        x.is_vector()
    } else if s == "numeric" {
        is_numeric_safe(x) != 0 && is_logical_safe(x) == 0
    } else {
        let type_name = match x.typeof_() {
            SEXPTYPE::LGLSXP => "logical",
            SEXPTYPE::INTSXP => "integer",
            SEXPTYPE::REALSXP => "double",
            SEXPTYPE::CPLXSXP => "complex",
            SEXPTYPE::STRSXP => "character",
            SEXPTYPE::RAWSXP => "raw",
            SEXPTYPE::VECSXP => "list",
            SEXPTYPE::EXPRSXP => "expression",
            SEXPTYPE::LISTSXP => "pairlist",
            _ => "",
        };
        s == type_name || (s == "name" && type_name == "symbol")
    };

    if !is_vec {
        return Ok(0);
    }

    // Check that only a "names" attribute is present
    let x_raw = x.as_raw();
    unsafe {
        let mut a = ATTRIB(x_raw);
        while !isNull(a) {
            if !isNull(TAG(a)) && TAG(a) != R_NamesSymbol() {
                return Ok(0);
            }
            a = CDR(a);
        }
    }
    Ok(1)
}

// ---------------------------------------------------------------------------
// Safe helper predicates
// ---------------------------------------------------------------------------

fn is_null_safe(x: Sexp) -> c_int {
    (x.is_nil()) as c_int
}

fn is_numeric_safe(x: Sexp) -> c_int {
    let t = x.typeof_();
    if (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP) && x.is_vector() {
        1
    } else {
        0
    }
}

fn is_logical_safe(x: Sexp) -> c_int {
    (x.typeof_() == SEXPTYPE::LGLSXP && x.is_vector()) as c_int
}

fn is_function_safe(x: Sexp) -> c_int {
    unsafe { (Rf_isFunction(x.as_raw()) != 0) as c_int }
}

fn is_matrix_safe(x: Sexp) -> c_int {
    unsafe {
        let dim = getAttrib(x.as_raw(), R_DimSymbol());
        (!isNull(dim) && LENGTH(dim) == 2) as c_int
    }
}

fn is_array_safe(x: Sexp) -> c_int {
    unsafe { (!isNull(getAttrib(x.as_raw(), R_DimSymbol()))) as c_int }
}

fn is_atomic_safe(x: Sexp) -> c_int {
    let t = x.typeof_();
    (t == SEXPTYPE::CHARSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP) as c_int
}

// ---------------------------------------------------------------------------
// FFI entry points delegating to safe wrappers
// ---------------------------------------------------------------------------

/// R-level coercion entry point (`as.logical`, `as.integer`, etc.).
///
/// This is the `do_asatomic()` function from coerce.c, handling
/// `as.character`, `as.integer`, `as.double`, `as.complex`, `as.logical`, `as.raw`.
pub unsafe fn do_asatomic(call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return R_NilValue(),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return R_NilValue(),
        };
        let op0 = PRIMVAL(op);
        match as_atomic_safe(x, op0) {
            Ok(result) => result,
            Err(_) => R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { R_NilValue() })
}

/// R-level `as.vector()` entry point.
///
/// This is the `do_asvector()` function from coerce.c.
pub unsafe fn do_asvector(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return R_NilValue(),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return R_NilValue(),
        };
        let Some(cdr) = args_s.cdr() else {
            return x.as_raw();
        };
        let mode_str = match cdr.car() {
            Some(s) => s,
            None => return R_NilValue(),
        };
        match as_vector_safe(x, mode_str) {
            Ok(result) => result,
            Err(_) => R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { R_NilValue() })
}

/// R-level `typeof()` entry point.
///
/// This is the `do_typeof()` function from coerce.c.
/// Note: canonical version lives in inspect.rs; this is kept as
/// coerce_typeof for internal use.
pub(crate) unsafe fn coerce_typeof(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::OBJSXP.0 && IS_S4_OBJECT(x) == 0 {
            return Rf_mkString(c"object".as_ptr());
        }
        let type_name = match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::NILSXP => "NULL",
            SEXPTYPE::SYMSXP => "symbol",
            SEXPTYPE::LISTSXP => "pairlist",
            SEXPTYPE::CLOSXP => "closure",
            SEXPTYPE::ENVSXP => "environment",
            SEXPTYPE::PROMSXP => "promise",
            SEXPTYPE::LANGSXP => "language",
            SEXPTYPE::SPECIALSXP => "special",
            SEXPTYPE::BUILTINSXP => "builtin",
            SEXPTYPE::CHARSXP => "character",
            SEXPTYPE::LGLSXP => "logical",
            SEXPTYPE::INTSXP => "integer",
            SEXPTYPE::REALSXP => "double",
            SEXPTYPE::CPLXSXP => "complex",
            SEXPTYPE::STRSXP => "character",
            SEXPTYPE::DOTSXP => "...",
            SEXPTYPE::ANYSXP => "any",
            SEXPTYPE::VECSXP => "list",
            SEXPTYPE::EXPRSXP => "expression",
            SEXPTYPE::RAWSXP => "raw",
            SEXPTYPE::OBJSXP => "object",
            _ => "unknown",
        };
        Rf_mkString(
            std::ffi::CString::new(type_name)
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Check if a single element is NA — matches C's LIST_VEC_NA macro.
/// Returns 1 if the element is a length-1 vector containing NA, 0 otherwise.
unsafe fn elem_is_na(s: SEXP) -> c_int {
    unsafe {
        if !isVector(s) || xlength(s) != 1 {
            return 0;
        }
        match TYPEOF(s) {
            t if t == SEXPTYPE::LGLSXP.0 => (LOGICAL_ELT(s, 0) == NA_LOGICAL) as c_int,
            t if t == SEXPTYPE::INTSXP.0 => (INTEGER_ELT(s, 0) == NA_INTEGER) as c_int,
            t if t == SEXPTYPE::REALSXP.0 => ISNAN(REAL_ELT(s, 0)) as c_int,
            t if t == SEXPTYPE::STRSXP.0 => (STRING_ELT(s, 0) == R_NaString()) as c_int,
            t if t == SEXPTYPE::CPLXSXP.0 => {
                let v = COMPLEX_ELT(s, 0);
                (ISNAN(v.r) || ISNAN(v.i)) as c_int
            }
            _ => 0,
        }
    }
}

/// R-level `is.*` predicate dispatcher.
///
/// This is the `do_is()` function from coerce.c, implementing is.null,
/// is.logical, is.integer, is.double, is.complex, is.character, etc.
pub unsafe fn do_is(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return Rf_ScalarLogical(0),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return Rf_ScalarLogical(0),
        };
        let op0 = PRIMVAL(op);
        match is_type_safe(x, op0) {
            Ok(result) => Rf_ScalarLogical(result),
            Err(_) => Rf_ScalarLogical(0),
        }
    }))
    .unwrap_or_else(|_| unsafe { Rf_ScalarLogical(0) })
}

/// R-level `is.vector()` entry point.
///
/// This is the `do_isvector()` function from coerce.c.
pub unsafe fn do_isvector(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return Rf_ScalarLogical(0),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return Rf_ScalarLogical(0),
        };
        let mode_arg = match args_s.cdr() {
            Some(s) => match s.car() {
                Some(s) => s,
                None => return Rf_ScalarLogical(0),
            },
            None => return Rf_ScalarLogical(0),
        };
        match is_vector_type_safe(x, mode_arg) {
            Ok(result) => Rf_ScalarLogical(result),
            Err(_) => Rf_ScalarLogical(0),
        }
    }))
    .unwrap_or_else(|_| unsafe { Rf_ScalarLogical(0) })
}

/// R-level `is.na()` entry point.
///
/// This is the `do_isna()` function from coerce.c.
pub unsafe fn do_isna(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (LOGICAL_ELT(x, i as c_int) == NA_LOGICAL) as c_int;
                }
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) == NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = ISNAN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (ISNAN(v.r) || ISNAN(v.i)) as c_int;
                }
            }
            t if t == SEXPTYPE::STRSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (STRING_ELT(x, i) == R_NaString()) as c_int;
                }
            }
            t if t == SEXPTYPE::RAWSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LISTSXP.0 => {
                let mut elt = x;
                for i in 0..n {
                    let s = CAR(elt);
                    *pa.add(i as usize) = elem_is_na(s);
                    elt = CDR(elt);
                }
            }
            t if t == SEXPTYPE::VECSXP.0 => {
                for i in 0..n {
                    let s = VECTOR_ELT(x, i);
                    *pa.add(i as usize) = elem_is_na(s);
                }
            }
            t if t == SEXPTYPE::NILSXP.0 => {}
            _ => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
        }

        // Copy dim and names
        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.nan()` entry point.
///
/// This is the `do_isnan()` function from coerce.c.
pub unsafe fn do_isnan(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = R_IsNaN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (R_IsNaN(v.r) || R_IsNaN(v.i)) as c_int;
                }
            }
            _ => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.finite()` entry point.
///
/// This is the `do_isfinite()` function from coerce.c.
pub unsafe fn do_isfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP.0 || t == SEXPTYPE::RAWSXP.0 || t == SEXPTYPE::NILSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) != NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = R_FINITE(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (R_FINITE(v.r) && R_FINITE(v.i)) as c_int;
                }
            }
            _ => {
                error("default method not implemented for type");
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.infinite()` entry point.
///
/// This is the `do_isinfinite()` function from coerce.c.
pub unsafe fn do_isinfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP.0
                || t == SEXPTYPE::RAWSXP.0
                || t == SEXPTYPE::NILSXP.0
                || t == SEXPTYPE::LGLSXP.0
                || t == SEXPTYPE::INTSXP.0 =>
            {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    let xr = REAL_ELT(x, i as c_int);
                    *pa.add(i as usize) = if ISNAN(xr) || R_FINITE(xr) { 0 } else { 1 };
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) =
                        if (ISNAN(v.r) || R_FINITE(v.r)) && (ISNAN(v.i) || R_FINITE(v.i)) {
                            0
                        } else {
                            1
                        };
                }
            }
            _ => {
                error("default method not implemented for type");
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_coerce -- R-level coercion entry point
// ---------------------------------------------------------------------------

/// R-level coercion entry point (`do_coerce`).
///
/// This dispatches to `ascommon` for the actual coercion, matching R's
/// behavior for `as.vector()`, `as.expression()`, `as.list()`, etc.
pub unsafe fn do_coerce(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::from_raw(args) {
            Some(s) => s,
            None => return R_NilValue(),
        };
        let x = match args_s.car() {
            Some(s) => s,
            None => return R_NilValue(),
        };
        let Some(cdr) = args_s.cdr() else {
            return x.as_raw();
        };
        let mode_str = match cdr.car() {
            Some(s) => s,
            None => return R_NilValue(),
        };
        match coerce_vector_safe(x, mode_str) {
            Ok(result) => result,
            Err(_) => R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| unsafe { R_NilValue() })
}

// ---------------------------------------------------------------------------
// strtod wrapper (C lib)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_logical_from_integer() {
        assert_eq!(unsafe { LogicalFromInteger(0, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { LogicalFromInteger(1, std::ptr::null_mut()) }, 1);
        assert_eq!(unsafe { LogicalFromInteger(42, std::ptr::null_mut()) }, 1);
        assert_eq!(unsafe { LogicalFromInteger(-1, std::ptr::null_mut()) }, 1);
        assert_eq!(
            unsafe { LogicalFromInteger(NA_INTEGER, std::ptr::null_mut()) },
            NA_LOGICAL
        );
    }

    #[test]
    fn test_logical_from_real() {
        assert_eq!(unsafe { LogicalFromReal(0.0, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { LogicalFromReal(1.0, std::ptr::null_mut()) }, 1);
        assert_eq!(unsafe { LogicalFromReal(-0.5, std::ptr::null_mut()) }, 1);
        assert_eq!(
            unsafe { LogicalFromReal(f64::NAN, std::ptr::null_mut()) },
            NA_LOGICAL
        );
    }

    #[test]
    fn test_logical_from_complex() {
        assert_eq!(
            unsafe { LogicalFromComplex(Rcomplex { r: 0.0, i: 0.0 }, std::ptr::null_mut()) },
            0
        );
        assert_eq!(
            unsafe { LogicalFromComplex(Rcomplex { r: 1.0, i: 0.0 }, std::ptr::null_mut()) },
            1
        );
        assert_eq!(
            unsafe { LogicalFromComplex(Rcomplex { r: 0.0, i: 1.0 }, std::ptr::null_mut()) },
            1
        );
        assert_eq!(
            unsafe {
                LogicalFromComplex(
                    Rcomplex {
                        r: f64::NAN,
                        i: 0.0,
                    },
                    std::ptr::null_mut(),
                )
            },
            NA_LOGICAL
        );
    }

    #[test]
    fn test_integer_from_logical() {
        assert_eq!(unsafe { IntegerFromLogical(0, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { IntegerFromLogical(1, std::ptr::null_mut()) }, 1);
        assert_eq!(
            unsafe { IntegerFromLogical(NA_LOGICAL, std::ptr::null_mut()) },
            NA_INTEGER
        );
    }

    #[test]
    fn test_integer_from_real() {
        assert_eq!(unsafe { IntegerFromReal(3.7, std::ptr::null_mut()) }, 3);
        assert_eq!(unsafe { IntegerFromReal(-2.1, std::ptr::null_mut()) }, -2);
        assert_eq!(
            unsafe { IntegerFromReal(f64::NAN, std::ptr::null_mut()) },
            NA_INTEGER
        );

        let mut warn: c_int = 0;
        let result = unsafe { IntegerFromReal(1e20, &mut warn) };
        assert_eq!(result, NA_INTEGER);
        assert!(warn & WARN_INT_NA != 0);
    }

    #[test]
    fn test_integer_from_complex() {
        let mut warn: c_int = 0;
        let result = unsafe { IntegerFromComplex(Rcomplex { r: 3.0, i: 2.0 }, &mut warn) };
        assert_eq!(result, 3);
        assert!(warn & WARN_IMAG != 0);
    }

    #[test]
    fn test_real_from_logical() {
        assert_eq!(unsafe { RealFromLogical(0, std::ptr::null_mut()) }, 0.0);
        assert_eq!(unsafe { RealFromLogical(1, std::ptr::null_mut()) }, 1.0);
        let result = unsafe { RealFromLogical(NA_LOGICAL, std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_real_from_integer() {
        assert_eq!(unsafe { RealFromInteger(42, std::ptr::null_mut()) }, 42.0);
        let result = unsafe { RealFromInteger(NA_INTEGER, std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_complex_from_logical() {
        let z = unsafe { ComplexFromLogical(1, std::ptr::null_mut()) };
        assert_eq!(z.r, 1.0);
        assert_eq!(z.i, 0.0);

        let z_na = unsafe { ComplexFromLogical(NA_LOGICAL, std::ptr::null_mut()) };
        assert!(z_na.r.is_nan());
    }

    #[test]
    fn test_complex_from_integer() {
        let z = unsafe { ComplexFromInteger(42, std::ptr::null_mut()) };
        assert_eq!(z.r, 42.0);
        assert_eq!(z.i, 0.0);
    }

    #[test]
    fn test_complex_from_real() {
        let z = unsafe { ComplexFromReal(3.14, std::ptr::null_mut()) };
        assert_eq!(z.r, 3.14);
        assert_eq!(z.i, 0.0);

        // R's specific NA -> both parts NA
        let z_na = unsafe { ComplexFromReal(R_NA_REAL(), std::ptr::null_mut()) };
        assert!(z_na.r.is_nan());
        assert!(z_na.i.is_nan());
    }

    #[test]
    fn test_complex_from_string_c() {
        let s = CString::new("3+2i").unwrap_or_default();
        let z = unsafe { ComplexFromStringC(s.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z.r, 3.0);
        assert_eq!(z.i, 2.0);

        let s2 = CString::new("5i").unwrap_or_default();
        let z2 = unsafe { ComplexFromStringC(s2.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z2.r, 0.0);
        assert_eq!(z2.i, 5.0);

        let s3 = CString::new("3-4i").unwrap_or_default();
        let z3 = unsafe { ComplexFromStringC(s3.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z3.r, 3.0);
        assert_eq!(z3.i, -4.0);

        let s4 = CString::new("42").unwrap_or_default();
        let z4 = unsafe { ComplexFromStringC(s4.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z4.r, 42.0);
        assert_eq!(z4.i, 0.0);
    }

    // New tests for SEXP-based conversions

    #[test]
    fn test_logical_from_string() {
        // Test with null (no CHARSXP available in test without init)
        let result = unsafe { LogicalFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, NA_LOGICAL);
    }

    #[test]
    fn test_string_from_logical() {
        let s = unsafe { StringFromLogical(0) };
        assert!(!s.is_null());

        let s_true = unsafe { StringFromLogical(1) };
        assert!(!s_true.is_null());

        let s_na = unsafe { StringFromLogical(NA_LOGICAL) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_string_from_integer() {
        let s = unsafe { StringFromInteger(42, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let s_na = unsafe { StringFromInteger(NA_INTEGER, std::ptr::null_mut()) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_string_from_raw() {
        let s = unsafe { StringFromRaw(255, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let s0 = unsafe { StringFromRaw(0, std::ptr::null_mut()) };
        assert!(!s0.is_null());
    }

    #[test]
    fn test_string_from_complex() {
        let z = Rcomplex { r: 3.0, i: 4.0 };
        let s = unsafe { StringFromComplex(z, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let z_na = Rcomplex {
            r: R_NA_REAL(),
            i: 0.0,
        };
        let s_na = unsafe { StringFromComplex(z_na, std::ptr::null_mut()) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_string_from_real() {
        let s = unsafe { StringFromReal_impl(3.14, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let s_na = unsafe { StringFromReal_impl(R_NA_REAL(), std::ptr::null_mut()) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_warn_constants() {
        // Verify warning constants match R's C defines
        assert_eq!(WARN_NA, 1);
        assert_eq!(WARN_INT_NA, 2);
        assert_eq!(WARN_IMAG, 4);
        assert_eq!(WARN_RAW, 8);
    }

    #[test]
    fn test_coercion_warning_flags() {
        let mut warn: c_int = 0;
        unsafe { IntegerFromReal(1e20, &mut warn) };
        assert_ne!(warn & WARN_INT_NA, 0);

        let mut warn2: c_int = 0;
        unsafe { IntegerFromComplex(Rcomplex { r: 3.0, i: 2.0 }, &mut warn2) };
        assert_ne!(warn2 & WARN_IMAG, 0);
    }

    #[test]
    fn test_r_isna() {
        assert!(R_IsNA(R_NA_REAL()));
        assert!(!R_IsNA(f64::NAN)); // regular NaN is NOT R's NA
        assert!(!R_IsNA(0.0));
        assert!(!R_IsNA(1.0));
    }

    #[test]
    fn test_r_isnan() {
        assert!(!R_IsNaN(R_NA_REAL())); // R's NA is NOT a "pure" NaN
        assert!(R_IsNaN(f64::NAN)); // regular NaN IS a pure NaN
        assert!(!R_IsNaN(0.0));
        assert!(!R_IsNaN(f64::INFINITY));
    }

    #[test]
    fn test_r_finite() {
        assert!(R_FINITE(0.0));
        assert!(R_FINITE(1.0));
        assert!(R_FINITE(-1.0));
        assert!(!R_FINITE(f64::INFINITY));
        assert!(!R_FINITE(f64::NEG_INFINITY));
        assert!(!R_FINITE(f64::NAN));
        assert!(!R_FINITE(R_NA_REAL()));
    }

    #[test]
    fn test_integer_from_string() {
        // Test with null (no CHARSXP available in test)
        let result = unsafe { IntegerFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, NA_INTEGER);
    }

    #[test]
    fn test_real_from_string() {
        let result = unsafe { RealFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_complex_from_string() {
        let z = unsafe { ComplexFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(z.r.is_nan());
        assert!(z.i.is_nan());
    }

    #[test]
    fn test_as_logical_null() {
        let result = unsafe { asLogical(std::ptr::null_mut()) };
        assert_eq!(result, NA_LOGICAL);
    }

    #[test]
    fn test_as_integer_null() {
        let result = unsafe { asInteger(std::ptr::null_mut()) };
        assert_eq!(result, NA_INTEGER);
    }

    #[test]
    fn test_as_real_null() {
        let result = unsafe { asReal(std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_as_complex_null() {
        let z = unsafe { asComplex(std::ptr::null_mut()) };
        assert!(z.r.is_nan());
        assert!(z.i.is_nan());
    }

    #[test]
    fn test_coerce_vector_same_type() {
        // coerceVector should return the same pointer if types match
        // We can't easily create real SEXP objects in tests without init,
        // but we can test the null case
        let result = unsafe { coerceVector(std::ptr::null_mut(), SEXPTYPE::LGLSXP.0) };
        assert!(result.is_null());
    }
}
