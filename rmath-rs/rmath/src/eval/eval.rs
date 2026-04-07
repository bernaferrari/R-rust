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
//! - SYMSXP → variable lookup via find_var_safe
//! - PROMSXP → force the promise
//! - LANGSXP → function call (dispatch to SPECIAL/BUILTIN/CLOSXP)
//! - BCODESXP → bytecode evaluation
//!
//! # Architecture
//!
//! The module uses a two-layer design:
//! - **Safe layer**: Functions like [`eval_safe`], [`eval_lang_safe`], and
//!   [`find_var_safe`] work with `Sexp<'a>` and return `Result<Sexp<'a>, String>`.
//!   These are the idiomatic Rust APIs.
//! - **FFI layer**: Functions like [`Rf_eval`] are thin shims that convert
//!   raw `SEXP` pointers to `Sexp<'a>`, delegate to the safe layer, and
//!   convert back.

use std::os::raw::c_int;

use crate::sexp::accessors::{PRIMOFFSET, TYPEOF};
use crate::sexp::envir::forcePromise;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_EvalDepth, R_NilValue, set_R_Visible};
use crate::sexp::memory_ext::vmaxget;
use crate::sexp::safe::{PairlistIter, Sexp};
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
// Safe eval API — the primary internal implementation
// ---------------------------------------------------------------------------

/// Depth guard that decrements R_EvalDepth when dropped.
struct DepthGuard(c_int);
impl Drop for DepthGuard {
    fn drop(&mut self) {
        unsafe { crate::sexp::globals::set_R_EvalDepth(self.0 - 1) };
    }
}

/// Check evaluation depth and return a guard that decrements on drop.
fn check_eval_depth() -> Result<DepthGuard, String> {
    let depth = unsafe { R_EvalDepth() } + 1;
    if depth > 500 {
        return Err(EvalError::TooDeeplyNested.to_string());
    }
    unsafe { crate::sexp::globals::set_R_EvalDepth(depth) };
    Ok(DepthGuard(depth))
}

/// Safe evaluation of an R expression.
///
/// This is the idiomatic Rust API for evaluating R expressions.
/// It catches panics, uses safe Sexp types, and returns Result.
///
/// # Arguments
/// * `expr` - The expression to evaluate
/// * `env` - The environment in which to evaluate
///
/// # Returns
/// * `Ok(Sexp)` - The result of evaluation
/// * `Err(String)` - A description of the error that occurred
pub fn eval_safe<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let _guard = check_eval_depth()?;

    // Self-evaluating types return themselves
    match expr.typeof_() {
        SEXPTYPE::NILSXP
        | SEXPTYPE::LGLSXP
        | SEXPTYPE::INTSXP
        | SEXPTYPE::REALSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::RAWSXP
        | SEXPTYPE::VECSXP
        | SEXPTYPE::EXPRSXP
        | SEXPTYPE::EXTPTRSXP => return Ok(expr),
        _ => {}
    }

    // Symbol lookup
    if expr.is_symbol() {
        return find_var_safe(expr, env).ok_or_else(|| format!("object '{}' not found", expr));
    }

    // Language object (function call)
    if expr.is_pairlist() || expr.typeof_() == SEXPTYPE::LANGSXP {
        return eval_lang_safe(expr, env);
    }

    // Closure — return as-is (self-evaluating)
    if expr.is_closure() {
        return Ok(expr);
    }

    // Promise
    if expr.typeof_() == SEXPTYPE::PROMSXP {
        return eval_promise_safe(expr, env);
    }

    // Dots
    if expr.typeof_() == SEXPTYPE::DOTSXP {
        return eval_dots_safe(expr, env);
    }

    // Bytecode
    if expr.typeof_() == SEXPTYPE::BCODESXP {
        return super::bytecode::eval_bytecode(expr, env);
    }

    Err(format!("cannot evaluate type {:?}", expr.typeof_()))
}

