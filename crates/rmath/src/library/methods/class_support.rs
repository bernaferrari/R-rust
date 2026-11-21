/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/class_support.c
 *
 *  Stubs for class support utilities.
 */

use crate::mainutils::errors::Rf_error;
use crate::sexp::accessors::{SET_S4_OBJECT, TYPEOF};
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::*;
use crate::sexp::memory_ext::allocSExp;

/// R_get_primname - get the name of a primitive function.
/// Delegates to getPRIMNAME in main/names.rs.
pub fn R_get_primname(object: SEXP) -> SEXP {
    let t = TYPEOF(object);
    if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
        let msg = c"'R_get_primname' called on a non-primitive";

        Rf_error(msg.as_ptr());
    }
    let name = crate::main::names::getPRIMNAME(object);
    if name.is_null() {
        let s = c"";
        unsafe { Rf_mkString(s.as_ptr()) }
    } else {
        unsafe { Rf_mkString(name) }
    }
}

/// new_object - create a new object from a class definition.
/// Registered as .Call in the methods package.
/// Note: not #[no_mangle] to avoid conflict with graphapp::new_object.
pub(crate) fn new_object(class_def: SEXP) -> SEXP {
    crate::mainutils::objects::R_do_new_object(class_def)
}

/// GNU `Rf_allocS4Object()` — empty S4 object used as `.defaultPrototype`.
pub fn Rf_allocS4Object() -> SEXP {
    let s = allocSExp(SEXPTYPE::S4SXP);
    SET_S4_OBJECT(s);
    s
}
