#![deny(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

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
//! Rust code should enter through [`EvalContext`] or [`eval_expr`], which work
//! with owner-scoped `Sexp<'a>` handles and return `Result<Sexp<'a>, String>`.
//! The C-shaped [`Rf_eval`] entrypoint is crate-local translation scaffolding
//! for ported code that still passes raw `SEXP` pointers.

use std::ffi::CString;
use std::os::raw::c_int;

use crate::sexp::accessors::{CHAR, PRINTNAME, TYPEOF};
use crate::sexp::envir::{find_fun_result, forcePromise};
use crate::sexp::ffi::{SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::object::{Sexp, SexpError};
use crate::sexp::symbol::{R_DotsSymbol, symbol_name_from_ptr};

use super::apply::{apply_builtin_safe, apply_closure_safe, apply_special_safe};
#[allow(unused_imports)]
pub use super::error::EvalError;
#[allow(unused_imports)]
pub use super::limits::{
    EvalLimits, EvalTimerGuard, check_eval_depth, eval_with_limits, get_eval_limits,
    reset_eval_limits, set_eval_limits,
};
#[allow(unused_imports)]
pub use super::primitive::{
    PRIMNAME, PRIMPRINT, PrimFun as PRIMFUN, PrimitiveDescriptor, get_fun_tab_entry, get_primfun,
};

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

// ---------------------------------------------------------------------------
// SEXPTYPE constants for pattern matching
// ---------------------------------------------------------------------------

const NILSXP: c_int = SEXPTYPE::NILSXP.as_c_int();
const SYMSXP: c_int = SEXPTYPE::SYMSXP.as_c_int();
const LISTSXP: c_int = SEXPTYPE::LISTSXP.as_c_int();
const CLOSXP: c_int = SEXPTYPE::CLOSXP.as_c_int();
const ENVSXP: c_int = SEXPTYPE::ENVSXP.as_c_int();
const PROMSXP: c_int = SEXPTYPE::PROMSXP.as_c_int();
const LANGSXP: c_int = SEXPTYPE::LANGSXP.as_c_int();
const SPECIALSXP: c_int = SEXPTYPE::SPECIALSXP.as_c_int();
const BUILTINSXP: c_int = SEXPTYPE::BUILTINSXP.as_c_int();
const CHARSXP: c_int = SEXPTYPE::CHARSXP.as_c_int();
const LGLSXP: c_int = SEXPTYPE::LGLSXP.as_c_int();
const INTSXP: c_int = SEXPTYPE::INTSXP.as_c_int();
const REALSXP: c_int = SEXPTYPE::REALSXP.as_c_int();
const CPLXSXP: c_int = SEXPTYPE::CPLXSXP.as_c_int();
const STRSXP: c_int = SEXPTYPE::STRSXP.as_c_int();
const DOTSXP: c_int = SEXPTYPE::DOTSXP.as_c_int();
const ANYSXP: c_int = SEXPTYPE::ANYSXP.as_c_int();
const VECSXP: c_int = SEXPTYPE::VECSXP.as_c_int();
const EXPRSXP: c_int = SEXPTYPE::EXPRSXP.as_c_int();
const BCODESXP: c_int = SEXPTYPE::BCODESXP.as_c_int();
const EXTPTRSXP: c_int = SEXPTYPE::EXTPTRSXP.as_c_int();
const WEAKREFSXP: c_int = SEXPTYPE::WEAKREFSXP.as_c_int();
const RAWSXP: c_int = SEXPTYPE::RAWSXP.as_c_int();
const OBJSXP: c_int = SEXPTYPE::OBJSXP.as_c_int();

// ---------------------------------------------------------------------------
// Safe eval API — the primary internal implementation
// ---------------------------------------------------------------------------

/// Rust-shaped evaluator bound to one environment.
///
/// This is the preferred entrypoint for Rust code. It keeps expression and
/// environment ownership in the type system; raw `SEXP` pointers should only
/// reach this layer after an arena or session has wrapped them as `Sexp`.
#[derive(Clone, Debug)]
pub struct EvalContext<'a> {
    env: Sexp<'a>,
}

impl<'a> EvalContext<'a> {
    /// Create an evaluator for `env`.
    pub fn new(env: Sexp<'a>) -> Self {
        EvalContext { env }
    }

    /// Return the environment used by this evaluator.
    pub fn env(self) -> Sexp<'a> {
        self.env
    }

    /// Evaluate an expression in this context.
    pub fn eval(self, expr: Sexp<'a>) -> Result<Sexp<'a>, String> {
        if !self.env.clone().is_owner_scoped() {
            return Err("eval context environment is not owner-scoped".to_string());
        }
        if !expr.clone().is_owner_scoped() {
            return Err("eval expression is not owner-scoped".to_string());
        }
        eval_expr(expr, self.env)
    }
}

/// Evaluate an expression using owner-scoped Rust handles.
///
/// This function is the Rust-shaped evaluator entrypoint. It performs the
/// evaluator-side cancellation/visibility setup that the legacy raw `Rf_eval`
/// shim used to own, then delegates to the safe evaluator implementation.
pub fn eval_expr<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let _timer = EvalTimerGuard::start_if_needed();
    crate::sexp::instance::check_cancellation();
    super::runtime::set_visible(TRUE);

    match eval_safe(expr.clone(), env) {
        Ok(result) => Ok(result),
        Err(message) if is_simple_warning_hook_call(expr) => {
            Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) })
        }
        Err(message) => Err(message),
    }
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
    // Dynamically constructed calls can contain literal heap values that are
    // reachable only through this expression while recursive evaluation runs.
    let _expr_root = unsafe { crate::sexp::protect::protect(expr.clone().as_raw()) };
    let _env_root = unsafe { crate::sexp::protect::protect(env.clone().as_raw()) };
    let _guard = check_eval_depth()?;

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| eval_safe_inner(expr, env)));

    match result {
        Ok(inner) => inner,
        Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
            Ok(signal) => match *signal {
                crate::sexp::context::RSignal::Error { message } => Err(message),
                other => std::panic::panic_any(other),
            },
            Err(payload) => match payload.downcast::<crate::sexp::context::RError>() {
                Ok(err) => Err(err.message.clone()),
                Err(payload) => std::panic::resume_unwind(payload),
            },
        },
    }
}

