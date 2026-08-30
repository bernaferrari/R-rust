#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/objects.c -- S-style generic functions and class support.
//!
//! This module provides the core S3/S4 method dispatch infrastructure, including
//! UseMethod, NextMethod, standardGeneric, inherits, and related helpers.
//!
//! Original file: r-source/src/main/objects.c (1,879 lines)

pub(crate) use libc;
pub(crate) use std::collections::{HashMap, HashSet};
pub(crate) use std::ffi::{CStr, CString};
pub(crate) use std::os::raw::{c_char, c_int};
pub(crate) use std::ptr;

pub(crate) use crate::eval::attrib_core::{
    R_ClassSymbol, R_data_class, getAttrib, isObject, setAttrib,
};
pub(crate) use crate::eval::eval::Rf_eval;
pub(crate) use crate::sexp::accessors::*;
pub(crate) use crate::sexp::constructors::*;
pub(crate) use crate::sexp::context::{R_GlobalContext, RCNTXT};
pub(crate) use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
pub(crate) use crate::sexp::globals::*;
pub(crate) use crate::sexp::memory_ext::allocList;
pub(crate) use crate::sexp::protect::protect;
pub(crate) use crate::sexp::symbol::Rf_install;

mod apply_args;
mod class_prims;
mod helpers;
mod nextmethod;
mod primitive_methods;
mod s4;
mod standard_generic;
mod state;
#[cfg(test)]
mod tests;
mod usemethod;

pub use self::apply_args::*;
pub use self::class_prims::*;
pub use self::helpers::*;
pub use self::nextmethod::*;
pub use self::primitive_methods::*;
pub use self::s4::*;
pub use self::standard_generic::*;
pub use self::state::*;
pub use self::usemethod::*;
