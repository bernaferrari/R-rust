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
//!     do_coerce, do_asCharacterFactor, asCharacterFactor
//!     do_asatomic, do_asvector, do_typeof, do_is, do_isvector
//!     do_isna, do_isnan, do_isfinite, do_isinfinite

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_LevelsSymbol, R_NamesSymbol, getAttrib,
    setAttrib,
};
use crate::mainutils::relop::PRIMVAL;
use crate::mainutils::subset::installTrChar;
use crate::mainutils::util_main::type2char;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE,
};
use crate::sexp::globals::{R_GlobalEnv, R_MissingArg, R_NaString as R_GlobalNaString, R_NilValue};
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod as_helpers;
mod atomic;
mod call;
mod lang;
mod safe;
mod vector;
mod warn;

pub use self::as_helpers::*;
pub use self::atomic::*;
pub use self::call::*;
pub use self::lang::*;
pub use self::safe::*;
pub use self::vector::*;
pub use self::warn::*;
#[cfg(test)]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" {
    fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double;
}

// wasm32: no C library to link against — port of the C-locale strtod(3)
// semantics the coercers rely on (leading whitespace skipped, decimal and
// exponent forms, inf/nan spellings; endptr marks the first byte after the
// longest valid prefix, or nptr when nothing parses).
#[cfg(target_arch = "wasm32")]
unsafe fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double {
    unsafe {
        let b = CStr::from_ptr(s).to_bytes();
        let mut i = 0;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let num_start = i;
        let mut seen_digit = false;
        let mut seen_dot = false;
        let mut seen_exp = false;
        while i < b.len() {
            let c = b[i];
            match c {
                b'0'..=b'9' => seen_digit = true,
                b'+' | b'-' if i == num_start || (seen_exp && matches!(b[i - 1], b'e' | b'E')) => {}
                b'.' if !seen_dot && !seen_exp => seen_dot = true,
                b'e' | b'E' if seen_digit && !seen_exp => seen_exp = true,
                _ => break,
            }
            i += 1;
        }
        // Rust's parser accepts "inf"/"nan" too; C does as well.
        let text = std::str::from_utf8(&b[num_start..i]).unwrap_or("");
        let val: c_double = text.parse().unwrap_or_else(|_| {
            // try inf/nan spellings that Rust parses
            let t = text.to_ascii_lowercase();
            if t.starts_with("inf") || t.starts_with("+inf") || t.starts_with("-inf") {
                if t.starts_with('-') {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            } else if t.starts_with("nan") {
                f64::NAN
            } else {
                0.0
            }
        });
        if !endptr.is_null() {
            let end = if text.is_empty() {
                s
            } else {
                b.as_ptr().add(i) as *const c_char
            };
            *endptr = end as *mut c_char;
        }
        val
    }
}