fn symbol_name_for_error(expr: Sexp<'_>) -> String {
    if let Some(name) = symbol_name_from_ptr(expr.clone().as_raw()) {
        return name;
    }
    unsafe {
        if TYPEOF(expr.clone().as_raw()) == SEXPTYPE::SYMSXP {
            let pname = PRINTNAME(expr.as_raw());
            if !pname.is_null() {
                let bytes = CHAR(pname);
                if !bytes.is_null() {
                    return std::ffi::CStr::from_ptr(bytes)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
    }
    "<unknown>".to_string()
}

fn eval_safe_inner<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    match classify_expr(expr.clone()) {
        EvalKind::SelfEvaluating => Ok(expr),
        EvalKind::Symbol => {
            if let Some(value) = find_var_result(expr.clone(), env)? {
                return Ok(value);
            }
            match primitive_for_symbol(expr.clone()) {
                Some(primitive) => Ok(primitive),
                None => Err(format!(
                    "object '{}' not found",
                    symbol_name_for_error(expr)
                )),
            }
        }
        EvalKind::Language => eval_lang_safe(expr, env),
        EvalKind::Closure => Ok(expr),
        // Environments are first-class values (upstream Rf_eval returns
        // ENVSXP unchanged): `local({...})` blocks capture them via
        // `environment()`, and package shims return them as values.
        EvalKind::Environment => Ok(expr),
        EvalKind::Promise => eval_promise_safe(expr, env),
        EvalKind::Dots => eval_dots_safe(expr, env),
        EvalKind::Bytecode => eval_bytecode_safe(expr, env),
        EvalKind::Unsupported(kind) => Err(format!("cannot evaluate type {:?}", kind)),
    }
}

fn eval_bytecode_safe<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    if super::jit::get_R_disable_bytecode() != 0 {
        return Err("bytecode evaluation is disabled for this R session".to_string());
    }
    // Ensure depth guard for BC recursion (e.g. deep fib), like AST path.
    // This fixes depth limit for BC bodies.
    let _guard = super::limits::check_eval_depth().map_err(|e| e.to_string())?;
    let result = unsafe { super::bc_eval::bcEval(expr.as_raw(), env.as_raw()) };
    Ok(unsafe { Sexp::from_raw_unchecked(result) })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvalKind {
    SelfEvaluating,
    Symbol,
    Language,
    Closure,
    Environment,
    Promise,
    Dots,
    Bytecode,
    Unsupported(SEXPTYPE),
}

fn classify_expr(expr: Sexp<'_>) -> EvalKind {
    match expr.typeof_() {
        SEXPTYPE::NILSXP
        | SEXPTYPE::LISTSXP
        | SEXPTYPE::LGLSXP
        | SEXPTYPE::INTSXP
        | SEXPTYPE::REALSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::RAWSXP
        | SEXPTYPE::VECSXP
        // eval.c Rf_eval: an expression vector is a VALUE and is returned
        // unchanged; only do_eval() (R-level eval()) walks its elements.
        | SEXPTYPE::EXPRSXP
        | SEXPTYPE::EXTPTRSXP
        // Function objects are values too.  A call may legitimately contain
        // a function object in head position (for example, higher-order
        // helpers construct calls from an already matched `FUN`).  GNU R's
        // Rf_eval returns these objects unchanged before eval_lang dispatches
        // them; rejecting them here made `outer(..., paste)` fail while
        // evaluating the constructed call head.
        | SEXPTYPE::BUILTINSXP
        | SEXPTYPE::SPECIALSXP
        | SEXPTYPE::OBJSXP => EvalKind::SelfEvaluating,

        SEXPTYPE::SYMSXP => EvalKind::Symbol,
        SEXPTYPE::LANGSXP => EvalKind::Language,
        SEXPTYPE::CLOSXP => EvalKind::Closure,
        SEXPTYPE::ENVSXP => EvalKind::Environment,
        SEXPTYPE::PROMSXP => EvalKind::Promise,
        SEXPTYPE::DOTSXP => EvalKind::Dots,
        SEXPTYPE::BCODESXP => EvalKind::Bytecode,
        kind => EvalKind::Unsupported(kind),
    }
}

/// Safe evaluation of a language object (function call).
pub(crate) fn eval_lang_safe<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let fun = e
        .clone()
        .try_car()
        .clone()
        .map_err(|err| sexp_err("empty call", err))?;
    let args = e
        .clone()
        .try_cdr()
        .clone()
        .map_err(|err| sexp_err("missing args", err))?;

    // R uses function-position lookup for symbolic call heads: non-function
    // bindings are skipped while walking enclosing environments.
    let fun_val = if fun.clone().typeof_() == SEXPTYPE::SYMSXP {
        match find_fun_result(fun.clone(), rho.clone())? {
            Some(value) => value,
            None => match primitive_for_symbol(fun.clone()) {
                Some(primitive) => primitive,
                None => {
                    // Upstream findFun3 raises R_FunctionNotFoundError with
                    // the LANGSXP being evaluated, so the top-level render
                    // attributes the error to that call: `Error in <call> :
                    // could not find function "<name>"`.
                    let name = unsafe { get_symbol_name(fun.as_raw()) };
                    crate::mainutils::errors::errorcall_str(
                        e.as_raw(),
                        &format!("could not find function \"{name}\""),
                    );
                }
            },
        }
    } else {
        eval_safe(fun, rho.clone())?
    };

    // Dispatch based on function type
    match fun_val.clone().typeof_() {
        SEXPTYPE::CLOSXP => apply_closure_safe(fun_val, e, args, rho),
        SEXPTYPE::SPECIALSXP => apply_special_safe(fun_val, e, args, rho),
        SEXPTYPE::BUILTINSXP => apply_builtin_safe(fun_val, e, args, rho),
        _ => Err(format!("cannot call type {:?}", fun_val.typeof_())),
    }
}

pub(crate) fn primitive_for_symbol<'a>(symbol: Sexp<'a>) -> Option<Sexp<'a>> {

    let name = unsafe { get_symbol_name(symbol.as_raw()) };
    if crate::eval::builtin::is_hidden_builtin_name(&name) {
        return None;
    }
    if crate::eval::builtin::evaluated_builtin_handler(&name).is_some() {
        let primitive =
            unsafe { crate::eval::primitive::make_primitive_binding(&name, SEXPTYPE::BUILTINSXP) };
        if !primitive.is_null() && primitive != unsafe { R_NilValue() } {
            return Some(unsafe { Sexp::from_raw_unchecked(primitive) });
        }
    }
    if let Some(primitive) = CString::new(name.as_str())
        .ok()
        .map(|n| unsafe { crate::mainutils::names::R_Primitive(n.as_ptr()) })
        .filter(|primitive| !primitive.is_null() && *primitive != unsafe { R_NilValue() })
        .map(|primitive| unsafe { Sexp::from_raw_unchecked(primitive) })
    {
        return Some(primitive);
    }
    if crate::eval::builtin::unevaluated_builtin_handler(&name).is_some() {
        let primitive =
            unsafe { crate::eval::primitive::make_primitive_binding(&name, SEXPTYPE::BUILTINSXP) };
        if !primitive.is_null() && primitive != unsafe { R_NilValue() } {
            return Some(unsafe { Sexp::from_raw_unchecked(primitive) });
        }
    }
    None
}

/// Safe variable lookup using Sexp types.
///
/// Walks the environment chain looking for a symbol binding.
pub fn find_var_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> Option<Sexp<'a>> {
    find_var_result(symbol, rho).ok().flatten()
}

/// Raise R's missing-argument error, attributed like upstream.
///
/// Upstream `Rf_eval`'s SYMSXP case raises `R_MissingArgError(e,
/// getLexicalCall(rho))` — the call of the enclosing closure context — so the
/// top-level render shows `Error in f() : argument "x" is missing, with no
/// default`. `R_getCurrentCall()` returns that innermost context call here.
fn missing_arg_error(name: &str) -> ! {
    crate::mainutils::errors::errorcall_str(
        unsafe { crate::mainutils::errors::R_getCurrentCall() },
        &format!("argument \"{name}\" is missing, with no default"),
    )
}

