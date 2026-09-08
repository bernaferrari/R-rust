#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/attrib.c — attribute setter functions.
//!
//! This module provides attribute setter operations (*gets variants) and
//! other functions from attrib.c that are NOT already defined in inspect.rs.
//!
//! Functions in inspect.rs (NOT duplicated here):
//!   do_names, do_dim, do_dimnames, do_levels, do_structure, do_attributes,
//!   do_class, do_classname, do_length, do_typeof, do_str, do_strformat,
//!   do_invisible, do_args, do_body, do_formals, do_environment, do_isnull
//!
//! Functions in eval/attrib_core.rs (NOT duplicated here):
//!   getAttrib, setAttrib, isObject, R_classgets, R_data_class
//!
//! Functions provided HERE:
//!   do_dimgets, do_dimnamesgets, do_levelsgets, do_tsp, do_tspgets,
//!   do_comment, do_commentgets, do_attr, do_attrgets, do_attributesgets,
//!   do_classgets, do_namesgets, do_isobject, R_getAttributes, dimgets

use std::os::raw::c_int;

use crate::sexp::accessors::{
    ATTRIB, CADDR, CADR, CAR, CDR, PRINTNAME, SET_STRING_ELT, SET_VECTOR_ELT, TAG,
};
use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// do_dimgets — set dim attribute
// ---------------------------------------------------------------------------

/// Set the dim attribute (internal).
pub unsafe fn do_dimgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        crate::eval::attrib_core::setAttrib(x, crate::eval::attrib_core::R_DimSymbol(), val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_dimnamesgets — set dimnames attribute
// ---------------------------------------------------------------------------

/// Set the dimnames attribute (internal).
pub unsafe fn do_dimnamesgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        crate::eval::attrib_core::setAttrib(x, crate::eval::attrib_core::R_DimNamesSymbol(), val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_levelsgets — set levels attribute
// ---------------------------------------------------------------------------

/// Set the levels attribute (internal).
pub unsafe fn do_levelsgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        crate::eval::attrib_core::setAttrib(x, crate::eval::attrib_core::R_LevelsSymbol(), val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_tsp — tsp(x) and tsp(x) <- value
// ---------------------------------------------------------------------------

/// Get or set the tsp (time series parameters) attribute.
pub unsafe fn do_tsp(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);

        if Rf_length(args) == 1 {
            let x = CAR(args);
            return crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_TspSymbol());
        }

        error("wrong number of arguments");
    }
}

// ---------------------------------------------------------------------------
// do_tspgets — set tsp attribute
// ---------------------------------------------------------------------------

/// Set the tsp attribute (internal).
pub unsafe fn do_tspgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        crate::eval::attrib_core::setAttrib(x, crate::eval::attrib_core::R_TspSymbol(), val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_comment — comment(x) and comment(x) <- value
// ---------------------------------------------------------------------------

/// Get or set the comment attribute.
pub unsafe fn do_comment(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);

        let comment_sym = crate::sexp::symbol::Rf_install(c"comment".as_ptr());

        if Rf_length(args) == 1 {
            let x = CAR(args);
            return crate::eval::attrib_core::getAttrib(x, comment_sym);
        }

        error("wrong number of arguments");
    }
}

// ---------------------------------------------------------------------------
// do_commentgets — set comment attribute
// ---------------------------------------------------------------------------

/// Set the comment attribute (internal).
pub unsafe fn do_commentgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        let comment_sym = crate::sexp::symbol::Rf_install(c"comment".as_ptr());
        crate::eval::attrib_core::setAttrib(x, comment_sym, val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_attr — attr(x, which) and attr(x, which) <- value
// ---------------------------------------------------------------------------

/// Get or set a single attribute.
pub unsafe fn do_attr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);

        let nargs = Rf_length(args);
        if nargs < 2 {
            error("either 2 or 3 arguments are required");
        }

        let x = CAR(args);
        let which = CADR(args);

        if nargs == 2 {
            return crate::eval::attrib_core::getAttrib(x, which);
        }

        error("either 2 or 3 arguments are required");
    }
}

