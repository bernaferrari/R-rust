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
        eval_safe(fun.clone(), rho.clone())?
    };

    match fun_val.clone().typeof_() {
        SEXPTYPE::CLOSXP => apply_closure_safe(fun_val, e, args, rho),
        SEXPTYPE::SPECIALSXP => apply_special_safe(fun_val, e, args, rho),
        SEXPTYPE::BUILTINSXP => apply_builtin_safe(fun_val, e, args, rho),
        kind => {
            let head = if fun.clone().typeof_() == SEXPTYPE::SYMSXP {
                unsafe { get_symbol_name(fun.as_raw()) }
            } else {
                format!("{:?}", fun.clone().typeof_())
            };
            Err(format!("cannot call type {kind:?} for {head}"))
        }
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
    fn rnorm_named_mean_after_stats_load_matches_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(stats, quietly=TRUE))
set.seed(1); a <- rnorm(1, 0, mean = 10)
set.seed(1); b <- rnorm(1, mean = 10, sd = 0)
identical(typeof(rnorm), "closure") && identical(a, b) && isTRUE(all.equal(a, 10))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "stats rnorm matching: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
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
    fn summary_data_frame_returns_gnu_table() {

        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(

            r#"
dd <- data.frame(event = c(1, 9, 18, 14.74, 20, 23),
                 station = factor(c("117","1028","113","117","135","117")))
s <- summary(dd)
inherits(s, "table") && is.matrix(s) && typeof(s) == "character" &&
  grepl("Min.", s[1,1], fixed=TRUE)
"#,
        );

        let result = result.expect("summary.data.frame must return a table");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !captured.stdout.contains("[1] \"list\""),
            "summary(data.frame) must not fall through to typeof, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_language_objects_keep_class_and_pairlist_cells() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
obj <- structure(quote(stop("should not be evaluated")), class = "foo")
list(obj)
pairlist(obj)
structure(list(), attr = obj)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("stop(\"should not be evaluated\")")
                && captured.stdout.contains("attr(,\"class\")")
                && captured.stdout.contains("[1] \"foo\""),
            "language objects must print deparsed call plus class, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("[pairlist; length=0]"),
            "pairlist(obj) must print cells, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("attr(,\"attr\")"),
            "list attributes that are language objects must print, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_foo_dispatches_inside_lists_and_attributes() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
obj <- structure(quote(stop("should not be evaluated")), class = "foo")
print.foo <- function(x, ...) cat("dispatched\n")
list(obj)
structure(list(), attr = obj)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("dispatched"),
            "print.foo must run for classed list children, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("stop(\"should not be evaluated\")"),
            "default language print must not run after print.foo, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn recursive_print_prefers_s4_show_over_print() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
print.callS4Class <- function(x, ...) stop("should not be dispatched")
.CallS4Class <- setClass("callS4Class", slots = c(x = "numeric"))
setMethod("show", "callS4Class", function(object) cat("S4 show!\n"))
x <- .CallS4Class(x = 1)
list(x)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("S4 show!"),
            "recursive print must call show() for S4 objects, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("should not be dispatched"),
            "print.callS4Class must not run, got {:?}",
            captured.stdout
        );
    }


    #[test]
    fn recursive_print_forwards_user_print_arguments() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
obj <- structure(quote(stop("should not be evaluated")), class = "foo")
print.foo <- function(x, other = FALSE, digits = 0L, ...) {
    cat("digits: ", digits, "\n")
    stopifnot(other, digits == 4, !...length())
}
print(list(obj), digits = 4, other = TRUE)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("digits:  4"),
            "recursive print must forward digits/other, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_primitive_includes_argsenv_formals() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(base::list)\ninvisible(NULL)\n",
        );
        assert_eq!(
            captured.stdout.trim_end(),
            "function (...)  .Primitive(\"list\")",
            "PrintSpecial must wrap ArgsEnv formals, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn dots_length_treats_empty_dots_as_zero() {

        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "h <- function(...) ...length(); identical(h(), 0L)",
        );
        let result = result.expect("empty ... must have length 0");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_matrix_honors_max_argument() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(matrix(nrow = 100, ncol = 4), max = 5)\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("omitted 99 rows"),
            "matrix print must honour max=, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("[2,]"),
            "truncated matrix must not print a second row, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_array_uses_gnu_slice_headers() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(array(dim = c(2, 2, 2)), max = 4)\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains(", , 1"),
            "3-D arrays must print GNU slice headers, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("omitted 1 slice"),
            "array print must honour max=, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn summary_true_prints_like_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "summary(TRUE)\ninvisible(NULL)\n",
        );
        assert_eq!(
            captured.stdout,
            "   Mode    TRUE \nlogical       1 \n",
            "summary(TRUE) must match GNU print.summaryDefault, got {:?}",
            captured.stdout
        );

    }

    #[test]
    fn summary_pi_prints_gnu_digits() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "options(digits=7); summary(pi)\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("3.142"),
            "named numeric summaryDefault must use digits=max(3,digits-3), got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("3.1 ") && !captured.stdout.contains("3.141593"),
            "must not use 1 decimal or full digits, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn str_ts_uses_digits_d_for_range_and_preview() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
z <- ts(c(112,118,132,129,121,135,148,148,136,119, rep(120, 134)), frequency=12, start=c(1949,1))
str(z)
y <- ts(c(200.1, 199.5, 199.4, 198.9, 199, 200.2, 198.6, 200, 200.3, 201.2))
str(y)
w <- ts(c(10.01, 10.07, 10.32, 9.75, 10.33, 10.13, 10.36, 10.32, 10.13, 10.16))
str(w)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("from 1949 to 1961:"),
            "str.ts must format tsp with digits.d=3, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("112 118 132 129 121 135 148 148 136 119 ..."),
            "integer-like ts preview is vec.len*2.5, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("200 200 199 199 199 ...")
                && !captured.stdout.contains("200.1"),
            "non-integer-like ts preview uses digits.d and 1.25*vec.len, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("10.01 10.07 10.32 9.75 10.33 ..."),
            "preview slice must share format() decimals, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_noquote_empty_character_omits_class() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(noquote(character(0)))\ninvisible(NULL)\n",
        );
        assert_eq!(
            captured.stdout.trim_end(),
            "character(0)",
            "GNU print.noquote strips class before print, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn rnorm_named_mean_after_positional_sd_matches_gnu() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
set.seed(1); a <- rnorm(1, 0, mean = 10)
set.seed(1); b <- rnorm(1, mean = 10, sd = 0)
set.seed(1); c <- rnorm(sd = 0, n = 1, mean = 10)
dup <- tryCatch(rnorm(1, mean = 1, mean = 2), error = function(e) e)
unk <- tryCatch(rnorm(1, foo = 2), error = function(e) e)
identical(a, b) && identical(a, c) && isTRUE(all.equal(a, 10)) &&
  grepl("formal argument \"mean\" matched by multiple actual arguments", conditionMessage(dup), fixed = TRUE) &&
  grepl("unused argument (foo = 2)", conditionMessage(unk), fixed = TRUE)
"#,
        );
        assert_eq!(
            result.expect("rnorm matching should evaluate").logical_elt(0),
            Some(TRUE),
            "stdout={:?} stderr={:?}",
            captured.stdout,
            captured.stderr
        );
    }

    #[test]
    fn str_named_character_quotes_like_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "str(c(F=0.3, `Tail area`=60))\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("chr [1:2] \"F\" \"Tail area\""),
            "str() of character names must quote, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn only_internal_generics_dispatch_s3_on_classed_args() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
x <- structure(pi, class="testit")
retracemem.testit <- function(x, previous=NULL) 42
res <- try(retracemem(x), silent=TRUE)
length.testit <- function(x) 99
stopifnot(inherits(res, "try-error") || !identical(res, 42))
stopifnot(identical(length(x), 99))
stopifnot(identical(typeof(`body<-`), "closure"))
stopifnot(identical(typeof(`formals<-`), "closure"))
TRUE
"#,
        );
        let result = result.expect("internal-generic S3 gate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn str_prints_nan_and_na_like_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "str(c(NaN, 1))\nstr(c(NA_real_, 1))\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("num [1:2] NaN 1")
                && captured.stdout.contains("num [1:2] NA 1"),
            "str() must distinguish NaN from NA, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn alist_keeps_missing_formals_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
a <- alist(x=, y=2)
stopifnot(identical(names(a), c("x","y")))
stopifnot(identical(a$y, 2))
g <- function(x) x+1
formals(g) <- a
stopifnot(identical(names(formals(g)), c("x","y")))
TRUE
"#,
        );
        let result = result.expect("alist");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }




    #[test]
    fn str_data_frame_aligns_names_and_omits_column_length() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
