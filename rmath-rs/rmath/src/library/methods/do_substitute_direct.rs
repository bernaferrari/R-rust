
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/do_substitute_direct.c
 *
 *  Stubs for direct substitution in evaluated objects.
 */

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/// do_substitute_direct - substitute in an evaluated object
/// with an explicit list as second argument.
pub unsafe fn do_substitute_direct(f: SEXP, _env: SEXP) -> SEXP {
    // Stub: return the expression unchanged
    f
}
