/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/utils.c
 *
 *  Stubs for methods package utility functions.
 */

use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::ffi::*;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// R_methods_test_MAKE_CLASS - create a class definition for testing.
pub fn R_methods_test_MAKE_CLASS(_className: SEXP) -> SEXP {
    unsupported("methods::R_methods_test_MAKE_CLASS")
}

/// R_methods_test_NEW - create a new object of a given class for testing.
pub fn R_methods_test_NEW(_className: SEXP) -> SEXP {
    unsupported("methods::R_methods_test_NEW")
}
