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
use crate::sexp::symbol::Rf_install;

unsafe fn slot_name_matches(names: SEXP, index: c_int, name: SEXP) -> bool {
    unsafe {
        let wanted = CHAR(name);
        if wanted.is_null() {
            return false;
        }
        let current = STRING_ELT(names, index as R_xlen_t);
        if current.is_null() {
            return false;
        }
        let current = CHAR(current);
        !current.is_null() && libc::strcmp(current, wanted) == 0
    }
}

/// R_get_slot - get the value of a slot in an S4 object.
/// Delegates to R_do_slot in main/attrib.rs.
pub unsafe fn R_get_slot(obj: SEXP, name: SEXP) -> SEXP {
    unsafe {
        if obj.is_null() || obj == R_NilValue() || name.is_null() || name == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(obj) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let names = crate::attrib_core::getAttrib(obj, Rf_install(c"names".as_ptr()));
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }

        for i in 0..LENGTH(names) {
            if slot_name_matches(names, i, name) {
                return VECTOR_ELT(obj, i as R_xlen_t);
            }
        }

        R_NilValue()
    }
}

/// R_set_slot - set the value of a slot in an S4 object.
/// Delegates to R_do_slot_assign in main/attrib.rs.
pub unsafe fn R_set_slot(obj: SEXP, name: SEXP, value: SEXP) -> SEXP {
    unsafe {
        if obj.is_null() || obj == R_NilValue() || name.is_null() || name == R_NilValue() {
            return obj;
        }
        if TYPEOF(obj) != SEXPTYPE::VECSXP {
            return obj;
        }

        let names = crate::attrib_core::getAttrib(obj, Rf_install(c"names".as_ptr()));
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return obj;
        }

        for i in 0..LENGTH(names) {
            if slot_name_matches(names, i, name) {
                SET_VECTOR_ELT(obj, i as R_xlen_t, value);
                return value;
            }
        }

        obj
    }
}

/// R_hasSlot - check if an S4 object has a given slot.
/// Delegates to R_has_slot in main/attrib.rs.
pub unsafe fn R_hasSlot(obj: SEXP, name: SEXP) -> SEXP {
    unsafe {
        let has_slot =
            if obj.is_null() || obj == R_NilValue() || name.is_null() || name == R_NilValue() {
                0
            } else if TYPEOF(obj) != SEXPTYPE::VECSXP {
                0
            } else {
                let names = crate::attrib_core::getAttrib(obj, Rf_install(c"names".as_ptr()));
                if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                    0
                } else {
                    let mut found = 0;
                    for i in 0..LENGTH(names) {
                        if slot_name_matches(names, i, name) {
                            found = 1;
                            break;
                        }
                    }
                    found
                }
            };
        let res = Rf_ScalarLogical(has_slot);
        res
    }
}
