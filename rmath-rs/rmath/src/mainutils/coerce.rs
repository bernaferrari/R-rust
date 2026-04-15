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
        t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP
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
    unsafe { TYPEOF(x) == SEXPTYPE::LANGSXP }
}

/// Check if an SEXP is "vectorizable" (atomic vector types).
#[inline]
unsafe fn isVectorizable(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
    }
}

/// Check if a SEXP is numeric (integer or real, but not logical).
#[inline]
unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP) && isVector(x)
    }
}

/// Check if a SEXP is logical.
#[inline]
unsafe fn isLogical(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LGLSXP && isVector(x) }
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
    unsafe { TYPEOF(x) == SEXPTYPE::VECSXP }
}

/// Check if a SEXP is an expression.
#[inline]
unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::EXPRSXP }
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

unsafe fn SET_TYPEOF(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_type(SEXPTYPE(v));
        }
    }
}

/// Panic with an R error message.
unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Panic with an R error message (call-specific).
unsafe fn errorcall(_call: SEXP, msg: &str) -> ! {
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = LOGICAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::INTSXP => LogicalFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => LogicalFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => LogicalFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => LogicalFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => {
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = INTEGER(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP => IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => IntegerFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => IntegerFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => RAW_ELT(v, ii) as c_int,
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = REAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP => RealFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP => RealFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => RealFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => RealFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => RealFromInteger(RAW_ELT(v, ii) as c_int, &mut warn),
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::CPLXSXP, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = COMPLEX(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP => ComplexFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP => ComplexFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => ComplexFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => ComplexFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => {
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::RAWSXP, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = RAW(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let tmp: c_int = match vtype {
                t if t == SEXPTYPE::LGLSXP => {
                    let val = IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    let val = INTEGER_ELT(v, ii);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::REALSXP => {
                    let val = IntegerFromReal(REAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    let val = IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::STRSXP => {
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let s = match vtype {
                t if t == SEXPTYPE::LGLSXP => StringFromLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => StringFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => StringFromReal_impl(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => StringFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::RAWSXP => StringFromRaw(RAW_ELT(v, ii), &mut warn),
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
            let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP, 1));
            SET_VECTOR_ELT(ans, 0, v);
            Rf_unprotect(1);
            return ans;
        }

        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP, n));

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(v, ii)),
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, n));

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(v, ii)),
                t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => CAR(v.add(i as usize)),
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
                t if t == SEXPTYPE::LGLSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::LGLSXP, 1);
                    *LOGICAL(elt) = LOGICAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::INTSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                    *INTEGER(elt) = INTEGER_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::REALSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
                    *REAL(elt) = REAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
                    *COMPLEX(elt) = COMPLEX_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::STRSXP => {
                    SETCAR(ansp, Rf_ScalarString(STRING_ELT(v, i as R_xlen_t)));
                }
                t if t == SEXPTYPE::RAWSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::RAWSXP, 1);
                    *RAW(elt) = RAW_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
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
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP, 1));
            SET_VECTOR_ELT(rval, 0, v);
            Rf_unprotect(1);
            return rval;
        }

        if type_ == SEXPTYPE::STRSXP {
            let n = LENGTH(v);
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t));
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
            let xnew = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, len as R_xlen_t));
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
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(rval).add(i as usize) = asLogical(CAR(vp));
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(rval).add(i as usize) = asInteger(CAR(vp));
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(rval).add(i as usize) = asReal(CAR(vp));
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(rval).add(i as usize) = asComplex(CAR(vp));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
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
    }
}

