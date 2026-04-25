#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Closure application — ports R's applyClosure from eval.c.
//!
//! Handles calling R closures (user-defined functions) by:
//! 1. Creating a new environment
//! 2. Binding formal parameters to actual arguments
//! 3. Evaluating the body in the new environment

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{BODY, CAR, CDR, SETCAR, SETCDR, SETTAG, TAG, TYPEOF};
use crate::sexp::envir::{addMissingVarsToNewEnv, defineVar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::{NewEnvironment, mkPROMISE};
use crate::sexp::object::{PairlistBuilder, PairlistIter, Sexp, SexpError};
use crate::sexp::symbol::R_DotsSymbol;

use super::eval::Rf_eval;

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

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

    let formals = closure
        .try_formals()
        .map_err(|err| sexp_err("closure formals lookup", err))?;
    let body = closure
        .try_body()
        .map_err(|err| sexp_err("closure body lookup", err))?;
    let cloenv = closure
        .try_cloenv()
        .map_err(|err| sexp_err("closure environment lookup", err))?;

    // Match arguments to formals
    let matched = match_args_safe(formals, args)?;

    // Create new environment with matched arguments
    let new_env = create_env_safe(matched, cloenv)?;

    // Bind the matched arguments into the new environment
    let frame = new_env
        .try_frame()
        .map_err(|err| sexp_err("new closure environment frame lookup", err))?;
    for cell in PairlistIter::new(frame) {
        let sym = cell
            .try_tag()
            .map_err(|err| sexp_err("matched argument tag lookup", err))?;
        if !sym.is_nil() {
            let val = cell
                .try_car()
                .map_err(|err| sexp_err("matched argument value lookup", err))?;
            unsafe {
                defineVar(sym.as_raw(), val.as_raw(), new_env.as_raw());
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

    let mut builder = PairlistBuilder::new();
    let mut formal_iter = PairlistIter::new(formals);
    let mut arg_iter = PairlistIter::new(args);

    for formal in &mut formal_iter {
        let arg = arg_iter.next();
        let tag = formal
            .try_tag()
            .map_err(|err| sexp_err("formal argument tag lookup", err))?;

        let val = if let Some(ref a) = arg {
            a.try_car()
                .map_err(|err| sexp_err("actual argument value lookup", err))?
        } else {
            unsafe { Sexp::from_raw_unchecked(R_MissingArg()) }
        };

        builder
            .push(val, (!tag.is_nil()).then_some(tag))
            .map_err(|err| sexp_err("matched argument pairlist build", err))?;
    }

    unsafe { builder.finish_as() }.map_err(|err| sexp_err("matched argument pairlist wrap", err))
}

/// Safe environment creation.
///
/// Creates a new environment with the given bindings as its frame
/// and the given parent as its enclosing environment.
pub fn create_env_safe<'a>(bindings: Sexp<'a>, parent: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let env = unsafe { NewEnvironment(bindings.as_raw(), parent.as_raw(), ptr::null_mut()) };
    Sexp::try_from_raw(env).map_err(|err| sexp_err("failed to create environment", err))
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
    if op.is_null() || TYPEOF(op) != SEXPTYPE::CLOSXP {
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

                let formals = match closure.try_formals() {
                    Ok(f) => f,
                    Err(_) => return R_NilValue(),
                };
                let cloenv = match closure.try_cloenv() {
                    Ok(e) => e,
                    Err(_) => return R_NilValue(),
                };

                let promised_args = crate::eval::dispatch::promiseArgs(arglist, rho);
                let matched = match match_closure_args(formals.as_raw(), promised_args) {
                    Ok(m) => m,
                    Err(_) => return R_NilValue(),
                };

                let new_env = match create_env_safe(Sexp::from_raw_unchecked(matched), cloenv) {
                    Ok(e) => e,
                    Err(_) => return R_NilValue(),
                };

                install_default_promises(formals.as_raw(), matched, new_env.as_raw());

                new_env.as_raw()
            }
            _ => R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| R_NilValue())
}

unsafe fn exact_tag_name_equal(left: SEXP, right: SEXP) -> bool {
    unsafe {
        if left.is_null() || right.is_null() || left == R_NilValue() || right == R_NilValue() {
            return false;
        }

        let left_name = crate::sexp::accessors::PRINTNAME(left);
        let right_name = crate::sexp::accessors::PRINTNAME(right);
        if left_name.is_null() || right_name.is_null() {
            return false;
        }

        crate::sexp::accessors::CHAR(left_name) == crate::sexp::accessors::CHAR(right_name)
    }
}

unsafe fn match_closure_args(formals: SEXP, supplied: SEXP) -> Result<SEXP, ()> {
    unsafe {
        let mut supplied_cells = Vec::new();
        let mut cur = supplied;
        while !cur.is_null() && cur != R_NilValue() {
            supplied_cells.push(cur);
            cur = CDR(cur);
        }
        let mut used = vec![false; supplied_cells.len()];

        let mut result = PairlistBuilder::new();
        let mut positional = 0usize;

        let mut formal = formals;
        while !formal.is_null() && formal != R_NilValue() {
            let formal_tag = TAG(formal);
            let mut value = R_MissingArg();
            let mut matched_index = None;

            if formal_tag == R_DotsSymbol() {
                value = collect_unused_args(&supplied_cells, &mut used);
            } else {
                for (idx, supplied_cell) in supplied_cells.iter().enumerate() {
                    if !used[idx] && exact_tag_name_equal(formal_tag, TAG(*supplied_cell)) {
                        matched_index = Some(idx);
                        break;
                    }
                }

                if matched_index.is_none() {
                    while positional < supplied_cells.len() {
                        let supplied_cell = supplied_cells[positional];
                        let supplied_tag = TAG(supplied_cell);
                        if !used[positional]
                            && (supplied_tag.is_null() || supplied_tag == R_NilValue())
                        {
                            matched_index = Some(positional);
                            positional += 1;
                            break;
                        }
                        positional += 1;
                    }
                }

                if let Some(idx) = matched_index {
                    used[idx] = true;
                    value = CAR(supplied_cells[idx]);
                }
            }

            result.push_raw(value, formal_tag).map_err(|_| ())?;
            formal = CDR(formal);
        }

        if used.iter().any(|used| !*used) {
            return Err(());
        }

        Ok(result.finish_raw())
    }
}

unsafe fn collect_unused_args(supplied_cells: &[SEXP], used: &mut [bool]) -> SEXP {
    unsafe {
        let mut dots = PairlistBuilder::new();

        for (idx, supplied_cell) in supplied_cells.iter().enumerate() {
            if used[idx] {
                continue;
            }
            used[idx] = true;
            let _ = dots.push_raw(CAR(*supplied_cell), TAG(*supplied_cell));
        }

        dots.finish_with_type(SEXPTYPE::DOTSXP)
            .unwrap_or_else(|_| R_NilValue())
    }
}

unsafe fn install_default_promises(formals: SEXP, frame: SEXP, new_env: SEXP) {
    unsafe {
        let mut formal = formals;
        let mut actual = frame;

        while !formal.is_null()
            && formal != R_NilValue()
            && !actual.is_null()
            && actual != R_NilValue()
        {
            if CAR(actual) == R_MissingArg() && CAR(formal) != R_MissingArg() {
                SETCAR(actual, mkPROMISE(CAR(formal), new_env));
            }
            formal = CDR(formal);
            actual = CDR(actual);
        }
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
