
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/utils.c
 *
 *  Stubs for methods package utility functions.
 */

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/// R_methods_test_MAKE_CLASS - create a class definition for testing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_methods_test_MAKE_CLASS(_className: SEXP) -> SEXP {
    R_NilValue()
}

/// R_methods_test_NEW - create a new object of a given class for testing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_methods_test_NEW(_className: SEXP) -> SEXP {
    R_NilValue()
}
