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

use std::os::raw::c_double;

use crate::sexp::ffi::R_NA_BIT_PATTERN;

pub mod helpers;
pub mod rlevel;
pub mod scalar;
#[cfg(test)]
mod tests;
pub mod vector;

// ---------------------------------------------------------------------------
// Constants and NA helpers (used everywhere)
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
pub const NA_REAL: c_double = f64::NAN;

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
// Re-exports from submodules
// ---------------------------------------------------------------------------

pub use helpers::{
    CoercionWarning, WARN_IMAG, WARN_INT_NA, WARN_NA, WARN_RAW, isComplex, isInteger, isLogical,
    isNumeric, isReal, isVectorList,
};

pub use scalar::{
    ComplexFromInteger, ComplexFromLogical, ComplexFromReal, ComplexFromString, ComplexFromStringC,
    IntegerFromComplex, IntegerFromLogical, IntegerFromReal, IntegerFromString, LogicalFromComplex,
    LogicalFromInteger, LogicalFromReal, LogicalFromString, RealFromComplex, RealFromInteger,
    RealFromLogical, RealFromString, StringFromComplex, StringFromInteger, StringFromLogical,
    StringFromRaw,
};

pub use vector::{asComplex, asInteger, asLogical, asLogical2, asRaw, asReal, coerceVector};

pub use rlevel::{
    asBool, asCharacterFactor, asRbool, do_asCharacterFactor, do_asatomic, do_ascoerce,
    do_asvector, do_coerce, do_is, do_isfinite, do_isinfinite, do_isna, do_isnan, do_isvector,
};
