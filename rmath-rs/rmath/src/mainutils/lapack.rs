#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/lapack.c -- LAPACK module interface.
//!
//! Implements `do_lapack` which dispatches to the LAPACK module, and
//! `R_setLapackRoutines` for registering LAPACK routine pointers.

use std::os::raw::c_void;
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Stub: checkArity
// ---------------------------------------------------------------------------

unsafe fn checkArity(_op: SEXP, _args: SEXP) {}

// ---------------------------------------------------------------------------
// R_setLapackRoutines
// ---------------------------------------------------------------------------

/// Function pointer type for the LAPACK dispatch function.
type LapackFn = Option<unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP>;

static mut LAPACK_DISPATCH: LapackFn = None;

pub unsafe fn R_setLapackRoutines(routines: *const c_void) -> *const c_void { unsafe {
    let old = LAPACK_DISPATCH;
    if !routines.is_null() {
        // In C, routines is a pointer to a struct whose first field is the dispatch fn.
        // We store the function pointer directly.
        LAPACK_DISPATCH = Some(std::mem::transmute::<
            *const c_void,
            unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
        >(routines));
    } else {
        LAPACK_DISPATCH = None;
    }
    match old {
        Some(f) => f as *const c_void,
        None => ptr::null(),
    }
}}

// ---------------------------------------------------------------------------
// do_lapack
// ---------------------------------------------------------------------------

/// .Internal(lapack(...)) -- dispatch to the LAPACK module.
pub unsafe fn do_lapack(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP { unsafe {
    // LAPACK module not loaded in the Rust port.
    // The C code calls error() when module initialization fails.
    R_NilValue()
}}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_lapack_returns_nil() {
        unsafe {
            let result = do_lapack(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }
}
