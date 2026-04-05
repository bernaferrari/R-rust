
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/tests.c
 *
 *  Stubs for methods package test utilities.
 */

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/// R_dummy_extern_place - placeholder for external pointer initializers.
/// This should never actually be called; it just signals an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_dummy_extern_place() -> SEXP {
    R_NilValue()
}

/// R_externalptr_prototype_object - create the prototype for externalptr objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_externalptr_prototype_object() -> SEXP {
    R_NilValue()
}