/// Checked variable lookup using typed SEXP field access.
///
/// `Ok(None)` means the binding was not found. `Err` means the environment
/// chain or binding cells were structurally invalid for the operation.
pub(crate) fn find_var_result<'a>(
    symbol: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Option<Sexp<'a>>, String> {
    if symbol == unsafe { Sexp::from_raw_unchecked(R_DotsSymbol()) } {
        return Ok(None);
    }

    // GNU Rf_eval SYMSXP: DDVAL names (`..1`, `..2`, ...) go through
    // ddfindVar, not ordinary findVar.
    if crate::sexp::envir::dd_val(symbol.clone()).is_some() {
        let Some(value) = crate::sexp::envir::dd_find_var_safe(symbol.clone(), rho) else {
            return Ok(None);
        };
        if value.clone().as_raw() == unsafe { R_MissingArg() } {
            let name = unsafe { get_symbol_name(symbol.as_raw()) };
            missing_arg_error(&name);
        }
        return Ok(Some(value));
    }

    // GNU Rf_eval SYMSXP: findVar is unforced. A MissingArg *binding* is
    // a missing formal. A promise whose forced value is the empty symbol
    // (lapply/vapply over formals) is a real value.
    let binding = crate::sexp::envir::find_var_binding_result(symbol.clone(), rho.clone())?;
    let Some(binding) = binding else {
        return Ok(None);
    };
    if binding.clone().as_raw() == unsafe { R_MissingArg() } {
        let name = unsafe { get_symbol_name(symbol.as_raw()) };
        missing_arg_error(&name);
    }
    if binding.clone().typeof_() == SEXPTYPE::PROMSXP {
        return eval_promise_safe(binding, rho).map(Some);
    }
    Ok(Some(binding))

}

/// Safe promise evaluation.
fn eval_promise_safe<'a>(prom: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    // If already evaluated, return the value
    let val = prom
        .clone()
        .try_prvalue()
        .clone()
        .map_err(|err| sexp_err("promise value lookup", err))?;
    if val.clone().as_raw() != unsafe { R_UnboundValue() } {
        return Ok(val);
    }

    // Force the promise
    let raw_result = unsafe { forcePromise(prom.as_raw()) };
    Sexp::try_from_raw(raw_result).map_err(|err| sexp_err("forced promise result", err))
}

/// Safe dots evaluation.
fn eval_dots_safe<'a>(_dots: Sexp<'a>, _rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    Err(EvalError::IncorrectDotsContext.to_string())
}

// Application of closures, specials, and builtins lives in `eval::apply`.
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
unsafe fn eval_inner_safe<'a>(e: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    if e.is_null() {
        return Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) });
    }

    super::runtime::set_visible(TRUE);

    let expr = unsafe { Sexp::from_raw_unchecked(e) };
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
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
unsafe fn eval_dispatch<'a>(t: c_int, e: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    let expr = unsafe { Sexp::from_raw_unchecked(e) };
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
    eval_safe(expr, env)
}

/// Evaluate a symbol (SYMSXP) — variable lookup (legacy).
unsafe fn eval_symbol<'a>(e: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    let expr = unsafe { Sexp::from_raw_unchecked(e) };
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
    eval_safe(expr, env)
}

/// Extract the name of a symbol for error messages.
unsafe fn get_symbol_name(sym: SEXP) -> String {
    let pname = unsafe { crate::sexp::accessors::PRINTNAME(sym) };
    if pname.is_null() {
        return "???".to_string();
    }
    let s = unsafe { crate::sexp::accessors::CHAR(pname) };
    if s.is_null() {
        return "???".to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(s) }
        .to_str()
        .unwrap_or("???")
        .to_string()
}

// ---------------------------------------------------------------------------
// Raw eval function — thin shim delegating to eval_safe
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This is the equivalent of R's `eval()` from src/main/eval.c.
/// It is the main dispatch function of the interpreter.
///
/// # Safety
///
/// `e` and `rho` must be valid SEXP pointers (or null).
#[must_use]
pub(crate) unsafe fn Rf_eval(e: SEXP, rho: SEXP) -> SEXP {
    match (Sexp::from_raw(e), Sexp::from_raw(rho)) {
        (Some(expr), Some(env)) => match eval_expr(expr, env) {
            Ok(result) => unsafe { super::jit::handle_exec_continuation(result.as_raw()) },
            Err(msg) => {
                std::panic::panic_any(crate::sexp::context::RSignal::Error { message: msg });
            }
        },
        _ => unsafe { R_NilValue() },
    }
}

fn is_simple_warning_hook_call(expr: Sexp<'_>) -> bool {
    let Ok(fun) = expr.try_car() else {
        return false;
    };
    if !fun.clone().is_symbol() {
        return false;
    }
    matches!(
        symbol_name_from_ptr(fun.as_raw()).as_deref(),
        Some(".signalSimpleWarning")
    )
}

/// Internal eval implementation (legacy, delegates to safe version).
pub(crate) unsafe fn eval_inner(e: SEXP, rho: SEXP) -> SEXP {
    unsafe { Rf_eval(e, rho) }
}

// ---------------------------------------------------------------------------
// eval_lang — evaluate a language/function call (legacy, delegates to safe)
// ---------------------------------------------------------------------------

/// Evaluate a LANGSXP (function call expression) — legacy wrapper.
unsafe fn eval_lang<'a>(e: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    let expr = unsafe { Sexp::from_raw_unchecked(e) };
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
    eval_lang_safe(expr, env)
}

/// Evaluate a SPECIAL function (arguments not evaluated) — legacy wrapper.
unsafe fn eval_special<'a>(e: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    let fun = unsafe { Sexp::from_raw_unchecked(op) };
    let call = unsafe { Sexp::from_raw_unchecked(e) };
    let arglist = unsafe { Sexp::from_raw_unchecked(args) };
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
    apply_special_safe(fun, call, arglist, env)
}

/// Evaluate a BUILTIN function (arguments evaluated first) — legacy wrapper.
unsafe fn eval_builtin<'a>(e: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    let fun = unsafe { Sexp::from_raw_unchecked(op) };
    let call = unsafe { Sexp::from_raw_unchecked(e) };
    let arglist = unsafe { Sexp::from_raw_unchecked(args) };
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
    apply_builtin_safe(fun, call, arglist, env)
}

/// Evaluate a CLOSXP (user-defined function) — legacy wrapper.
unsafe fn eval_closure<'a>(e: SEXP, op: SEXP, rho: SEXP) -> Result<Sexp<'a>, String> {
    let fun = unsafe { Sexp::from_raw_unchecked(op) };
    let call = unsafe { Sexp::from_raw_unchecked(e) };
    let args = unsafe { Sexp::from_raw_unchecked(e) }
        .try_cdr()
        .map_err(|err| sexp_err("missing args", err))?;
    let env = unsafe { Sexp::from_raw_unchecked(rho) };
    apply_closure_safe(fun, call, args, env)
}

// ---------------------------------------------------------------------------
// eval with visibility preservation (for C code calling eval)
// ---------------------------------------------------------------------------

/// Evaluate an expression, preserving the R_Visible flag.
///
/// This is the equivalent of R's `evalKeepVis()` from errors.c.
pub(crate) unsafe fn eval_keep_vis(e: SEXP, rho: SEXP) -> SEXP {
    let _visibility = super::runtime::VisibilityGuard::new();
    let val = unsafe { Rf_eval(e, rho) };
    val
}

// ---------------------------------------------------------------------------
// do_withVisible -- evaluate and return list(value, visible)
// ---------------------------------------------------------------------------

/// Evaluate expression and return `list(value = <result>, visible = <flag>)`.
///
/// Ported from R's `do_withVisible()` in eval.c.
/// This is a special `.Internal`.
pub(crate) unsafe fn do_withVisible(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::attrib_core::{R_NamesSymbol, setAttrib};
    use crate::sexp::accessors::{CAR, SET_STRING_ELT, SET_VECTOR_ELT};
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_mkChar};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::protect::protect;
    use std::os::raw::c_char;

    unsafe {
        let x = Rf_eval(CAR(args), rho);
        let _x_guard = protect(x);

        let ret = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _ret_guard = protect(ret);

        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        let _names_guard = protect(nm);

        SET_STRING_ELT(nm, 0, Rf_mkChar(b"value\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"visible\0".as_ptr() as *const c_char));

        SET_VECTOR_ELT(ret, 0, x);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarLogical(super::runtime::visible()));

        setAttrib(ret, R_NamesSymbol(), nm);

        ret
    }
}

