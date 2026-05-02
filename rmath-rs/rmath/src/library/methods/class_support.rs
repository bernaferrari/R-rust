/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/class_support.c
 *
 *  Stubs for class support utilities.
 */

use std::ffi::CString;

use crate::mainutils::errors::Rf_error;
use crate::sexp::accessors::TYPEOF;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::*;

/// R_get_primname - get the name of a primitive function.
/// Delegates to getPRIMNAME in main/names.rs.
pub unsafe fn R_get_primname(object: SEXP) -> SEXP {
    let t = unsafe { TYPEOF(object) };
    if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
        let msg = CString::new("'R_get_primname' called on a non-primitive").unwrap_or_default();
        unsafe {
            Rf_error(msg.as_ptr());
        }
    }
    let name = unsafe { crate::main::names::getPRIMNAME(object) };
    if name.is_null() {
        let s = CString::new("").unwrap_or_default();
        unsafe { Rf_mkString(s.as_ptr()) }
    } else {
        unsafe { Rf_mkString(name) }
    }
}

/// new_object - create a new object from a class definition.
/// Registered as .Call in the methods package.
/// Note: not #[no_mangle] to avoid conflict with graphapp::new_object.
pub(crate) unsafe fn new_object(class_def: SEXP) -> SEXP {
    unsafe { crate::mainutils::objects::R_do_new_object(class_def) }
}
