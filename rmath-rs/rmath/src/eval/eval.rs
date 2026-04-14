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
/// Looks up the canonical R_FunTab and returns the C function pointer at the given offset.
pub unsafe fn get_fun_tab_entry(offset: c_int) -> Option<PRIMFUN> {
    let tab = crate::mainutils::names::R_FunTab;
    let idx = offset as usize;
    if idx < tab.len() && !tab[idx].is_sentinel() {
        tab[idx].cfun
    } else {
        None
    }
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

fn eval_safe_inner<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
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

    if expr.is_symbol() {
        return find_var_safe(expr, env).ok_or_else(|| format!("object '{}' not found", expr));
    }

    if expr.is_pairlist() || expr.typeof_() == SEXPTYPE::LANGSXP {
        return eval_lang_safe(expr, env);
    }

    if expr.is_closure() {
        return Ok(expr);
    }

    if expr.typeof_() == SEXPTYPE::PROMSXP {
        return eval_promise_safe(expr, env);
    }

    if expr.typeof_() == SEXPTYPE::DOTSXP {
        return eval_dots_safe(expr, env);
    }

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

    let evaled_args =
        unsafe { super::dispatch::evalList(args.as_raw(), rho.as_raw(), call.as_raw(), -1) };

    let op_name = unsafe {
        let fun_sym = crate::sexp::accessors::CAR(call.as_raw());
        let pname = crate::sexp::accessors::PRINTNAME(fun_sym);
        if !pname.is_null() {
            let s = crate::sexp::accessors::CHAR(pname);
            if !s.is_null() {
                std::ffi::CStr::from_ptr(s)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    let tmp = match op_name.as_str() {
        "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" => unsafe {
            super::arithmetic::do_arith(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        },
        "<" | ">" | "<=" | ">=" | "==" | "!=" => unsafe {
            super::arithmetic::do_relop(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        },
        "abs" | "sqrt" | "log" | "log10" | "exp" | "ceiling" | "floor" | "sign" => unsafe {
            super::arithmetic::do_math1(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        },
        "length" => unsafe {
            super::arithmetic::do_length(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        },
        "sum" | "min" | "max" | "prod" | "range" => unsafe {
            super::arithmetic::do_summary(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        },
        "is.numeric" | "is.integer" | "is.double" | "is.logical" | "is.character" | "is.null" => unsafe {
            super::arithmetic::do_is_type(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        },
        "c" => unsafe {
            crate::mainutils::essentials::do_c(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "seq" => unsafe {
            crate::mainutils::essentials::do_seq(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rep" => unsafe {
            crate::mainutils::essentials::do_rep(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "paste" => unsafe {
            crate::mainutils::essentials::do_paste(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "paste0" => unsafe {
            crate::mainutils::essentials::do_paste0(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "cat" => unsafe {
            crate::mainutils::essentials::do_cat(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print" => unsafe {
            crate::mainutils::essentials::do_print(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "typeof" => unsafe {
            crate::mainutils::essentials::do_typeof(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.na" => unsafe {
            crate::mainutils::essentials::do_is_na(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "names" => unsafe {
            crate::mainutils::essentials::do_names(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "which" => unsafe {
            crate::mainutils::essentials::do_which(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "ifelse" => unsafe {
            crate::mainutils::essentials::do_ifelse(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "table" => unsafe {
            crate::mainutils::essentials::do_table(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.integer" => unsafe {
            crate::mainutils::essentials::do_as_integer(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.double" => unsafe {
            crate::mainutils::essentials::do_as_double(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.character" => unsafe {
            crate::mainutils::essentials::do_as_character(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.logical" => unsafe {
            crate::mainutils::essentials::do_as_logical(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.vector" => unsafe {
            crate::mainutils::essentials::do_as_vector(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.list" => unsafe {
            crate::mainutils::essentials::do_as_list(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "nchar" => unsafe {
            crate::mainutils::essentials::do_nchar(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "substr" => unsafe {
            crate::mainutils::essentials::do_substr(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "tolower" => unsafe {
            crate::mainutils::essentials::do_tolower(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "toupper" => unsafe {
            crate::mainutils::essentials::do_toupper(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "set.seed" => unsafe {
            crate::mainutils::rng_dispatch::do_set_seed(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "RNGkind" => unsafe {
            crate::mainutils::rng_dispatch::do_RNGkind(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "runif" => unsafe {
            crate::mainutils::rng_dispatch::do_runif(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rnorm" => unsafe {
            crate::mainutils::rng_dispatch::do_rnorm(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rpois" => unsafe {
            crate::mainutils::rng_dispatch::do_rpois(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rexp" => unsafe {
            crate::mainutils::rng_dispatch::do_rexp(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sample" => unsafe {
            crate::mainutils::rng_dispatch::do_sample(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "apply" => unsafe {
            crate::mainutils::essentials::do_apply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "tapply" => unsafe {
            crate::mainutils::essentials::do_tapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "mapply" => unsafe {
            crate::mainutils::essentials::do_mapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "outer" => unsafe {
            crate::mainutils::essentials::do_outer(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sweep" => unsafe {
            crate::mainutils::essentials::do_sweep(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "trimws" => unsafe {
            crate::mainutils::essentials::do_trimws(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sprintf" => unsafe {
            crate::mainutils::essentials::do_sprintf(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "gsub" => unsafe {
            crate::mainutils::essentials::do_gsub(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sub" => unsafe {
            crate::mainutils::essentials::do_sub(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "strsplit" => unsafe {
            crate::mainutils::essentials::do_strsplit(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "pmin" => unsafe {
            crate::mainutils::essentials::do_pmin(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "pmax" => unsafe {
            crate::mainutils::essentials::do_pmax(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "which.min" => unsafe {
            crate::mainutils::essentials::do_which_min(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "which.max" => unsafe {
            crate::mainutils::essentials::do_which_max(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "append" => unsafe {
            crate::mainutils::essentials::do_append(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "head" => unsafe {
            crate::mainutils::essentials::do_head(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "tail" => unsafe {
            crate::mainutils::essentials::do_tail(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "[" => unsafe {
            crate::mainutils::essentials::do_subset(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setdiff" => unsafe {
            crate::mainutils::essentials::do_setdiff(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "union" => unsafe {
            crate::mainutils::essentials::do_union(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "intersect" => unsafe {
            crate::mainutils::essentials::do_intersect(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setequal" => unsafe {
            crate::mainutils::essentials::do_setequal(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.finite" => unsafe {
            crate::mainutils::essentials::do_is_finite(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.infinite" => unsafe {
            crate::mainutils::essentials::do_is_infinite(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.nan" => unsafe {
            crate::mainutils::essentials::do_is_nan(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.matrix" => unsafe {
            crate::mainutils::essentials::do_is_matrix(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.array" => unsafe {
            crate::mainutils::essentials::do_is_array(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.list" => unsafe {
            crate::mainutils::essentials::do_is_list(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "chartr" => unsafe {
            crate::mainutils::essentials::do_chartr(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "format" => unsafe {
            crate::mainutils::essentials::do_format(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "NROW" => unsafe {
            crate::mainutils::essentials::do_NROW(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "NCOL" => unsafe {
            crate::mainutils::essentials::do_NCOL(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "lengths" => unsafe {
            crate::mainutils::essentials::do_lengths(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rownames" => unsafe {
            crate::mainutils::essentials::do_rownames(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "colnames" => unsafe {
            crate::mainutils::essentials::do_colnames(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "class" => unsafe {
            crate::mainutils::essentials::do_class_get(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "list" => unsafe {
            crate::mainutils::essentials::do_list(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "data.frame" => unsafe {
            crate::mainutils::essentials::do_data_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Names" => unsafe {
            crate::mainutils::essentials::do_Names(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "attr" => unsafe {
            crate::mainutils::essentials::do_attr(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "noquote" => unsafe {
            crate::mainutils::essentials::do_noquote(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "deparse" => unsafe {
            crate::mainutils::essentials::do_deparse(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "nargs" => unsafe {
            crate::mainutils::essentials::do_nargs(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "useMethod" => unsafe {
            crate::mainutils::essentials::do_usemethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "missing" => unsafe {
            crate::mainutils::essentials::do_missing(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "parent.frame" => unsafe {
            crate::mainutils::essentials::do_parent_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sys.call" => unsafe {
            crate::mainutils::essentials::do_sys_call(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sys.frame" => unsafe {
            crate::mainutils::essentials::do_sys_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getwd" => unsafe {
            crate::mainutils::essentials::do_getwd(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setwd" => unsafe {
            crate::mainutils::essentials::do_setwd(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "dir.exists" => unsafe {
            crate::mainutils::essentials::do_dir_exists(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "file.create" => unsafe {
            crate::mainutils::essentials::do_file_create(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "unlink" => unsafe {
            crate::mainutils::essentials::do_unlink(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "nzchar" => unsafe {
            crate::mainutils::essentials::do_nzchar(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "lapply" => unsafe {
            crate::mainutils::essentials::do_lapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sapply" => unsafe {
            crate::mainutils::essentials::do_sapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "vapply" => unsafe {
            crate::mainutils::essentials::do_vapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Map" => unsafe {
            crate::mainutils::essentials::do_map(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Filter" => unsafe {
            crate::mainutils::essentials::do_filter(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "do.call" => unsafe {
            crate::mainutils::essentials::do_do_call(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.atomic" => unsafe {
            crate::mainutils::essentials::do_is_atomic(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.recursive" => unsafe {
            crate::mainutils::essentials::do_is_recursive(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.object" => unsafe {
            crate::mainutils::essentials::do_is_object(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "file" => unsafe {
            crate::mainutils::essentials::do_file(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "url" => unsafe {
            crate::mainutils::essentials::do_url(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "close" => unsafe {
            crate::mainutils::essentials::do_close(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "flush" => unsafe {
            crate::mainutils::essentials::do_flush(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.matrix" => unsafe {
            crate::mainutils::essentials::do_print_matrix(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.list" => unsafe {
            crate::mainutils::essentials::do_print_list(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "summary" => unsafe {
            crate::mainutils::essentials::do_summary_default(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str" => unsafe {
            crate::mainutils::essentials::do_str(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.data.frame" => unsafe {
            crate::mainutils::essentials::do_as_data_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "c.list" => unsafe {
            crate::mainutils::essentials::do_c_list(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "unlist" => unsafe {
            crate::mainutils::essentials::do_unlist(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "list.get" => unsafe {
            crate::mainutils::essentials::do_list_get(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "list.set" => unsafe {
            crate::mainutils::essentials::do_list_set(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // S3 print/summary dispatch
        "print.default" => unsafe {
            crate::mainutils::essentials::do_print_default(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.data.frame" => unsafe {
            crate::mainutils::essentials::do_print_data_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.table" => unsafe {
            crate::mainutils::essentials::do_print_table(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.factor" => unsafe {
            crate::mainutils::essentials::do_print_factor(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "summary.data.frame" => unsafe {
            crate::mainutils::essentials::do_summary_data_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "format.data.frame" => unsafe {
            crate::mainutils::essentials::do_format_data_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Matrix/linear algebra
        "crossprod" => unsafe {
            crate::mainutils::essentials::do_crossprod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "tcrossprod" => unsafe {
            crate::mainutils::essentials::do_tcrossprod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "det" => unsafe {
            crate::mainutils::essentials::do_det(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "solve" => unsafe {
            crate::mainutils::essentials::do_solve(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Environment functions
        "emptyenv" => unsafe {
            crate::mainutils::essentials::do_emptyenv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "baseenv" => unsafe {
            crate::mainutils::essentials::do_baseenv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "globalenv" => unsafe {
            crate::mainutils::essentials::do_globalenv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "new.env" => unsafe {
            crate::mainutils::essentials::do_new_env(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "environment" => unsafe {
            crate::mainutils::essentials::do_environment(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "lockBinding" => unsafe {
            crate::mainutils::essentials::do_lockBinding(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "unlockBinding" => unsafe {
            crate::mainutils::essentials::do_unlockBinding(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "bindingIsLocked" => unsafe {
            crate::mainutils::essentials::do_bindingIsLocked(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "makeActiveBinding" => unsafe {
            crate::mainutils::essentials::do_makeActiveBinding(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "lockEnvironment" => unsafe {
            crate::mainutils::essentials::do_lockEnvironment(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "environmentIsLocked" => unsafe {
            crate::mainutils::essentials::do_environmentIsLocked(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // R runtime essentials
        "version" => unsafe {
            crate::mainutils::essentials::do_version(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "R.version" => unsafe {
            crate::mainutils::essentials::do_R_version(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "args" => unsafe {
            crate::mainutils::essentials::do_args(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "formals" => unsafe {
            crate::mainutils::essentials::do_formals(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "body" => unsafe {
            crate::mainutils::essentials::do_body(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // String/vector completion
        "charmatch" => unsafe {
            crate::mainutils::essentials::do_charmatch(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "pmatch" => unsafe {
            crate::mainutils::essentials::do_pmatch(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "strtoi" => unsafe {
            crate::mainutils::essentials::do_strtoi(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "strtrim" => unsafe {
            crate::mainutils::essentials::do_strtrim(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Math2 builtins
        "round" => unsafe {
            crate::mainutils::essentials::do_round(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "signif" => unsafe {
            crate::mainutils::essentials::do_signif(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "trunc" => unsafe {
            crate::mainutils::essentials::do_trunc(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "log2" => unsafe {
            crate::mainutils::essentials::do_log2(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // R runtime
        "eval" => unsafe {
            crate::mainutils::essentials::do_eval(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "substitute" => unsafe {
            crate::mainutils::essentials::do_substitute(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "quote" => unsafe {
            crate::mainutils::essentials::do_quote(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "parse" => unsafe {
            crate::mainutils::essentials::do_parse(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Error system
        "conditionMessage" => unsafe {
            crate::mainutils::essentials::do_conditionMessage(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "conditionCall" => unsafe {
            crate::mainutils::essentials::do_conditionCall(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "simpleError" => unsafe {
            crate::mainutils::essentials::do_simpleError(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "simpleWarning" => unsafe {
            crate::mainutils::essentials::do_simpleWarning(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "withRestarts" => unsafe {
            crate::mainutils::essentials::do_withRestarts(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // S3/S4
        "isS4" => unsafe {
            crate::mainutils::essentials::do_isS4(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is" => unsafe {
            crate::mainutils::essentials::do_is(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "S3_class" => unsafe {
            crate::mainutils::essentials::do_S3_class(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setClass" => unsafe {
            crate::mainutils::essentials::do_setClass(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setValidity" => unsafe {
            crate::mainutils::essentials::do_setValidity(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "isVirtualClass" => unsafe {
            crate::mainutils::essentials::do_isVirtualClass(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // S4 class system
        "new" => unsafe {
            crate::mainutils::essentials::do_new(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "show" => unsafe {
            crate::mainutils::essentials::do_show(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "slotNames" => unsafe {
            crate::mainutils::essentials::do_slotNames(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "slot" => unsafe {
            crate::mainutils::essentials::do_slot(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "set_slot" => unsafe {
            crate::mainutils::essentials::do_set_slot(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "extends" => unsafe {
            crate::mainutils::essentials::do_extends(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "isSealedClass" => unsafe {
            crate::mainutils::essentials::do_isSealedClass(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sealClass" => unsafe {
            crate::mainutils::essentials::do_sealClass(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "representation" => unsafe {
            crate::mainutils::essentials::do_representation(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "containsClass" => unsafe {
            crate::mainutils::essentials::do_containsClass(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "possibleExtends" => unsafe {
            crate::mainutils::essentials::do_possibleExtends(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setReplaceMethod" => unsafe {
            crate::mainutils::essentials::do_setReplaceMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getMethod" => unsafe {
            crate::mainutils::essentials::do_getMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "removeGeneric" => unsafe {
            crate::mainutils::essentials::do_removeGeneric(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "removeMethod" => unsafe {
            crate::mainutils::essentials::do_removeMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "isGeneric" => unsafe {
            crate::mainutils::essentials::do_isGeneric(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "isMethod" => unsafe {
            crate::mainutils::essentials::do_isMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "findMethod" => unsafe {
            crate::mainutils::essentials::do_findMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "findMethods" => unsafe {
            crate::mainutils::essentials::do_findMethods(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "showMethods" => unsafe {
            crate::mainutils::essentials::do_showMethods(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getGenerics" => unsafe {
            crate::mainutils::essentials::do_getGenerics(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getMethods" => unsafe {
            crate::mainutils::essentials::do_getMethods(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "existsMethod" => unsafe {
            crate::mainutils::essentials::do_existsMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "hasMethod" => unsafe {
            crate::mainutils::essentials::do_hasMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "selectMethod" => unsafe {
            crate::mainutils::essentials::do_selectMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // I/O
        "scan" => unsafe {
            crate::mainutils::essentials::do_scan(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "write.table" => unsafe {
            crate::mainutils::essentials::do_write_table(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "readLines" => unsafe {
            crate::mainutils::essentials::do_readLines(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "writeLines" => unsafe {
            crate::mainutils::essentials::do_writeLines(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sink" => unsafe {
            crate::mainutils::essentials::do_sink(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Data manipulation
        "order" => unsafe {
            crate::mainutils::essentials::do_order(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rank" => unsafe {
            crate::mainutils::essentials::do_rank(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "duplicated" => unsafe {
            crate::mainutils::essentials::do_duplicated(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "anyDuplicated" => unsafe {
            crate::mainutils::essentials::do_anyDuplicated(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "match" => unsafe {
            crate::mainutils::essentials::do_match(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "findInterval" => unsafe {
            crate::mainutils::essentials::do_findInterval(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "cut" => unsafe {
            crate::mainutils::essentials::do_cut(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // String operations
        "startsWith" => unsafe {
            crate::mainutils::essentials::do_startsWith(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "endsWith" => unsafe {
            crate::mainutils::essentials::do_endsWith(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_pad" => unsafe {
            crate::mainutils::essentials::do_str_pad(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_count" => unsafe {
            crate::mainutils::essentials::do_str_count(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_replace" => unsafe {
            crate::mainutils::essentials::do_str_replace(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // R runtime type checks
        "is.language" => unsafe {
            crate::mainutils::essentials::do_is_language(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.call" => unsafe {
            crate::mainutils::essentials::do_is_call(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.symbol" => unsafe {
            crate::mainutils::essentials::do_is_symbol(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.name" => unsafe {
            crate::mainutils::essentials::do_is_name(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.pairlist" => unsafe {
            crate::mainutils::essentials::do_is_pairlist(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.function" => unsafe {
            crate::mainutils::essentials::do_is_function(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.expression" => unsafe {
            crate::mainutils::essentials::do_is_expression(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.environment" => unsafe {
            crate::mainutils::essentials::do_is_environment(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // S3
        "setOldClass" => unsafe {
            crate::mainutils::essentials::do_setOldClass(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "methods" => unsafe {
            crate::mainutils::essentials::do_methods(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Matrix
        "lower.tri" => unsafe {
            crate::mainutils::essentials::do_lower_tri(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "upper.tri" => unsafe {
            crate::mainutils::essentials::do_upper_tri(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Math/Statistics
        "cov" => unsafe {
            crate::mainutils::essentials::do_cov(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "cor" => unsafe {
            crate::mainutils::essentials::do_cor(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "scale" => unsafe {
            crate::mainutils::essentials::do_scale(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "rle" => unsafe {
            crate::mainutils::essentials::do_rle(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "inverse.rle" => unsafe {
            crate::mainutils::essentials::do_inverse_rle(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Matrix
        "which_array" => unsafe {
            crate::mainutils::essentials::do_which_array(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // R runtime
        "commandArgs" => unsafe {
            crate::mainutils::essentials::do_commandArgs(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getOption" => unsafe {
            crate::mainutils::essentials::do_getOption(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "options" => unsafe {
            crate::mainutils::essentials::do_options(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "interactive" => unsafe {
            crate::mainutils::essentials::do_interactive(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is_interactive" => unsafe {
            crate::mainutils::essentials::do_is_interactive(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getRversion" => unsafe {
            crate::mainutils::essentials::do_getRversion(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "R.version.string" => unsafe {
            crate::mainutils::essentials::do_R_version_string(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "R.Version" => unsafe {
            crate::mainutils::essentials::do_R_Version(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // List operations
        "list.append" => unsafe {
            crate::mainutils::essentials::do_list_append(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "list.prepend" => unsafe {
            crate::mainutils::essentials::do_list_prepend(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "compact" => unsafe {
            crate::mainutils::essentials::do_compact(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "keep" => unsafe {
            crate::mainutils::essentials::do_keep(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "discard" => unsafe {
            crate::mainutils::essentials::do_discard(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // String operations
        "str_detect" => unsafe {
            crate::mainutils::essentials::do_str_detect(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_extract" => unsafe {
            crate::mainutils::essentials::do_str_extract(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete data operations
        "reshape" => unsafe {
            crate::mainutils::essentials::do_reshape(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "complete.cases" => unsafe {
            crate::mainutils::essentials::do_complete_cases(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "na.omit" => unsafe {
            crate::mainutils::essentials::do_na_omit(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "na.exclude" => unsafe {
            crate::mainutils::essentials::do_na_exclude(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is_complete" => unsafe {
            crate::mainutils::essentials::do_is_complete(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete string/vector
        "str_interp" => unsafe {
            crate::mainutils::essentials::do_str_interp(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_wrap" => unsafe {
            crate::mainutils::essentials::do_str_wrap(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "path_package" => unsafe {
            crate::mainutils::essentials::do_path_package(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "system.file" => unsafe {
            crate::mainutils::essentials::do_system_file(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime
        "ls_args" => unsafe {
            crate::mainutils::essentials::do_ls_args(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "deparse1" => unsafe {
            crate::mainutils::essentials::do_deparse1(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "dput" => unsafe {
            crate::mainutils::essentials::do_dput(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "dget" => unsafe {
            crate::mainutils::essentials::do_dget(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "bquote" => unsafe {
            crate::mainutils::essentials::do_bquote(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete S3
        "rownames_to_column" => unsafe {
            crate::mainutils::essentials::do_rownames_to_column(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "column_to_rownames" => unsafe {
            crate::mainutils::essentials::do_column_to_rownames(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "relocate" => unsafe {
            crate::mainutils::essentials::do_relocate(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete I/O
        "cat_args" => unsafe {
            crate::mainutils::essentials::do_cat_args(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "message_args" => unsafe {
            crate::mainutils::essentials::do_message_args(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "packageStartupMessage" => unsafe {
            crate::mainutils::essentials::do_package_startup_message(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Environment completion
        "parent.env" => unsafe {
            crate::mainutils::essentials::do_parent_env(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "set_parent.env" => unsafe {
            crate::mainutils::essentials::do_set_parent_env(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "env_name" => unsafe {
            crate::mainutils::essentials::do_env_name(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "environmentName" => unsafe {
            crate::mainutils::essentials::do_environment_name(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is_empty" => unsafe {
            crate::mainutils::essentials::do_is_empty(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // S3 print dispatch
        "print.integer" => unsafe {
            crate::mainutils::essentials::do_print_integer(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.numeric" => unsafe {
            crate::mainutils::essentials::do_print_numeric(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.logical" => unsafe {
            crate::mainutils::essentials::do_print_logical(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.character" => unsafe {
            crate::mainutils::essentials::do_print_character(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.complex" => unsafe {
            crate::mainutils::essentials::do_print_complex(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.function" => unsafe {
            crate::mainutils::essentials::do_print_function(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.environment" => unsafe {
            crate::mainutils::essentials::do_print_environment(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.formula" => unsafe {
            crate::mainutils::essentials::do_print_formula(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.call" => unsafe {
            crate::mainutils::essentials::do_print_call(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.pairlist" => unsafe {
            crate::mainutils::essentials::do_print_pairlist(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "print.raw" => unsafe {
            crate::mainutils::essentials::do_print_raw(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // S3 summary dispatch
        "summary.numeric" => unsafe {
            crate::mainutils::essentials::do_summary_numeric(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "summary.integer" => unsafe {
            crate::mainutils::essentials::do_summary_integer(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "summary.logical" => unsafe {
            crate::mainutils::essentials::do_summary_logical(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "summary.character" => unsafe {
            crate::mainutils::essentials::do_summary_character(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // R runtime type checks
        "is.single" => unsafe {
            crate::mainutils::essentials::do_is_single(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.vector" => unsafe {
            crate::mainutils::essentials::do_is_vector(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.scalar" => unsafe {
            crate::mainutils::essentials::do_is_scalar(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.named" => unsafe {
            crate::mainutils::essentials::do_is_named(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.unsorted" => unsafe {
            crate::mainutils::essentials::do_is_unsorted(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.loaded" => unsafe {
            crate::mainutils::essentials::do_is_loaded(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Function type checks
        "is.primitive" => unsafe {
            crate::mainutils::essentials::do_is_primitive(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.generic" => unsafe {
            crate::mainutils::essentials::do_is_generic(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Data frame checks
        "is.data.frame" => unsafe {
            crate::mainutils::essentials::do_is_data_frame(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete S3 coercion
        "as.complex" => unsafe {
            crate::mainutils::essentials::do_as_complex(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.raw" => unsafe {
            crate::mainutils::essentials::do_as_raw(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as" => unsafe {
            crate::mainutils::essentials::do_as(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete I/O
        "capture.output" => unsafe {
            crate::mainutils::essentials::do_capture_output(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "withVisible" => unsafe {
            crate::mainutils::essentials::do_with_visible(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "invisible" => unsafe {
            crate::mainutils::essentials::do_invisible(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "suppressWarnings" => unsafe {
            crate::mainutils::essentials::do_suppress_warnings(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "suppressMessages" => unsafe {
            crate::mainutils::essentials::do_suppress_messages(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "force" => unsafe {
            crate::mainutils::essentials::do_force(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime
        "isTRUE" => unsafe {
            crate::mainutils::essentials::do_is_true(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "isFALSE" => unsafe {
            crate::mainutils::essentials::do_is_false(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "anyNA" => unsafe {
            crate::mainutils::essentials::do_any_na(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "allNA" => unsafe {
            crate::mainutils::essentials::do_all_na(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "anyNaN" => unsafe {
            crate::mainutils::essentials::do_any_nan(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "allNaN" => unsafe {
            crate::mainutils::essentials::do_all_nan(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete list operations
        "modifyList" => unsafe {
            crate::mainutils::essentials::do_modify_list(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "splice" => unsafe {
            crate::mainutils::essentials::do_splice(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "flatten" => unsafe {
            crate::mainutils::essentials::do_flatten(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "split" => unsafe {
            crate::mainutils::essentials::do_split(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "melt" => unsafe {
            crate::mainutils::essentials::do_melt(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "cast" => unsafe {
            crate::mainutils::essentials::do_cast(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — with/within/transform
        "with" => unsafe {
            crate::mainutils::essentials::do_with(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "within" => unsafe {
            crate::mainutils::essentials::do_within(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "transform" => unsafe {
            crate::mainutils::essentials::do_transform(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete base R — table operations, factors, aggregation
        "prop.table" => unsafe {
            crate::mainutils::essentials::do_prop_table(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "addmargins" => unsafe {
            crate::mainutils::essentials::do_addmargins(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "ftable" => unsafe {
            crate::mainutils::essentials::do_ftable(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "xtabs" => unsafe {
            crate::mainutils::essentials::do_xtabs(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "aggregate" => unsafe {
            crate::mainutils::essentials::do_aggregate(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "ave" => unsafe {
            crate::mainutils::essentials::do_ave(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "by" => unsafe {
            crate::mainutils::essentials::do_by(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "interaction" => unsafe {
            crate::mainutils::essentials::do_interaction(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "relevel" => unsafe {
            crate::mainutils::essentials::do_relevel(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "factor" => unsafe {
            crate::mainutils::essentials::do_factor(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.factor" => unsafe {
            crate::mainutils::essentials::do_is_factor(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "is.ordered" => unsafe {
            crate::mainutils::essentials::do_is_ordered(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "levels" => unsafe {
            crate::mainutils::essentials::do_levels(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "nlevels" => unsafe {
            crate::mainutils::essentials::do_nlevels(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete string operations — str_locate, str_sub
        "str_locate" => unsafe {
            crate::mainutils::essentials::do_str_locate(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_locate_all" => unsafe {
            crate::mainutils::essentials::do_str_locate_all(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_sub" => unsafe {
            crate::mainutils::essentials::do_str_sub(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "str_sub_all" => unsafe {
            crate::mainutils::essentials::do_str_sub_all(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — Sys.* functions, R.home
        "R.home" => unsafe {
            crate::mainutils::essentials::do_R_home(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.getenv" => unsafe {
            crate::mainutils::essentials::do_Sys_getenv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.setenv" => unsafe {
            crate::mainutils::essentials::do_Sys_setenv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.unsetenv" => unsafe {
            crate::mainutils::essentials::do_Sys_unsetenv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.time" => unsafe {
            crate::mainutils::essentials::do_Sys_time(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.sleep" => unsafe {
            crate::mainutils::essentials::do_Sys_sleep(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.Date" => unsafe {
            crate::mainutils::essentials::do_Sys_Date(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.timezone" => unsafe {
            crate::mainutils::essentials::do_Sys_timezone(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.localeconv" => unsafe {
            crate::mainutils::essentials::do_Sys_localeconv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.getlocale" => unsafe {
            crate::mainutils::essentials::do_Sys_getlocale(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Sys.setlocale" => unsafe {
            crate::mainutils::essentials::do_Sys_setlocale(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete data operations — subset
        "subset" => unsafe {
            crate::mainutils::essentials::do_subset_named(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete I/O — enhanced cat, message, warning
        "cat_enhanced" => unsafe {
            crate::mainutils::essentials::do_cat_enhanced(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "message_enhanced" => unsafe {
            crate::mainutils::essentials::do_message_enhanced(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "warning_enhanced" => unsafe {
            crate::mainutils::essentials::do_warning_enhanced(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — match.call, sys.nframe, sys.function, on.exit
        "match.call" => unsafe {
            crate::mainutils::essentials::do_match_call(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sys.nframe" => unsafe {
            crate::mainutils::essentials::do_sys_nframe(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sys.function" => unsafe {
            crate::mainutils::essentials::do_sys_function(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "on.exit" => unsafe {
            crate::mainutils::essentials::do_on_exit(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete I/O — read.csv, write.csv, read.table
        "read.csv" => unsafe {
            crate::mainutils::essentials::do_read_csv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "write.csv" => unsafe {
            crate::mainutils::essentials::do_write_csv(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "read.table" => unsafe {
            crate::mainutils::essentials::do_read_table(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete S3 generics — as.matrix, as.numeric
        "as.matrix" => unsafe {
            crate::mainutils::essentials::do_as_matrix(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "as.numeric" => unsafe {
            crate::mainutils::essentials::do_as_numeric(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — par, getGraphicsEvent
        "par" => unsafe {
            crate::mainutils::essentials::do_par(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "getGraphicsEvent" => unsafe {
            crate::mainutils::essentials::do_getGraphicsEvent(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
        "Rprof" => unsafe {
            crate::mainutils::essentials::do_Rprof(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "Rprofmem" => unsafe {
            crate::mainutils::essentials::do_Rprofmem(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "gc" => unsafe {
            crate::mainutils::essentials::do_gc(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "gcinfo" => unsafe {
            crate::mainutils::essentials::do_gcinfo(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "memory.size" => unsafe {
            crate::mainutils::essentials::do_memory_size(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "object.size" => unsafe {
            crate::mainutils::essentials::do_object_size(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete I/O — European CSV, delimited, fixed-width
        "read.csv2" => unsafe {
            crate::mainutils::essentials::do_read_csv2(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "write.csv2" => unsafe {
            crate::mainutils::essentials::do_write_csv2(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "read.delim" => unsafe {
            crate::mainutils::essentials::do_read_delim(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "read.fwf" => unsafe {
            crate::mainutils::essentials::do_read_fwf(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "readChar" => unsafe {
            crate::mainutils::essentials::do_readChar(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "writeChar" => unsafe {
            crate::mainutils::essentials::do_writeChar(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete S3 — method dispatch
        "getS3method" => unsafe {
            crate::mainutils::essentials::do_getS3method(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "hasS3method" => unsafe {
            crate::mainutils::essentials::do_hasS3method(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "registerS3method" => unsafe {
            crate::mainutils::essentials::do_registerS3method(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setGeneric" => unsafe {
            crate::mainutils::essentials::do_setGeneric(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "setMethod" => unsafe {
            crate::mainutils::essentials::do_setMethod(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — serialization
        "Random.seed" => unsafe {
            crate::mainutils::essentials::do_Random_seed(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "loadRDS" => unsafe {
            crate::mainutils::essentials::do_loadRDS(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "saveRDS" => unsafe {
            crate::mainutils::essentials::do_saveRDS(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — parallel operations
        "mclapply" => unsafe {
            crate::mainutils::essentials::do_mclapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "future_lapply" => unsafe {
            crate::mainutils::essentials::do_future_lapply(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "foreach" => unsafe {
            crate::mainutils::essentials::do_foreach(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete error handling — calling handlers and restarts
        "withCallingHandlers" => unsafe {
            crate::mainutils::essentials::do_withCallingHandlers(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "computeRestarts" => unsafe {
            crate::mainutils::essentials::do_computeRestarts(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "findRestart" => unsafe {
            crate::mainutils::essentials::do_findRestart(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "restarts" => unsafe {
            crate::mainutils::essentials::do_restarts(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete package system
        "library" => unsafe {
            crate::mainutils::essentials::do_library(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "require" => unsafe {
            crate::mainutils::essentials::do_require(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "installed.packages" => unsafe {
            crate::mainutils::essentials::do_installed_packages(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "find.package" => unsafe {
            crate::mainutils::essentials::do_find_package(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        // Complete R runtime — source, demo, example
        "source" => unsafe {
            crate::mainutils::essentials::do_source(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "sys.source" => unsafe {
            crate::mainutils::essentials::do_sys_source(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "demo" => unsafe {
            crate::mainutils::essentials::do_demo(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        "example" => unsafe {
            crate::mainutils::essentials::do_example(
                call.as_raw(),
                fun.as_raw(),
                evaled_args,
                rho.as_raw(),
            )
        },
        _ => {
            if let Some(primfun) = unsafe { get_primfun(fun.as_raw()) } {
                unsafe { primfun(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw()) }
            } else {
                eprintln!("Warning: builtin function '{}' not implemented", op_name);
                unsafe { R_NilValue() }
            }
        }
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
#[must_use]
#[unsafe(no_mangle)]
pub unsafe fn Rf_eval(e: SEXP, rho: SEXP) -> SEXP {
    set_R_Visible(TRUE);

    match (Sexp::from_raw(e), Sexp::from_raw(rho)) {
        (Some(expr), Some(env)) => match eval_safe(expr, env) {
            Ok(result) => result.as_raw(),
            Err(msg) => {
                std::panic::panic_any(crate::sexp::context::RSignal::Error { message: msg });
            }
        },
        _ => R_NilValue(),
    }
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

// ---------------------------------------------------------------------------
// do_withVisible -- evaluate and return list(value, visible)
// ---------------------------------------------------------------------------

/// Evaluate expression and return `list(value = <result>, visible = <flag>)`.
///
/// Ported from R's `do_withVisible()` in eval.c.
/// This is a special `.Internal`.
pub unsafe fn do_withVisible(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::attrib_core::{R_NamesSymbol, setAttrib};
    use crate::sexp::accessors::{CAR, SET_STRING_ELT, SET_VECTOR_ELT};
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_mkChar};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_Visible;
    use crate::sexp::protect::{Rf_protect, Rf_unprotect};
    use std::os::raw::c_char;

    unsafe {
        let x = Rf_eval(CAR(args), rho);
        Rf_protect(x);

        let ret = Rf_allocVector(SEXPTYPE::VECSXP.0, 2);
        Rf_protect(ret);

        let nm = Rf_allocVector(SEXPTYPE::STRSXP.0, 2);
        Rf_protect(nm);

        SET_STRING_ELT(nm, 0, Rf_mkChar(b"value\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"visible\0".as_ptr() as *const c_char));

        SET_VECTOR_ELT(ret, 0, x);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarLogical(R_Visible()));

        setAttrib(ret, R_NamesSymbol(), nm);

        Rf_unprotect(3);
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
pub unsafe fn do_recall(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::closure::applyClosure;
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CAR, TYPEOF};
    use crate::sexp::context::{R_GlobalContext, ctxt_flags::CTXT_RETURN};
    use crate::sexp::envir::findFun;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::{Rf_protect, Rf_unprotect};
    use std::os::raw::c_char;

    unsafe {
        let mut cptr = R_GlobalContext();

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
        let s = (*R_GlobalContext()).sysparent;

        // Walk context stack again to find the closure context for sysparent
        let mut cptr2 = R_GlobalContext();
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
            } else if TYPEOF(CAR(ctx.call)) == SEXPTYPE::SYMSXP.0 {
                findFun(CAR(ctx.call), ctx.sysparent)
            } else {
                Rf_eval(CAR(ctx.call), ctx.sysparent)
            }
        };

        Rf_protect(fun);

        if TYPEOF(fun) != SEXPTYPE::CLOSXP.0 {
            Rf_unprotect(1);
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
        Rf_unprotect(1);
        ans
    }
}
