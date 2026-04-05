#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Internal helpers for coerce.

use std::os::raw::c_int;
use std::sync::OnceLock;

use crate::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore};
use crate::sexp::globals::R_NilValue;
use crate::sexp::safe::Sexp;

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
pub(crate) fn get_na_string() -> SEXP {
    static NA_STRING_VAL: OnceLock<usize> = OnceLock::new();
    let val = NA_STRING_VAL.get_or_init(|| {
        let mut node = SexprecCore::new_vector(SEXPTYPE::CHARSXP, 2);
        node.sxpinfo.set_gp(1);
        Box::into_raw(Box::new(node)) as usize
    });
    *val as SEXP
}

/// Safe version: get the NA_STRING sentinel as a `Sexp<'a>`.
pub fn get_na_string_safe<'a>() -> Sexp<'a> {
    unsafe { Sexp::from_raw_unchecked(get_na_string()) }
}

/// Check if a `Sexp` is the NA_STRING sentinel.
pub fn is_na_string(x: Sexp) -> bool {
    x.as_raw() == get_na_string()
}

// ---------------------------------------------------------------------------
// Internal helper: logical string cache
// ---------------------------------------------------------------------------

static LGL_CACHE: OnceLock<usize> = OnceLock::new();

pub(crate) fn get_logical_cache() -> SEXP {
    let val = LGL_CACHE.get_or_init(|| {
        let node = SexprecCore::new_vector(SEXPTYPE::STRSXP, 2);
        Box::into_raw(Box::new(node)) as usize
    });
    *val as SEXP
}

/// Safe version: get the logical string cache as a `Sexp<'a>`.
pub fn get_logical_cache_safe<'a>() -> Sexp<'a> {
    unsafe { Sexp::from_raw_unchecked(get_logical_cache()) }
}

// ---------------------------------------------------------------------------
// Safe Sexp-based type predicates
// ---------------------------------------------------------------------------

/// Check if a `Sexp` is a vector atomic type.
#[inline]
pub fn is_vector_atomic(x: Sexp) -> bool {
    x.typeof_().is_atomic_type()
}

/// Check if a `Sexp` is a vector type (atomic or list).
#[inline]
pub fn is_vector(x: Sexp) -> bool {
    x.typeof_().is_vector_type()
}

/// Check if a `Sexp` is a vector list type.
#[inline]
pub fn is_vector_list(x: Sexp) -> bool {
    let t = x.typeof_();
    t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP
}

/// Check if a `Sexp` is real (double) and a vector.
#[inline]
pub fn is_real(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::REALSXP && is_vector(x)
}

/// Check if a `Sexp` is complex and a vector.
#[inline]
pub fn is_complex(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::CPLXSXP && is_vector(x)
}

/// Check if a `Sexp` is integer and a vector.
#[inline]
pub fn is_integer(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::INTSXP && is_vector(x)
}

/// Check if a `Sexp` is a function.
#[inline]
pub fn is_function(x: Sexp) -> bool {
    let t = x.typeof_();
    t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP
}

/// Check if a `Sexp` is an environment.
#[inline]
pub fn is_environment(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::ENVSXP
}

/// Check if a `Sexp` is a symbol.
#[inline]
pub fn is_symbol(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::SYMSXP
}

/// Check if a `Sexp` is a string vector.
#[inline]
pub fn is_string(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::STRSXP
}

/// Check if a `Sexp` is NULL/R_NilValue.
#[inline]
pub fn is_null(x: Sexp) -> bool {
    x.is_nil()
}

/// Check if a `Sexp` is a list (pairlist).
#[inline]
pub fn is_list(x: Sexp) -> bool {
    x.is_pairlist()
}

/// Check if a `Sexp` is a language object.
#[inline]
pub fn is_language(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::LANGSXP
}

/// Check if a `Sexp` is "vectorizable" (atomic vector types).
#[inline]
pub fn is_vectorizable(x: Sexp) -> bool {
    matches!(
        x.typeof_(),
        SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP
    )
}

/// Check if a `Sexp` is numeric (integer or real, but not logical).
#[inline]
pub fn is_numeric(x: Sexp) -> bool {
    let t = x.typeof_();
    (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP) && is_vector(x)
}

/// Check if a `Sexp` is logical.
#[inline]
pub fn is_logical(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::LGLSXP && is_vector(x)
}

/// Check if a `Sexp` has the OBJECT bit set.
#[inline]
pub fn is_object(x: Sexp) -> bool {
    x.is_object()
}

/// Check if a `Sexp` is a new list (generic vector).
#[inline]
pub fn is_new_list(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::VECSXP
}

/// Check if a `Sexp` is an expression.
#[inline]
pub fn is_expression(x: Sexp) -> bool {
    x.typeof_() == SEXPTYPE::EXPRSXP
}

// ---------------------------------------------------------------------------
// Internal helpers: xlength, type predicates (raw FFI versions)
// ---------------------------------------------------------------------------

/// Get the extended length of an SEXP (handles NULL).
#[inline]
pub(crate) unsafe fn xlength(x: SEXP) -> R_xlen_t {
    unsafe { XLENGTH(x) }
}

