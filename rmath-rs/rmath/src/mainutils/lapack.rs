#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/lapack.c -- LAPACK module interface.
//!
//! Implements `do_lapack` which dispatches to the LAPACK module, and
//! `R_setLapackRoutines` for registering LAPACK routine pointers.

use std::os::raw::c_void;
use std::ptr;
use std::sync::Mutex;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// checkArity
// ---------------------------------------------------------------------------

unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

// ---------------------------------------------------------------------------
// R_setLapackRoutines
// ---------------------------------------------------------------------------

/// Function pointer type for the LAPACK dispatch function.
type LapackFn = Option<unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP>;

static LAPACK_DISPATCH: Mutex<LapackFn> = Mutex::new(None);

pub unsafe fn R_setLapackRoutines(routines: *const c_void) -> *const c_void {
    unsafe {
        let mut guard = LAPACK_DISPATCH.lock().unwrap_or_else(|e| e.into_inner());
        let old = *guard;
        if !routines.is_null() {
            *guard = Some(std::mem::transmute::<
                *const c_void,
                unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
            >(routines));
        } else {
            *guard = None;
        }
        match old {
            Some(f) => f as *const c_void,
            None => ptr::null(),
        }
    }
}

// ---------------------------------------------------------------------------
// do_lapack
// ---------------------------------------------------------------------------

/// .Internal(lapack(...)) -- dispatch to the LAPACK module.
pub fn do_lapack(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let guard = LAPACK_DISPATCH.lock().unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(f) => f(call, op, args, rho),
            None => R_NilValue(),
        }
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
