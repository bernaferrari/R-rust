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
/// Internal typed dispatcher. Safe callers enter through `RSession`, which
/// activates and validates the owner. Calling this dispatcher directly requires
/// the explicit owner, rooting and payload-loan contract of `eval`.
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
    /// # Safety
    /// Activate the live owner of all inputs and retain their reachable graphs
    /// through allocation and R reentry. No Rust payload loan may cross execution.
    pub unsafe fn eval(self, expr: Sexp<'a>) -> Result<Sexp<'a>, String> {
        if !self.env.clone().is_owner_scoped() {
            return Err("eval context environment is not owner-scoped".to_string());
        }
        if !expr.clone().is_owner_scoped() {
            return Err("eval expression is not owner-scoped".to_string());
        }
        unsafe {
            /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
            eval_expr(expr, self.env)
        }
    }
}

mod builtins;
mod entry;
mod lookup;

pub use builtins::*;
pub use entry::*;
pub use lookup::*;

#[cfg(test)]
mod tests_a;
#[cfg(test)]
mod tests_b;
#[cfg(test)]
mod tests_c;
#[cfg(test)]
mod tests_d;