/// Check if an SEXP is a vector atomic type.
#[inline]
pub(crate) unsafe fn isVectorAtomic(x: SEXP) -> bool {
    unsafe { Rf_isVectorAtomic(x) != 0 }
}

/// Check if an SEXP is a vector type (atomic or list).
#[inline]
pub(crate) unsafe fn isVector(x: SEXP) -> bool {
    unsafe { Rf_isVector(x) != 0 }
}

/// Check if an SEXP is a vector list type.
#[inline]
pub(crate) unsafe fn isVectorList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0
    }
}

/// Check if a SEXP is real (double) and a vector.
#[inline]
pub(crate) unsafe fn isReal(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::REALSXP.0 && isVector(x) }
}

/// Check if a SEXP is complex and a vector.
#[inline]
pub(crate) unsafe fn isComplex(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::CPLXSXP.0 && isVector(x) }
}

/// Check if a SEXP is integer and a vector.
#[inline]
pub(crate) unsafe fn isInteger(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::INTSXP.0 && isVector(x) }
}

/// Check if an SEXP is a function.
#[inline]
pub(crate) unsafe fn isFunction(x: SEXP) -> bool {
    unsafe { Rf_isFunction(x) != 0 }
}

/// Check if an SEXP is an environment.
#[inline]
pub(crate) unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { Rf_isEnvironment(x) != 0 }
}

/// Check if an SEXP is a symbol.
#[inline]
pub(crate) unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { Rf_isSymbol(x) != 0 }
}

/// Check if an SEXP is a string vector.
#[inline]
pub(crate) unsafe fn isString(x: SEXP) -> bool {
    unsafe { Rf_isString(x) != 0 }
}

/// Check if an SEXP is NULL.
#[inline]
pub(crate) unsafe fn isNull(x: SEXP) -> bool {
    unsafe { Rf_isNull(x) != 0 }
}

/// Check if an SEXP is a list (pairlist).
#[inline]
pub(crate) unsafe fn isList(x: SEXP) -> bool {
    unsafe { Rf_isList(x) != 0 }
}

/// Check if an SEXP is a language object.
#[inline]
pub(crate) unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LANGSXP.0 }
}

/// Check if an SEXP is "vectorizable" (atomic vector types).
#[inline]
pub(crate) unsafe fn isVectorizable(x: SEXP) -> bool {
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
pub(crate) unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        (t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0) && isVector(x)
    }
}

/// Check if a SEXP is logical.
#[inline]
pub(crate) unsafe fn isLogical(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LGLSXP.0 && isVector(x) }
}

/// Check if an SEXP has the S4 object bit set.
#[inline]
pub(crate) unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.gp() >> 4) as c_int & 1
    }
}

/// Check if an SEXP has the OBJECT bit set.
#[inline]
pub(crate) unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

/// Check if an SEXP is a new list (generic vector).
#[inline]
pub(crate) unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::VECSXP.0 }
}

/// Check if a SEXP is an expression.
#[inline]
pub(crate) unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::EXPRSXP.0 }
}

/// Check if a SEXP is a matrix (has non-null dim attribute of length 2).
#[inline]
pub(crate) unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        !isNull(dim) && LENGTH(dim) == 2
    }
}

/// Check if a SEXP is an array (has non-null dim attribute).
#[inline]
pub(crate) unsafe fn isArray(x: SEXP) -> bool {
    unsafe { !isNull(getAttrib(x, R_DimSymbol())) }
}

/// Panic with an R error message.
pub(crate) unsafe fn error(msg: &str) {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Panic with an R error message (call-specific).
pub(crate) unsafe fn errorcall(_call: SEXP, msg: &str) {
    unsafe {
        error(msg);
    }
}

/// Get R_NaString -- returns the NA string CHARSXP.
pub(crate) fn R_NaString() -> SEXP {
    get_na_string()
}

/// Get R_BlankString -- returns the empty string CHARSXP.
pub(crate) fn R_BlankString() -> SEXP {
    unsafe { Rf_mkChar(c"".as_ptr()) }
}

// ---------------------------------------------------------------------------
// SHALLOW_DUPLICATE_ATTRIB and CLEAR_ATTRIB helpers
// ---------------------------------------------------------------------------

/// Copy attributes from `from` to `to` (shallow duplicate).
pub(crate) unsafe fn SHALLOW_DUPLICATE_ATTRIB(to: SEXP, from: SEXP) {
    unsafe {
        let attr_from = ATTRIB(from);
        if !isNull(attr_from) {
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
pub(crate) unsafe fn CLEAR_ATTRIB(x: SEXP) {
    unsafe {
        let attr = ATTRIB(x);
        if !isNull(attr) {
            SET_ATTRIB(x, R_NilValue());
            if OBJECT(x) != 0 {
                SET_OBJECT(x, 0);
            }
            if IS_S4_OBJECT(x) != 0 {
                (*x).sxpinfo.set_gp((*x).sxpinfo.gp() & !(1 << 4));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CoercionWarning
// ---------------------------------------------------------------------------

/// Issue coercion warnings based on the warning flags.
pub(crate) unsafe fn CoercionWarning(warn: c_int) {
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
