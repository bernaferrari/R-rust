//! Android embedding layer for the rmath-rs R interpreter.
//!
//! This module provides a clean, safe API surface for embedding the R
//! interpreter in Android applications via UniFFI (or any FFI framework).
//!
//! # Design Principles
//!
//! - **No raw pointers cross the FFI boundary.** All inputs/outputs are
//!   owned Rust types (strings, numbers, Vec) or opaque handles.
//! - **Thread-safe.** Each `RSession` owns its own arena and can be
//!   used from any thread (no global state).
//! - **Minimal surface.** Only the operations needed for Android embedding
//!   are exposed — eval, print, math, ALTREP.
//! - **Zero-cost.** The safe wrappers compile down to the same operations
//!   as the internal code.

use crate::eval::parser;
use crate::sexp::RSession as CoreRSession;
use crate::sexp::builder;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, SEXPTYPE};
use crate::sexp::output;
use crate::sexp::safe::Sexp;
use crate::sexp::session::CancellationFlag;

// ---------------------------------------------------------------------------
// RSession — per-thread interpreter context
// ---------------------------------------------------------------------------

/// An isolated R interpreter session.
///
/// Each session has its own memory arena and evaluation environment.
/// This is the primary entry point for Android embedding.
///
/// # Thread Safety
///
/// `RSession` is `Send` but not `Sync`. Each thread should create its
/// own session. Internally, the arena uses `RefCell` which is not `Sync`.
pub struct RSession {
    core: CoreRSession,
}

unsafe impl Send for RSession {}

fn extract_numeric_value(s: Sexp<'_>) -> f64 {
    match s.typeof_() {
        SEXPTYPE::INTSXP => s.integer_elt(0).unwrap_or(0) as f64,
        SEXPTYPE::REALSXP => s.real_elt(0).unwrap_or(0.0),
        SEXPTYPE::LGLSXP => {
            let v = s.logical_elt(0).unwrap_or(0);
            if v == 1 {
                1.0
            } else if v == 0 {
                0.0
            } else {
                f64::NAN
            }
        }
        _ => 0.0,
    }
}

fn result_from_sexp(sexp: Sexp<'_>) -> RResult {
    RResult {
        value: extract_numeric_value(sexp),
        typed: RValue::from_sexp(sexp),
        output: output::format_sexp_direct(sexp),
    }
}

fn error_result(message: impl Into<String>) -> RResult {
    RResult {
        value: 0.0,
        typed: RValue::Error(message.into()),
        output: String::new(),
    }
    .with_error_output()
}

impl RSession {
    pub fn new() -> Self {
        RSession {
            core: CoreRSession::new(),
        }
    }

    pub fn close(&mut self) {
        self.core.close();
    }

    pub fn is_active(&self) -> bool {
        self.core.is_active()
    }

    pub fn set_cancellation_flag(&mut self, flag: Option<CancellationFlag>) {
        self.core.set_cancellation_flag(flag);
    }

    pub fn eval_integer(&mut self, value: i32) -> RResult {
        match self
            .core
            .with_arena(|arena| builder::scalar_integer_in(arena, value))
        {
            Some(Some(s)) => result_from_sexp(s),
            _ => allocation_error(),
        }
    }

    pub fn eval_real(&mut self, value: f64) -> RResult {
        match self
            .core
            .with_arena(|arena| builder::scalar_real_in(arena, value))
        {
            Some(Some(s)) => result_from_sexp(s),
            _ => allocation_error(),
        }
    }

    pub fn eval_int_vector(&mut self, values: &[i32]) -> RResult {
        match self
            .core
            .with_arena(|arena| builder::int_vec_in(arena, values))
        {
            Some(Some(s)) => result_from_sexp(s),
            _ => allocation_error(),
        }
    }

    pub fn eval_real_vector(&mut self, values: &[f64]) -> RResult {
        match self
            .core
            .with_arena(|arena| builder::real_vec_in(arena, values))
        {
            Some(Some(s)) => result_from_sexp(s),
            _ => allocation_error(),
        }
    }

