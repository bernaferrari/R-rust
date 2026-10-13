//! Vectorized arithmetic and comparison builtin operations.
#![deny(unsafe_op_in_unsafe_fn)]
// Translated C loops write result elements through the SexpMut mutation guard
// (sexp::object); the guard is the in-crate mutation path — a containment
// boundary (construction is crate-internal), not a uniqueness proof.
//!
//! These handle the core numeric operators (+, -, *, /, ^, %%, %/%),
//! comparison operators (<, >, <=, >=, ==, !=), and unary operators (!, -).
//!
//! In R, these are "builtin" functions — arguments are evaluated before
//! the function is called, unlike "special" forms.
//!
//! All binary operations support R's recycling rule: shorter vectors are
//! recycled to match the length of the longer operand.

use std::ffi::{CStr, CString};

use crate::sexp::accessors::{
    CAR, CDR, CHAR, COMPLEX_ELT, INTEGER_ELT, LENGTH, LOGICAL_ELT, PRINTNAME, RAW_ELT, REAL_ELT,
    SET_COMPLEX_ELT, SET_INTEGER_ELT, SET_LOGICAL_ELT, SET_RAW_ELT, SET_REAL_ELT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};

use crate::sexp::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::constructors::{
    Rf_ScalarComplex, Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons,
    Rf_lang2, Rf_lang3, Rf_mkChar,
};

use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP, SEXPTYPE,
    TRUE,
};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::numeric::NumericVector;
use crate::sexp::object::{Sexp, SexpMut};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod binop;
mod comparison;
mod datetime;
mod math;
mod ops;
mod summary;
#[cfg(test)]
mod tests;

pub(crate) use binop::*;
pub(crate) use comparison::*;
pub(crate) use datetime::*;
pub(crate) use math::*;
pub(crate) use ops::*;
pub(crate) use summary::*;
