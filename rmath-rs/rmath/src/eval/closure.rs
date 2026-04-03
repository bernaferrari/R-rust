#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Closure application — ports R's applyClosure from eval.c.
//!
//! Handles calling R closures (user-defined functions) by:
//! 1. Creating a new environment
//! 2. Binding formal parameters to actual arguments
//! 3. Evaluating the body in the new environment

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{
    BODY, CADDR, CAR, CDDR, CDR, CLOENV, FORMALS, LENGTH, Rf_isNull, SET_CLOENV, SET_NAMED, SETCAR,
    SETCDR, SETTAG, TAG, TYPEOF,
};
use crate::sexp::context::{Rf_begincontext, Rf_endcontext, ctxt_flags};
use crate::sexp::envir::{
    CheckFormals, addMissingVarsToNewEnv, defineVar, forcePromise, matchArgs,
};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_MissingArg, R_NilValue, set_R_Visible};
use crate::sexp::memory_ext::NewEnvironment;
use crate::sexp::protect::Rf_protect;

use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// applyClosure — the main closure application function
// ---------------------------------------------------------------------------

/// Apply a closure to arguments.
///
/// This is the equivalent of R's `applyClosure()` from eval.c.
///
/// Parameters:
/// - call: the original call (for error reporting)
/// - op: the closure (CLOSXP)
/// - arglist: the evaluated or promised argument list
/// - rho: the calling environment
/// - suppliedenv: the environment of the caller (for sys.call/sys.parent)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn applyClosure(
    call: SEXP,
    op: SEXP,
    arglist: SEXP,
    rho: SEXP,
    suppliedenv: SEXP,
    _R_verbose: c_int,
) -> SEXP {
    unsafe {
        if op.is_null() || TYPEOF(op) != SEXPTYPE::CLOSXP.0 {
            return R_NilValue();
        }

        let formals = FORMALS(op);
        let body = BODY(op);
        let cloenv = CLOENV(op);

        // Match arguments to formals
        let matched = matchArgs(formals, arglist, call);
        Rf_protect(matched);

        // Create new evaluation environment
        let newrho = NewEnvironment(matched, cloenv, ptr::null_mut());
        Rf_protect(newrho);

        // Bind the matched arguments into the new environment
        let mut a = matched;
        while !a.is_null() && a != R_NilValue() {
            let sym = crate::sexp::accessors::TAG(a);
            if !sym.is_null() {
                let val = CAR(a);
                defineVar(sym, val, newrho);
            }
            a = CDR(a);
        }

        // Add missing arguments
        addMissingVarsToNewEnv(formals, arglist, newrho);

        // Set up a context for the closure call
        let _ctxt = Rf_begincontext(
            ctxt_flags::CTXT_FUNCTION,
            call,
            newrho,
            0, // sysparent (C-level, not used in Rust port)
            None,
            op,
            arglist,
        );

        // Evaluate the body
        let result = Rf_eval(body, newrho);

        // End context
        Rf_endcontext(_ctxt);

        result
    }
}

// ---------------------------------------------------------------------------
// make_applyClosure_env — create environment for closure application
// ---------------------------------------------------------------------------

/// Create the environment for a closure application.
///
/// This is a helper that separates environment creation from body evaluation.
pub unsafe fn make_applyClosure_env(op: SEXP, arglist: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let formals = FORMALS(op);
        let cloenv = CLOENV(op);

        let matched = matchArgs(formals, arglist, ptr::null_mut());
        Rf_protect(matched);

        let newrho = NewEnvironment(matched, cloenv, ptr::null_mut());
        Rf_protect(newrho);

        // Bind arguments
        let mut a = matched;
        while !a.is_null() && a != R_NilValue() {
            let sym = crate::sexp::accessors::TAG(a);
            if !sym.is_null() {
                defineVar(sym, CAR(a), newrho);
            }
            a = CDR(a);
        }

        addMissingVarsToNewEnv(formals, arglist, newrho);

        newrho
    }
}

// ---------------------------------------------------------------------------
// R_execClosure — execute a closure body in a new environment
// ---------------------------------------------------------------------------

/// Execute a closure, returning the result.
///
/// Uses catch_unwind for error recovery.
pub unsafe fn R_execClosure(
    op: SEXP,
    arglist: SEXP,
    rho: SEXP,
) -> Result<SEXP, crate::sexp::context::RError> {
    unsafe {
        let newrho = make_applyClosure_env(op, arglist, rho);
        let body = BODY(op);

        // Use catch_unwind for error recovery
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(body, newrho)));

        match result {
            Ok(val) => Ok(val),
            Err(payload) => {
                if let Some(err) = payload.downcast_ref::<crate::sexp::context::RError>() {
                    Err(crate::sexp::context::RError {
                        message: err.message.clone(),
                    })
                } else {
                    Err(crate::sexp::context::RError {
                        message: "unknown error".to_string(),
                    })
                }
            }
        }
    }
}
