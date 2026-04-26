#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

//! R's core evaluator — ports R's src/main/eval.c.
//!
//! The evaluator is the heart of the R interpreter. It handles:
//! - Expression evaluation (eval)
//! - Closure application (applyClosure)
//! - Special forms (if, while, for, repeat, function, begin, etc.)
//! - Assignment operators (<-, <<-, =)
//! - Argument evaluation and matching
//! - Method dispatch

pub(crate) mod apply;
pub(crate) mod arithmetic;
pub(crate) mod assignment;
pub(crate) mod attrib_core;
pub(crate) mod bc_eval;
pub(crate) mod bc_stack;
pub(crate) mod builtin;
pub(crate) mod bytecode;
pub(crate) mod closure;
pub(crate) mod complex_arith;
pub(crate) mod context;
pub(crate) mod defaults;
pub(crate) mod dispatch;
pub(crate) mod error;
#[allow(clippy::module_inception)]
pub mod eval;
pub(crate) mod jit;
pub(crate) mod limits;
pub(crate) mod missing;
pub(crate) mod parser;
pub(crate) mod primitive;
pub(crate) mod profiling;
pub(crate) mod special;
pub(crate) mod symbols;

pub use eval::{
    EvalContext, EvalError, EvalLimits, PrimitiveDescriptor, eval as eval_sexp, eval_expr,
    eval_safe, find_var_safe, get_eval_limits, reset_eval_limits, set_eval_limits,
};

#[cfg(test)]
mod integration_tests;