df <- data.frame(Time=c(1,2,3,4,5,7), demand=c(8.3,10.3,19,16,15.6,19.8))
attr(df, "reference") <- "A1.4, p. 270"
str(df)
f <- ordered(c("Qn1","Qn1","Qn2"), levels=c("Qn1","Qn2","Qn3"))
g <- factor(c("Qn1","Qn1","Qn2"), levels=c("Qn1","Qn2","Qn3"), ordered=TRUE)
str(f)
str(g)
fm <- uptake ~ conc | Plant
attr(fm, ".Environment") <- emptyenv()
str(fm)
labs <- list(x="Ambient carbon dioxide concentration", y="CO2 uptake rate")
attr(df, "labels") <- labs
str(df)
m <- matrix(1:4, 2, 2, dimnames=list(c("a","b"), c("c","d")))
lst <- list(cov=m, center=c(0,0))
str(lst)
d <- as.Date(c("2007-11-11", NA))
str(d)
str(c(1.5, NA_real_))
str(c("MALE", NA_character_))



invisible(NULL)

"#,
        );
        assert!(
            captured.stdout.contains("$ Time  : num  1 2 3 4 5 7")
                && captured.stdout.contains("$ demand: num  8.3 10.3 19 16 15.6 19.8")
                && captured.stdout.contains("- attr(*, \"reference\")= chr \"A1.4, p. 270\""),
            "data.frame str must align names, omit [1:n], print extra attrs, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.matches("Ord.factor w/ 3 levels \"Qn1\"<\"Qn2\"<\"Qn3\": 1 1 2").count()
                >= 2,
            "both ordered() and factor(ordered=TRUE) must be Ord.factor, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("Class 'formula'  language uptake ~ conc | Plant")
                && captured.stdout.contains(".Environment")
                && captured.stdout.contains("R_EmptyEnv"),
            "formula str must match GNU Class/language header, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("- attr(*, \"labels\")=List of 2")
                && captured.stdout.contains("$ x: chr \"Ambient carbon dioxide concentration\"")
                && captured.stdout.contains("$ y: chr \"CO2 uptake rate\""),
            "named list attrs must be List of N with $ children, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("$ cov   : int [1:2, 1:2]")
                && captured.stdout.contains("  ..- attr(*, \"dimnames\")=List of 2")
                && captured.stdout.contains("$ center:"),
            "list matrix components must carry nested dimnames, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("Date[1:2], format: \"2007-11-11\" NA")
                && captured.stdout.contains("num [1:2] 1.5 NA")
                && captured.stdout.contains("chr [1:2] \"MALE\" NA"),
            "Date/NA str must match GNU, got {:?}",
            captured.stdout
        );






    }




    #[test]
    fn summary_mixed_range_shares_common_decimals() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "options(digits=7); summary(c(1,100))\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("1.00")
                && captured.stdout.contains("100.00")
                && captured.stdout.contains("50.50"),
            "named numeric summary must share format() decimals, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("   1 ") && !captured.stdout.split_whitespace().any(|w| w == "1"),
            "must not trim 1.00 to 1, got {:?}",
            captured.stdout
        );
    }


    #[test]
    fn options_max_print_inf_warns_then_errors() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
e1 <- tryCatch(options(max.print=Inf), error=function(e)e)
inherits(e1, "error")
"#,
        );
        let result = result.expect("max.print=Inf must error");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        let text = format!("{}{}", captured.stdout, captured.stderr);
        assert!(
            text.contains("In options(max.print = Inf) : NAs introduced by coercion to integer range"),
            "asInteger(Inf) must warn with the options() call like GNU, got stdout={:?} stderr={:?}",
            captured.stdout,
            captured.stderr
        );

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
local({
  new <- 42
  w <- tryCatch(
    example(new),
    warning = function(e) conditionMessage(e),
    error = function(e) conditionMessage(e)
  )
  msg <- paste(as.character(w), collapse = " ")
  !grepl("42", msg, fixed = TRUE) && (is.character(w) && grepl("new", msg, fixed = TRUE) || !is.character(w))

})
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
    fn methods_setmethod_caret_ops_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("foo", representation(x="numeric"))
xx <- new("foo", x=1)
ff <- args(getGeneric("^"))
body(ff) <- "testit"
setMethod("^", "foo", ff)
g <- getGeneric("^")
s <- methods:::.matchSigLength(matchSignature("foo", g), g, environment(g), TRUE)
identical(as.character(s), c("foo","ANY")) &&
  isTRUE(all(c("ANY#ANY","foo#ANY") %in% ls(environment(g)$.MTable, all.names=TRUE))) &&
  identical(g(xx), "testit") &&
  identical(xx^2, "testit")
"#,
        );
        let result = result.expect("setMethod(^) must store foo#ANY and dispatch like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn primitives_internal_generics_dispatch_s3_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
x <- structure(pi, class="testit")
xx <- structure("OK", class="testOK")
ok <- TRUE
for (f in c("cumvar", "dim", "dimnames", "xtfrm")) {
  method <- paste(f, "testit", sep=".")
  ff <- get(f, .GenericArgsEnv)
  body(ff) <- xx
  assign(method, ff, .GlobalEnv)
  res <- eval(substitute(ff(x), list(ff=as.name(f))))
  ok <- ok && identical(res, xx)
  rm(list=method, envir=.GlobalEnv)
}
assign("levels<-.testit", function(x, value) xx, .GlobalEnv)
y <- x
ok <- ok && identical(eval(substitute(`levels<-`(y, value=pi))), xx)
ok
"#,
        );
        let result = result.expect("internal generics must UseMethod like GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn unlist_as_vector_lengths_are_gnu_closures() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("foo", representation(x="numeric", y="numeric"))
xx <- new("foo", x=1, y=2)
ff <- args(getGeneric("unlist"))
body(ff) <- "testit"
setMethod("unlist", "foo", ff)
identical(typeof(unlist), "closure") &&
  identical(typeof(as.vector), "closure") &&
  identical(typeof(lengths), "closure") &&
  identical(unlist(list(1, 2:3)), c(1, 2, 3)) &&
  identical(as.vector(c(a=1), "any"), 1) &&
  identical(as.integer(lengths(list(1:2, 3))), c(2L, 1L)) &&
  identical(getGeneric("unlist")(xx), "testit")
"#,
        );
        let result = result.expect("unlist/as.vector/lengths must be GNU closures");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn stop_pastes_arguments_and_primitives_reject_wrong_names() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
sm <- tryCatch(stop("failure on ", "abs"), error=function(e) conditionMessage(e))
am <- tryCatch(do.call(abs, list(zZ=NULL)), error=function(e) conditionMessage(e))
nm <- tryCatch(do.call(nargs, list(zZ=NULL)), error=function(e) conditionMessage(e))
grepl("failure on abs", sm, fixed=TRUE) &&
  grepl("does not match|unused argument", am) &&
  grepl("requires 0", nm, fixed=TRUE)
"#,
        );
        let result = result.expect("stop paste and primitive name checks must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn exists_honors_envir_and_implicit_summary_generics() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e <- new.env(parent=emptyenv())
invisible(require(methods, quietly=TRUE))
g <- getGeneric("sum")
!exists("sum", envir=e, inherits=FALSE) &&
  !exists("sum", envir=e, inherits=TRUE) &&
  !exists("sum", envir=.GlobalEnv, inherits=FALSE) &&
  isTRUE(exists("sum", envir=.GlobalEnv, inherits=TRUE)) &&
  isTRUE(exists("sum", envir=baseenv(), inherits=FALSE)) &&
  !is.primitive(g) &&
  identical(names(formals(g)), c("x", "...", "na.rm")) &&
  is(g, "genericFunction")
"#,
        );
        let result = result.expect("exists() and getGeneric(sum) must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn compiled_remove_source_keeps_extracted_missing_formals() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- function(z) is.name(z) && !missing(z)
g <- utils::removeSource(function(x) 1)
isTRUE(f(formals(function(x) x)[[1]])) &&
  identical(names(formals(g)), "x") &&
  identical(g(1), 1)
"#,
        );
        let result = result.expect("GETVAR must return a supplied empty-name value");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn implicit_norm_generic_is_standard_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