// ---------------------------------------------------------------------------
// do_recall -- re-invoke the calling generic function
// ---------------------------------------------------------------------------

/// Implements R's `Recall()` — re-invokes the calling generic.
///
/// Ported from R's `do_recall()` in eval.c.
/// This is a special `.Internal`.
pub(crate) unsafe fn do_recall(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::closure::applyClosure;
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CAR, TYPEOF};
    use crate::sexp::context::ctxt_flags::CTXT_RETURN;
    use crate::sexp::envir::findFun;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::protect;
    use std::os::raw::c_char;

    unsafe {
        let top = super::runtime::global_context();
        let mut cptr = top;

        // Walk context stack to find the closure context for this environment
        while !cptr.is_null() {
            let ctx = &*cptr;
            if (ctx.callflag & CTXT_RETURN) != 0 && ctx.cloenv == rho {
                break;
            }
            cptr = ctx.nextcontext;
        }

        // Get the args from the context if found
        let recall_args = if !cptr.is_null() {
            (*cptr).promiseargs
        } else {
            args
        };

        // Get the sysparent (the env Recall was called from)
        if top.is_null() {
            Rf_error(b"'Recall' called from outside a closure\0".as_ptr() as *const c_char);
        }
        let s = (*top).sysparent;

        // Walk context stack again to find the closure context for sysparent
        let mut cptr2 = top;
        while !cptr2.is_null() {
            let ctx = &*cptr2;
            if (ctx.callflag & CTXT_RETURN) != 0 && ctx.cloenv == s {
                break;
            }
            cptr2 = ctx.nextcontext;
        }

        if cptr2.is_null() {
            Rf_error(b"'Recall' called from outside a closure\0".as_ptr() as *const c_char);
        }

        // Get the function from callfun, or look it up
        let fun = {
            let ctx = &*cptr2;
            if !ctx.callfun.is_null() && ctx.callfun != R_NilValue() {
                ctx.callfun
            } else if TYPEOF(CAR(ctx.call)) == SEXPTYPE::SYMSXP {
                findFun(CAR(ctx.call), ctx.sysparent)
            } else {
                Rf_eval(CAR(ctx.call), ctx.sysparent)
            }
        };

        let _fun_guard = protect(fun);

        if TYPEOF(fun) != SEXPTYPE::CLOSXP {
            Rf_error(b"'Recall' called from outside a closure\0".as_ptr() as *const c_char);
        }

        let ans = applyClosure(
            (*cptr2).call,
            fun,
            recall_args,
            (*cptr2).sysparent,
            R_NilValue(),
            1,
        );
        ans
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::builder::scalar_integer_in;
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_lang2};
    use crate::sexp::session::RSession;
    use crate::sexp::symbol::Rf_install;

    #[test]
    fn eval_context_evaluates_owner_scoped_expression() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|arena| {
                scalar_integer_in(arena, 123)
                    .expect("scalar allocation should succeed")
                    .as_raw()
            })
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");
        let env = session.global_env().expect("global env should exist");

        let result = EvalContext::new(env)
            .eval(expr)
            .expect("self-evaluating scalar should evaluate");

        assert_eq!(result.integer_elt(0), Some(123));
    }

    #[test]
    fn missing_formal_errors_when_forced_through_promise() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture("f <- function(x) x\nf()");
        let err = result.expect_err("forcing a missing formal should fail");
        assert_eq!(err.message, "argument \"x\" is missing, with no default");
    }

    #[test]
    fn primitive_descriptor_exposes_funtab_metadata() {
        let _session = RSession::new();
        let primitive = unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) };
        let descriptor =
            unsafe { PrimitiveDescriptor::from_raw(primitive) }.expect("primitive descriptor");

        assert_eq!(descriptor.name, "+");
        assert_eq!(descriptor.kind, BUILTINSXP);
        assert!(descriptor.table_index >= 0);
        assert_eq!(unsafe { PRIMNAME(primitive) }, "+");
        assert_eq!(unsafe { PRIMPRINT(primitive) }, descriptor.print_flag);
    }

    #[test]
    fn eval_classifier_names_core_evaluation_phases() {
        let _session = RSession::new();
        unsafe {
            let int_expr = Sexp::from_raw(Rf_ScalarInteger(1)).expect("integer scalar");
            assert_eq!(classify_expr(int_expr), EvalKind::SelfEvaluating);

            let symbol = Sexp::from_raw(Rf_install(c"x".as_ptr())).expect("symbol");
            assert_eq!(classify_expr(symbol), EvalKind::Symbol);

            let call = Sexp::from_raw(Rf_lang2(Rf_install(c"quote".as_ptr()), Rf_ScalarInteger(1)))
                .expect("language call");
            assert_eq!(classify_expr(call), EvalKind::Language);

            let expr_vec = Sexp::from_raw(crate::sexp::constructors::Rf_allocVector(
                SEXPTYPE::EXPRSXP,
                0,
            ))
            .expect("expression vector");
            // eval.c Rf_eval returns expression vectors unchanged; only
            // R-level eval() (do_eval) walks their elements.
            assert_eq!(classify_expr(expr_vec), EvalKind::SelfEvaluating);
        }
    }

    #[test]
    fn builtin_function_object_is_self_evaluating() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("f <- paste\nidentical(eval(f), f)");

        let result = result.expect("builtin function object should evaluate as a value");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn bytecode_disabled_errors_before_interpreting_payload() {
        let mut session = RSession::new();
        let raw_bcode = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::BCODESXP))
            .expect("session should be active");
        let bcode = session
            .sexp(raw_bcode)
            .expect("bytecode belongs to session");
        let env = session.global_env().expect("global env should exist");

        crate::sexp::instance::with_required_current_instance(|inst| unsafe {
            (*inst).eval_state.disable_bytecode = TRUE;
        });

        let err = EvalContext::new(env)
            .eval(bcode)
            .expect_err("disabled bytecode should not execute");
        assert!(err.contains("bytecode evaluation is disabled"));
    }

    #[test]
    fn eval_context_rejects_unowned_expression_handles() {
        let mut session = RSession::new();
        let raw = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let expr = Sexp::from_raw(raw).expect("legacy raw wrapper should construct");
        let env = session.global_env().expect("global env should exist");

        let err = EvalContext::new(env)
            .eval(expr)
            .expect_err("unowned expression should be rejected");
        assert!(err.contains("expression is not owner-scoped"));
    }

    #[test]
    fn ddval_first_dot_evaluates_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("f <- function(...) ..1; identical(f(10), 10)");
        let result = result.expect("..1 should read the first dots element");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn ddval_empty_dots_uses_gnu_message() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture("f <- function(...) ..1; f()");
        let err = result.expect_err("empty ... should error");
        assert!(
            err.message
                .contains("the ... list contains fewer than 1 element"),
            "{}",
            err.message
        );
    }

    #[test]
    fn ddval_second_dot_uses_gnu_plural_message() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("h <- function(...) ..2; h(1)");
        let err = result.expect_err("short ... should error");
        assert!(
            err.message
                .contains("the ... list contains fewer than 2 elements"),
            "{}",
            err.message
        );
    }

    #[test]
    fn ddval_incorrect_context_uses_gnu_message() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture("..1");
        let err = result.expect_err("top-level ..1 should error");
        assert!(
            err.message
                .contains("..1 used in an incorrect context, no ... to look in"),
            "{}",
            err.message
        );

        let (result, _, _) = session.eval_script_with_output_capture("g <- function() ..1; g()");
        let err = result.expect_err("..1 without ... should error");
        assert!(
            err.message
                .contains("..1 used in an incorrect context, no ... to look in"),
            "{}",
            err.message
        );
    }

    #[test]
    fn missing_ddval_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "m <- function(...) missing(..1); identical(c(m(), m(1)), c(TRUE, FALSE))",
        );
        let result = result.expect("missing(..1) should follow GNU Nth-cell rules");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn missing_stays_true_for_unsupplied_default() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "f <- function(x=1, slots) { force(slots); missing(x) }; identical(c(f(slots=0), f(2, slots=0), { y <- 0; g <- function(x=1) { y <- x; missing(x) }; g() }), c(TRUE, FALSE, TRUE))",
        );
        let result = result.expect("missing() must follow GNU default-formal rules");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn missing_is_false_after_assigning_formal() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "f <- function(x) { x <- 5; missing(x) }; identical(c(f(), f(1)), c(FALSE, FALSE))",
        );
        let result = result.expect("assigning a formal must clear missing()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_load_does_not_steal_lexical_enclosure() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "x <- 99; f <- function() x; invisible(require(methods, quietly=TRUE)); identical(f(), 99)",
        );
        let result = result.expect("user closures must keep lexical scope after methods loads");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn colon_builtin_evaluates_integer_range() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(typeof(`:`), \"special\") && identical(typeof(1L:3L), \"integer\") && identical(as.integer(1:3), c(1L, 2L, 3L))",
        );
        let result = result.expect("':' must be GNU's special colon");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn methods_new_classrepresentation_is_s4() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); isS4(x) && identical(as.character(class(x))[1], \"classRepresentation\")",
        );
        let result = result.expect("GNU new(classRepresentation) must return an S4 object");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_identc_compares_class_name_contents() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); isTRUE(methods:::.identC(\"classRepresentation\", \"classRepresentation\")) && !isTRUE(methods:::.identC(\"classRepresentation\", \"ClassUnionRepresentation\"))",
        );
        let result = result.expect(".identC must compare CHARSXP contents like GNU Seql");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_generic_closure_restores_s4_bit() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); g <- getGeneric(\"show\"); isS4(g) && identical(as.character(class(g))[1], \"standardGeneric\") && identical(as.character(g@generic)[1], \"show\")",
        );
        let result = result.expect("lazy-loaded generics must keep the GNU S4 bit");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_at_assign_stores_s4_slot() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); x@virtual <- TRUE; isTRUE(x@virtual)",
        );
        let result = result.expect("@<- must persist S4 slots like GNU installAttrib");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_slot_assign_via_call() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); slot(x, \"virtual\", FALSE) <- TRUE; isTRUE(x@virtual)",
        );
        let result = result.expect("slot<- must call GNU R_do_slot_assign");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn slot_assign_default_check_does_not_bind_value_as_check() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("SlotChk", slots = c(x = "numeric"))