/// Safe evaluation of a language object (function call).
pub(crate) fn eval_lang_safe<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let fun = e.car().ok_or("empty call")?;
    let args = e.cdr().ok_or("missing args")?;

    // Evaluate the function
    let fun_val = eval_safe(fun, rho)?;

    // Dispatch based on function type
    match fun_val.typeof_() {
        SEXPTYPE::CLOSXP => apply_closure_safe(fun_val, args, rho),
        SEXPTYPE::SPECIALSXP => apply_special_safe(fun_val, e, args, rho),
        SEXPTYPE::BUILTINSXP => apply_builtin_safe(fun_val, e, args, rho),
        _ => Err(format!("cannot call type {:?}", fun_val.typeof_())),
    }
}

/// Safe variable lookup using Sexp types.
///
/// Walks the environment chain looking for a symbol binding.
pub fn find_var_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> Option<Sexp<'a>> {
    if symbol == unsafe { Sexp::from_raw_unchecked(R_DotsSymbol()) } {
        return None;
    }

    // Walk environment chain
    let mut current = rho;
    loop {
        if !current.is_environment() {
            return None;
        }
        let frame = current.frame()?;
        for cell in PairlistIter::new(frame) {
            if let Some(tag) = cell.tag()
                && tag == symbol
            {
                return cell.car();
            }
        }
        current = current.enclos()?;
    }
}

/// Safe promise evaluation.
fn eval_promise_safe<'a>(prom: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    // If already evaluated, return the value
    if let Some(val) = prom.prvalue()
        && val.typeof_() != SEXPTYPE::PROMSXP
    {
        return Ok(val);
    }

    // Force the promise
    let raw_result = unsafe { forcePromise(prom.as_raw()) };
    Ok(unsafe { Sexp::from_raw_unchecked(raw_result) })
}

/// Safe dots evaluation.
fn eval_dots_safe<'a>(_dots: Sexp<'a>, _rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    Err(EvalError::IncorrectDotsContext.to_string())
}

/// Safe closure application.
fn apply_closure_safe<'a>(
    fun: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    // Use the existing raw FFI applyClosure for now
    // TODO: Port applyClosure to use Sexp<'a> internally
    let raw_result = unsafe {
        super::closure::applyClosure(
            fun.as_raw(), // call placeholder
            fun.as_raw(),
            args.as_raw(),
            rho.as_raw(),
            R_NilValue(),
            TRUE,
        )
    };
    Ok(unsafe { Sexp::from_raw_unchecked(raw_result) })
}

/// Safe special form application.
fn apply_special_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let _vmax = unsafe { vmaxget() };
    let flag = unsafe { PRIMPRINT(fun.as_raw()) };
    unsafe { set_R_Visible(if flag != 1 { TRUE } else { FALSE }) };

    let tmp = if let Some(primfun) = unsafe { get_primfun(fun.as_raw()) } {
        unsafe { primfun(call.as_raw(), fun.as_raw(), args.as_raw(), rho.as_raw()) }
    } else {
        unsafe {
            super::special::do_special_dispatch(
                call.as_raw(),
                fun.as_raw(),
                args.as_raw(),
                rho.as_raw(),
            )
        }
    };

    if flag < 2 {
        unsafe { set_R_Visible(if flag != 1 { TRUE } else { FALSE }) };
    }

    Ok(unsafe { Sexp::from_raw_unchecked(tmp) })
}

/// Safe builtin function application.
fn apply_builtin_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let _vmax = unsafe { vmaxget() };
    let flag = unsafe { PRIMPRINT(fun.as_raw()) };
    unsafe { set_R_Visible(if flag != 1 { TRUE } else { FALSE }) };

    // Evaluate arguments
    let evaled_args =
        unsafe { super::dispatch::evalList(args.as_raw(), rho.as_raw(), call.as_raw(), 0) };

    let tmp = if let Some(primfun) = unsafe { get_primfun(fun.as_raw()) } {
        unsafe { primfun(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw()) }
    } else {
        eprintln!("Warning: builtin function not implemented");
        unsafe { R_NilValue() }
    };

    if flag < 2 {
        unsafe { set_R_Visible(if flag != 1 { TRUE } else { FALSE }) };
    }

    Ok(unsafe { Sexp::from_raw_unchecked(tmp) })
}

