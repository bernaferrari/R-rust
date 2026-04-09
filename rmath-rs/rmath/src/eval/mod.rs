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

pub mod assignment;
pub mod attrib_core;
pub mod bc_eval;
pub mod bc_stack;
pub mod builtin;
pub mod bytecode;
pub mod closure;
pub mod context;
pub mod dispatch;
#[allow(clippy::module_inception)]
pub mod eval;
pub mod parser;
pub mod special;
pub mod symbols;

#[cfg(test)]
mod integration_tests;