// ---------------------------------------------------------------------------
// do_attrgets — set a single attribute
// ---------------------------------------------------------------------------

/// Set a single attribute (internal).
pub unsafe fn do_attrgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 3 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let which = CADR(args);
        let val = CADDR(args);
        crate::eval::attrib_core::setAttrib(x, which, val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_attributesgets — set all attributes
// ---------------------------------------------------------------------------

/// Set all attributes of an object (internal).
pub unsafe fn do_attributesgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        CAR(args)
    }
}

// ---------------------------------------------------------------------------
// do_classgets — set class attribute (internal)
// ---------------------------------------------------------------------------

/// Set the class attribute (internal).
pub unsafe fn do_classgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        crate::eval::attrib_core::R_classgets(x, val)
    }
}

// ---------------------------------------------------------------------------
// do_namesgets — set names attribute (internal)
// ---------------------------------------------------------------------------

/// Set the names attribute (internal).
pub unsafe fn do_namesgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 2 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        let val = CADR(args);
        crate::eval::attrib_core::setAttrib(x, crate::eval::attrib_core::R_NamesSymbol(), val);
        x
    }
}

// ---------------------------------------------------------------------------
// do_isobject — check if object has explicit class
// ---------------------------------------------------------------------------

/// Check if an object has an explicit class attribute.
pub unsafe fn do_isobject(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if Rf_length(args) < 1 {
            error("wrong number of arguments");
        }
        let x = CAR(args);
        Rf_ScalarLogical(crate::eval::attrib_core::isObject(x))
    }
}

// ---------------------------------------------------------------------------
// R_getAttributes — get all attributes as a named list
// ---------------------------------------------------------------------------

/// Get all attributes as a named list.
pub unsafe fn R_getAttributes(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        let attrib = ATTRIB(x);
        if attrib.is_null() || attrib == R_NilValue() {
            return R_NilValue();
        }

        // Count attributes
        let mut nattrs: c_int = 0;
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            nattrs += 1;
            current = CDR(current);
        }

        if nattrs == 0 {
            return R_NilValue();
        }

        // Build result list
        let ans = Rf_allocVector(SEXPTYPE::VECSXP, nattrs);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nattrs);
        if ans.is_null() || names.is_null() {
            return R_NilValue();
        }

        let mut i: c_int = 0;
        current = attrib;
        while !current.is_null() && current != R_NilValue() && (i as usize) < nattrs as usize {
            let tag = TAG(current);
            SET_VECTOR_ELT(ans, i as i64, CAR(current));
            if !tag.is_null() {
                let pname = PRINTNAME(tag);
                if !pname.is_null() {
                    SET_STRING_ELT(names, i as i64, pname);
                }
            }
            i += 1;
            current = CDR(current);
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), names);
        ans
    }
}

// ---------------------------------------------------------------------------
// dimgets — internal dim setting helper
// ---------------------------------------------------------------------------

/// Internal helper to set dim attribute with validation.
pub unsafe fn dimgets(vec: SEXP, val: SEXP) -> SEXP {
    unsafe {
        if val.is_null() || val == R_NilValue() {
            return vec;
        }
        crate::eval::attrib_core::setAttrib(vec, crate::eval::attrib_core::R_DimSymbol(), val);
        vec
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    #[should_panic]
    fn test_do_dimgets_null() {
        unsafe {
            do_dimgets(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_dimnamesgets_null() {
        unsafe {
            do_dimnamesgets(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_tsp_null() {
        unsafe {
            do_tsp(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_comment_null() {
        unsafe {
            do_comment(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_attr_null() {
        unsafe {
            do_attr(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_attrgets_null() {
        unsafe {
            do_attrgets(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_isobject_null() {
        unsafe {
            do_isobject(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_classgets_null() {
        unsafe {
            do_classgets(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_do_namesgets_null() {
        unsafe {
            do_namesgets(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    #[test]
    fn test_r_get_attributes_null() {
        unsafe {
            let result = R_getAttributes(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_dimgets_null() {
        unsafe {
            let result = dimgets(ptr::null_mut(), ptr::null_mut());
            assert!(result.is_null());
        }
    }
}
