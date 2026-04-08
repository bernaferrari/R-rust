#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/clippath.c -- graphics clip path API.
//!
//! Implements `R_GE_clipPathFillRule` for interrogating gradient SEXPs.

use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::INTEGER;
use crate::sexp::ffi::SEXP;

// ---------------------------------------------------------------------------
// Local wrappers for cross-module functions
// ---------------------------------------------------------------------------

unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::attrib_core::getAttrib(x, what) }
}

unsafe fn install(name: *const c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

// ---------------------------------------------------------------------------
// R_GE_clipPathFillRule
// ---------------------------------------------------------------------------

/// Get the fill rule from a clip path SEXP.
/// Must match R structures in library/grDevices/R/clippath.R.
pub unsafe fn R_GE_clipPathFillRule(path: SEXP) -> c_int {
    unsafe {
        if path.is_null() {
            return 0;
        }
        let rule = getAttrib(path, install(b"rule\0".as_ptr() as *const c_char));
        if rule.is_null() {
            return 0;
        }
        *INTEGER(rule)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clippath_null() {
        unsafe {
            let result = R_GE_clipPathFillRule(ptr::null_mut());
            assert_eq!(result, 0);
        }
    }
}