    /// Compute a mathematical function on a single value.
    pub fn math_unary(&self, func: RMathFunc, x: f64) -> f64 {
        match func {
            RMathFunc::Abs => x.abs(),
            RMathFunc::Sqrt => libm::sqrt(x),
            RMathFunc::Ceil => libm::ceil(x),
            RMathFunc::Floor => libm::floor(x),
            RMathFunc::Trunc => libm::trunc(x),
            RMathFunc::Exp => libm::exp(x),
            RMathFunc::Log => libm::log(x),
            RMathFunc::Log2 => libm::log2(x),
            RMathFunc::Log10 => libm::log10(x),
            RMathFunc::Sin => libm::sin(x),
            RMathFunc::Cos => libm::cos(x),
            RMathFunc::Tan => libm::tan(x),
            RMathFunc::Asin => libm::asin(x),
            RMathFunc::Acos => libm::acos(x),
            RMathFunc::Atan => libm::atan(x),
            RMathFunc::Gamma => libm::tgamma(x),
            RMathFunc::Lgamma => libm::lgamma(x),
        }
    }

    /// Compute a Bessel function.
    pub fn bessel(&self, func: RBesselFunc, x: f64, alpha: f64, scaled: bool) -> f64 {
        match func {
            RBesselFunc::J => crate::special::bessel::bessel_j(x, alpha),
            RBesselFunc::Y => crate::special::bessel::bessel_y(x, alpha),
            RBesselFunc::I => crate::special::bessel::bessel_i(x, alpha, scaled),
            RBesselFunc::K => crate::special::bessel::bessel_k(x, alpha, scaled),
        }
    }

    /// Probability density function for the normal distribution.
    pub fn dnorm(&self, x: f64, mean: f64, sd: f64, log: bool) -> f64 {
        crate::dist::normal::dnorm(x, mean, sd, if log { 1 } else { 0 })
    }

    pub fn pnorm(&self, x: f64, mean: f64, sd: f64, lower_tail: bool, log: bool) -> f64 {
        crate::dist::normal::pnorm(
            x,
            mean,
            sd,
            if lower_tail { 1 } else { 0 },
            if log { 1 } else { 0 },
        )
    }

    pub fn qnorm(&self, p: f64, mean: f64, sd: f64, lower_tail: bool, log: bool) -> f64 {
        crate::dist::normal::qnorm(
            p,
            mean,
            sd,
            if lower_tail { 1 } else { 0 },
            if log { 1 } else { 0 },
        )
    }

    pub fn unif_rand(&self) -> f64 {
        self.core.unif_rand()
    }

    pub fn set_seed(&self, i1: u32, i2: u32) {
        self.core.set_seed(i1, i2);
    }

    pub fn norm_rand(&self) -> f64 {
        self.core.norm_rand()
    }

    /// Parse and evaluate an R expression.
    ///
    /// Parses and evaluates code against this session's isolated global
    /// environment.
    pub fn eval(&mut self, code: &str) -> RResult {
        let sexp = match self.core.with_arena(|arena| parser::parse(code, arena)) {
            Some(Ok(sexp)) => sexp,
            Some(Err(e)) => return error_result(e.to_string()),
            None => return error_result("session is closed"),
        };

        if sexp.is_null() {
            return RResult {
                value: 0.0,
                typed: RValue::Null,
                output: "NULL".to_string(),
            };
        }

        match self.core.eval(sexp) {
            Ok(result) => match Sexp::from_raw(result) {
                Some(result) => result_from_sexp(result),
                None => RResult {
                    value: 0.0,
                    typed: RValue::Null,
                    output: "NULL".to_string(),
                },
            },
            Err(e) => error_result(e.to_string()),
        }
    }
}

fn allocation_error() -> RResult {
    error_result("allocation failed")
}

