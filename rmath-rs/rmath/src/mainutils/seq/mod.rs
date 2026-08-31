#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/seq.c -- sequence generation.
//!
//! Implements `:`, `seq.int()`, `seq_len()`, `seq_along()`, `rep()`,
//! `rep.int()`, `rep_len()`, and `sequence()`.
//!
//! Split into domain submodules (extracted verbatim from the former
//! single-file module):
//!   helpers  -- constants, SEXPTYPE values, local shims, shared helpers
//!   intrange -- R_compact_intrange (compact integer ranges)
//!   colon    -- `:` operator (cross_colon, seq_colon, do_colon) and the
//!               seq.int()/seq_len()/seq_along() primitives
//!   rep      -- rep2/rep3/rep4 and the rep()/rep.int()/rep_len() primitives
//!   datetime -- Date/POSIXct support for seq() (seq.Date/seq.POSIXt)
//!   sequence -- sequence()

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDDDR, CDDR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME,
    RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
    XLENGTH, translateChar,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_allocVector3, Rf_isInteger, Rf_isNull,
    Rf_isReal, Rf_isVector, Rf_length, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_FINITE, R_xlen_t, SEXP};
use crate::sexp::globals::{R_MissingArg, R_NilValue};

mod colon;
mod datetime;
mod helpers;
mod intrange;
mod rep;
mod sequence;

pub use self::colon::*;
pub use self::datetime::*;
pub use self::helpers::*;
pub use self::intrange::*;
pub use self::rep::*;
pub use self::sequence::*;

#[cfg(test)]
mod tests;