g <- implicitGeneric("norm")
isS4(g) && is(g, "standardGeneric") &&
  identical(as.character(g@generic)[1], "norm") &&
  identical(as.character(attr(g, "generic"))[1], "norm")
"#,
        );
        let result = result.expect("methods must register the implicit norm generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn as_method_definition_sees_methods_namespace() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
m <- methods:::asMethodDefinition(function(x) x)
identical(environment(methods:::asMethodDefinition), asNamespace("methods")) &&
  is(m, "MethodDefinition")
"#,
        );
        let result = result.expect("asMethodDefinition must close over methods");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn make_generic_builds_norm_standard_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
fdef <- getFunction("norm", mustFind=FALSE)
body(fdef) <- substitute(standardGeneric(NAME), list(NAME="norm"))
g <- methods:::makeGeneric(
  "norm", fdef,
  fdefault=getFunction("norm", generic=FALSE, mustFind=FALSE),
  package="base", signature=c("x","type")
)
isS4(g) && is(g, "standardGeneric") && identical(as.character(g@generic)[1], "norm")
"#,
        );
        let result = result.expect("makeGeneric(norm) must produce a standardGeneric");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setter_call_does_not_eval_language_replacement() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- function(e, v) { body(e) <- v; e }
g <- f(function(x) 1, quote(y))
identical(body(g), quote(y))
"#,
        );
        let result = result.expect("body<- must store a language value");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn remove_source_does_not_call_standardgeneric_body() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
g <- utils::removeSource(function(x, type, ...) standardGeneric("norm"))
h <- utils::removeSource(function(x) x + 1)
identical(body(g), quote(standardGeneric("norm"))) && identical(h(1), 2)
"#,
        );
        let result = result.expect("removeSource must not evaluate language bodies");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn language_subassign_preserves_call() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e <- quote(x + 1)
e[2] <- list(e[[2]])
identical(e, quote(x + 1))
"#,
        );
        let result = result.expect("[<- on language must keep the call");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setgeneric_norm_uses_implicit_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("norm")
is(norm, "standardGeneric") && identical(as.character(norm@generic)[1], "norm")
"#,
        );
        let result = result.expect("setGeneric(norm) must install the implicit generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_norm_dispatches_character_type() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("zzz", slots = c(x = "NULL"))
setMethod("norm", c(x = "zzz", type = "character"), function(x, type, ...) type)
setMethod("rcond", c(x = "zzz", norm = "character"), function(x, norm, ...) norm)
x <- new("zzz")
identical(norm(x, "O"), "O") && identical(norm(x), "O") &&
  identical(rcond(x, "O"), "O") && identical(rcond(x), "O")
"#,
        );
        let result = result.expect("setMethod(norm/rcond) must dispatch like classes-methods.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_implicit_norm_rcond_unmodified() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("zzz", slots = c(x = "NULL"))
setMethod( "norm", c(x = "zzz", type = "character"),
          function (x, type, ...) type)
setMethod("rcond", c(x = "zzz", norm = "character"),
          function (x, norm, ...) norm)
m4 <- list(getMethod( "norm", c(x = "ANY", type = "missing")),
           getMethod("rcond", c(x = "ANY", norm = "missing")),
           selectMethod( "norm", c(x = "zzz", type = "missing")),
           selectMethod("rcond", c(x = "zzz", norm = "missing")))
f4 <- lapply(m4, getDataPart)
x <- new("zzz")
stopifnot(all(vapply(m4, is, FALSE, "MethodDefinition")),
          identical(f4[3:4], f4[1:2]),
          identical( norm(x, "O"), "O"),
          identical( norm(x     ), "O"),
          identical(rcond(x, "O"), "O"),
          identical(rcond(x     ), "O"),
          removeGeneric( "norm"),
          removeGeneric("rcond"),
          removeClass("zzz"))
TRUE
"#,
        );
        let result = result.expect("unmodified classes-methods.R:10-29");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn unlist_empty_lists_keep_gnu_type() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(unlist(list(list(), list()), recursive=FALSE)), "list") &&
  identical(length(unlist(list(list(), list()), recursive=FALSE)), 0L) &&
  identical(typeof(unlist(list(integer(0), integer(0)))), "integer") &&
  is.null(unlist(list(NULL, NULL)))
"#,
        );
        let result = result.expect("empty unlist must keep GNU AnswerType");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setclass_contains_empty_slot_superclass() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
foo <- setClass("foo")
bar <- setClass("bar", contains = "foo")
isClass("bar") && extends("bar", "foo")
"#,
        );
        let result = result.expect("setClass(contains=) must complete empty slots");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn seq_int_named_to_before_from_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(seq.int(to = 3, from = 1), 1:3)",
        );
        let result = result.expect("seq.int must matchArgs, not check1arg the first tag");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_subset_callnextmethod_matches_classes_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
foo <- setClass("foo")
bar <- setClass("bar", contains = "foo")
setMethod("[", "foo",  function(x, i, j, ..., flag = FALSE, drop = FALSE) { flag })
setMethod("[", "bar", function(x, i, j, ..., flag = FALSE, drop = FALSE) { callNextMethod() })
BAR <- new("bar")
identical(BAR[1L], FALSE) && identical(BAR[1L, , flag=TRUE], TRUE)
"#,
        );
        let result = result.expect("classes-methods.R callNextMethod must forward flag=");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_as_vector_subassign() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(stuff="numeric"))
as.vector.A <- function (x, mode="any") x@stuff
v <- c(3.5, 0.1)
a <- new("A", stuff=v)
x <- y <- numeric(10)
x[3:4] <- a
y[3:4] <- v
identical(x, y)
"#,
        );
        let result = result.expect("classes-methods.R as.vector S3 method in [<-");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_ops_extra_arg_and_dots() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("MyInteger", representation("integer"))
i <- new("MyInteger", 1L)
m <- matrix(1:6, 2, 3)
setGeneric("genericExtraArg",
           function(x, y, extra) standardGeneric("genericExtraArg"),
           signature="x")
setMethod("genericExtraArg", "ANY", function(x, y=NULL) y)
f <- function(...) length(list(...))
setGeneric("f")
setMethod("f", "character", function(...){ callNextMethod() })
identical(i*m, m) &&
  identical(genericExtraArg("foo", 1L), 1L) &&
  identical(f(1, 2, 3), 3L) &&
  identical(f("a", "b", "c"), 3L)
"#,
        );
        let result = result.expect("classes-methods.R Ops, rematch NULL default, dots");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_dots_missing_and_forwarding() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x, ..., a = b) { b <- "a"; a }
setGeneric("f", signature = "...")
f2 <- function(...) f(...)
identical(f(a=1), 1) && identical(f(), "a") && identical(f2(a=1), 1)
"#,
        );
        let result = result.expect("classes-methods.R missing-arg dots dispatch");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_method_selection_error() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x) x
setGeneric("f")
setMethod("f", signature("NULL"), function(x) NULL)
err <- tryCatch(f(stop("this is mentioned")), error = identity)
identical(err$message, "error in evaluating the argument 'x' in selecting a method for function 'f': this is mentioned")
"#,
        );
        let result = result.expect("classes-methods.R method-selection error wrap");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_oldclass_union_recache() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
where <- environment()
setClass("UnionMemberForOldClassRecache", contains = "VIRTUAL", where = where)
setClassUnion("UnionForOldClassRecache", "UnionMemberForOldClassRecache",
              where = where)
setClass("ParentForOldClassRecache",
         contains = c("UnionForOldClassRecache", "VIRTUAL"), where = where)
setClass("ChildForOldClassRecache",
         contains = c("ParentForOldClassRecache", "VIRTUAL"), where = where)
setOldClass(c("ChildForOldClassRecache", "oldClass"),
            S4Class = "ChildForOldClassRecache", where = where)
union <- getClass("UnionForOldClassRecache", where = where)
child <- getClass("ChildForOldClassRecache", where = where)
"ChildForOldClassRecache" %in% names(union@subclasses) &&
  "UnionForOldClassRecache" %in% names(child@contains)
"#,
        );
        let result = result.expect("classes-methods.R setOldClass union recache");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_setoldclass_s3class_tails() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("oldClassChildForAs",
              "oldClassParentForAs",
              "oldClassGrandParentForAs"))
