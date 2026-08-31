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


mod warn;
mod atomic;
mod vector;
mod as_helpers;
mod safe;
mod lang;
mod call;

pub use self::warn::*;
pub use self::atomic::*;
pub use self::vector::*;
pub use self::as_helpers::*;
pub use self::safe::*;
pub use self::lang::*;
pub use self::call::*;
#[cfg(test)]
mod tests;

unsafe extern "C" {
    fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double;
}
