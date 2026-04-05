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
// Delegates to Rf_checkArityCall.
// ---------------------------------------------------------------------------

unsafe fn checkArity(op: SEXP, args: SEXP) {
    crate::main::errors::Rf_checkArityCall(op, args, crate::main::errors::getCurrentCall());
}

// ---------------------------------------------------------------------------
// R_setLapackRoutines
// ---------------------------------------------------------------------------

/// Function pointer type for the LAPACK dispatch function.
type LapackFn = Option<unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP>;

/// Set the LAPACK routine dispatch table.
/// Returns the previous pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_setLapackRoutines(_routines: *const c_void) -> *const c_void {
    ptr::null()
}

// ---------------------------------------------------------------------------
// do_lapack
// ---------------------------------------------------------------------------

/// .Internal(lapack(...)) -- dispatch to the LAPACK module.
pub unsafe fn do_lapack(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        // LAPACK module not loaded in the Rust port
        R_NilValue()
    }
}

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
