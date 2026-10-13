//! Essentials domain module `conditions` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang2,
    Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod condition_objects;
mod env_lookup;
mod handlers;
mod owned_tests;
mod raise;
mod restarts;
mod try_catch;
mod warning_signal;
mod with_restarts;

pub(crate) use condition_objects::*;
pub(crate) use env_lookup::*;
pub(crate) use handlers::*;
pub(crate) use raise::*;
pub(crate) use restarts::*;
pub(crate) use try_catch::*;
pub(crate) use warning_signal::*;
pub(crate) use with_restarts::*;

#[cfg(test)]
mod mget_tests;

mod mget;
