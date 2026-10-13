#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/arithmetic.c — arithmetic utility functions.
//!
//! This module ports the standalone arithmetic functions that don't require
//! SEXP or R interpreter internals.
//!
//! Ported standalone functions:
//!   R_ValueOfNA, R_NaN_is_R_NA, R_IsNA, R_IsNaN,
//!   myfmod, myfloor,
//!   R_integer_plus, R_integer_minus, R_integer_times, R_integer_divide,
//!   Rsqrt, Rexp, Rexpm1, Rlog1p, Rsin, Rtan, Rcos, Rasin, Ratan
//!
//! Already ported elsewhere:
//!   R_pow, R_pow_di, R_finite → special/mlutils.rs

use std::os::raw::c_int;

use crate::fprec::{fprec, fround};
use crate::sexp::accessors::{CADR, CAR, COMPLEX, INTEGER, LOGICAL, NAMED, REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector3, Rf_length};
use crate::sexp::ffi::Rcomplex;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut, SexpResult};
use crate::sexp::protect::protect;
use crate::special::cospi::{cospi, sinpi, tanpi};
use crate::special::gamma::{gammafn, lgammafn};
use crate::special::mlutils::R_pow;
use crate::special::polygamma::{digamma, trigamma};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

mod constants;
mod dispatch;
#[allow(unused_imports)]
pub(crate) use dispatch::*; // tests.rs chains here via use super::*
mod math;
mod scalar;
#[cfg(test)]
mod tests;

pub use constants::*;
pub use math::*;
pub use scalar::*;
