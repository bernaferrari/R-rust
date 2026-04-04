#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unsafe_op_in_unsafe_fn
)]

//! Core eval() function — the heart of the R interpreter.
//!
//! This module ports R's `eval()` function from src/main/eval.c.
//! It handles expression evaluation by dispatching based on SEXPTYPE:
//! - Self-evaluating types (NILSXP, LGLSXP, INTSXP, etc.) → return as-is
//! - SYMSXP → variable lookup via R_findVar
//! - PROMSXP → force the promise
//! - LANGSXP → function call (dispatch to SPECIAL/BUILTIN/CLOSXP)
//! - BCODESXP → bytecode evaluation

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::mainutils::errors::R_MissingArgError;
use crate::sexp::accessors::{CAR, CDR, PRIMOFFSET, TYPEOF};
use crate::sexp::envir::{findFun, forcePromise, R_findVar};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{set_R_Visible, R_EvalDepth, R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::memory_ext::vmaxget;
use crate::sexp::protect::Rf_protect;
use crate::sexp::safe::Sexp;
use crate::sexp::symbol::R_DotsSymbol;

// ---------------------------------------------------------------------------
// SEXPTYPE constants for pattern matching
// ---------------------------------------------------------------------------

const NILSXP: c_int = SEXPTYPE::NILSXP.0;
const SYMSXP: c_int = SEXPTYPE::SYMSXP.0;
const LISTSXP: c_int = SEXPTYPE::LISTSXP.0;
const CLOSXP: c_int = SEXPTYPE::CLOSXP.0;
const ENVSXP: c_int = SEXPTYPE::ENVSXP.0;
const PROMSXP: c_int = SEXPTYPE::PROMSXP.0;
const LANGSXP: c_int = SEXPTYPE::LANGSXP.0;
const SPECIALSXP: c_int = SEXPTYPE::SPECIALSXP.0;
const BUILTINSXP: c_int = SEXPTYPE::BUILTINSXP.0;
const CHARSXP: c_int = SEXPTYPE::CHARSXP.0;
const LGLSXP: c_int = SEXPTYPE::LGLSXP.0;
const INTSXP: c_int = SEXPTYPE::INTSXP.0;
const REALSXP: c_int = SEXPTYPE::REALSXP.0;
const CPLXSXP: c_int = SEXPTYPE::CPLXSXP.0;
const STRSXP: c_int = SEXPTYPE::STRSXP.0;
const DOTSXP: c_int = SEXPTYPE::DOTSXP.0;
const ANYSXP: c_int = SEXPTYPE::ANYSXP.0;
const VECSXP: c_int = SEXPTYPE::VECSXP.0;
const EXPRSXP: c_int = SEXPTYPE::EXPRSXP.0;
const BCODESXP: c_int = SEXPTYPE::BCODESXP.0;
const EXTPTRSXP: c_int = SEXPTYPE::EXTPTRSXP.0;
const WEAKREFSXP: c_int = SEXPTYPE::WEAKREFSXP.0;
const RAWSXP: c_int = SEXPTYPE::RAWSXP.0;
const OBJSXP: c_int = SEXPTYPE::OBJSXP.0;

// ---------------------------------------------------------------------------
// Primitive function dispatch
// ---------------------------------------------------------------------------

/// Function pointer type for primitive functions (SPECIAL and BUILTIN).
pub type PRIMFUN = unsafe extern "C" fn(
    SEXP, // call
    SEXP, // op (the function)
    SEXP, // args
    SEXP, // rho (environment)
) -> SEXP;

/// Get the primitive function pointer for a SPECIAL or BUILTIN.
pub unsafe fn get_primfun(op: SEXP) -> Option<PRIMFUN> {
    if op.is_null() {
        return None;
    }
    let t = TYPEOF(op);
    if t != SPECIALSXP && t != BUILTINSXP {
        return None;
    }
    let offset = PRIMOFFSET(op);
    if offset < 0 {
        return None;
    }
    get_fun_tab_entry(offset)
}

/// Get a function table entry by offset.
///
/// This is a stub — the full implementation would use R_FunTab.
pub unsafe fn get_fun_tab_entry(offset: c_int) -> Option<PRIMFUN> {
    let _ = offset;
    None
}

/// Check the PRIMPRINT flag (visibility hint for primitives).
pub unsafe fn PRIMPRINT(op: SEXP) -> c_int {
    if op.is_null() {
        return 0;
    }
    let t = TYPEOF(op);
    if t != SPECIALSXP && t != BUILTINSXP {
        return 0;
    }
    0
}

/// Get the PRIMNAME for a primitive.
pub unsafe fn PRIMNAME(op: SEXP) -> &'static str {
    "unknown"
}

// ---------------------------------------------------------------------------
// Eval error type
// ---------------------------------------------------------------------------

