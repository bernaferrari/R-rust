#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/lapack.c -- LAPACK module interface.
//!
//! Implements `do_lapack` which dispatches to the LAPACK module, and
//! `R_setLapackRoutines` for registering LAPACK routine pointers.

use std::os::raw::c_void;
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_required_current_instance;

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

#[derive(Default)]
pub(crate) struct LapackRuntimeState {
    dispatch: LapackFn,
}

pub unsafe fn R_setLapackRoutines(routines: *const c_void) -> *const c_void {
    unsafe {
        let new_dispatch = if !routines.is_null() {
            Some(std::mem::transmute::<
                *const c_void,
                unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
            >(routines))
        } else {
            None
        };
        let old = with_required_current_instance(|instance| {
            let old = instance.lapack_state.dispatch;
            instance.lapack_state.dispatch = new_dispatch;
            old
        });
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
        match with_required_current_instance(|instance| instance.lapack_state.dispatch) {
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
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    unsafe extern "C" fn return_op(_call: SEXP, op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
        op
    }

    unsafe extern "C" fn return_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
        args
    }

    #[test]
    fn test_do_lapack_returns_nil() {
        unsafe {
            let _session = crate::sexp::session::RSession::new();
            let result = do_lapack(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn lapack_dispatch_is_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let old = R_setLapackRoutines(return_op as *const c_void);
            assert!(old.is_null());
            let op = 0x01usize as SEXP;
            let args = 0x02usize as SEXP;
            assert_eq!(do_lapack(ptr::null_mut(), op, args, ptr::null_mut()), op);

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            assert_eq!(
                do_lapack(ptr::null_mut(), op, args, ptr::null_mut()),
                R_NilValue()
            );
            R_setLapackRoutines(return_args as *const c_void);
            assert_eq!(do_lapack(ptr::null_mut(), op, args, ptr::null_mut()), args);

            set_current_instance(&mut first);
            assert_eq!(do_lapack(ptr::null_mut(), op, args, ptr::null_mut()), op);

            clear_current_instance();
        }
    }
}