o <- new("SlotChk")
slot(o, "x") <- 1:3
identical(as.numeric(o@x), c(1, 2, 3))
"#,
        );
        let result = result.expect("slot(obj, name) <- value must not bind value to check=");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn new_copies_slots_from_superclass_object() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("TrackN", slots = c(x = "numeric", y = "numeric"))
setClass("TrackCurveN", contains = "TrackN", slots = c(smooth = "numeric"))
t1 <- new("TrackN", x = 1:4, y = 5:8)
t2 <- new("TrackCurveN", t1, smooth = 9:12)
identical(as.numeric(t2@x), as.numeric(1:4)) &&
  identical(as.numeric(t2@y), as.numeric(5:8)) &&
  identical(as.numeric(t2@smooth), as.numeric(9:12))
"#,

        );
        let result = result.expect("new(Class, super, slot=) must copy superclass slots");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn as_assign_superclass_replace_does_not_crash() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("AsTrk", slots = c(x = "numeric", y = "numeric"))
setClass("AsCurve", contains = "AsTrk", slots = c(smooth = "numeric"))
t1 <- new("AsTrk", x = 1:3, y = 4:6)
o <- new("AsCurve")
as(o, "AsTrk") <- t1
identical(as.numeric(o@x), as.numeric(1:3)) &&
  identical(as.numeric(o@y), as.numeric(4:6))
"#,
        );
        let result = result.expect("as<- superclass replace must not crash");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn register_s3method_is_invisible() {
        let mut session = RSession::new();
        let (result, stdout, _) = session.eval_script_with_output_capture(
            r#"
registerS3method("print", "RegInv", function(x) invisible(x))
identical(withVisible(registerS3method("print", "RegInv2", function(x) x))$visible, FALSE)
"#,
        );
        let result = result.expect("registerS3method must be invisible");
        assert!(
            !stdout.stdout.contains("NULL"),
            "registerS3method must not auto-print NULL, got {:?}",
            stdout.stdout
        );
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }




    #[test]
    fn methods_externalptr_typeof_and_class() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); p <- methods:::.newExternalptr(); identical(typeof(p), \"externalptr\") && identical(class(p), \"externalptr\") && is(p, \"externalptr\")",
        );
        let result = result.expect("externalptr must have GNU typeof/class");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn invisible_is_gnu_builtin() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(typeof(invisible), \"builtin\") && identical(invisible(1L), 1L)",
        );
        let result = result.expect("invisible must be GNU's evaluated builtin");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_named_mean_leaves_positional_for_sd() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "set.seed(1); a <- rnorm(1, 0, mean = 10); set.seed(1); b <- rnorm(1, mean = 10, sd = 0); identical(a, b) && identical(a, 10)",
        );
        let result = result.expect("rnorm must match GNU exact-then-positional formals");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_partial_names_and_docall_match_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "set.seed(1); a <- rnorm(1, m=10, s=0); set.seed(1); b <- do.call(rnorm, list(1, mean=10, sd=0)); identical(a, b) && identical(a, 10)",
        );
        let result = result.expect("rnorm partial names and do.call must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_unused_argument_errors_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "inherits(tryCatch(rnorm(1, foo=1), error=function(e) e), \"error\")",
        );
        let result = result.expect("unused rnorm argument must error");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn language_implicit_class_follows_gnu_lang2str() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(class(quote((x))), "(") &&
  identical(class(quote({1})), "{") &&
  identical(class(quote(if (TRUE) 1)), "if") &&
  identical(class(quote(sin(x))), "call") &&
  identical(class(expression((x))), "expression")
"#,
        );
        let result = result.expect("class() of language objects must use GNU lang2str");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn usemethod_dispatches_on_paren_and_expression() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
abc <- function(x, ...) UseMethod("abc", x)
abc.default <- function(x, ...) "default"
"abc.(" <- function(x) "paren"
abc.expression <- function(x) "expr"
identical(abc(expression((x))), "expr") &&
  identical(abc(quote((x))), "paren") &&
  identical(abc(quote(sin(x))), "default")
