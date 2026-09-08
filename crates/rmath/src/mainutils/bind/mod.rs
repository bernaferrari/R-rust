#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/bind.c
//!
//! This module implements R's `c()`, `unlist()`, `cbind()`, and `rbind()`
//! functions, along with their supporting type-coercion helpers.
//!
//! Key exported functions:
//!   do_c, do_c_dflt, do_unlist, do_bind, do_cbind, do_rbind, ItemName
//!
//! Module-private helpers:
//!   AnswerType, ListAnswer, StringAnswer, LogicalAnswer, IntegerAnswer,
//!   RealAnswer, ComplexAnswer, RawAnswer, NewBase, NewName, ItemName,
//!   NewExtractNames, namesCount, c_Extract_opt, cbind, rbind,
//!   SetRowNames, SetColNames, HasNames

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_data_class, getAttrib, isObject, setAttrib};
use crate::eval::dispatch::DispatchOrEval;
use crate::eval::dispatch::promiseArgs;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::protect::protect;

// Local integer constants for SEXPTYPE values, usable in match patterns
const NILSXP_I: c_int = 0;
const SYMSXP_I: c_int = 1;
const LISTSXP_I: c_int = 2;
const PROMSXP_I: c_int = 5;
const LANGSXP_I: c_int = 6;
const CHARSXP_I: c_int = 9;
const LGLSXP_I: c_int = 10;
const INTSXP_I: c_int = 13;
const REALSXP_I: c_int = 14;
const CPLXSXP_I: c_int = 15;
const STRSXP_I: c_int = 16;
const VECSXP_I: c_int = 19;
const EXPRSXP_I: c_int = 20;
const RAWSXP_I: c_int = 24;
const DOTSXP_I: c_int = 17;
mod answers;
mod cunlist;
mod names;
mod rcbind;
mod runtime;

pub use self::answers::*;
pub use self::cunlist::*;
pub use self::names::*;
pub use self::rcbind::*;
pub use self::runtime::*;

#[cfg(test)]
mod tests;
