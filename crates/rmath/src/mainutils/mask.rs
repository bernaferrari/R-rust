#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Graphics mask API.
//! Ported from mask.c metadata helpers.

use std::os::raw::c_int;

use crate::sexp::accessors::{INTEGER, LENGTH, TYPEOF};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::symbol::Rf_install;

/// Get the mask type from a mask SEXP.
/// Must match R structures in library/grDevices/R/mask.R.
pub unsafe fn R_GE_maskType(mask: *mut std::ffi::c_void) -> c_int {
    unsafe {
        let mask = mask as SEXP;
        if mask.is_null() || mask == R_NilValue() {
            return 0;
        }
        let kind = crate::sexp::attrib_core::getAttrib(mask, Rf_install(c"type".as_ptr()));
        if kind.is_null()
            || kind == R_NilValue()
            || TYPEOF(kind) != SEXPTYPE::INTSXP
            || LENGTH(kind) == 0
        {
            return 0;
        }
        *INTEGER(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::session::RSession;

    #[test]
    fn mask_type_reads_type_attribute() {
        let _session = RSession::new();
        unsafe {
            let mask = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, 0);
            crate::sexp::attrib_core::setAttrib(
                mask,
                Rf_install(c"type".as_ptr()),
                Rf_ScalarInteger(2),
            );
            assert_eq!(R_GE_maskType(mask.cast()), 2);
        }
    }

    #[test]
    fn mask_type_defaults_to_zero_for_missing_metadata() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_GE_maskType(std::ptr::null_mut()), 0);
            let mask = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, 0);
            assert_eq!(R_GE_maskType(mask.cast()), 0);
        }
    }
}
