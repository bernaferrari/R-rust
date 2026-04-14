#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unsafe_op_in_unsafe_fn
)]

//! Closure application — ports R's applyClosure from eval.c.
//!
//! Handles calling R closures (user-defined functions) by:
//! 1. Creating a new environment
//! 2. Binding formal parameters to actual arguments
//! 3. Evaluating the body in the new environment

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{BODY, TYPEOF};
use crate::sexp::envir::{addMissingVarsToNewEnv, defineVar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::NewEnvironment;
use crate::sexp::safe::{PairlistIter, Sexp};

use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// Safe closure application — the primary internal implementation
// ---------------------------------------------------------------------------

/// Safe closure application using Sexp<'a>.
///
/// This is the idiomatic Rust API for applying R closures.
/// It extracts formals, body, and environment from the closure,
/// matches arguments to formals, creates a new evaluation environment,
/// and evaluates the body.
pub fn apply_closure_safe<'a>(
    closure: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    if !closure.is_closure() {
        return Err("not a closure".to_string());
    }

    let formals = closure.formals().ok_or("closure has no formals")?;
    let body = closure.body().ok_or("closure has no body")?;
    let cloenv = closure.cloenv().ok_or("closure has no environment")?;

    // Match arguments to formals
    let matched = match_args_safe(formals, args)?;

    // Create new environment with matched arguments
    let new_env = create_env_safe(matched, cloenv)?;

    // Bind the matched arguments into the new environment
    if let Some(frame) = new_env.frame() {
        for cell in PairlistIter::new(frame) {
            if let Some(sym) = cell.tag()
                && let Some(val) = cell.car()
            {
                unsafe {
                    defineVar(sym.as_raw(), val.as_raw(), new_env.as_raw());
                }
            }
        }
    }

    // Add missing arguments
    unsafe {
        addMissingVarsToNewEnv(formals.as_raw(), args.as_raw(), new_env.as_raw());
    }

    // Evaluate body in new environment
    crate::eval::eval::eval_safe(body, new_env)
}

/// Safe argument matching using Sexp<'a> and PairlistIter.
///
/// Matches actual arguments to formal parameters, building a new
/// pairlist with the matched values.
pub fn match_args_safe<'a>(formals: Sexp<'a>, args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    if formals.is_nil() {
        return Ok(args);
    }

    crate::sexp::memory::with_arena(|arena| {
        let mut result: SEXP = ptr::null_mut();
        let mut tail: SEXP = ptr::null_mut();

        let mut formal_iter = PairlistIter::new(formals);
        let mut arg_iter = PairlistIter::new(args);

        for formal in &mut formal_iter {
            let arg = arg_iter.next();
            let tag = formal.tag();

            let val = if let Some(ref a) = arg {
                a.car()
                    .unwrap_or_else(|| unsafe { Sexp::from_raw_unchecked(R_NilValue()) })
            } else {
                unsafe { Sexp::from_raw_unchecked(R_MissingArg()) }
            };

            let cell = arena.cons(
                val.as_raw(),
                ptr::null_mut(),
                tag.map(|t| t.as_raw()).unwrap_or(ptr::null_mut()),
            );

            if result.is_null() {
                result = cell;
                tail = cell;
            } else {
                unsafe {
                    (*tail).data.listsxp.cdrval = cell;
                }
                tail = cell;
            }
        }

        Ok(unsafe { Sexp::from_raw_unchecked(result) })
    })
}

/// Safe environment creation.
///
/// Creates a new environment with the given bindings as its frame
/// and the given parent as its enclosing environment.
pub fn create_env_safe<'a>(bindings: Sexp<'a>, parent: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let env = unsafe { NewEnvironment(bindings.as_raw(), parent.as_raw(), ptr::null_mut()) };
    Sexp::from_raw(env).ok_or("failed to create environment".to_string())
}

// ---------------------------------------------------------------------------
// FFI closure functions — thin shims delegating to safe versions
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
pub unsafe fn applyClosure(
    call: SEXP,
    op: SEXP,
    arglist: SEXP,
    rho: SEXP,
    suppliedenv: SEXP,
    _R_verbose: c_int,
) -> SEXP {
    if op.is_null() || TYPEOF(op) != SEXPTYPE::CLOSXP.0 {
        return R_NilValue();
    }

    let newrho = make_applyClosure_env(op, arglist, rho);
    if newrho.is_null() || newrho == R_NilValue() {
        return R_NilValue();
    }

    let body = BODY(op);
    if body.is_null() {
        return R_NilValue();
    }

    let ctx = crate::sexp::context::Rf_begincontext(
        crate::sexp::context::ctxt_flags::CTXT_FUNCTION
            | crate::sexp::context::ctxt_flags::CTXT_RETURN,
        call,
        newrho,
        rho,
        None,
        op,
        ptr::null_mut(),
    );

    struct CtxGuard(*mut crate::sexp::context::RCNTXT);
    impl Drop for CtxGuard {
        fn drop(&mut self) {
            unsafe {
                crate::sexp::context::Rf_endcontext(self.0);
            }
        }
    }
    let _ctx_guard = CtxGuard(ctx);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::eval::eval::Rf_eval(body, newrho)
    }));

    match result {
        Ok(val) => val,
        Err(payload) => crate::sexp::context::handle_closure_signal(payload),
    }
}

// ---------------------------------------------------------------------------
// make_applyClosure_env — create environment for closure application
// ---------------------------------------------------------------------------

/// Create the environment for a closure application.
///
/// This is a helper that separates environment creation from body evaluation.
pub unsafe fn make_applyClosure_env(op: SEXP, arglist: SEXP, rho: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match (
            Sexp::from_raw(op),
            Sexp::from_raw(arglist),
            Sexp::from_raw(rho),
        ) {
            (Some(closure), Some(args), Some(env)) => {
                if !closure.is_closure() {
                    return R_NilValue();
                }

                let formals = match closure.formals() {
                    Some(f) => f,
                    None => return R_NilValue(),
                };
                let cloenv = match closure.cloenv() {
                    Some(e) => e,
                    None => return R_NilValue(),
                };

                let matched = match match_args_safe(formals, args) {
                    Ok(m) => m,
                    Err(_) => return R_NilValue(),
                };

                let new_env = match create_env_safe(matched, cloenv) {
                    Ok(e) => e,
                    Err(_) => return R_NilValue(),
                };

                // Bind arguments
                if let Some(frame) = new_env.frame() {
                    for cell in PairlistIter::new(frame) {
                        if let Some(sym) = cell.tag()
                            && let Some(val) = cell.car()
                        {
                            defineVar(sym.as_raw(), val.as_raw(), new_env.as_raw());
                        }
                    }
                }

                unsafe {
                    addMissingVarsToNewEnv(formals.as_raw(), args.as_raw(), new_env.as_raw());
                }

                new_env.as_raw()
            }
            _ => R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| R_NilValue())
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
    let newrho = make_applyClosure_env(op, arglist, rho);
    if newrho.is_null() || newrho == R_NilValue() {
        return Err(crate::sexp::context::RError {
            message: "failed to create closure environment".to_string(),
        });
    }

    let body = BODY(op);

    // Use catch_unwind for error recovery
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(body, newrho)));

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