identical(attr(getClass("oldClassGrandParentForAs")@prototype, ".S3Class"),
          "oldClassGrandParentForAs") &&
  identical(attr(getClass("oldClassParentForAs")@prototype, ".S3Class"),
            c("oldClassParentForAs", "oldClassGrandParentForAs")) &&
  identical(attr(getClass("oldClassChildForAs")@prototype, ".S3Class"),
            c("oldClassChildForAs", "oldClassParentForAs", "oldClassGrandParentForAs"))
"#,
        );
        let result = result.expect("setOldClass must accumulate .S3Class tails");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_as_s4_from_oldclass_upcast() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("oldClassChildForAs",
              "oldClassParentForAs",
              "oldClassGrandParentForAs"))
setClass("GrandParentShimForAs", contains = "oldClassGrandParentForAs")
setClass("ParentShimForAs",
         contains = c("oldClassParentForAs", "GrandParentShimForAs"))
setClass("S4ChildForAs",
         slots = list(extra = "character"),
         contains = "ParentShimForAs")
object <- new("S4ChildForAs",
              structure(list(),
                        class = c("oldClassParentForAs",
                                  "oldClassGrandParentForAs")),
              extra = "x")
parent <- as(object, "ParentShimForAs")
grandparent <- as(object, "GrandParentShimForAs")
isS4(parent) && is(parent, "ParentShimForAs") &&
  identical(as.character(class(parent)), "ParentShimForAs") &&
  isS4(grandparent) && is(grandparent, "GrandParentShimForAs") &&
  identical(as.character(class(grandparent)), "GrandParentShimForAs")
"#,
        );
        let result = result.expect("classes-methods.R as() S4-from-old-class upcast");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_cancoerce_multiclass_s3() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(stuff="numeric"))
setOldClass("foo")
setAs("foo", "A", function(from) new("A", foo=from))
o3 <- structure(1:7, class = c("foo", "bar"))
canCoerce(o3, "A")
"#,
        );
        let result = result.expect("classes-methods.R canCoerce length(class)>1");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_basegeneric_missing_signature() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("BaseGeneric", function(x, y, ...) standardGeneric("BaseGeneric"))
setMethod("BaseGeneric", signature(x = "numeric", y = "numeric"), function(x,y, ...) x + y)
errXY <- try(BaseGeneric(X = 1, Y = 2))
err1  <- try(BaseGeneric(1))
err1Y <- try(BaseGeneric(1, Y = 2))
identical(3, BaseGeneric(1, 2)) &&
  inherits(errXY, "try-error") &&
  grepl('x = "missing", y = "missing"', attr(errXY,"condition")$message) &&
  inherits(err1,  "try-error") &&
  grepl('x = "numeric", y = "missing"', attr(err1, "condition")$message) &&
  identical(err1, err1Y)
"#,
        );
        let result = result.expect("classes-methods.R BaseGeneric missing-arg signatures");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn try_call_less_errors_are_identical_across_expressions() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e1 <- try(stop(simpleError("x")), silent=TRUE)
e2 <- try((function() stop(simpleError("x")))(), silent=TRUE)
identical(e1, e2) &&
  identical(as.character(e1), "Error : x\n") &&
  is.null(attr(e1, "condition")$call)
"#,
        );
        let result = result.expect("try() call-less errors share GNU Error : prefix");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }



    #[test]
    fn classes_methods_sealclass() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("fooSeal", slots = c(name = "character"), sealed = TRUE)
ok1 <- isSealedClass("fooSeal")
ok2 <- inherits(try(setClass("fooSeal"), silent=TRUE), "try-error")
ok3 <- isTRUE(removeClass("fooSeal"))
setClass("fooSeal")
sealClass("fooSeal")
ok4 <- isSealedClass("fooSeal")
ok5 <- isTRUE(removeClass("fooSeal"))
ok1 && ok2 && ok3 && ok4 && ok5
"#,
        );
        let result = result.expect("classes-methods.R sealClass");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn is_base_namespace_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
isBaseNamespace(.BaseNamespaceEnv) &&
  identical(asNamespace("base"), .BaseNamespaceEnv) &&
  !isBaseNamespace(asNamespace("methods"))
"#,
        );
        let result = result.expect("isBaseNamespace is GNU namespace.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn objects_is_gnu_ls_alias() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists("objects", envir=baseenv(), inherits=FALSE) &&
  typeof(objects) == "builtin" &&
  identical(objects(envir=baseenv()), ls(envir=baseenv()))
"#,
        );
        let result = result.expect("GNU attach.R: objects is an ls primitive");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_namespace_lists_class_metadata_bindings() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
ns <- asNamespace("methods")
sum(startsWith(ls(envir=ns, all.names=TRUE), ".__C__")) > 0 &&
  exists("cacheMetaData", envir=ns, inherits=FALSE)
"#,
        );
        let result = result.expect("methods ns lists sourced .__C__ class bindings");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn getgenerics_keeps_package_attribute() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
gens <- methods:::.getGenerics(asNamespace("methods"))
length(gens) > 0 && identical(typeof(attr(gens, "package")), "character") &&
  length(attr(gens, "package")) == length(gens)
"#,
        );
        let result = result.expect(".getGenerics must keep the package attribute");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn show_body_assign_after_methods_onload_cache() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
out <- capture.output(show(`body<-`))
any(grepl("showMethods(`body<-`)", out, fixed=TRUE))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!("show(`body<-`) after methods load: {e}\nstdout={}", output.stdout)
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn library_binding_is_gnu_default_library() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists(".Library", envir=baseenv(), inherits=FALSE) &&
  is.character(.Library) &&
  length(.Library) == 1L &&
  nzchar(.Library)
"#,
        );
        let result = result.expect("GNU .Library is R.home('library')");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rmpkg_strips_package_prefix() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"identical(.rmpkg("package:methods"), "methods") && identical(.rmpkg("methods"), "methods")"#,
        );
        let result = result.expect("GNU attach.R .rmpkg");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn trace_is_gnu_base_wrapper() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists("trace", envir=baseenv(), inherits=FALSE) &&
  is.function(trace) &&
  exists("untrace", envir=baseenv(), inherits=FALSE) &&
  is.function(untrace)
"#,
        );
        let result = result.expect("GNU methodsSupport.R: trace/untrace");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn tracing_state_toggles_do_trace() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
old <- tracingState(TRUE)
n <- 0L
tracingState(FALSE)
.doTrace(n <- 1L)
off <- n
tracingState(TRUE)
.doTrace(n <- 2L)
on <- n
tracingState(old)
identical(off, 0L) && identical(on, 2L)
"#,
        );
        let result = result.expect("tracingState must gate .doTrace");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn classes_methods_setis_simple_as() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", slots = c(x = "NULL"))
setClass("B", slots = c(x = "NULL"))
setIs("A", "B",
      test = function(.) { TRUE },
      coerce = function(.) new("B"),
      replace = function(., value) new("B"))
B <- as(new("A"), "B")
identical(B, new("B"))
"#,
        );
        match result {
            Ok(v) => assert_eq!(v.logical_elt(0), Some(TRUE)),
            Err(err) => panic!("setIs/as: {err}\nstdout={}", output.stdout),
        }
    }

    #[test]
    fn classes_methods_toeplitz_two_arg() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists("toeplitz", envir=baseenv(), inherits=FALSE) &&
  identical(
    as.vector(toeplitz(c(-1, 0, 0), c(-1, 11, 0))),
    c(-1, 0, 0, 11, -1, 0, 0, 11, -1)
  ) &&
  identical(as.vector(toeplitz(1:3)), c(1L, 2L, 3L, 2L, 1L, 2L, 3L, 2L, 1L))
"#,
        );
        let result = result.expect("GNU toeplitz(x, r) first column/row");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_toeplitz_implicit_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Atoep", slots = c(x = "NULL"))
x <- c(-1, 0, 0)
r <- c(-1, 11, 0)
T3 <- toeplitz(x, r)
g <- implicitGeneric("toeplitz")
env_ok <- identical(environment(get("toeplitz", envir=baseenv(), inherits=FALSE)), asNamespace("stats"))
setMethod("toeplitz", "Atoep", function(x, ...) x)
identical(names(formals(g)), c("x", "...")) &&
  env_ok &&
  identical(T3, toeplitz(x, r)) &&
  is(selectMethod(toeplitz, "numeric"), "MethodDefinition") &&
  removeGeneric("toeplitz")
