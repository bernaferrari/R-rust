/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/tests.c
 *
 *  Stubs for methods package test utilities.
 */

use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::ffi::*;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// R_dummy_extern_place - placeholder for external pointer initializers.
/// This should never actually be called; it just signals an error.
pub fn R_dummy_extern_place() -> SEXP {
    unsupported("methods::R_dummy_extern_place")
}

/// R_externalptr_prototype_object - create the prototype for externalptr objects.
pub fn R_externalptr_prototype_object() -> SEXP {
    unsupported("methods::R_externalptr_prototype_object")
}