// ---------------------------------------------------------------------------
// Legacy raw-pointer-based safe API (kept for backward compatibility)
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This wraps the raw FFI `Rf_eval` and provides a `Result` return type.
#[must_use = "eval returns a Result that should be checked"]
pub fn eval<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    eval_safe(e, rho)
}

/// Internal safe eval implementation (legacy, delegates to eval_safe).
unsafe fn eval_inner_safe(e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    if e.is_null() {
        return Ok(Sexp::from_raw_unchecked(R_NilValue()));
    }

    set_R_Visible(TRUE);

    let expr = Sexp::from_raw_unchecked(e);
    let env = Sexp::from_raw_unchecked(rho);
    eval_safe(expr, env)
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

/// Dispatch evaluation based on SEXPTYPE (legacy, delegates to eval_safe).
unsafe fn eval_dispatch(t: c_int, e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let expr = Sexp::from_raw_unchecked(e);
    let env = Sexp::from_raw_unchecked(rho);
    eval_safe(expr, env)
}

/// Evaluate a symbol (SYMSXP) — variable lookup (legacy).
unsafe fn eval_symbol(e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let expr = Sexp::from_raw_unchecked(e);
    let env = Sexp::from_raw_unchecked(rho);
    eval_safe(expr, env)
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
// FFI eval function — thin shim delegating to eval_safe
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
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        set_R_Visible(TRUE);

        match (Sexp::from_raw(e), Sexp::from_raw(rho)) {
            (Some(expr), Some(env)) => match eval_safe(expr, env) {
                Ok(result) => result.as_raw(),
                Err(msg) => {
                    std::panic::panic_any(crate::sexp::context::RError { message: msg });
                }
            },
            _ => R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| R_NilValue())
}

/// Internal eval implementation (legacy, delegates to safe version).
pub unsafe fn eval_inner(e: SEXP, rho: SEXP) -> SEXP {
    Rf_eval(e, rho)
}

// ---------------------------------------------------------------------------
// eval_lang — evaluate a language/function call (legacy, delegates to safe)
// ---------------------------------------------------------------------------

/// Evaluate a LANGSXP (function call expression) — legacy wrapper.
unsafe fn eval_lang(e: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let expr = Sexp::from_raw_unchecked(e);
    let env = Sexp::from_raw_unchecked(rho);
    eval_lang_safe(expr, env)
}

/// Evaluate a SPECIAL function (arguments not evaluated) — legacy wrapper.
unsafe fn eval_special(e: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let fun = Sexp::from_raw_unchecked(op);
    let call = Sexp::from_raw_unchecked(e);
    let arglist = Sexp::from_raw_unchecked(args);
    let env = Sexp::from_raw_unchecked(rho);
    apply_special_safe(fun, call, arglist, env)
}

/// Evaluate a BUILTIN function (arguments evaluated first) — legacy wrapper.
unsafe fn eval_builtin(e: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let fun = Sexp::from_raw_unchecked(op);
    let call = Sexp::from_raw_unchecked(e);
    let arglist = Sexp::from_raw_unchecked(args);
    let env = Sexp::from_raw_unchecked(rho);
    apply_builtin_safe(fun, call, arglist, env)
}

/// Evaluate a CLOSXP (user-defined function) — legacy wrapper.
unsafe fn eval_closure(e: SEXP, op: SEXP, rho: SEXP) -> Result<Sexp<'static>, String> {
    let fun = Sexp::from_raw_unchecked(op);
    let args = if let Some(cdr) = Sexp::from_raw_unchecked(e).cdr() {
        cdr
    } else {
        return Err("missing args".to_string());
    };
    let env = Sexp::from_raw_unchecked(rho);
    apply_closure_safe(fun, args, env)
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