"#,
        );
        let result = result.expect("UseMethod must see GNU implicit language classes");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn mode_and_str_of_paren_language_follow_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(mode(quote((x))), "(") &&
  identical(mode(quote(sin(x))), "call") &&
  identical(mode(quote({1})), "call") &&
  identical(mode(quote(if (TRUE) 1)), "call") &&
  identical(class(formals(function(a = 1) NULL)), "pairlist") &&
  identical(mode(formals(function(a = 1) NULL)), "pairlist") &&
  identical(class(new.env()), "environment")
"#,


        );
        let result = result.expect("mode() of calls is ( vs call; class uses type2str");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture("str(quote((x)))\n");
        assert!(
            captured.stdout.contains(r#"language, mode "(": (x)"#),
            "str() of paren language must include GNU mode suffix, got {:?}",
            captured.stdout
        );
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture("str(quote(if (TRUE) 1))\n");
        assert!(
            captured.stdout.contains(" language if (TRUE) 1"),
            "str() of if-call must not add a mode suffix, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains(r#"mode "if""#),
            "str() must not treat lang2str heads as mode(), got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn match_closure_args_follows_gnu_three_pass() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- function(abc, abd, ...) list(abc, abd, list(...))
identical(f(abc = 1, abd = 2, extra = 3), list(1, 2, list(extra = 3))) &&
  identical(f(1, 2, 3), list(1, 2, list(3))) &&
  identical(f(abd = 2, abc = 1), list(1, 2, list())) &&
  identical(f(abc = 1, ab = 2), list(1, 2, list()))
"#,
        );
        let result = result.expect("closure matching must follow GNU matchArgs");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn formatc_default_width_is_digits_plus_one() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"identical(formatC(2^30, digits = 12), "   1073741824")"#,
        );
        let result = result.expect("formatC default width is digits+1");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_digits_argument_is_honored() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(c(2.44140624e-04, 8), digits = 1)\n",
        );
        assert!(
            captured.stdout.contains("[1] 2e-04 8e+00")
                || captured.stdout.contains("[1] 0.0002 8"),
            "print(..., digits=1) must honor digits, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn cat_null_still_emits_sep() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"cat(NULL, "x"); cat("\n"); cat(if (FALSE) "\n", formatC(1, width = 2), ":", "\n")"#,
        );
        assert!(
            captured.stdout.contains(" x\n") && captured.stdout.contains("  1 :"),
            "cat(NULL, ...) must keep sep, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn format_info_honors_digits_argument() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
x2 <- c(0.099999994, 0.2)
v <- 6:8
names(v) <- v
m <- sapply(v, format.info, x = x2)
identical(as.vector(m), c(3L, 1L, 0L, 10L, 8L, 0L, 11L, 9L, 0L))
"#,
        );
        let result = result.expect("format.info digits must follow GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn namesgets_coerces_integer_to_character() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
v <- 6:8
names(v) <- v
identical(names(v), c("6", "7", "8")) &&
  identical(dimnames(sapply(v, format.info, x = c(0.099999994, 0.2)))[[2]], c("6", "7", "8"))
"#,
        );
        let result = result.expect("names<- must store character names");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn signif_recycles_digits_like_gnu_math2() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
z <- c(2.002566e-308, 2.447581e-308)
m <- outer(z, 0:3, signif)
identical(format(m[, 1], digits = 1), format(m[, 2], digits = 1)) &&
  !identical(format(m[, 2], scientific = TRUE), format(m[, 4], scientific = TRUE))
"#,
        );
        let result = result.expect("signif must recycle a digits vector");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn cat_uses_scientific_for_large_whole_doubles() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "cat(signif(1.234567891234567e27, 1), \"\\n\")\n",
        );
        assert!(
            captured.stdout.contains("1e+27"),
            "cat of signif(1e27, 1) must be scientific, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("10000000000000000"),
            "cat must not dump the full integer mantissa, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn signif_empty_operands_follow_gnu_math2() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(signif(numeric(0), 3), numeric(0)) &&
  identical(signif(numeric(0)), numeric(0)) &&
  inherits(tryCatch(signif(1:3, numeric(0)), error = identity), "error")
"#,
        );
        let result = result.expect("signif empty operands must follow GNU math2");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn cat_honors_live_digits_for_scientific() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "options(digits=8); cat(signif(1.234567891234567e27, 8), \"\\n\")\n",
        );
        assert!(
            captured.stdout.contains("1.2345679e+27"),
            "cat at digits=8 must keep 8 sig digits, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_character_matrix_and_noquote_follow_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
m1 <- matrix(letters[1:24], 6, 4)
m1
noquote(m1)
m1
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("[,1]") && captured.stdout.contains("[1,]"),
            "character matrix must print as a matrix, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("\"a\""),
            "default character matrix print must quote cells, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains(" a    ") || captured.stdout.contains("[1,] a"),
            "noquote character matrix must drop quotes, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("[1] \"a\" \"b\" \"c\""),
            "character matrix must not flatten to a quoted vector, got {:?}",
            captured.stdout
        );
        let quoted_blocks = captured.stdout.matches("\"a\"").count();
        assert!(
            quoted_blocks >= 2,
            "noquote must not mutate m1; later print must still quote, got {:?}",
            captured.stdout
        );

    }

    #[test]
    fn format_data_frame_prints_like_gnu() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
zz <- data.frame("(row names)" = c("aaaaa", "b"), check.names = FALSE)
format(zz)
invisible(NULL)
"#,
        );
        let _ = result;
        assert!(
            captured.stdout.contains("1       aaaaa") && captured.stdout.contains("2           b"),
            "format(data.frame) must use data.frame layout, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("$") && !captured.stdout.contains("[1] \"19\""),
            "format(data.frame) must not list-print, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn expand_model_frame_honors_subset_and_na_expand() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
set.seed(321)
dd <- data.frame(x = 1:5, y = rnorm(5), z = c(1, 2, NA, 4, 5))
model <- glm(y ~ x, data = dd, subset = 1:4, na.action = na.omit)
a <- expand.model.frame(model, "z", na.expand = FALSE)
b <- expand.model.frame(model, "z", na.expand = TRUE)
is.data.frame(a) && is.data.frame(b) &&
  identical(names(a), c("y","x","z")) &&
  identical(row.names(a), c("1","2","4")) &&
  identical(a$z, c(1,2,4)) &&
  identical(row.names(b), c("1","2","3","4")) &&
  identical(b$z, c(1,2,NA,4))
"#,
        );
        let result = result.expect("expand.model.frame must honor subset and na.expand");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }














    #[test]
    fn try_catch_finally_runs_after_body() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "x <- character(); r <- tryCatch({ x <- c(x, \"b\"); 1L }, finally = { x <- c(x, \"f\") }); identical(r, 1L) && identical(x, c(\"b\", \"f\"))",
        );
        let result = result.expect("tryCatch finally must run after the body");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn deparse_pi_uses_dbl_dig_not_options_digits() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(deparse(pi), \"3.14159265358979\")",
        );
        let result = result.expect("deparse must pin R_print.digits to DBL_DIG");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_echo_deparses_expression_wrapper_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines('dPut <- function(x, control = c("quoteExpression", "showAttributes", "niceNames", "keepInteger")) dput(x, control = control)', f)
out <- capture.output(source(f, echo = TRUE))
identical(out[grepl("dPut <-", out)][1], '> dPut <- function(x, control = c("quoteExpression", ')
"#,
        );
        let result = result.expect("source(echo=TRUE) must wrap like GNU expression-deparse");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_echo_truncates_at_max_deparse_length() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("hasReal <- function(x) { if (is.double(x) || is.complex(x)) !all((x == round(x, 3)) | is.na(x)) else if (is.logical(x) || is.integer(x) || is.symbol(x) || is.call(x) || is.environment(x) || is.character(x)) FALSE else FALSE }", f)