"#,
        );
        let result = result.expect("classes-methods.R toeplitz implicit generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn classes_methods_trace_coerce_signature() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
trr <- quote(list(.Generic, .Method, .defined, .target))
sig <- c("ANY", "logical")
m0 <- selectMethod(coerce, signature = sig)
a0 <- as(0, "logical")
trace(coerce, tracer = trr, signature = sig)
m1 <- selectMethod(coerce, signature = sig)
a1 <- as(0, "logical")
untrace(coerce, signature = sig)
m2 <- selectMethod(coerce, signature = sig)
is(m0, "MethodDefinition") &&
  !is(m0, "MethodDefinitionWithTrace") &&
  is(m1, "MethodDefinitionWithTrace") &&
  identical(m0, m2) && identical(a0, a1)
"#,
        );
        let result = result.expect("classes-methods.R PR#18823 trace(coerce)");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn class_attribute_is_namedmax_on_return() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- structure(function(x) x, class = c("myfun", "function"))
cn <- class(f)
cn[] <- paste0(cn, "WithTrace")
identical(class(f), c("myfun", "function")) &&
  identical(cn, c("myfunWithTrace", "functionWithTrace"))
"#,
        );
        let result = result.expect("class() must not alias the live class attribute");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_trace_multiclass_function() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("myfun", "function"))
f <- structure(function(x) x, class = c("myfun", "function"))
n <- 0
suppressMessages(trace("f", quote(n <<- n + 1), print = FALSE))
f1 <- f(1)
untrace("f")
identical(f1, 1) && identical(n, 1) &&
  identical(class(f), c("myfun", "function")) &&
  identical(f(2), 2) && identical(n, 1)
"#,
        );
        let result = result.expect("classes-methods.R trace multi-string class()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn signature_class_names_slot_is_argument_names() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x) x
setGeneric("f")
setMethod("f", "numeric", function(x) x)
md <- selectMethod("f", "numeric")
identical(md@target@names, "x") && identical(md@defined@names, "x")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!("signature@names: {e}\nstdout={}\nstderr={}", output.stdout, output.stderr)
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_gnu_skip_path_without_matrix() {
        let mut session = RSession::new();
        let vendor = include_str!("../../../../tests/upstream-r/vendor/classes-methods.R");
        let lines: Vec<&str> = vendor.lines().collect();
        let mut src = String::from("invisible(require(methods, quietly=TRUE))\n");
        for (i, line) in lines.iter().enumerate() {
            let lineno = i + 1;
            // Matrix 47-120 and 287-305 omitted (GNU skip when Matrix is absent).
            if (47..=120).contains(&lineno) || (287..=305).contains(&lineno) {
                continue;
            }
            src.push_str(line);
            src.push('\n');
        }
        src.push_str("TRUE\n");
        let (result, output, _) = session.eval_script_with_output_capture(&src);
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R GNU skip path (Matrix omitted): {e}\nstdout={}\nstderr={}",

                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }


    #[test]
    fn methods_package_slot_assign_sets_attribute() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
w <- `packageSlot<-`("a", ".GlobalEnv")
identical(attr(w, "package"), ".GlobalEnv")
"#,
        );
        let result = result.expect("methods::packageSlot<- must set the package attribute");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_new_oldclass_function_trace_class_name() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("myfun", "function"))
setClass("myfunWithTrace", contains = c("myfun", "traceable"))
a <- new("myfunWithTrace")
isS4(a) && identical(as.character(class(a))[1], "myfunWithTrace")
"#,
        );
        let result = result.expect("new() must stamp S4 class on oldClass+function prototype");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_new_trace_class_with_function_def() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("myfun", "function"))