impl Default for RSession {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of an R evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct RResult {
    /// Legacy numeric scalar view for existing simple callers.
    pub value: f64,
    /// Owned typed value for Android/FFI callers that should not parse output.
    pub typed: RValue,
    /// R-style display output.
    pub output: String,
}

impl RResult {
    fn with_error_output(mut self) -> Self {
        if let RValue::Error(message) = &self.typed {
            self.output = format!("Error: {message}");
        }
        self
    }
}

/// Owned representation of evaluated R values suitable for FFI boundaries.
#[derive(Debug, Clone, PartialEq)]
pub enum RValue {
    Null,
    Logical(Option<bool>),
    Integer(Option<i32>),
    Real(Option<f64>),
    LogicalVector(Vec<Option<bool>>),
    IntegerVector(Vec<Option<i32>>),
    RealVector(Vec<Option<f64>>),
    StringVector(Vec<String>),
    List(Vec<RValue>),
    Unsupported { type_name: String, display: String },
    Error(String),
}

impl RValue {
    pub fn from_sexp(sexp: Sexp<'_>) -> Self {
        let len = sexp.len();
        match sexp.typeof_() {
            SEXPTYPE::NILSXP => RValue::Null,
            SEXPTYPE::LGLSXP => {
                let values = logical_values(sexp);
                if len == 1 {
                    values
                        .into_iter()
                        .next()
                        .map(RValue::Logical)
                        .unwrap_or(RValue::Null)
                } else {
                    RValue::LogicalVector(values)
                }
            }
            SEXPTYPE::INTSXP => {
                let values = integer_values(sexp);
                if len == 1 {
                    values
                        .into_iter()
                        .next()
                        .map(RValue::Integer)
                        .unwrap_or(RValue::Null)
                } else {
                    RValue::IntegerVector(values)
                }
            }
            SEXPTYPE::REALSXP => {
                let values = real_values(sexp);
                if len == 1 {
                    values
                        .into_iter()
                        .next()
                        .map(RValue::Real)
                        .unwrap_or(RValue::Null)
                } else {
                    RValue::RealVector(values)
                }
            }
            SEXPTYPE::STRSXP => RValue::StringVector(string_values(sexp)),
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                let mut values = Vec::with_capacity(len as usize);
                for i in 0..len {
                    if let Some(value) = sexp.vector_elt(i) {
                        values.push(RValue::from_sexp(value));
                    } else {
                        values.push(RValue::Null);
                    }
                }
                RValue::List(values)
            }
            _ => RValue::Unsupported {
                type_name: sexp_type_name(sexp.typeof_()).to_string(),
                display: output::format_sexp_direct(sexp),
            },
        }
    }
}

fn logical_values(sexp: Sexp<'_>) -> Vec<Option<bool>> {
    (0..sexp.len())
        .map(|i| match sexp.logical_elt(i) {
            Some(NA_LOGICAL) | None => None,
            Some(0) => Some(false),
            Some(_) => Some(true),
        })
        .collect()
}

fn integer_values(sexp: Sexp<'_>) -> Vec<Option<i32>> {
    (0..sexp.len())
        .map(|i| match sexp.integer_elt(i) {
            Some(NA_INTEGER) | None => None,
            Some(value) => Some(value),
        })
        .collect()
}

fn real_values(sexp: Sexp<'_>) -> Vec<Option<f64>> {
    (0..sexp.len())
        .map(|i| match sexp.real_elt(i) {
            Some(value) if value.to_bits() == R_NA_BIT_PATTERN => None,
            Some(value) => Some(value),
            None => None,
        })
        .collect()
}