out <- paste(capture.output(source(f, echo = TRUE)), collapse = "\n")
grepl(" .... [TRUNCATED] ", out, fixed = TRUE)
"#,
        );
        let result = result.expect("source(echo=TRUE) must honor max.deparse.length=150");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_factor_pads_to_widest_label_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"identical(capture.output(print(factor(c("a", NA, "b"), exclude=""))), c("[1] a    <NA> b   ", "Levels: a b <NA>"))"#,
        );
        let result = result.expect("print.factor must pad to <NA> width");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_table_1d_keeps_gnu_trailing_column_space() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
fx <- factor(c("a", NA, "b"), exclude="")
r <- replicate(3, capture.output(print(fx)))
identical(capture.output(print(table(r[2,]))), c("", "Levels: a b <NA> ", "               3 "))
"#,
        );
        let result = result.expect("print.table must emit GNU's trailing column space");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_max_deparse_length_inf_does_not_warn() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("1+1", f)
invisible(source(f, echo = TRUE, max.deparse.length = Inf))
TRUE
"#,
        );
        let result = result.expect("source(max.deparse.length=Inf) must be legal");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !output.stderr.contains("integer range") && !output.stdout.contains("integer range"),
            "Inf must not coerce through asInteger: stdout={:?} stderr={:?}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn s4_list_class_new_keeps_unnamed_data_part() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("mp1Port", slots = c(prec = "integer", d = "integer"))
setClass("mpPort", contains = "list")
m <- new("mpPort", list(new("mp1Port"), new("mp1Port", prec=1L, d=3:5)))
identical(length(m), 2L) && identical(typeof(m), "list")
"#,
        );
        let result = result.expect("new(list-class, list(...)) must keep .Data");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn complex_assign_does_not_leave_tmp_binding() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e2 <- quote(c(a = 1, b = 2))
names(e2)[2] <- "a b c"
!exists("*tmp*", inherits = FALSE)
"#,
        );
        let result = result.expect("applydefine must unbind *tmp*");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn failed_subassign_does_not_leave_tmp_binding() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
test <- 1:10
tryCatch(test[2:4] <- ls, error = function(e) NULL)
!exists("*tmp*", inherits = FALSE)
"#,

        );
        let result = result.expect("failed [<- must not leave *tmp*");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_returns_withvisible_list_invisibly() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("1+1", f)
r <- source(f, echo = FALSE)
identical(r, list(value = 2, visible = TRUE)) &&
  !withVisible(source(f, echo = FALSE))$visible
"#,
        );
        let result = result.expect("source() must return invisible(list(value, visible))");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn sys_source_returns_invisible_null() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("1+1", f)
e <- new.env()
r <- sys.source(f, envir = e)
is.null(r) && !withVisible(sys.source(f, envir = e))$visible
"#,
        );
        let result = result.expect("sys.source() must return invisible NULL");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn identical_ignores_function_env_and_srcref_by_default_flags() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
A <- function(x, y, ...) {
    B <- function(a, b, ...) { match.call() }
    B(x+y, ...)
}
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
id_epd <- function(expr, control = "all", ...) eval(pd0(expr, control=control, ...))
identical(A, id_epd(A), ignore.environment = TRUE, ignore.bytecode = TRUE, ignore.srcref = TRUE)
            "#,
        );
        let result = result.expect("check_EPD function identical must ignore env/srcref");
        assert_eq!(result.logical_elt(0), Some(TRUE));

    }

    #[test]
    fn all_equal_formula_ignores_environment() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
fm <- y ~ f(x)
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
id_epd <- function(expr, control = "all", ...) eval(pd0(expr, control=control, ...))
isTRUE(all.equal(fm, id_epd(fm), check.environment = FALSE))
"#,
        );
        let result = result.expect("all.equal.formula must ignore .Environment");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn deparse_all_preserves_na_list_names() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
LNA <- setNames(as.list(c(1,2,99)), c("A", "NA", NA))
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
identical(LNA, eval(pd0(LNA, control = "all")))
"#,
        );
        let result = result.expect("deparse(control=all) must keep NA list names");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn dput_quote_expression_wraps_language() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
dPut <- function(x, control = c("quoteExpression", "showAttributes", "niceNames", "keepInteger"))
    dput(x, control = control)
A <- function(x) { x }
identical(paste(capture.output(dPut(body(A))), collapse = "\n"), "quote({\n    x\n})")
"#,
        );
        let result = result.expect("dput(control=quoteExpression) must wrap language in quote()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn deparse_all_warns_when_sourceable_is_false() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
w <- character()
withCallingHandlers(
    deparse(y ~ x, control = "all"),
    warning = function(e) {
        w <<- conditionMessage(e)
        invokeRestart("muffleWarning")
    }
)
identical(w, "deparse may be incomplete")
"#,
        );
        let result = result.expect("deparse(control=all) on a formula must warn incomplete");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn summary_warnings_collapses_identical_deparse_warnings() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
options(warn = 0)
f <- function(x) deparse(x, control = "all")
invisible({
    f(y ~ x)
    f(a ~ b)
})
summary(warnings())
identical(class(summary(warnings())), "summary.warnings")
"#,
        );
        let result = result.expect("summary(warnings()) must run");
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "class(summary(warnings())) must be summary.warnings, output={output:?}"
        );
        assert!(
            output.stdout.contains("2 identical warnings:"),
            "expected collapsed identical-warnings header, got {output:?}"
        );
        assert!(
            output.stdout.contains("deparse may be incomplete"),
            "expected incomplete-deparse text, got {output:?}"
        );
        assert!(
            !output.stdout.contains("1x :"),
            "identical warnings must not print per-item 1x tags, got {output:?}"
        );
    }

    #[test]

    fn unlist_preserves_na_names_for_dput() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
LNA <- setNames(as.list(c(1, 2, 99)), c("A", "NA", NA))
iNA <- unlist(LNA)
identical(names(iNA), c("A", "NA", NA)) &&
  identical(paste(capture.output(dput(iNA)), collapse = "\n"),
            "structure(c(1, 2, 99), names = c(\"A\", \"NA\", NA))")
"#,
        );
        let result = result.expect("unlist must keep NA names so dput uses structure()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }





    #[test]
    fn all_equal_s4_formula_subclass_uses_language_path() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
mForm <- setClass("mFormAe", contains = "formula")
mf <- mForm(~ f(x))
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
id_epd <- function(expr, control = "all", ...) eval(pd0(expr, control=control, ...))
isTRUE(all.equal(mf, id_epd(mf), check.environment = FALSE))
"#,
        );
        let result = result.expect("S4 formula subclass all.equal must follow language/deparse");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }





    #[test]
    fn s4_class_representations_compare_slots() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("IdentA", contains = "formula")
setClass("IdentB", contains = "list")
!identical(getClass("IdentA"), getClass("IdentB"))
"#,
        );
        let result = result.expect("distinct S4 class defs must not be identical");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn formula_s4_subclass_uses_language_typeof() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
mForm <- setClass("mFormIdent", contains = "formula")
extends("mFormIdent", "oldClass") &&
  isS4(mf <- mForm(~ f(x))) &&
  identical(typeof(mf), "language") &&
  identical(mf, eval(parse(text = deparse(mf))))
