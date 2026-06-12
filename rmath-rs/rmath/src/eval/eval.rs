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

use crate::sexp::accessors::{CHAR, PRINTNAME, TYPEOF, VECTOR_ELT, XLENGTH};
use crate::sexp::envir::{find_fun_result, forcePromise};
use crate::sexp::ffi::{SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::object::{PairlistIter, Sexp, SexpError};
use crate::sexp::symbol::{R_DotsSymbol, symbol_name_bytes_equal, symbol_name_from_ptr};

use super::apply::{apply_builtin_safe, apply_closure_safe, apply_special_safe};
pub use super::error::EvalError;
pub use super::limits::{
    EvalLimits, EvalTimerGuard, check_eval_depth, eval_with_limits, get_eval_limits,
    reset_eval_limits, set_eval_limits,
};
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
#[derive(Clone, Copy, Debug)]
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
        if !self.env.is_owner_scoped() {
            return Err("eval context environment is not owner-scoped".to_string());
        }
        if !expr.is_owner_scoped() {
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

    match eval_safe(expr, env) {
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
    if let Some(name) = symbol_name_from_ptr(expr.as_raw()) {
        return name;
    }
    unsafe {
        if TYPEOF(expr.as_raw()) == SEXPTYPE::SYMSXP {
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
    match classify_expr(expr) {
        EvalKind::SelfEvaluating => Ok(expr),
        EvalKind::Symbol => {
            if let Some(value) = find_var_result(expr, env)? {
                return Ok(value);
            }
            match primitive_for_symbol(expr) {
                Some(primitive) => Ok(primitive),
                None => Err(format!(
                    "object '{}' not found",
                    symbol_name_for_error(expr)
                )),
            }
        }
        EvalKind::Language => eval_lang_safe(expr, env),
        EvalKind::Closure => Ok(expr),
        EvalKind::Promise => eval_promise_safe(expr, env),
        EvalKind::Dots => eval_dots_safe(expr, env),
        EvalKind::ExpressionVector => eval_expression_vector_safe(expr, env),
        EvalKind::Bytecode => eval_bytecode_safe(expr, env),
        EvalKind::Unsupported(kind) => Err(format!("cannot evaluate type {:?}", kind)),
    }
}

fn eval_expression_vector_safe<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let mut result = unsafe { Sexp::from_raw_unchecked(R_NilValue()) };
    let len = unsafe { XLENGTH(expr.as_raw()) };

    for index in 0..len {
        let raw_element = unsafe { VECTOR_ELT(expr.as_raw(), index) };
        if raw_element.is_null() || raw_element == unsafe { R_NilValue() } {
            continue;
        }
        let element = unsafe { Sexp::from_raw_unchecked(raw_element) };
        result = eval_safe(element, env)?;
    }

    Ok(result)
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
    Promise,
    Dots,
    ExpressionVector,
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
        | SEXPTYPE::EXTPTRSXP => EvalKind::SelfEvaluating,
        SEXPTYPE::EXPRSXP => EvalKind::ExpressionVector,
        SEXPTYPE::SYMSXP => EvalKind::Symbol,
        SEXPTYPE::LANGSXP => EvalKind::Language,
        SEXPTYPE::CLOSXP => EvalKind::Closure,
        SEXPTYPE::PROMSXP => EvalKind::Promise,
        SEXPTYPE::DOTSXP => EvalKind::Dots,
        SEXPTYPE::BCODESXP => EvalKind::Bytecode,
        kind => EvalKind::Unsupported(kind),
    }
}

/// Safe evaluation of a language object (function call).
pub(crate) fn eval_lang_safe<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let fun = e.try_car().map_err(|err| sexp_err("empty call", err))?;
    let args = e.try_cdr().map_err(|err| sexp_err("missing args", err))?;

    // R uses function-position lookup for symbolic call heads: non-function
    // bindings are skipped while walking enclosing environments.
    let fun_val = if fun.typeof_() == SEXPTYPE::SYMSXP {
        find_fun_result(fun, rho)?
            .or_else(|| primitive_for_symbol(fun))
            .ok_or_else(|| {
                format!("could not find function \"{}\"", unsafe {
                    get_symbol_name(fun.as_raw())
                })
            })?
    } else {
        eval_safe(fun, rho)?
    };

    // Dispatch based on function type
    match fun_val.typeof_() {
        SEXPTYPE::CLOSXP => apply_closure_safe(fun_val, e, args, rho),
        SEXPTYPE::SPECIALSXP => apply_special_safe(fun_val, e, args, rho),
        SEXPTYPE::BUILTINSXP => apply_builtin_safe(fun_val, e, args, rho),
        _ => Err(format!("cannot call type {:?}", fun_val.typeof_())),
    }
}

fn primitive_for_symbol<'a>(symbol: Sexp<'a>) -> Option<Sexp<'a>> {
    let name = unsafe { get_symbol_name(symbol.as_raw()) };
    if crate::eval::builtin::evaluated_builtin_handler(&name).is_some() {
        let primitive =
            unsafe { crate::eval::primitive::make_primitive_binding(&name, SEXPTYPE::BUILTINSXP) };
        if !primitive.is_null() && primitive != unsafe { R_NilValue() } {
            return Some(unsafe { Sexp::from_raw_unchecked(primitive) });
        }
    }
    CString::new(name.as_str())
        .ok()
        .map(|name| unsafe { crate::mainutils::names::R_Primitive(name.as_ptr()) })
        .filter(|primitive| !primitive.is_null() && *primitive != unsafe { R_NilValue() })
        .map(|primitive| unsafe { Sexp::from_raw_unchecked(primitive) })
}

