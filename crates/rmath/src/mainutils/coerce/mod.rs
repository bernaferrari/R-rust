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
//!     StringFromLogical, StringFromInteger, StringFromComplex, StringFromRaw
//!     (StringFromReal is printutils::StringFromReal; real→string coercion
//!     delegates to it, matching upstream coerce.c → printutils.c)
//!   Vector coercion:
//!     coerceVector -- main dispatcher
//!     coerceToLogical, coerceToInteger, coerceToReal, coerceToComplex,
//!     coerceToRaw, coerceToString, coerceToExpression, coerceToVectorList,
//!     coerceToPairList, coercePairList, coerceVectorList, coerceToSymbol
//!   Scalar accessors:
//!     asLogical, asLogical2, asInteger, asReal, asComplex
//!   R-level entry points:
//!     do_asCharacterFactor, asCharacterFactor
//!     do_coerce -- Rust compatibility alias for checked as.vector coercion
//!     do_asatomic, do_asvector, do_typeof, do_is, do_isvector
//!     do_isna, do_isnan, do_isfinite, do_isinfinite

use std::ffi::CStr;
use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::mainutils::relop::PRIMVAL;
use crate::mainutils::subset::installTrChar;
use crate::mainutils::util_main::type2char;
use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib,
};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE,
};
use crate::sexp::globals::{R_GlobalEnv, R_MissingArg, R_NaString as R_GlobalNaString, R_NilValue};
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;

/// The interned "levels" symbol.
fn R_LevelsSymbol() -> SEXP {
    crate::sexp::symbol::install_str("levels")
}

/// Set attribute through the evaluator setter (GNU names/class/tsp rules).
fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    crate::eval::attrib_core::setAttrib(x, which, value)
}

// ---------------------------------------------------------------------------
// Checked CHARSXP text and string constructors
// ---------------------------------------------------------------------------

/// Raw CHAR bytes of a CHARSXP (GNU `CHAR(x)`).
fn char_bytes(x: SEXP) -> Vec<u8> {
    crate::mainutils::printutils::charsxp_bytes(x)
}

/// CHAR text of a CHARSXP as an owned C string (up to its first NUL);
/// `None` for a null pointer, mirroring a null `CHAR(x)`.
fn char_cstring(x: SEXP) -> Option<std::ffi::CString> {
    if x.is_null() {
        return None;
    }
    let mut bytes = char_bytes(x);
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(nul);
    }
    std::ffi::CString::new(bytes).ok()
}

/// Lossy UTF-8 text of a CHARSXP's CHAR bytes.
fn char_lossy(x: SEXP) -> String {
    String::from_utf8_lossy(&char_bytes(x)).into_owned()
}

/// UTF-8 text of a CHARSXP's CHAR bytes, empty when invalid.
fn char_utf8(x: SEXP) -> String {
    String::from_utf8(char_bytes(x)).unwrap_or_default()
}

/// Checked CHARSXP for `text` on the active session heap. Like
/// `Rf_mkChar(CString::new(text).unwrap_or_default())`, an interior NUL
/// yields the empty string.
fn mk_char(text: &str) -> SEXP {
    let text = if text.contains('\0') { "" } else { text };
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .character(text)
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked STRSXP(1) for `text` (`Rf_mkString`); an interior NUL yields "".
fn mk_string(text: &str) -> SEXP {
    let text = if text.contains('\0') { "" } else { text };
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .strings(&[text])
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

mod any_na;
mod as_helpers;
mod atomic;
#[cfg(test)]
mod attribute_tests;
mod call;
mod lang;
mod pairlist;
#[cfg(test)]
mod pairlist_tests;
mod safe;
mod text_number;
mod vector;
mod vectorizable;
#[cfg(test)]
mod vectorizable_tests;
mod warn;

pub use self::as_helpers::*;
pub use self::atomic::*;
pub use self::call::*;
pub use self::lang::*;
pub use self::safe::*;
pub use self::vector::*;
pub use self::warn::*;
#[cfg(test)]
mod number_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod warning_ownership_tests;

#[cfg(test)]
mod logical_owner_tests;

#[cfg(test)]
mod integer_owner_tests;

#[cfg(test)]
mod real_owner_tests;

#[cfg(test)]
mod complex_owner_tests;

#[cfg(test)]
mod raw_owner_tests;

#[cfg(test)]
mod character_owner_tests;

#[cfg(test)]
mod same_type_owner_tests;

#[cfg(test)]
mod scalar_logical_tests;
