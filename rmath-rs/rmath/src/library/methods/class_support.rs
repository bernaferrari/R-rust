#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/class_support.c
 *
 *  Stubs for class support utilities.
 */

use std::ffi::CString;
use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/// R_get_primname - get the name of a primitive function.
/// Delegates to getPRIMNAME in main/names.rs.
pub unsafe fn R_get_primname(object: SEXP) -> SEXP {
    let t = TYPEOF(object);
    if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
        let msg = CString::new("'R_get_primname' called on a non-primitive").unwrap_or_default();
        crate::main::errors::Rf_error(msg.as_ptr());
    }
    let name = crate::main::names::getPRIMNAME(object);
    if name.is_null() {
        let s = CString::new("").unwrap_or_default();
        Rf_mkString(s.as_ptr())
    } else {
        Rf_mkString(name)
    }
}

/// new_object - create a new object from a class definition.
/// Registered as .Call in the methods package.
/// Note: not #[no_mangle] to avoid conflict with graphapp::new_object.
pub(crate) unsafe fn new_object(_class_def: SEXP) -> SEXP {
    R_NilValue()
}
