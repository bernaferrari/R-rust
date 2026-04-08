#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Graphics mask API stubs.
//! Ported from mask.c - these depend on R's SEXP type system.

use std::os::raw::c_int;

pub unsafe fn R_GE_maskType(_mask: *mut std::ffi::c_void) -> c_int {
    0
}