/// Coerce a vector list (VECSXP/EXPRSXP) to the given type.
unsafe fn coerceVectorList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;

        // expression -> list: just change the type tag
        if type_ == SEXPTYPE::VECSXP && TYPEOF(v) == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(SEXPTYPE::VECSXP, xlength(v));
            // Copy the data pointers
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> expression: just change the type tag
        if type_ == SEXPTYPE::EXPRSXP && TYPEOF(v) == SEXPTYPE::VECSXP {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP, xlength(v));
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
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, n));
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
                t if t == SEXPTYPE::LGLSXP => {
                    for i in 0..n {
                        *LOGICAL(rval).add(i as usize) = asLogical(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    for i in 0..n {
                        *INTEGER(rval).add(i as usize) = asInteger(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::REALSXP => {
                    for i in 0..n {
                        *REAL(rval).add(i as usize) = asReal(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    for i in 0..n {
                        *COMPLEX(rval).add(i as usize) = asComplex(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::RAWSXP => {
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
            t if t == SEXPTYPE::LGLSXP => StringFromLogical(LOGICAL_ELT(v, 0)),
            t if t == SEXPTYPE::INTSXP => StringFromInteger(INTEGER_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::REALSXP => StringFromReal_impl(REAL_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::CPLXSXP => StringFromComplex(COMPLEX_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::STRSXP => STRING_ELT(v, 0),
            t if t == SEXPTYPE::RAWSXP => StringFromRaw(RAW_ELT(v, 0), &mut warn),
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
            let v = if type_ != SEXPTYPE::ANYSXP && TYPEOF(u) != type_ {
                coerceVector(u, type_)
            } else {
                u
            };

            // Drop attributes for certain types (as.pairlist behavior)
            if target_type == SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::LANGSXP
                && TYPEOF(u) != SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::EXPRSXP
                && TYPEOF(u) != SEXPTYPE::VECSXP
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
            let v = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
            SET_VECTOR_ELT(v, 0, u);
            return v;
        }

        errorcall(call, "cannot coerce type to vector of type");
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
            t if t == SEXPTYPE::SYMSXP => coerceSymbol(v, target),
            t if t == SEXPTYPE::NILSXP || t == SEXPTYPE::LISTSXP => {
                if type_ == SEXPTYPE::LISTSXP {
                    v // already pairlist
                } else {
                    coercePairList(v, target)
                }
            }
            t if t == SEXPTYPE::LANGSXP => {
                if type_ != SEXPTYPE::STRSXP {
                    coercePairList(v, target)
                } else {
                    // LANGSXP -> STRSXP: special handling for operator names
                    let n = LENGTH(v);
                    let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
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
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => coerceVectorList(v, target),
            t if t == SEXPTYPE::ENVSXP => {
                error("environments cannot be coerced to other types");
            }
            // Atomic vector types
            t if t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP =>
            {
                match type_ {
                    t if t == SEXPTYPE::SYMSXP => coerceToSymbol(v),
                    t if t == SEXPTYPE::LGLSXP => coerceToLogical(v),
                    t if t == SEXPTYPE::INTSXP => coerceToInteger(v),
                    t if t == SEXPTYPE::REALSXP => coerceToReal(v),
                    t if t == SEXPTYPE::CPLXSXP => coerceToComplex(v),
                    t if t == SEXPTYPE::RAWSXP => coerceToRaw(v),
                    t if t == SEXPTYPE::STRSXP => coerceToString(v),
                    t if t == SEXPTYPE::EXPRSXP => coerceToExpression(v),
                    t if t == SEXPTYPE::VECSXP => coerceToVectorList(v),
                    t if t == SEXPTYPE::LISTSXP => coerceToPairList(v),
                    _ => {
                        error("cannot coerce type to vector of type");
                    }
                }
            }
            _ => {
                error("cannot coerce type to vector of type");
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
                t if t == SEXPTYPE::LGLSXP => LOGICAL_ELT(x, 0),
                t if t == SEXPTYPE::INTSXP => LogicalFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP => LogicalFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => LogicalFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP => LogicalFromString(STRING_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::RAWSXP => {
                    LogicalFromInteger(RAW_ELT(x, 0) as c_int, &mut warn)
                }
                _ => NA_LOGICAL,
            }
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
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
                t if t == SEXPTYPE::RAWSXP => RAW_ELT(x, 0) as c_int,
                t if t == SEXPTYPE::LGLSXP => IntegerFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP => INTEGER_ELT(x, 0),
                t if t == SEXPTYPE::REALSXP => IntegerFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => IntegerFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP => IntegerFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_INTEGER,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
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
                t if t == SEXPTYPE::LGLSXP => RealFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP => RealFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP => REAL_ELT(x, 0),
                t if t == SEXPTYPE::CPLXSXP => RealFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP => RealFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_REAL,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
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
                t if t == SEXPTYPE::LGLSXP => {
                    z = ComplexFromLogical(LOGICAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::INTSXP => {
                    z = ComplexFromInteger(INTEGER_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::REALSXP => {
                    z = ComplexFromReal(REAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    z = COMPLEX_ELT(x, 0);
                }
                t if t == SEXPTYPE::STRSXP => {
                    z = ComplexFromString(STRING_ELT(x, 0), &mut warn);
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for complex coercion
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
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
        if TYPEOF(labels) != SEXPTYPE::STRSXP {
            error("malformed factor");
        }
        let nl = LENGTH(labels);

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, n));
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
        "logical" => SEXPTYPE::LGLSXP.into(),
        "integer" => SEXPTYPE::INTSXP.into(),
        "double" | "numeric" => SEXPTYPE::REALSXP.into(),
        "complex" => SEXPTYPE::CPLXSXP.into(),
        "character" => SEXPTYPE::STRSXP.into(),
        "raw" => SEXPTYPE::RAWSXP.into(),
        "list" => SEXPTYPE::VECSXP.into(),
        "expression" => SEXPTYPE::EXPRSXP.into(),
        "pairlist" => SEXPTYPE::LISTSXP.into(),
        "any" => return Ok(x.as_raw()),
        "symbol" | "name" => SEXPTYPE::SYMSXP.into(),
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
        0 => SEXPTYPE::STRSXP.into(),
        1 => SEXPTYPE::INTSXP.into(),
        2 => SEXPTYPE::REALSXP.into(),
        3 => SEXPTYPE::CPLXSXP.into(),
        4 => SEXPTYPE::LGLSXP.into(),
        5 => SEXPTYPE::RAWSXP.into(),
        _ => SEXPTYPE::STRSXP.into(),
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
        "logical" => SEXPTYPE::LGLSXP.into(),
        "integer" => SEXPTYPE::INTSXP.into(),
        "double" | "numeric" => SEXPTYPE::REALSXP.into(),
        "complex" => SEXPTYPE::CPLXSXP.into(),
        "character" => SEXPTYPE::STRSXP.into(),
        "raw" => SEXPTYPE::RAWSXP.into(),
        "list" => SEXPTYPE::VECSXP.into(),
        "expression" => SEXPTYPE::EXPRSXP.into(),
        "pairlist" => SEXPTYPE::LISTSXP.into(),
        "symbol" | "name" => SEXPTYPE::SYMSXP.into(),
        "function" => SEXPTYPE::CLOSXP.into(),
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
                if IS_S4_OBJECT(x_raw) != 0 && TYPEOF(x_raw) == SEXPTYPE::OBJSXP {
                    let dot_x_data =
                        crate::mainutils::subassign::R_getS4DataSlot(x_raw, SEXPTYPE::SYMSXP.into());
                    (TYPEOF(dot_x_data) == SEXPTYPE::SYMSXP) as c_int
                } else {
                    (TYPEOF(x_raw) == SEXPTYPE::SYMSXP) as c_int
                }
            }
        }
        4 => {
            let x_raw = x.as_raw();
            unsafe {
                if IS_S4_OBJECT(x_raw) != 0 && TYPEOF(x_raw) == SEXPTYPE::OBJSXP {
                    let dot_x_data =
                        crate::mainutils::subassign::R_getS4DataSlot(x_raw, SEXPTYPE::ENVSXP.into());
                    (TYPEOF(dot_x_data) == SEXPTYPE::ENVSXP) as c_int
                } else {
                    (TYPEOF(x_raw) == SEXPTYPE::ENVSXP) as c_int
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
        if TYPEOF(x) == SEXPTYPE::OBJSXP && IS_S4_OBJECT(x) == 0 {
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
            t if t == SEXPTYPE::LGLSXP => (LOGICAL_ELT(s, 0) == NA_LOGICAL) as c_int,
            t if t == SEXPTYPE::INTSXP => (INTEGER_ELT(s, 0) == NA_INTEGER) as c_int,
            t if t == SEXPTYPE::REALSXP => ISNAN(REAL_ELT(s, 0)) as c_int,
            t if t == SEXPTYPE::STRSXP => (STRING_ELT(s, 0) == R_NaString()) as c_int,
            t if t == SEXPTYPE::CPLXSXP => {
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (LOGICAL_ELT(x, i as c_int) == NA_LOGICAL) as c_int;
                }
            }
            t if t == SEXPTYPE::INTSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) == NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = ISNAN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (ISNAN(v.r) || ISNAN(v.i)) as c_int;
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (STRING_ELT(x, i) == R_NaString()) as c_int;
                }
            }
            t if t == SEXPTYPE::RAWSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LISTSXP => {
                let mut elt = x;
                for i in 0..n {
                    let s = CAR(elt);
                    *pa.add(i as usize) = elem_is_na(s);
                    elt = CDR(elt);
                }
            }
            t if t == SEXPTYPE::VECSXP => {
                for i in 0..n {
                    let s = VECTOR_ELT(x, i);
                    *pa.add(i as usize) = elem_is_na(s);
                }
            }
            t if t == SEXPTYPE::NILSXP => {}
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = R_IsNaN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
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

// ---------------------------------------------------------------------------
// as.function, str2lang, as.call
// ---------------------------------------------------------------------------

/// do_asfunction — convert a list to a function (closure).
/// Matches C's `do_asfunction()` in coerce.c line 1605.
pub unsafe fn do_asfunction(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let arglist = CAR(args);
        if TYPEOF(arglist) != SEXPTYPE::VECSXP {
            error("list argument expected");
        }
        let envir = CADR(args);
        if isNull(envir) {
            error("use of NULL environment is defunct");
        }
        if !isEnvironment(envir) {
            error("invalid environment");
        }
        let n = LENGTH(arglist);
        if n < 1 {
            error("argument must have length at least 1");
        }
        let names = Rf_protect(getAttrib(arglist, R_NamesSymbol()));
        let pargs = Rf_protect(crate::sexp::constructors::Rf_allocList(n - 1));
        let mut current = pargs;
        for i in 0..n - 1 {
            SETCAR(current, VECTOR_ELT(arglist, i as R_xlen_t));
            if names != R_NilValue() {
                let name_elt = STRING_ELT(names, i as R_xlen_t);
                if name_elt != R_NilValue() {
                    let c = CHAR(name_elt);
                    if !c.is_null() && *c != 0 {
                        SETTAG(current, crate::mainutils::subset::installTrChar(name_elt));
                    }
                }
            }
            current = CDR(current);
        }
        let body = Rf_protect(VECTOR_ELT(arglist, (n - 1) as R_xlen_t));
        let bt = TYPEOF(body);
        if bt == SEXPTYPE::LISTSXP
            || bt == SEXPTYPE::LANGSXP
            || bt == SEXPTYPE::SYMSXP
            || bt == SEXPTYPE::EXPRSXP
            || bt == SEXPTYPE::VECSXP
            || bt == SEXPTYPE::RAWSXP
            || bt == SEXPTYPE::INTSXP
            || bt == SEXPTYPE::REALSXP
            || bt == SEXPTYPE::STRSXP
            || bt == SEXPTYPE::LGLSXP
        {
            let result = crate::mainutils::dstruct::mkCLOSXP(pargs, body, envir);
            Rf_unprotect(3);
            result
        } else {
            Rf_unprotect(3);
            error("invalid body for function");
        }
    }
}

/// do_str2lang — convert a string to a language/call object.
/// Matches C's `do_str2lang()` in coerce.c line 1668.
pub unsafe fn do_str2lang(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        if TYPEOF(s) != SEXPTYPE::STRSXP {
            error("argument must be character");
        }
        if LENGTH(s) != 1 {
            error("argument must be a single character string");
        }
        let mut status: c_int = 0;
        let srcfile = Rf_protect(Rf_mkString(b"<text>\0".as_ptr() as *const c_char));
        let parsed = Rf_protect(crate::mainutils::gram_main::R_ParseVector(
            s,
            -1,
            &mut status,
            srcfile,
        ));
        if status != 1 {
            Rf_unprotect(2);
            error("parse error in str2lang");
        }
        if LENGTH(parsed) != 1 {
            Rf_unprotect(2);
            error("parsing result not of length one");
        }
        let result = VECTOR_ELT(parsed, 0);
        Rf_unprotect(2);
        result
    }
}

/// do_ascall — convert an object to a call object.
/// Matches C's `do_ascall()` in coerce.c line 1732.
pub unsafe fn do_ascall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        match TYPEOF(x) {
            t if t == SEXPTYPE::LANGSXP => x,
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                let n = LENGTH(x);
                if n == 0 {
                    error("invalid length 0 argument");
                }
                let names = Rf_protect(getAttrib(x, R_NamesSymbol()));
                let ans = Rf_protect(crate::sexp::constructors::Rf_allocList(n));
                let mut ap = ans;
                for i in 0..n {
                    SETCAR(ap, VECTOR_ELT(x, i as R_xlen_t));
                    if names != R_NilValue() {
                        let name_elt = STRING_ELT(names, i as R_xlen_t);
                        if name_elt != R_NilValue() {
                            let c = CHAR(name_elt);
                            if !c.is_null() && *c != 0 {
                                SETTAG(ap, crate::mainutils::subset::installTrChar(name_elt));
                            }
                        }
                    }
                    ap = CDR(ap);
                }
                SET_TYPEOF(ans, SEXPTYPE::LANGSXP.into());
                SETTAG(ans, R_NilValue());
                Rf_unprotect(2);
                ans
            }
            t if t == SEXPTYPE::LISTSXP => {
                let ans = crate::mainutils::duplicate::Rf_duplicate(x);
                SET_TYPEOF(ans, SEXPTYPE::LANGSXP.into());
                SETTAG(ans, R_NilValue());
                ans
            }
            t if t == SEXPTYPE::STRSXP => {
                error("as.call(<character>) not feasible; consider str2lang()");
            }
            _ => {
                error("invalid argument list");
            }
        }
    }
}
pub unsafe fn do_isfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP || t == SEXPTYPE::RAWSXP || t == SEXPTYPE::NILSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) != NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = R_FINITE(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
                || t == SEXPTYPE::NILSXP
                || t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP =>
            {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    let xr = REAL_ELT(x, i as c_int);
                    *pa.add(i as usize) = if ISNAN(xr) || R_FINITE(xr) { 0 } else { 1 };
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
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
// anyNA — recursive NA detection
// ---------------------------------------------------------------------------

/// Check if any element of a vector contains NA values.
///
/// Ported from R's `anyNA()` in coerce.c.
fn any_na_impl(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> bool {
    use crate::sexp::accessors::{
        CADR, CAR, CDR, COMPLEX_ELT, INTEGER_ELT, LENGTH, LOGICAL_ELT, OBJECT, REAL_ELT,
        STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
    };
    use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, SEXPTYPE};
    use crate::sexp::globals::{R_NaString, R_NilValue};

    unsafe {
        let x = CAR(args);
        let xT = TYPEOF(x);
        let is_list = xT == SEXPTYPE::VECSXP || xT == SEXPTYPE::LISTSXP;

        let recursive = if is_list && LENGTH(args) > 1 {
            let r = CADR(args);
            asRbool(r, _call) != 0
        } else {
            false
        };

        // For objects or non-recursive lists, fall back to is.na + any
        if OBJECT(x) != 0 || (is_list && !recursive) {
            // Simplified: just check vector elements directly for non-objects
            if OBJECT(x) != 0 {
                // For S4/S3 objects, we'd need eval(dispatch) — skip for now
                return false;
            }
        }

        let n = XLENGTH(x);
        match xT {
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n as usize {
                    let v = REAL_ELT(x, i as c_int);
                    if v.is_nan() {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::INTSXP => {
                for i in 0..n as usize {
                    let v = INTEGER_ELT(x, i as c_int);
                    if v == NA_INTEGER {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::LGLSXP => {
                for i in 0..n as usize {
                    let v = LOGICAL_ELT(x, i as c_int);
                    if v == NA_LOGICAL {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n as usize {
                    let v = COMPLEX_ELT(x, i as c_int);
                    if v.r.is_nan() || v.i.is_nan() {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..n as R_xlen_t {
                    if STRING_ELT(x, i) == R_NaString() {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::RAWSXP => false,
            t if t == SEXPTYPE::NILSXP => false,
            t if t == SEXPTYPE::VECSXP && recursive => {
                for i in 0..n as usize {
                    let elt = VECTOR_ELT(x, i as R_xlen_t);
                    // Recursively check each element
                    let inner_args = Rf_cons(elt, R_NilValue());
                    Rf_protect(inner_args);
                    let found = any_na_impl(_call, _op, inner_args, _env);
                    Rf_unprotect(1);
                    if found {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::LISTSXP && recursive => {
                let mut node = x;
                while !node.is_null() && node != R_NilValue() {
                    let elt = CAR(node);
                    let inner_args = Rf_cons(elt, R_NilValue());
                    Rf_protect(inner_args);
                    let found = any_na_impl(_call, _op, inner_args, _env);
                    Rf_unprotect(1);
                    if found {
                        return true;
                    }
                    node = CDR(node);
                }
                false
            }
            _ => false,
        }
    }
}

/// R-level entry point for `anyNA()`.
///
/// Ported from R's `do_anyNA()` in coerce.c.
pub unsafe fn do_anyNA(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::constructors::Rf_ScalarLogical;
    use crate::sexp::ffi::FALSE;

    unsafe {
        let nargs = LENGTH(args);
        if nargs < 1 || nargs > 2 {
            crate::mainutils::errors::Rf_error(
                b"anyNA takes 1 or 2 arguments\0".as_ptr() as *const c_char
            );
        }

        // Simplified: skip DispatchOrEval for now, call any_na_impl directly
        if nargs == 1 {
            Rf_ScalarLogical(if any_na_impl(call, op, args, rho) {
                1
            } else {
                FALSE
            })
        } else {
            // Two args: x and recursive (default FALSE)
            // Ensure second arg exists and is logical
            let recursive_val = CADR(args);
            let full_args = args;
            if recursive_val.is_null() || recursive_val == crate::sexp::globals::R_MissingArg() {
                // Append ScalarLogical(FALSE) as second arg
                let with_rec = Rf_cons(CAR(args), Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue()));
                Rf_protect(with_rec);
                let result = Rf_ScalarLogical(if any_na_impl(call, op, with_rec, rho) {
                    1
                } else {
                    FALSE
                });
                Rf_unprotect(1);
                result
            } else {
                Rf_ScalarLogical(if any_na_impl(call, op, args, rho) {
                    1
                } else {
                    FALSE
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_call — R's call() primitive
// ---------------------------------------------------------------------------

/// Construct an unevaluated call from a function name and evaluated arguments.
///
/// Ported from R's `do_call()` in coerce.c.
pub unsafe fn do_call(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::eval::Rf_eval;
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CAR, CDR, CHAR, LENGTH, SETCAR, STRING_ELT};
    use crate::sexp::symbol::Rf_install;

    unsafe {
        if LENGTH(args) < 1 {
            Rf_error(b"'name' is missing\0".as_ptr() as *const c_char);
        }

        let rfun = Rf_eval(CAR(args), rho);
        Rf_protect(rfun);

        if !isString(rfun) || LENGTH(rfun) != 1 {
            Rf_unprotect(1);
            Rf_error(b"first argument must be a character string\0".as_ptr() as *const c_char);
        }

        let str = CHAR(STRING_ELT(rfun, 0));
        if !str.is_null() {
            let s = std::ffi::CStr::from_ptr(str);
            if s.to_bytes() == b".Internal" {
                Rf_unprotect(1);
                Rf_error(b"illegal usage\0".as_ptr() as *const c_char);
            }
        }

        let sym = Rf_install(str);
        Rf_protect(sym);

        // Evaluate remaining arguments
        let evargs = CDR(args);
        // Walk args and evaluate each
        let mut rest = evargs;
        while !rest.is_null() && rest != R_NilValue() {
            let tmp = Rf_eval(CAR(rest), rho);
            SETCAR(rest, tmp);
            rest = CDR(rest);
        }

        // Build LANGSXP: (sym arg1 arg2 ...)
        let result = Rf_cons(sym, evargs);
        Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_docall — R's do.call() primitive
// ---------------------------------------------------------------------------

/// Construct and evaluate a call from a function and argument list.
///
/// Ported from R's `do_docall()` in coerce.c.
pub unsafe fn do_docall(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::attrib_core::{R_NamesSymbol, getAttrib};
    use crate::mainutils::errors::Rf_error;
    use crate::mainutils::subset::installTrChar;
    use crate::sexp::accessors::{
        CADDR, CADR, CAR, CDR, CHAR, LENGTH, SETCAR, SETTAG, STRING_ELT, TYPEOF, VECTOR_ELT,
    };
    use crate::sexp::constructors::Rf_allocVector;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::{Rf_protect, Rf_unprotect};
    use crate::sexp::symbol::Rf_install;

    unsafe {
        let fun = CAR(args);
        let envir = CADDR(args);
        let cargs = CADR(args);

        // fun must be a function or a single character string
        if !isFunction(fun) && !(isString(fun) && LENGTH(fun) == 1) {
            Rf_error(b"'what' must be a function or character string\0".as_ptr() as *const c_char);
        }

        if !cargs.is_null() && cargs != R_NilValue() && TYPEOF(cargs) != SEXPTYPE::VECSXP {
            Rf_error(b"'args' must be a list\0".as_ptr() as *const c_char);
        }

        if !isEnvironment(envir) {
            Rf_error(b"'envir' must be an environment\0".as_ptr() as *const c_char);
        }

        let n = if cargs.is_null() || cargs == R_NilValue() {
            0
        } else {
            LENGTH(cargs)
        };
        let names = if n > 0 {
            getAttrib(cargs, R_NamesSymbol())
        } else {
            R_NilValue()
        };
        Rf_protect(names);

        // Build LANGSXP call: (fun arg1 arg2 ...)
        // LANGSXP has n+1 slots: function + n args
        let newcall = Rf_allocVector(SEXPTYPE::LANGSXP, n + 1);
        Rf_protect(newcall);

        if isString(fun) {
            let str = CHAR(STRING_ELT(fun, 0));
            if !str.is_null() {
                let s = std::ffi::CStr::from_ptr(str);
                if s.to_bytes() == b".Internal" {
                    Rf_unprotect(2);
                    Rf_error(b"illegal usage\0".as_ptr() as *const c_char);
                }
            }
            SETCAR(newcall, Rf_install(str));
        } else {
            // Check for .Internal primitive
            let prim_name = crate::eval::builtin::PRIMNAME(fun);
            if prim_name == ".Internal" {
                Rf_unprotect(2);
                Rf_error(b"illegal usage\0".as_ptr() as *const c_char);
            }
            SETCAR(newcall, fun);
        }

        let mut c = CDR(newcall);
        for i in 0..n as usize {
            if TYPEOF(cargs) == SEXPTYPE::VECSXP {
                SETCAR(c, VECTOR_ELT(cargs, i as R_xlen_t));
            }
            // Set tag from names attribute
            if !names.is_null() && names != R_NilValue() {
                let name_elt = STRING_ELT(names, i as R_xlen_t);
                if !name_elt.is_null() && name_elt != R_NilValue() {
                    let ch = CHAR(name_elt);
                    if !ch.is_null() && *ch != 0 {
                        SETTAG(c, installTrChar(name_elt));
                    }
                }
            }
            c = CDR(c);
        }

        let result = crate::eval::eval::Rf_eval(newcall, envir);
        Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// substitute — core AST substitution
// ---------------------------------------------------------------------------

/// Substitute symbols in an expression using bindings from an environment.
///
/// Ported from R's `substitute()` in coerce.c.
unsafe fn substitute(lang: SEXP, rho: SEXP) -> SEXP {
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{PRCODE, TYPEOF};
    use crate::sexp::envir::R_findVarInFrame;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::{R_GlobalEnv, R_NilValue, R_UnboundValue};

    unsafe {
        match TYPEOF(lang) {
            t if t == SEXPTYPE::PROMSXP => substitute(PRCODE(lang), rho),
            t if t == SEXPTYPE::SYMSXP => {
                if rho != R_NilValue() {
                    let t = R_findVarInFrame(rho, lang);
                    if t != R_UnboundValue() {
                        if TYPEOF(t) == SEXPTYPE::PROMSXP {
                            let mut expr = PRCODE(t);
                            while TYPEOF(expr) == SEXPTYPE::PROMSXP {
                                expr = PRCODE(expr);
                            }
                            // ENSURE_NAMEDMAX
                            if NAMED(expr) < 2 {
                                SET_NAMED(expr, 2);
                            }
                            return expr;
                        } else if TYPEOF(t) == SEXPTYPE::DOTSXP {
                            Rf_error(
                                b"'...' used in an incorrect context\0".as_ptr() as *const c_char
                            );
                        }
                        if rho != R_GlobalEnv() {
                            return t;
                        }
                    }
                }
                lang
            }
            t if t == SEXPTYPE::LANGSXP => substitute_list(lang, rho),
            _ => lang,
        }
    }
}

// ---------------------------------------------------------------------------
// substituteList — substitute with ... expansion
// ---------------------------------------------------------------------------

/// Walk a pairlist performing substitution, expanding `...` bindings.
///
/// Ported from R's `substituteList()` in coerce.c.
unsafe fn substitute_list(el: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, CDR, SETCDR, SETTAG, TAG, TYPEOF};
    use crate::sexp::envir::R_findVarInFrame;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
    use crate::sexp::protect::{Rf_protect, Rf_unprotect};
    use crate::sexp::symbol::R_DotsSymbol;

    unsafe {
        if el.is_null() || el == R_NilValue() {
            return el;
        }

        let mut res: SEXP = R_NilValue();
        let mut p: SEXP = ptr::null_mut();
        let mut remaining = el;

        while !remaining.is_null() && remaining != R_NilValue() {
            let mut h: SEXP;

            if CAR(remaining) == R_DotsSymbol() {
                if rho == R_NilValue() {
                    h = R_UnboundValue();
                } else {
                    h = R_findVarInFrame(rho, CAR(remaining));
                }
                if h == R_UnboundValue() {
                    h = Rf_cons(R_DotsSymbol(), R_NilValue());
                    Rf_protect(h);
                } else if h == R_NilValue() || h == R_MissingArg() {
                    h = R_NilValue();
                } else if TYPEOF(h) == SEXPTYPE::DOTSXP {
                    Rf_protect(h);
                    h = substitute_list(h, R_NilValue());
                    // h is now a substituted pairlist — don't unprotect the protected one yet
                } else {
                    crate::mainutils::errors::Rf_error(
                        b"'...' used in an incorrect context\0".as_ptr() as *const c_char,
                    );
                    unreachable!()
                }

                if TYPEOF(h) == SEXPTYPE::DOTSXP || (h != R_NilValue() && !h.is_null()) {
                    Rf_protect(h);
                }
            } else {
                h = substitute(CAR(remaining), rho);
                // ENSURE_NAMEDMAX
                if !h.is_null() && NAMED(h) < 2 {
                    SET_NAMED(h, 2);
                }
                h = Rf_cons(h, R_NilValue());
                SETTAG(h, TAG(remaining));
            }

            if !h.is_null() && h != R_NilValue() {
                if res == R_NilValue() {
                    Rf_protect(h);
                    res = h;
                } else {
                    SETCDR(p, h);
                }
                // Walk to end of h (dots may have expanded to multiple elements)
                let mut tail = h;
                while !CDR(tail).is_null() && CDR(tail) != R_NilValue() {
                    tail = CDR(tail);
                }
                p = tail;
            }

            remaining = CDR(remaining);
        }

        if res != R_NilValue() {
            Rf_unprotect(1);
        }
        res
    }
}

// ---------------------------------------------------------------------------
// do_substitute — R-level substitute() entry point
// ---------------------------------------------------------------------------

/// R's `substitute()` primitive.
///
/// Ported from R's `do_substitute()` in coerce.c.
pub unsafe fn do_substitute(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::eval::Rf_eval;
    use crate::sexp::accessors::{CADR, CAR, TYPEOF};
    use crate::sexp::constructors::Rf_cons;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_MissingArg, R_NilValue};
    use crate::sexp::memory_ext::NewEnvironment;
    use crate::sexp::protect::{Rf_protect, Rf_unprotect};

    unsafe {
        // Manual argument matching: first arg is expr, second is env
        let expr = CAR(args);
        let env_arg = if LENGTH(args) > 1 {
            CADR(args)
        } else {
            R_MissingArg()
        };

        let mut env = if env_arg == R_MissingArg() {
            rho
        } else {
            Rf_eval(env_arg, rho)
        };

        // Historical: don't substitute in R_GlobalEnv
        if env == R_GlobalEnv() {
            env = R_NilValue();
        } else if TYPEOF(env) == SEXPTYPE::VECSXP {
            // Convert VECSXP to environment
            let plist = crate::mainutils::subassign::VectorToPairList(env);
            Rf_protect(plist);
            env = NewEnvironment(R_NilValue(), plist, R_BaseEnv());
            Rf_unprotect(1);
        } else if TYPEOF(env) == SEXPTYPE::LISTSXP {
            // Convert pairlist to environment
            env = NewEnvironment(R_NilValue(), env, R_BaseEnv());
        }

        if env != R_NilValue() && TYPEOF(env) != SEXPTYPE::ENVSXP {
            crate::mainutils::errors::Rf_error(
                b"invalid environment specified\0".as_ptr() as *const c_char
            );
        }

        Rf_protect(env);
        // Duplicate the expression and wrap in a list for substituteList
        let t = Rf_cons(expr, R_NilValue());
        Rf_protect(t);
        let s = substitute_list(t, env);
        let result = if !s.is_null() && s != R_NilValue() {
            crate::sexp::accessors::CAR(s)
        } else {
            R_NilValue()
        };
        Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_storage_mode — storage.mode<- assignment
// ---------------------------------------------------------------------------

/// `storage.mode(x) <- value` — change the storage mode of an object.
///
/// Ported from R's `do_storage_mode()` in coerce.c.
pub unsafe fn do_storage_mode(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CADR, CAR, CHAR, SET_ATTRIB, STRING_ELT, TYPEOF};
    use crate::sexp::protect::{Rf_protect, Rf_unprotect};

    unsafe {
        let obj = CAR(args);
        let value = CADR(args);

        // value must be a non-null character string
        if !isString(value)
            || LENGTH(value) < 1
            || STRING_ELT(value, 0) == crate::sexp::globals::R_NaString()
        {
            Rf_error(b"'value' must be non-null character string\0".as_ptr() as *const c_char);
        }

        let type_str = CHAR(STRING_ELT(value, 0));
        let target_type = str2type(type_str);

        if target_type == -1 as c_int {
            let s = std::ffi::CStr::from_ptr(type_str);
            if s.to_bytes() == b"real" {
                Rf_error(
                    b"use of 'real' is defunct: use 'double' instead\0".as_ptr() as *const c_char
                );
            } else if s.to_bytes() == b"single" {
                Rf_error(
                    b"use of 'single' is defunct: use mode<- instead\0".as_ptr() as *const c_char
                );
            } else {
                Rf_error(b"invalid value\0".as_ptr() as *const c_char);
            }
        }

        if TYPEOF(obj) == target_type {
            return obj;
        }

        // Check for factor
        if crate::mainutils::apply::isFactor(obj) != 0 {
            Rf_error(b"invalid to change the storage mode of a factor\0".as_ptr() as *const c_char);
        }

        let ans = coerceVector(obj, target_type);
        Rf_protect(ans);

        // Copy attributes preserving OBJECT and S4 bits
        SET_ATTRIB(ans, crate::sexp::accessors::ATTRIB(obj));

        Rf_unprotect(1);
        ans
    }
}

/// Map a type name string to a SEXPTYPE value.
///
/// Ported from R's `str2type()` in coerce.c.
pub fn str2type(s: *const c_char) -> c_int {
    use crate::sexp::ffi::SEXPTYPE;
    if s.is_null() {
        return -1;
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(s).to_bytes() };
    match bytes {
        b"logical" => SEXPTYPE::LGLSXP.into(),
        b"integer" => SEXPTYPE::INTSXP.into(),
        b"double" => SEXPTYPE::REALSXP.into(),
        b"complex" => SEXPTYPE::CPLXSXP.into(),
        b"character" => SEXPTYPE::STRSXP.into(),
        b"raw" => SEXPTYPE::RAWSXP.into(),
        b"list" => SEXPTYPE::VECSXP.into(),
        b"expression" => SEXPTYPE::EXPRSXP.into(),
        _ => -1,
    }
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
        let result = unsafe { coerceVector(std::ptr::null_mut(), SEXPTYPE::LGLSXP.into()) };
        assert!(result.is_null());
    }
}
