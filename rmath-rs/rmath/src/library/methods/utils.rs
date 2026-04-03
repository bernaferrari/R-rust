#![allow(unsafe_op_in_unsafe_fn)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/utils.c
 *
 *  Utility functions for the methods package.
 */

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

/// new_object - create a new object from a class definition (stub).
/// Uses pub(crate) to avoid duplicate symbol conflict with other packages.
#[allow(dead_code)]
pub(crate) unsafe fn new_object(_class_def: SEXP) -> SEXP {
    R_NilValue()
}