fn string_values(sexp: Sexp<'_>) -> Vec<String> {
    (0..sexp.len())
        .map(|i| {
            sexp.string_elt(i)
                .and_then(|chars| chars.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn sexp_type_name(t: SEXPTYPE) -> &'static str {
    match t {
        SEXPTYPE::NILSXP => "NULL",
        SEXPTYPE::SYMSXP => "symbol",
        SEXPTYPE::LISTSXP => "pairlist",
        SEXPTYPE::CLOSXP => "closure",
        SEXPTYPE::ENVSXP => "environment",
        SEXPTYPE::PROMSXP => "promise",
        SEXPTYPE::LANGSXP => "language",
        SEXPTYPE::SPECIALSXP => "special",
        SEXPTYPE::BUILTINSXP => "builtin",
        SEXPTYPE::CHARSXP => "char",
        SEXPTYPE::LGLSXP => "logical",
        SEXPTYPE::INTSXP => "integer",
        SEXPTYPE::REALSXP => "double",
        SEXPTYPE::CPLXSXP => "complex",
        SEXPTYPE::STRSXP => "character",
        SEXPTYPE::DOTSXP => "dots",
        SEXPTYPE::ANYSXP => "any",
        SEXPTYPE::VECSXP => "list",
        SEXPTYPE::EXPRSXP => "expression",
        SEXPTYPE::BCODESXP => "bytecode",
        SEXPTYPE::EXTPTRSXP => "externalptr",
        SEXPTYPE::WEAKREFSXP => "weakref",
        SEXPTYPE::RAWSXP => "raw",
        SEXPTYPE::S4SXP => "S4",
        _ => "unknown",
    }
}

/// Supported mathematical functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RMathFunc {
    Abs,
    Sqrt,
    Ceil,
    Floor,
    Trunc,
    Exp,
    Log,
    Log2,
    Log10,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Gamma,
    Lgamma,
}

/// Supported Bessel function families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RBesselFunc {
    J,
    Y,
    I,
    K,
}

// ---------------------------------------------------------------------------
// Free functions for simple operations (no session needed)
// ---------------------------------------------------------------------------

/// Compute `dnorm(x, mean, sd)`.
pub fn dnorm_free(x: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::dnorm(x, mean, sd, 0)
}

pub fn pnorm_free(x: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::pnorm(x, mean, sd, 1, 0)
}

pub fn qnorm_free(p: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::qnorm(p, mean, sd, 1, 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_new() {
        let mut session = RSession::new();
        assert_eq!(session.core.with_arena(|arena| arena.node_count()), Some(0));
    }

    #[test]
    fn test_session_eval_integer() {
        let mut session = RSession::new();
        let result = session.eval_integer(42);
        assert_eq!(result.value, 42.0);
        assert!(result.output.contains("42"));
        assert_eq!(result.typed, RValue::Integer(Some(42)));
    }

    #[test]
    fn test_session_eval_real() {
        let mut session = RSession::new();
        let result = session.eval_real(3.14);
        assert!((result.value - 3.14).abs() < f64::EPSILON);
        assert!(result.output.contains("3.14"));
    }

    #[test]
    fn test_session_eval_int_vector() {
        let mut session = RSession::new();
        let result = session.eval_int_vector(&[1, 2, 3]);
        assert!(result.output.contains("1"));
        assert_eq!(
            result.typed,
            RValue::IntegerVector(vec![Some(1), Some(2), Some(3)])
        );
    }

    #[test]
    fn test_eval_returns_owned_typed_values() {
        let mut session = RSession::new();
        let strings = session.eval("c(\"a\", \"b\")");
        let logical = session.eval("TRUE");
        let list = session.eval("list(1, \"x\")");

        assert_eq!(
            strings.typed,
            RValue::StringVector(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(logical.typed, RValue::Logical(Some(true)));
        assert_eq!(
            list.typed,
            RValue::List(vec![
                RValue::Real(Some(1.0)),
                RValue::StringVector(vec!["x".to_string()])
            ])
        );
    }

    #[test]
    fn test_session_math_unary() {
        let session = RSession::new();
        assert!((session.math_unary(RMathFunc::Sqrt, 4.0) - 2.0).abs() < 1e-10);
        assert!((session.math_unary(RMathFunc::Exp, 0.0) - 1.0).abs() < 1e-10);
        assert!(session.math_unary(RMathFunc::Log, 1.0).abs() < 1e-10);
        assert!(session.math_unary(RMathFunc::Sin, 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_session_bessel() {
        let session = RSession::new();
        let j0 = session.bessel(RBesselFunc::J, 0.0, 0.0, false);
        assert!((j0 - 1.0).abs() < 1e-10);

        let i0 = session.bessel(RBesselFunc::I, 0.0, 0.0, false);
        assert!((i0 - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_session_dnorm_pnorm() {
        let session = RSession::new();
        let d = session.dnorm(0.0, 0.0, 1.0, false);
        assert!((d - 0.3989422804014327).abs() < 1e-10);

        let p = session.pnorm(0.0, 0.0, 1.0, true, false);
        assert!((p - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_free_functions() {
        assert!((dnorm_free(0.0, 0.0, 1.0) - 0.3989422804014327).abs() < 1e-10);
        assert!((pnorm_free(0.0, 0.0, 1.0) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_eval_integer_literal() {
        let mut session = RSession::new();
        let result = session.eval("42");
        assert!(
            (result.value - 42.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_real_literal() {
        let mut session = RSession::new();
        let result = session.eval("3.14");
        assert!(
            (result.value - 3.14).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_addition() {
        let mut session = RSession::new();
        let result = session.eval("1 + 2");
        assert!(
            (result.value - 3.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_subtraction() {
        let mut session = RSession::new();
        let result = session.eval("10 - 3");
        assert!(
            (result.value - 7.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_multiplication() {
        let mut session = RSession::new();
        let result = session.eval("4 * 5");
        assert!(
            (result.value - 20.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_division() {
        let mut session = RSession::new();
        let result = session.eval("10 / 3");
        assert!(
            (result.value - 3.3333333333333335).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_power() {
        let mut session = RSession::new();
        let result = session.eval("2 ^ 10");
        assert!(
            (result.value - 1024.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_precedence() {
        let mut session = RSession::new();
        let result = session.eval("2 + 3 * 4");
        assert!(
            (result.value - 14.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_comparison() {
        let mut session = RSession::new();
        let lt = session.eval("1 < 2");
        assert!((lt.value - 1.0).abs() < 1e-10);

        let gt = session.eval("3 > 5");
        assert!((gt.value - 0.0).abs() < 1e-10);

        let eq = session.eval("7 == 7");
        assert!((eq.value - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_eval_assignment() {
        let mut session = RSession::new();
        let assign_result = session.eval("x <- 42");
        assert!(
            !assign_result.output.contains("Error"),
            "assign failed: {}",
            assign_result.output
        );

        let lookup = session.eval("x");
        assert!(
            !lookup.output.contains("Error"),
            "lookup failed: {}",
            lookup.output
        );
    }

    #[test]
    fn test_eval_multiple_expressions_returns_last_value() {
        let mut session = RSession::new();
        let result = session.eval("x <- c(10, 20, 30)\nx");
        assert_eq!(result.output, "[1] 10 20 30");
    }

    #[test]
    fn test_eval_integer_subassignment() {
        let mut session = RSession::new();
        let result = session.eval("x <- c(10, 20, 30)\nx[2] <- 99\nx");
        assert_eq!(result.output, "[1] 10 99 30");
    }

    #[test]
    fn test_eval_named_vector_names() {
        let mut session = RSession::new();
        let result = session.eval("names(c(a = 1, b = 2))");
        assert_eq!(result.output, "[1] \"a\" \"b\"");
    }

    #[test]
    fn test_eval_lapply_prints_list() {
        let mut session = RSession::new();
        let result = session.eval("lapply(c(1, 2), function(x) x + 1)");
        assert_eq!(result.output, "[[1]]\n[1] 2\n\n[[2]]\n[1] 3");
    }

    #[test]
    fn test_eval_dollar_list_access() {
        let mut session = RSession::new();
        let exact = session.eval("x <- list(a = 1, b = 2)\nx$a");
        let partial = session.eval("x <- list(alpha = 11, beta = 22)\nx$al");
        let missing = session.eval("x <- list(a = 1)\nx$b");
        assert_eq!(exact.output, "[1] 1");
        assert_eq!(partial.output, "[1] 11");
        assert_eq!(missing.output, "NULL");
    }

    #[test]
    fn test_eval_mean_numeric() {
        let mut session = RSession::new();
        let numeric = session.eval("mean(c(1, 2, 3))");
        let sequence = session.eval("mean(1:4)");
        let na = session.eval("mean(c(1, NA))");
        let na_removed = session.eval("mean(c(1, NA), na.rm = TRUE)");
        assert_eq!(numeric.output, "[1] 2");
        assert_eq!(sequence.output, "[1] 2.5");
        assert_eq!(na.output, "[1] NA");
        assert_eq!(na_removed.output, "[1] 1");
    }

    #[test]
    fn test_eval_summary_na_rm() {
        let mut session = RSession::new();
        let sum = session.eval("sum(c(1, NA, 3), na.rm = TRUE)");
        let min = session.eval("min(c(3, NA, 1), na.rm = TRUE)");
        let range = session.eval("range(c(3, NA, 1), na.rm = TRUE)");
        assert_eq!(sum.output, "[1] 4");
        assert_eq!(min.output, "[1] 1");
        assert_eq!(range.output, "[1] 1 3");
    }

    #[test]
    fn test_eval_factor_labels() {
        let mut session = RSession::new();
        let result = session.eval("x <- factor(c(\"b\", \"a\", \"b\", \"c\"))\nx");
        assert_eq!(result.output, "[1] b a b c\nLevels: a b c");
    }

    #[test]
    fn test_eval_closure_positional_arg() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x) x + 1\nf(41)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_lexical_scope() {
        let mut session = RSession::new();
        let result =
            session.eval("make <- function(x) function(y) x + y\nadd2 <- make(2)\nadd2(40)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_default_arg() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x, y = x + 1) y\nf(41)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_lazy_unused_arg() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x) 1\nf(unknown_symbol)");
        assert_eq!(result.output, "[1] 1");
    }

    #[test]
    fn test_eval_closure_named_args() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x, y) x + y\nf(y = 40, x = 2)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_return() {
        let mut session = RSession::new();
        let result = session.eval("f <- function() return(42)\nf()");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_missing_arg() {
        let mut session = RSession::new();
        let missing = session.eval("f <- function(x) missing(x)\nf()");
        let present = session.eval("f <- function(x) missing(x)\nf(1)");
        assert_eq!(missing.output, "[1] TRUE");
        assert_eq!(present.output, "[1] FALSE");
    }

    #[test]
    fn test_eval_missing_arg_error() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x) x\nf()");
        assert_eq!(
            result.output,
            "Error: argument \"x\" is missing, with no default"
        );
    }

    #[test]
    fn test_sessions_keep_globals_isolated_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        assert!(!left.eval("x <- 11").output.contains("Error"));
        assert!(!right.eval("x <- 29").output.contains("Error"));

        let left_x = left.eval("x");
        let right_x = right.eval("x");

        assert!((left_x.value - 11.0).abs() < 1e-10, "left: {left_x:?}");
        assert!((right_x.value - 29.0).abs() < 1e-10, "right: {right_x:?}");
    }

    #[test]
    fn test_parallel_sessions_keep_globals_isolated() {
        let left = std::thread::spawn(|| {
            let mut session = RSession::new();
            assert!(!session.eval("x <- 101").output.contains("Error"));
            session.eval("x").value
        });
        let right = std::thread::spawn(|| {
            let mut session = RSession::new();
            assert!(!session.eval("x <- 202").output.contains("Error"));
            session.eval("x").value
        });

        assert!((left.join().expect("left session panicked") - 101.0).abs() < 1e-10);
        assert!((right.join().expect("right session panicked") - 202.0).abs() < 1e-10);
    }

    #[test]
    fn test_sessions_keep_rng_isolated_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let left_first = left.unif_rand();
        let right_first = right.unif_rand();
        let left_second = left.unif_rand();
        let right_second = right.unif_rand();

        assert_eq!(left_first, right_first);
        assert_eq!(left_second, right_second);
        assert_ne!(left_first, left_second);
    }

    #[test]
    fn test_session_seed_is_local() {
        let left = RSession::new();
        let right = RSession::new();

        left.set_seed(10, 20);
        right.set_seed(10, 20);
        assert_eq!(left.unif_rand(), right.unif_rand());

        left.set_seed(30, 40);
        assert_ne!(left.unif_rand(), right.unif_rand());
    }

    #[test]
    fn test_eval_null() {
        let mut session = RSession::new();
        let result = session.eval("NULL");
        assert_eq!(result.output, "NULL");
    }

    #[test]
    fn test_eval_unary_minus() {
        let mut session = RSession::new();
        let result = session.eval("-5");
        assert!(
            (result.value - (-5.0)).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_complex_expr() {
        let mut session = RSession::new();
        let result = session.eval("(1 + 2) * (3 + 4)");
        assert!(
            (result.value - 21.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_true_false() {
        let mut session = RSession::new();
        let t = session.eval("TRUE");
        assert!((t.value - 1.0).abs() < 1e-10);

        let f = session.eval("FALSE");
        assert!((f.value - 0.0).abs() < 1e-10);
    }
}