setClass("myfunWithTrace", contains = c("myfun", "traceable"))
f <- structure(function(x) x, class = c("myfun", "function"))
a <- new("myfunWithTrace", f)
isS4(a) && identical(as.character(class(a))[1], "myfunWithTrace")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!("new(myfunWithTrace, def): {e}\nstdout={}\nstderr={}", output.stdout, output.stderr)
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_dispatch_on_after_require_and_reload() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
options(useFancyQuotes=FALSE)
invisible(require(methods, quietly=TRUE))
on1 <- .isMethodsDispatchOn()
invisible(require(stats4, quietly=TRUE))
detach("package:methods")
invisible(require("methods", quietly=TRUE))
on1 && .isMethodsDispatchOn()
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                ".isMethodsDispatchOn after require/reload: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn traced_generic_uses_default_after_setmethod_widens_signature() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x, y) c(x, y)
setGeneric("f")
setMethod("f", c("character", "character"), function(x, y) paste(x, y))
labs <- sort(ls(environment(f)$.AllMTable, all.names=TRUE))
trace("f", quote(x <- c("A", x)), exit = quote(xy <<- c(x, "Z")), print = FALSE)
identical(labs, c("ANY#ANY", "character#character")) &&
  identical(f(4, 5), c("A", "4", "5")) &&
  identical(xy, c("A", "4", "Z"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "traced generic default: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn cbind2_s4_method_used_by_cbind() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("myMat", representation(x = "numeric"))
setMethod("cbind2", signature(x = "myMat", y = "missing"), function(x,y) x)
m <- new("myMat", x = c(1, pi))
identical(m, methods:::cbind(m)) && identical(m, cbind(m))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind2: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_head_through_callgeneric_local() {
        let mut session = RSession::new();
        let vendor = include_str!("../../../../tests/upstream-r/vendor/reg-S4.R");
        let src: String = vendor.lines().take(730).collect::<Vec<_>>().join("\n");
        let (result, output, _) = session.eval_script_with_output_capture(&src);
        result.unwrap_or_else(|e| {
            panic!(
                "reg-S4.R through its cbind/rbind: {e}\nstdout={}\nstderr={}",











                output.stdout, output.stderr
            )
        });
    }

    #[test]
    fn reg_s4_list_class_subset_keeps_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("L", contains = "list")
setMethod("[", signature(x="L", i="ANY", j="missing",drop="missing"),
          function(x,i,j,drop) new(class(x), x@.Data[i]))
x <- new("L", 1:3)
x2 <- x[-2]
isS4(x2) && identical(as.character(class(x2))[1], "L") &&
  identical(unlist(x2), (1:3)[-2]) &&
  identical(unlist(x[2]), 2L)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 list [ method: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn gnu_median_is_closure_with_default() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(median), "closure") &&
  identical(typeof(median.default), "closure") &&
  identical(median(1:3), 2L) &&
  identical(median(c(1, 3)), 2) &&
  identical(sort(c(3, 1, NA)), c(1, 3)) &&
  isTRUE(2 == list(2)) &&
  identical(as.vector(2 == list(1, 2, 3)), c(FALSE, TRUE, FALSE))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "gnu median wrapper: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_median_simple_list_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("L", contains = "list")
setMethod("Compare", signature(e1="L", e2="ANY"),
          function(e1,e2) sapply(e1, .Generic, e2=e2))
setMethod("Summary", "L",
	  function(x, ..., na.rm=FALSE) {x <- unlist(x); callNextMethod()})
setMethod("[", signature(x="L", i="ANY", j="missing",drop="missing"),
          function(x,i,j,drop) new(class(x), x@.Data[i]))
setMethod("xtfrm", "L", function(x) xtfrm(unlist(x@.Data)))
mean.L <- function(x, ...) new("L", mean(unlist(x@.Data), ...))
x <- new("L", 1:3); x2 <- x[-2]
identical(unlist(x2), (1:3)[-2]) &&
  is(mx <- median(x), "L") && isTRUE(mx == 2) &&
  isTRUE(median(x2) == x[2])

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "median S4 L: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_sig_as_packageslot_and_factor_validity() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
assertError <- tools::assertError
setClass("SIG", contains="signature")
pkg_ok <- packageSlot(class(S <- new("SIG"))) == ".GlobalEnv" &&
  packageSlot(class(ss <- new("signature"))) == "methods" &&
  packageSlot(class(as(S, "signature"))) == "methods"
ok.f <- gl(3,5, labels = letters[1:3])
bad.f <- structure(rep(1:3, each=5), levels=c("a","a","b"), class="factor")
validObject(ok.f)
bad_ok <- inherits(tryCatch(validObject(bad.f), error=function(e) e), "error")
setClass("myF", contains = "factor")
validObject(new("myF", ok.f))
myf_bad <- inherits(tryCatch(validObject(new("myF", bad.f)), error=function(e) e), "error")
removeClass("myF")
as_ok <- is.vector(as(structure(1:3, class = "foobar"), "vector"))
setClass("numWithId", representation(id = "character"), contains = "numeric")
x <- new("numWithId", 1:3, id = "An Example")
setMethod("xtfrm", "numWithId", function(x) x@.Data)
pkg_ok && bad_ok && myf_bad && as_ok && identical(xtfrm(x), 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "SIG/factor/xtfrm: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_subset_callnextmethod_drop_and_slot_names() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("C1", representation(a = "numeric"))
setClass("C2", contains = "C1")
setMethod("[", "C1", function(x,i,j,...,drop=TRUE)
	  cat("drop in C1-[ :", drop, "\n"))
setMethod("[", "C2", function(x,i,j,...,drop=TRUE) {
    cat("drop in C2-[ :", drop, "\n")
    callNextMethod()
})
x <- new("C1"); y <- new("C2")
o1 <- paste(capture.output(x[1, drop=FALSE]), collapse="\n")
o2 <- paste(capture.output(y[1, drop=FALSE]), collapse="\n")
grepl("drop in C1-[ : FALSE", o1, fixed=TRUE) &&
  grepl("drop in C2-[ : FALSE", o2, fixed=TRUE) &&
  grepl("drop in C1-[ : FALSE", o2, fixed=TRUE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callNextMethod drop: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_reserved_slot_names_except_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
problNames <- c("names", "dimnames", "row.names",
                "class", "comment", "dim", "tsp")
myTry <- function(expr, ...) tryCatch(expr, error = function(e) e)
tstSlotname <- function(nm) {
    r <- myTry(setClass("foo", representation =
                        structure(list("character"), names = nm)))
    if(is(r, "error")) return(r$message)
    ch <- LETTERS[1:5]
    x <- myTry(do.call(new, structure(list("foo", ch), names=c("", nm))))
    if(is(x, "error")) return(x$message)
    y <- myTry(new("foo"));		 if(is(y, "error")) return(y$message)
    r <- myTry(capture.output(show(x))); if(is(r, "error")) return(r$message)
    r <- myTry(capture.output(show(y))); if(is(r, "error")) return(r$message)
    slot(y, nm) <- slot(x, nm)
    stopifnot(validObject(x), identical(x,y), identical(slot(x, nm), ch))
    return(TRUE)
}
R <- sapply(problNames, tstSlotname, simplify = FALSE)
is.character(R[["class"]]) && all(vapply(R[names(R) != "class"], isTRUE, NA))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "reserved slot names: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_sample_implicit_generic_from_base() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("C1", representation(a = "numeric"))
setClass("C2", contains = "C1")
setMethod("sample", "C2",
          function(x, size, replace=FALSE, prob=NULL) {"sample.C2"})
is(sample,"standardGeneric") &&
  identical(sample@signature, c("x", "size")) &&
  identical(packageSlot(sample), "base") &&
  identical({set.seed(3); sample(3)}, 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "sample implicit generic: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_qqplot_generic_and_nested_slot_subassign() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
qq_ok <- is.function(qqplot) && identical(class(qqplot), "function")
setGeneric("qqplot", function(x, y, ...) standardGeneric("qqplot"))
qq_gen <- is(qqplot, "standardGeneric") && identical(qqplot@signature, c("x","y"))
setClass("foo", representation(x = "numeric"))
f <- new("foo", x = pi*1:2)
L <- list()
L$A <- f
L$A@x[] <- 7
dup_ok <- !identical(f, L$A) && identical(L$A@x, c(7, 7)) &&
  isTRUE(all.equal(f@x, pi*1:2))
qq_ok && qq_gen && dup_ok
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "qqplot/nested slot: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_classunion_prototype_intorchar() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClassUnion("OptionalPOSIXct", c("POSIXct", "NULL"))
setClassUnion("IntOrChar", c("integer", "character"))
is.null(getClass("OptionalPOSIXct")@prototype) &&
  is.integer(getClass("IntOrChar")@prototype) &&
  "IntOrChar" %in% extends(getClass("character")) &&
  "IntOrChar" %in% extends(getClass("integer")) &&
  identical(isGeneric("&&"), FALSE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classUnion prototype: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }
    #[test]
    fn reg_s4_mapply_length_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(aa="integer"))
aa <- 11:16
a <- new("A", aa=aa)
setMethod(length, "A", function(x) length(x@aa))
setMethod(`[[`,   "A", function(x, i, j, ...) x@aa[[i]])
setMethod(`[`,    "A", function(x, i, j, ...) new("A", aa = x@aa[i]))
len_ok <- length(a) == 6 && identical(a[[5]], aa[[5]]) &&
  identical(a, rev(rev(a))) && identical(rev(a)@aa, rev(aa))
map_ok <- identical(mapply(`*`, aa, rep(1:3, 2)), mapply(`*`, a,  rep(1:3, 2)))
len_ok && map_ok

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 mapply length: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_is_unsorted_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(aa="integer"))
aa <- 11:16
a <- new("A", aa=aa)
setMethod(length, "A", function(x) length(x@aa))
setMethod(`[`, "A", function(x, i, j, ...) new("A", aa = x@aa[i]))
setMethod("is.unsorted", "A", function(x, na.rm, strictly)
    is.unsorted(x@aa, na.rm=na.rm, strictly=strictly))
!is.unsorted(a) && is.unsorted(rev(a)) &&
  identical(rev(a)@aa, rev(aa))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "is.unsorted S4: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_callgeneric_do_call() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("fun", function(x, ...) standardGeneric("fun"))
setMethod("fun", "character", identity)
setMethod("fun", "numeric", function(x) {
  x <- as.character(x)
  callGeneric()
})
identical(fun(1), do.call(fun, list(1)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callGeneric: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_source_textconnection_srcref() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
source(textConnection("x <- 42L\n"))
identical(x, 42L)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "source textConnection: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }










    #[test]
    fn reg_s4_getsrcref_on_sourced_function() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
source(textConnection("f <- function(x) x\n"), keep.source = TRUE)
invisible(getSrcref(f))
is.function(getSrcref)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "getSrcref: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }
    #[test]
    fn reg_s4_help_try_and_identical_s4_bit() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Foo", representation(name="character"), contains="matrix")
f <- new("Foo", name="Sam", matrix())
m <- as(f, "matrix")
foo_ok <- isS4(m. <- asS4(m)) && identical(m, f@.Data) && .hasSlot(f, "name") && !isS4(m)
a <- 1:5
b <- setClass("B", "integer")(a)
eq_ok <- is.character(all.equal(a, b))
attributes(a) <- attributes(b)
mismatch_ok <- if (!isS4(a)) !identical(a, b) else TRUE
if (!isS4(a)) a <- asS4(a)
foo_ok && eq_ok && mismatch_ok && identical(a, b) && isS4(a)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "identical S4 bit: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_rbind2_a_and_its_matrix() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(a = "matrix"))
setMethod("initialize", signature(.Object = "A"),
    function(.Object, y) {
      .Object@a <- y
      .Object
    })
setMethod("rbind2", signature(x = "A", y = "matrix"),
    function(x, y, ...) {
      x@a <- rbind(x@a, y)
      x
    })
setMethod("dim", "A", function(x) dim(x@a))
mat1 <- matrix(1:9, nrow = 3)
obj1 <- new("A", 10*mat1)
om1 <- rbind(obj1, mat1)
a_ok <- identical(om1, rbind2(obj1, mat1))
removeClass("A")
setClass("its", representation("matrix", dates="POSIXt"))
m <- outer(1:3, setNames(1:5, LETTERS[1:5]))
im <- new("its", m, dates=as.POSIXct(Sys.Date()))
ii  <- rbind(im, im-1)
i.i <- cbind(im, im-7)
its_ok <- identical(m, im@.Data) &&
  identical(m, rbind(im)) && identical(m, cbind(im)) &&
  identical(ii, rbind(m, m-1)) && identical(i.i, cbind(m, m-7))
a_ok && its_ok
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind2/its: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }






































































    #[test]
    fn cbind2_default_negative_deparse_level_does_not_redispatch() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Num", contains="numeric")
a <- new("Num", 1:3)
identical(as.vector(cbind(a)), 1:3) && identical(as.vector(cbind2(a)), 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind2 -1L: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }





    #[test]
    fn show_print_s4_bit_on_matrix_does_not_recurse() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Foo", representation(name="character"), contains="matrix")
(f <- new("Foo", name="Sam", matrix()))
m <- as(f, "matrix")
stopifnot(isS4(m. <- asS4(m)), identical(m, f@.Data), .hasSlot(f, "name"))
show(m.)
print(m.)
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "show/print S4 matrix: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
        assert!(
            output.stdout.contains("[,1]") || output.stdout.contains("NA"),
            "expected matrix PrintValueRec, got stdout={}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("An object of class \"S4\""),
            "recursion-guard stub still printed: {}",
            output.stdout
        );

    }

    #[test]
    fn cbind_mixed_s4_and_atomic_uses_cbind2_default() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Num", contains="numeric")
a <- new("Num", 1:3)
r <- cbind(a, 4)
identical(dim(r), c(3L, 2L)) && identical(as.vector(r)[1:3], c(1, 2, 3))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind mixed: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn rbind_null_then_data_frame_binds_columns() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
df <- data.frame(a = 1:2)
r <- rbind(NULL, df)
identical(r$a, 1:2)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind NULL df: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn cbind_untagged_symbol_names_the_column() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- 1:3
identical(colnames(cbind(x)), "x") && identical(rownames(rbind(x)), "x")
"#,

        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind names: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn language_subset_minus_one_names_assign() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
f <- function(x, extrarg = TRUE) NULL
cl <- quote(Gfun(m2))
mc <- match.call(f, cl, expand.dots = FALSE)
stopifnot(identical(deparse(mc), "Gfun(x = m2)"))
stopifnot(identical(names(mc[-1L]), "x"))
mc[-1L] <- lapply(names(mc[-1L]), as.name)
stopifnot(identical(deparse(mc), "Gfun(x = x)"))
stopifnot(identical(deparse(`names<-`(quote(f(x = 1)), NULL)), "f(1)"))
e <- quote(f(x = 1))
names(e) <- NULL
stopifnot(identical(deparse(e), "f(1)"))
qq <- quote(f(a))
err <- tryCatch({ qq[-1L] <- list(); "NOERROR" }, error = function(e) e$message)
stopifnot(identical(err, "replacement has length zero"))
TRUE

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "lang subset names assign: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn is_namespace_loaded_reports_base() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(isNamespaceLoaded("base"), TRUE) &&
  identical(isNamespaceLoaded("no_such_namespace_zzz"), FALSE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "isNamespaceLoaded: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn aic_pfit_uses_s3_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
pfit <- function(data) {
    m <- mean(data)
    loglik <- sum(dpois(data, m))
    ans <- list(par = m, loglik = loglik)
    class(ans) <- "pfit"
    ans
}
AIC.pfit <- function(object, ..., k = 2) -2 * object$loglik + k
identical(AIC(pfit(1:10)), AIC.pfit(pfit(1:10)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "AIC.pfit: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn unlist_of_logicals_stays_logical() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(unlist(list(TRUE, FALSE))), "logical") &&
  identical(unlist(list(TRUE, FALSE)), c(TRUE, FALSE)) &&
  identical(typeof(unlist(list(TRUE, 1L))), "integer")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "unlist logicals: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn s4_containing_array_and_ts() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
t. <- ts(1:10, frequency = 4, start = c(1959, 2))
setClass("Arr", contains = "array")
x <- new("Arr", cbind(17))
setClass("Ts", contains = "ts")
tt <- new("Ts", t.)
t2 <- as(t., "Ts")
setClass("ts2", representation(x = "Ts", y = "ts"))
tt2 <- new("ts2", x = t2, y = t.)
stopifnot(isTRUE(all.equal(getOption("ts.eps"), 1e-5)),
          dim(x) == c(1, 1),
          is(tt, "ts"), is(t2, "ts"),
          length(tt) == length(t.),
          identical(tt2@x, t2), identical(tt2@y, t.))
TRUE
"#,






        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "Arr/Ts classes: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn setmethod_wrong_formals_order_signals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(tools, quietly=TRUE))
setGeneric("test1", function(x, printit = TRUE, name = "tmp")
           standardGeneric("test1"))
tryCatch({
  tools::assertCondition(
    setMethod("test1", "numeric", function(x, name, printit) match.call()),
    "warning", "error")
  TRUE
}, error = function(e) FALSE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "setMethod formals: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn stats4_getclass_mle_from_where() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
c1 <- getClass("mle", where = "stats4")
c2 <- getClass("mle", where = "package:stats4")
s1 <- getMethod("summary", "mle", where = "stats4")
s2 <- getMethod("summary", "mle", where = "package:stats4")
!is.null(methods:::.getClassesFromCache("mle")) &&
  is(c1, "classRepresentation") &&
  is(s1, "MethodDefinition") &&
  identical(c1, c2) && identical(s1, s2) &&
  is(getClass("mle"), "classRepresentation")

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "stats4 mle where: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn attributes_null_clears_s4_bit() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("moo", representation("matrix"))
x <- new("moo", .Data = matrix(1:4, 2))
attributes(x) <- NULL
a <- !isS4(x)
y <- new("moo", .Data = matrix(1:4, 2))
attributes(y) <- list()
a && !isS4(y)

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "attributes<- NULL S4: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_moo_matrix_data_slot() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("moo", representation("matrix"))
m <- matrix(1:4, 2, dimnames= list(NULL, c("A","B")))
nf <- new("moo", .Data = m)
n2 <- new("moo", 3:1, 3,2)
n3 <- new("moo", 1:6, ncol=2)
identical(m, as(nf, "matrix")) &&
  identical(matrix(3:1,3,2), as(n2, "matrix")) &&
  identical(matrix(1:6,ncol=2), as(n3, "matrix"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "moo matrix .Data: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_arr_ts_contains() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
t. <- ts(1:10, frequency = 4, start = c(1959, 2))
setClass("Arr", contains= "array"); x <- new("Arr", cbind(17))
setClass("Ts",  contains= "ts");   tt <- new("Ts", t.); t2 <- as(t., "Ts")
setClass("ts2", representation(x = "Ts", y = "ts"))
tt2 <- new("ts2", x=t2, y=t.)
all(dim(x) == c(1,1)) && is(tt, "ts") && is(t2, "ts") &&
  length(tt) == length(t.) &&
  identical(tt2@x, t2) && identical(tt2@y, t.)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "Arr/Ts contains: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_rbind_generic_dots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
a <- identical(rbind(1), matrix(1,1,1))
setGeneric("rbind", function(..., deparse.level=1)
	   standardGeneric("rbind"), signature = "...")
a && identical(rbind(1), matrix(1,1,1))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind generic dots: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn gnu_order_is_closure_with_formals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(names(formals(order)), c("...", "na.last", "decreasing", "method")) &&
  !is.primitive(order) &&
  identical(order(c(3, 1, 2)), c(2L, 3L, 1L))

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "gnu order formals: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn reg_s4_order_setgeneric_dots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("order", signature="...",
	   function (..., na.last=TRUE, decreasing=FALSE)
	   standardGeneric("order"))
identical(rbind(1), matrix(1,1,1))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "order setGeneric dots: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }














    #[test]
    fn getgenerics_stats4_lists_exported_generics() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
e4 <- as.environment("package:stats4")
gg4 <- getGenerics(e4)
em <- as.environment("package:methods")
ggm <- getGenerics(em)
gms <- c("addNextMethod", "body<-", "cbind2", "initialize",
	 "loadMethod", "Ops", "rbind2", "show")
stopifnot(c("BIC", "coef", "confint", "logLik", "plot", "profile",
            "show", "summary", "update", "vcov") %in% gg4,
          unlist(lapply(gg4, function(g) !is.null(getGeneric(g, where = e4)))),
          unlist(lapply(gg4, function(g) !is.null(getGeneric(g)))),
          isGeneric("show", where=e4),
          hasMethods("show", where=e4),
          unlist(lapply(ggm, function(g) !is.null(getGeneric(g, where = em)))),
          gms %in% ggm,
          gms %in% tools:::get_S4_generics_with_methods(em),
          identical(as.character(gg4),
                    tools:::get_S4_generics_with_methods(e4)))
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "getGenerics(stats4): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }








    #[test]
    fn as_double_uses_as_numeric_s4_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("c1", "numeric")
setClass("c2", "numeric")
x_c1 <- new("c1")
setMethod("as.numeric", "c1", function(x, ...) 42+pi)
setMethod(as.double, "c2", function(x, ...) x@.Data+pi)
x_c2 <- new("c2", pi)
identical(as.numeric(x_c1), as.double(x_c1)) &&
  identical(as.double(x_c1), 42+pi) &&
  identical(as.numeric(x_c2), as.double(x_c2)) &&
  isTRUE(all.equal(as.vector(as.numeric(x_c2)), pi + pi))


"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "as.double/as.numeric: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn aic_pfit_survives_stats4_generic() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
pfit <- function(data) {
    m <- mean(data)
    loglik <- sum(dpois(data, m))
    ans <- list(par = m, loglik = loglik)
    class(ans) <- "pfit"
    ans
}
AIC.pfit <- function(object, ..., k = 2) -2 * object$loglik + k
identical(AIC(pfit(1:10)), AIC.pfit(pfit(1:10)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "AIC.pfit stats4: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }
























    #[test]
    fn callgeneric_after_unclass_uses_default() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("Gfun", function(x, ...) standardGeneric("Gfun"),
           useAsDefault = function(x, ...) sum(x, ...))
setClass("mmat2", contains="matrix")
setMethod(Gfun, signature(x = "mmat2"),
          function(x, extrarg = TRUE) {
              x <- unclass(x)
              callGeneric()
          })
m2 <- new("mmat2", matrix(1:12, 3,4))
identical(Gfun(m2), 78L)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callGeneric: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn callgeneric_passes_extra_formals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("Gfun", function(x, ...) standardGeneric("Gfun"),
           useAsDefault = function(x, ...) sum(x, ...))
setClass("mmat2", contains="matrix")
setMethod(Gfun, signature(x = "mmat2"),
          function(x, extrarg = TRUE) {
              x <- unclass(x)
              callGeneric()
          })
m2 <- new("mmat2", diag(3))
identical(Gfun(m2, extrarg = FALSE), 3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callGeneric extrarg: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }












    #[test]
    fn rematch_definition_wraps_extra_formals_in_local() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("Gfun", function(x, ...) standardGeneric("Gfun"),
           useAsDefault = function(x, ...) sum(x, ...))
setClass("mmat2", contains="matrix")
setMethod(Gfun, signature(x = "mmat2"),
          function(x, extrarg = TRUE) {
              x <- unclass(x)
              callGeneric()
          })
isRematched(getMethod("Gfun", "mmat2"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rematch: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

























    #[test]
    fn hashed_env_names_include_hash_bindings() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
e <- new.env(hash=TRUE, parent=emptyenv())
assign("brob#ANY", 1, envir=e)
assign(".hidden", 2, envir=e)
identical(sort(names(e)), sort(c("brob#ANY", ".hidden")))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "hashed env names: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn is_na_preserves_matrix_dim() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
m <- matrix(c(1L, NA_integer_, 3L, 4L), 2, 2)
identical(dim(is.na(m)), c(2L, 2L)) &&
  identical(as.integer(colSums(is.na(m))), c(1L, 0L))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "is.na dim: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn logic_group_selectmethod_inherited_and() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("brob", contains="numeric")
logic2 <- function(e1,e2) e1
setMethod("Logic", signature("brob", "ANY"), logic2)
setMethod("Logic", signature("ANY", "brob"), logic2)
m <- selectMethod("&", c("brob","brob"), optional=TRUE)
is(m, "MethodDefinition")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "selectMethod &: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn logic_group_brob_and_errors() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
stopifnot(all(getGroupMembers("Logic") %in% c("&", "|")),
          any(getGroupMembers("Ops") == "Logic"))
setClass("brob", contains="numeric")
b <- new("brob", 3.14)
logic.brob.error <- function(nm)
    stop("logic operator '", nm, "' not applicable to brobs")
logic2 <- function(e1,e2) logic.brob.error(.Generic)
setMethod("Logic", signature("brob", "ANY"), logic2)
setMethod("Logic", signature("ANY", "brob"), logic2)
generic_ok <- isTRUE(tryCatch({ getGeneric("&")(b, b); FALSE }, error=function(e)
    grepl("not applicable to brobs", conditionMessage(e), fixed=TRUE)))
prim_ok <- isTRUE(tryCatch({ b & b; FALSE }, error=function(e)
    grepl("not applicable to brobs", conditionMessage(e), fixed=TRUE)))
generic_ok && prim_ok
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "logic group brob: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }






























    #[test]
    fn stats4_hasmethods_coef_after_require() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
isTRUE(isGeneric("coef")) && isTRUE(hasMethods("coef"))
"#,


        );
        let result = result.unwrap_or_else(|e| {

            panic!(
                "stats4 hasMethods(coef): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }


    #[test]
    fn as_environment_null_is_defunct_like_gnu() {

        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
inherits(tryCatch(as.environment(NULL), error=function(e) e), "error") &&
  grepl("as.environment\\(NULL\\)' is defunct",
        tryCatch(as.environment(NULL), error=function(e) conditionMessage(e)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "as.environment(NULL): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_is_usemethod_closure_and_primitive_builtins_match_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(print), "closure") &&
  identical(typeof(sum), "builtin") &&
  isTRUE(is.primitive(sum)) &&
  isTRUE(!is.primitive(print)) &&
  isTRUE(!is.primitive(function(x) x))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "print/sum primitive: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_print_on_s4_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("bar", representation(a="numeric"))
setMethod("print", "bar", function(x, ...) cat("S4 print method\n"))
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "setMethod(print): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_autoprint_uses_show_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("bar", representation(a="numeric"))
foo <- new("bar", a=pi)
setMethod("show", "bar", function(object){cat("show method\n")})
foo
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 auto-print show: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            output.stdout.contains("show method"),
            "auto-print of S4 bar must call show(); stdout={:?}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("[object; length=0]")
                && !output.stdout.contains("[unknown; length=0]"),
            "auto-print must not emit port object stub; stdout={:?}",
            output.stdout
        );
    }

































































    #[test]
    fn t_and_f_are_symbols_bound_to_logicals() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(T, TRUE) && identical(F, FALSE) &&
  identical(typeof(quote(F())[[1]]), "symbol") &&
  identical(quote(F())[[1]], as.symbol("F")) &&
  identical(
    paste(deparse(substitute(F(), list(F = quote(n <<- n + 1)))), collapse = " "),
    "(n <<- n + 1)()"
  ) &&
  { F <- 5; identical(F, 5) }
"#,

        );
        let result = result.expect("GNU T/F are symbols, not parser keywords");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }





















    #[test]
    fn methods_namespace_exports_body_assign() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
exists(".__NAMESPACE__.", envir=asNamespace("methods"), inherits=FALSE) &&
  "body<-" %in% names(.getNamespaceInfo(asNamespace("methods"), "exports")) &&
  identical(methods:::.minimalName("body<-", "methods", qName=TRUE, chkXport=TRUE), "`body<-`")
"#,
        );
        let result = result.expect("methods .__NAMESPACE__. exports include body<-");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn show_getgeneric_body_assign_backticks() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
any(grepl("showMethods(`body<-`)", capture.output(show(getGeneric("body<-"))), fixed=TRUE))
"#,
        );
        let result = result.expect("show(getGeneric(body<-)) backticks");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_generic_data_part_does_not_clear_live_object() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
obj <- `body<-`
invisible(obj@.Data)
isS4(obj) && identical(as.character(obj@generic)[1], "body<-")
"#,
        );
        let result = result.expect("getDataPart must not unset S4 on the live generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }












    #[test]
    fn methods_namespace_has_no_empty_c_or_rep() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
!exists("rep", envir=asNamespace("methods"), inherits=FALSE) &&
  !exists("c", envir=asNamespace("methods"), inherits=FALSE)
"#,
        );
        let result = result.expect("methods namespace must not bind empty c/rep");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }




    #[test]
    fn gnu_norm_rcond_are_closures_with_implicit_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(norm), "closure") &&
  identical(names(formals(norm)), c("x", "type")) &&
  identical(typeof(rcond), "closure") &&
  identical(names(formals(rcond)), c("x", "norm", "triangular", "uplo", "..."))
"#,
        );

        let result = result.expect("norm/rcond implicit methods must match GNU classes-methods.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn seq_along_dispatches_length_and_warns_on_coercion() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- structure(pi, class="testit")
length.testit <- function(x) "OK"
try(eval(substitute(ff(x), list(ff=as.name("seq_along")))), silent=TRUE)
TRUE
"#,
        );
        let result = result.expect("seq_along must dispatch length()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        let text = format!("{output:?}");
        assert!(
            text.contains("NAs introduced by coercion"),
            "missing coercion warning: {text}"
        );
        assert!(
            text.contains("In eval"),
            "missing GNU In-eval attribution: {text}"
        );
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