"#,
        );
        let result = result.expect("S4 formula subclass must deparse/parse like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }





    #[test]
    fn s4_list_dput_includes_data_part() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("mp1Dput", slots = c(prec = "integer", d = "integer"))
setClass("mpDput", contains = "list")
m <- new("mpDput", list(new("mp1Dput", prec=1L, d=3:5)))
out <- paste(capture.output(dput(m)), collapse = "\n")
grepl(".Data", out, fixed = TRUE) && grepl("prec = 1L", out, fixed = TRUE)
"#,
        );
        let result = result.expect("dput of a list-class S4 object must emit .Data");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }










    #[test]
    fn capture_output_writes_local_text_connection() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(utils::capture.output(cat(\"hi\\n\")), \"hi\")",
        );
        let result = result.expect("capture.output must assign the local textConnection");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn with_autoprint_capture_output_splits_gnu_lines() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
CO <- utils::capture.output
out <- CO(withAutoprint({ x <- 1:2; cat("x=", x, "\n") }))
identical(out[1], paste0(getOption("prompt"), "x <- 1:2")) &&
  length(out) >= 3L &&
  identical(out[3], "x= 1 2 ")
"#,
        );
        let result = result.expect("withAutoprint capture.output must emit GNU lines");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn with_autoprint_is_gnu_source_wrapper() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
is.function(withAutoprint) &&
  identical(names(formals(withAutoprint))[1:3], c("exprs", "evaluated", "local")) &&
  grepl("source(", paste(deparse(body(withAutoprint)), collapse = "\n"), fixed = TRUE)
"#,
        );
        let result = result.expect("withAutoprint must be GNU's source() wrapper");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }



    #[test]
    fn unlist_recursive_false_keeps_list_of_lists() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
u <- unlist(list(list(.Data = "list"), list()), recursive = FALSE)
identical(typeof(u), "list") && identical(names(u), ".Data") && identical(u$.Data, "list")
"#,
        );
        let result = result.expect("unlist(recursive=FALSE) must concatenate lists like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn set_class_contains_list_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly = TRUE))
setClass("mListPort", contains = "list")
s <- getClass("mListPort")@slots
identical(typeof(s), "list") && identical(names(s), ".Data") && identical(as.character(s$.Data), "list")
"#,
        );
        let result = result.expect("setClass(contains='list') must use GNU methods::setClass");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn unlist_recursive_false_splices_mixed_atomic_and_list() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
u <- unlist(list(1:2, list(3)), recursive = FALSE)
identical(typeof(u), "list") && identical(as.numeric(unlist(u)), c(1, 2, 3))
"#,
        );
        let result = result.expect("unlist(recursive=FALSE) must splice atomics like GNU c()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn example_uses_unevaluated_topic_name() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly = TRUE))
w <- tryCatch(example(new), warning = function(e) conditionMessage(e), error = function(e) conditionMessage(e))
msg <- paste(w, collapse = " ")
!grepl("topic '3'", msg, fixed = TRUE) &&
  (grepl("'new'", msg, fixed = TRUE) || grepl("lazyLoadDBexec", msg, fixed = TRUE))
"#,

        );
        let result = result.expect("example(new) must look up topic new, not evaluate new");
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "output={output:?}"
        );
    }


    #[test]
    fn find_package_null_lists_attached_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly = TRUE))
any(grepl("/methods$", find.package(NULL)))
"#,
        );
        let result = result.expect("find.package(NULL) must list attached package paths");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }




    #[test]
    fn utils_namespace_loads_without_windows_s3_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "is.environment(asNamespace(\"utils\")) && is.function(utils::getAnywhere)",
        );
        let result = result.expect("utils namespace must load with lazy S3 methods");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }



    #[test]
    fn rep_is_gnu_special() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(typeof(rep), \"special\") && identical(rep(1L, 3L), c(1L, 1L, 1L))",
        );
        let result = result.expect("rep must be GNU's special");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_definition_data_slot_is_the_closure() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"MethodDefinition\"); isS4(x) && typeof(x) == \"closure\" && is.function(x@.Data) && isTRUE(validObject(x))",
        );
        let result = result.expect("MethodDefinition@.Data is the closure like GNU getDataPart");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_setmethod_dispatches_on_s4_class() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"foo\", representation(x=\"numeric\", y=\"numeric\")); xx <- new(\"foo\", x=1, y=2); ff <- args(getGeneric(\"$\")); body(ff) <- \"testit\"; setMethod(\"$\", \"foo\", ff); identical(getGeneric(\"$\")(xx), \"testit\") && identical(xx$x, \"testit\")",
        );
        let result = result.expect("setMethod + $ dispatch must match GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_match_signature_for_subset_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); s <- matchSignature(\"foo\", getGeneric(\"[\")); identical(as.character(s), \"foo\") && identical(names(s), \"x\")",
        );
        let result = result.expect("matchSignature([) must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_setmethod_subset_generic_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"foo\", representation(x=\"numeric\", y=\"numeric\")); xx <- new(\"foo\", x=1, y=2); ff <- args(getGeneric(\"[\")); body(ff) <- \"testit\"; setMethod(\"[\", \"foo\", ff); identical(getGeneric(\"[\")(xx), \"testit\") && identical(xx[], \"testit\")",
        );
        let result = result.expect("setMethod([) must match GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_setmethod_double_bracket_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"foo\", representation(x=\"numeric\", y=\"numeric\")); xx <- new(\"foo\", x=1, y=2); ff <- args(getGeneric(\"[[\")); body(ff) <- \"testit\"; setMethod(\"[[\", \"foo\", ff); identical(getGeneric(\"[[\")(xx), \"testit\") && identical(xx[[\"x\"]], \"testit\")",
        );
        let result = result.expect("setMethod([[) must match GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }



    #[test]
    fn methods_setclass_defines_s4_class() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"C_new_object\", slots=c(x=\"numeric\")); x <- new(\"C_new_object\"); isS4(x) && identical(as.character(class(x))[1], \"C_new_object\")",
        );
        let result = result.expect("setClass + new must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_null_slot_roundtrips_gnu_sentinel() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); x@validity <- NULL; is.null(x@validity) && isTRUE(methods:::.hasSlot(x, \"validity\"))",
        );
        let result = result.expect("NULL slots must store GNU pseudo_NULL and still count as present");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn language_double_bracket_and_dollar_assign() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "fdef <- quote(function(from, to = TO, strict = TRUE) NULL); fdef[[2L]]$to <- \"C_new_object\"; identical(as.character(fdef[[2L]]$to), \"C_new_object\")",
        );
        let result = result.expect("[[ and $<- must work on language/pairlist like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_new_accepts_named_slots() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"C_new_object\", slots=c(x=\"numeric\")); x <- new(\"C_new_object\", x=4:6); isS4(x) && identical(as.numeric(x@x), c(4,5,6))",
        );
        let result = result.expect("new(Class, slot=) must match GNU initialize");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn as_function_defaults_envir_and_returns_functions() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "f <- function(z) z + 1L; identical(as.function(f), f) && identical(as.function(alist(x=, x + 1L))(2L), 3L)",
        );
        let result = result.expect("as.function must default envir like GNU parent.frame()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }



    #[test]
    fn class_of_null_is_null_string() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(class(NULL), \"NULL\")",
        );
        let result = result.expect("class(NULL) must be GNU's implicit NULL class");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setdiff_null_has_gnu_length_zero() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(setdiff(NULL, \"x\"), NULL) && length(setdiff(NULL, \"x\")) == 0L && is.null(setdiff(NULL, \"x\"))",
        );
        let result = result.expect("setdiff(NULL, *) must be NULL with length 0");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }







}
