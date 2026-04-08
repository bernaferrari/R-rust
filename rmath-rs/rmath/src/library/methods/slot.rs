
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/slot.c
 *
 *  S4 slot access functions. These delegate to the main implementations
 *  in main/attrib.rs (R_do_slot, R_do_slot_assign, R_has_slot).
 */

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/// R_get_slot - get the value of a slot in an S4 object.
/// Delegates to R_do_slot in main/attrib.rs.
pub unsafe fn R_get_slot(obj: SEXP, name: SEXP) -> SEXP {
    crate::main::attrib::R_do_slot(obj, name)
}

/// R_set_slot - set the value of a slot in an S4 object.
/// Delegates to R_do_slot_assign in main/attrib.rs.
pub unsafe fn R_set_slot(obj: SEXP, name: SEXP, value: SEXP) -> SEXP {
    crate::main::attrib::R_do_slot_assign(obj, name, value)
}

/// R_hasSlot - check if an S4 object has a given slot.
/// Delegates to R_has_slot in main/attrib.rs.
pub unsafe fn R_hasSlot(obj: SEXP, name: SEXP) -> SEXP {
    let res = Rf_ScalarLogical(crate::main::attrib::R_has_slot(obj, name));
    res
}