/// Safe variable lookup using Sexp types.
///
/// Walks the environment chain looking for a symbol binding.
pub fn find_var_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> Option<Sexp<'a>> {
    find_var_result(symbol, rho).ok().flatten()
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

    // Walk environment chain
    let mut current = rho;
    loop {
        if !current.is_environment() {
            return Ok(None);
        }
        let frame = current
            .try_frame()
            .map_err(|err| sexp_err("environment frame lookup", err))?;
        for cell in PairlistIter::new(frame) {
            let tag = cell
                .try_tag()
                .map_err(|err| sexp_err("binding tag lookup", err))?;
            if symbol_name_bytes_equal(tag.as_raw(), symbol.as_raw()) {
                let val = cell
                    .try_car()
                    .map_err(|err| sexp_err("binding value lookup", err))?;
                if val.as_raw() == unsafe { R_MissingArg() } {
                    let name = unsafe { get_symbol_name(symbol.as_raw()) };
                    std::panic::panic_any(crate::sexp::context::RSignal::Error {
                        message: format!("argument \"{}\" is missing, with no default", name),
                    });
                }
                if val.typeof_() == SEXPTYPE::PROMSXP {
                    let forced = unsafe { forcePromise(val.as_raw()) };
                    return Sexp::try_from_raw(forced)
                        .map(Some)
                        .map_err(|err| sexp_err("forced promise value", err));
                }
                return Ok(Some(val));
            }
        }
        current = current
            .try_enclos()
            .map_err(|err| sexp_err("enclosing environment lookup", err))?;
    }
}

/// Safe promise evaluation.
fn eval_promise_safe<'a>(prom: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    // If already evaluated, return the value
    let val = prom
        .try_prvalue()
        .map_err(|err| sexp_err("promise value lookup", err))?;
    if val.as_raw() != unsafe { R_UnboundValue() } {
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
    if !fun.is_symbol() {
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
            if ctx.callflag == CTXT_RETURN && ctx.cloenv == rho {
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
            if ctx.callflag == CTXT_RETURN && ctx.cloenv == s {
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
            assert_eq!(classify_expr(expr_vec), EvalKind::ExpressionVector);
        }
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

        crate::sexp::instance::with_required_current_instance(|inst| {
            inst.eval_state.disable_bytecode = TRUE;
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
}
