/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/utils.c
 *
 *  methods package utility functions.
 */

use std::ffi::{CStr, CString};

use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF};
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;

unsafe fn class_name_to_cstring(class_name: SEXP) -> Option<CString> {
    unsafe {
        if class_name.is_null() || class_name == R_NilValue() {
            return None;
        }
        let charsxp = match TYPEOF(class_name) {
            kind if kind == SEXPTYPE::STRSXP.as_c_int() && LENGTH(class_name) > 0 => {
                STRING_ELT(class_name, 0)
            }
            kind if kind == SEXPTYPE::CHARSXP.as_c_int() => class_name,
            _ => return None,
        };
        if charsxp.is_null() {
            return None;
        }
        let ptr = CHAR(charsxp);
        if ptr.is_null() {
            return None;
        }
        CString::new(CStr::from_ptr(ptr).to_bytes()).ok()
    }
}

/// R_methods_test_MAKE_CLASS - create a class definition for testing.
pub unsafe fn R_methods_test_MAKE_CLASS(className: SEXP) -> SEXP {
    unsafe {
        let Some(class_name) = class_name_to_cstring(className) else {
            return R_NilValue();
        };
        crate::mainutils::objects::R_do_MAKE_CLASS(class_name.as_ptr())
    }
}

/// R_methods_test_NEW - create a new object of a given class for testing.
pub unsafe fn R_methods_test_NEW(className: SEXP) -> SEXP {
    unsafe {
        let class_def = R_methods_test_MAKE_CLASS(className);
        if class_def.is_null() || class_def == R_NilValue() {
            return R_NilValue();
        }
        crate::mainutils::objects::R_do_new_object(class_def)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::attrib_core::{R_ClassSymbol, getAttrib};
    use crate::sexp::constructors::Rf_mkString;

    #[test]
    fn methods_test_make_class_registers_class_definition() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let class = Rf_mkString(c"UtilityClass".as_ptr());
            let class_def = R_methods_test_MAKE_CLASS(class);

            assert!(!class_def.is_null());
            assert!(crate::mainutils::objects::s4_class("UtilityClass").is_some());
        }
    }

    #[test]
    fn methods_test_new_creates_s4_object() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let class = Rf_mkString(c"UtilityObject".as_ptr());
            let object = R_methods_test_NEW(class);

            assert!(!object.is_null());
            let class_attr = getAttrib(object, R_ClassSymbol());
            assert_eq!(TYPEOF(class_attr), SEXPTYPE::STRSXP);
            let first = STRING_ELT(class_attr, 0);
            assert_eq!(CStr::from_ptr(CHAR(first)).to_bytes(), b"UtilityObject");
        }
    }
}