/// Errors that can occur during evaluation.
#[derive(Debug)]
pub enum EvalError {
    TooDeeplyNested,
    IncorrectDotsContext,
    ObjectNotFound(String),
    MissingArgument,
    FunctionNotFound(String),
    NonFunction,
    UnimplementedType(c_int),
    BytecodeNotImplemented,
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::TooDeeplyNested => write!(f, "evaluation nested too deeply"),
            EvalError::IncorrectDotsContext => write!(f, "'...' used in an incorrect context"),
            EvalError::ObjectNotFound(name) => write!(f, "object '{}' not found", name),
            EvalError::MissingArgument => write!(f, "missing argument"),
            EvalError::FunctionNotFound(name) => write!(f, "could not find function \"{}\"", name),
            EvalError::NonFunction => write!(f, "attempt to apply non-function"),
            EvalError::UnimplementedType(t) => write!(f, "unimplemented type in eval: {}", t),
            EvalError::BytecodeNotImplemented => {
                write!(f, "bytecode evaluation not yet implemented")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Safe eval API
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This is the primary safe API for evaluating R expressions.
/// It wraps the raw FFI `Rf_eval` and provides a `Result` return type.
///
/// # Arguments
/// * `e` - The expression to evaluate
/// * `rho` - The environment in which to evaluate
///
/// # Returns
/// * `Ok(Sexp)` - The result of evaluation
/// * `Err(String)` - A description of the error that occurred
#[must_use]
pub fn eval<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    // SAFETY: `e` and `rho` are constructed from valid `Sexp` wrappers,
    // so their raw pointers are guaranteed non-null and valid.
    unsafe { eval_inner_safe(e.as_raw(), rho.as_raw()) }
}

/// Internal safe eval implementation.
unsafe fn eval_inner_safe(e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    if e.is_null() {
        return Ok(Sexp::from_raw_unchecked(R_NilValue()));
    }

    set_R_Visible(TRUE);

    let t = TYPEOF(e);

    // Self-evaluating types — return immediately
    if is_self_evaluating(t) {
        return Ok(Sexp::from_raw_unchecked(e));
    }

    // Check evaluation depth
    let depth = R_EvalDepth() + 1;
    if depth > 500 {
        return Err(EvalError::TooDeeplyNested.to_string());
    }
    crate::sexp::globals::set_R_EvalDepth(depth);

    let result = eval_dispatch(t, e, rho);

    // Restore depth
    crate::sexp::globals::set_R_EvalDepth(depth - 1);

    result
}

/// Check if a SEXPTYPE is self-evaluating (returns as-is without further evaluation).
fn is_self_evaluating(t: c_int) -> bool {
    matches!(
        t,
        NILSXP
            | LISTSXP
            | LGLSXP
            | INTSXP
            | REALSXP
            | STRSXP
            | CPLXSXP
            | RAWSXP
            | OBJSXP
            | SPECIALSXP
            | BUILTINSXP
            | ENVSXP
            | CLOSXP
            | VECSXP
            | EXPRSXP
            | EXTPTRSXP
            | WEAKREFSXP
    )
}

/// Dispatch evaluation based on SEXPTYPE.
unsafe fn eval_dispatch(t: c_int, e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    match t {
        // Symbol lookup
        SYMSXP => eval_symbol(e, rho),

        // Promise — force it
        PROMSXP => Ok(Sexp::from_raw_unchecked(forcePromise(e))),

        // Language (function call)
        LANGSXP => eval_lang(e, rho),

        // Bytecode
        BCODESXP => {
            eprintln!("Warning: bytecode evaluation not yet implemented");
            Ok(Sexp::from_raw_unchecked(R_NilValue()))
        }

        // DOTSXP in wrong context
        DOTSXP => Err(EvalError::IncorrectDotsContext.to_string()),

        _ => Err(EvalError::UnimplementedType(t).to_string()),
    }
}

/// Evaluate a symbol (SYMSXP) — variable lookup.
unsafe fn eval_symbol(e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    if e == R_DotsSymbol() {
        return Err(EvalError::IncorrectDotsContext.to_string());
    }

    let tmp = R_findVar(e, rho);

    if tmp == R_UnboundValue() {
        let name = get_symbol_name(e);
        return Err(EvalError::ObjectNotFound(name).to_string());
    }

    if tmp == R_MissingArg() {
        R_MissingArgError(e, ptr::null_mut(), std::ptr::null::<c_char>());
        return Err(EvalError::MissingArgument.to_string());
    }

    if TYPEOF(tmp) == PROMSXP {
        Ok(Sexp::from_raw_unchecked(forcePromise(tmp)))
    } else {
        Ok(Sexp::from_raw_unchecked(tmp))
    }
}

/// Extract the name of a symbol for error messages.
unsafe fn get_symbol_name(sym: SEXP) -> String {
    let pname = crate::sexp::accessors::PRINTNAME(sym);
    if pname.is_null() {
        return "???".to_string();
    }
    let s = crate::sexp::accessors::CHAR(pname);
    if s.is_null() {
        return "???".to_string();
    }
    std::ffi::CStr::from_ptr(s)
        .to_str()
        .unwrap_or("???")
        .to_string()
}

// ---------------------------------------------------------------------------
// FFI eval function
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This is the equivalent of R's `eval()` from src/main/eval.c.
/// It is the main dispatch function of the interpreter.
///
/// # Safety
///
/// `e` and `rho` must be valid SEXP pointers (or null).
#[unsafe(no_mangle)]
#[must_use]
pub unsafe extern "C" fn Rf_eval(e: SEXP, rho: SEXP) -> SEXP {
    match eval_inner_safe(e, rho) {
        Ok(result) => result.as_raw(),
        Err(msg) => {
            std::panic::panic_any(crate::sexp::context::RError { message: msg });
        }
    }
}

/// Internal eval implementation (legacy, delegates to safe version).
pub unsafe fn eval_inner(e: SEXP, rho: SEXP) -> SEXP {
    Rf_eval(e, rho)
}

// ---------------------------------------------------------------------------
// eval_lang — evaluate a language/function call
// ---------------------------------------------------------------------------

/// Evaluate a LANGSXP (function call expression).
unsafe fn eval_lang(e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let fun = CAR(e);
    let args = CDR(e);

    // Find the function
    let op = if TYPEOF(fun) == SYMSXP {
        findFun(fun, rho)
    } else {
        Rf_eval(fun, rho)
    };

    if op == R_UnboundValue() {
        let name = if TYPEOF(fun) == SYMSXP {
            get_symbol_name(fun)
        } else {
            "???".to_string()
        };
        return Err(EvalError::FunctionNotFound(name).to_string());
    }

    Rf_protect(op);

    match TYPEOF(op) {
        // Special — arguments not evaluated
        SPECIALSXP => eval_special(e, op, args, rho),

        // Builtin — arguments evaluated first
        BUILTINSXP => eval_builtin(e, op, args, rho),

        // Closure — full function call
        CLOSXP => eval_closure(e, op, rho),

        _ => Err(EvalError::NonFunction.to_string()),
    }
}

/// Evaluate a SPECIAL function (arguments not evaluated).
unsafe fn eval_special(e: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let _vmax = vmaxget();
    Rf_protect(e);

    let flag = PRIMPRINT(op);
    set_R_Visible(if flag != 1 { TRUE } else { FALSE });

    let tmp = if let Some(primfun) = get_primfun(op) {
        primfun(e, op, args, rho)
    } else {
        super::special::do_special_dispatch(e, op, args, rho)
    };

    if flag < 2 {
        set_R_Visible(if flag != 1 { TRUE } else { FALSE });
    }

    Ok(Sexp::from_raw_unchecked(tmp))
}

/// Evaluate a BUILTIN function (arguments evaluated first).
unsafe fn eval_builtin(e: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let _vmax = vmaxget();
    Rf_protect(e);

    // Evaluate arguments
    let evaled_args = super::dispatch::evalList(args, rho, e, 0);
    Rf_protect(evaled_args);

    let flag = PRIMPRINT(op);
    set_R_Visible(if flag != 1 { TRUE } else { FALSE });

    let tmp = if let Some(primfun) = get_primfun(op) {
        primfun(e, op, evaled_args, rho)
    } else {
        eprintln!("Warning: builtin function not implemented");
        R_NilValue()
    };

    if flag < 2 {
        set_R_Visible(if flag != 1 { TRUE } else { FALSE });
    }

    Ok(Sexp::from_raw_unchecked(tmp))
}

/// Evaluate a CLOSXP (user-defined function).
unsafe fn eval_closure(e: SEXP, op: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let args = CDR(e);
    let pargs = super::dispatch::promiseArgs(args, rho);
    Rf_protect(pargs);
    let result = super::closure::applyClosure(e, op, pargs, rho, R_NilValue(), TRUE);
    Ok(Sexp::from_raw_unchecked(result))
}

// ---------------------------------------------------------------------------
// eval with visibility preservation (for C code calling eval)
// ---------------------------------------------------------------------------

/// Evaluate an expression, preserving the R_Visible flag.
///
/// This is the equivalent of R's `evalKeepVis()` from errors.c.
pub unsafe fn eval_keep_vis(e: SEXP, rho: SEXP) -> SEXP {
    let oldvis = crate::sexp::globals::R_Visible();
    let val = Rf_eval(e, rho);
    set_R_Visible(oldvis);
    val
}
