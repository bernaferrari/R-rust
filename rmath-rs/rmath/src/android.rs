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

fn result_from_eval(sexp: Sexp<'_>, captured: output::RCapturedOutput, visible: bool) -> RResult {
    let mut display = String::new();
    display.push_str(&captured.stdout);
    display.push_str(&captured.stderr);
    if visible {
        if !display.is_empty() && !display.ends_with('\n') {
            display.push('\n');
        }
        display.push_str(&output::format_sexp_direct(sexp));
    }
    RResult {
        value: extract_numeric_value(sexp),
        typed: RValue::from_sexp(sexp),
        output: display,
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

        let (result, captured, visible) = self.core.eval_with_output_capture(sexp);
        match result {
            Ok(result) => match Sexp::from_raw(result) {
                Some(result) => result_from_eval(result, captured, visible),
                None => RResult {
                    value: 0.0,
                    typed: RValue::Null,
                    output: if visible {
                        "NULL".to_string()
                    } else {
                        String::new()
                    },
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
    fn test_eval_honors_visibility_for_assignment_and_invisible() {
        let mut session = RSession::new();

        let assigned = session.eval("x <- 42");
        assert_eq!(assigned.output, "");
        assert_eq!(assigned.typed, RValue::Real(Some(42.0)));

        let invisible = session.eval("invisible(7)");
        assert_eq!(invisible.output, "");
        assert_eq!(invisible.typed, RValue::Real(Some(7.0)));

        let visible = session.eval("x");
        assert_eq!(visible.output, "[1] 42");
    }

    #[test]
    fn test_eval_captures_explicit_output_without_implicit_reprint() {
        let mut session = RSession::new();

        let cat = session.eval("cat(\"hello\")");
        assert_eq!(cat.output, "hello");
        assert_eq!(cat.typed, RValue::Null);

        let printed = session.eval("print(1)");
        assert_eq!(printed.output, "[1] 1\n");
        assert_eq!(printed.typed, RValue::Real(Some(1.0)));
    }

    #[test]
    fn test_stopifnot_is_invisible_on_success_and_errors_on_failure() {
        let mut session = RSession::new();

        let ok = session.eval("stopifnot(TRUE)");
        assert_eq!(ok.output, "");
        assert_eq!(ok.typed, RValue::Null);

        let err = session.eval("stopifnot(FALSE)");
        assert!(matches!(err.typed, RValue::Error(_)));
        assert!(err.output.contains("FALSE is not TRUE"));
    }

    #[test]
    fn test_with_visible_returns_visible_list_with_captured_flag() {
        let mut session = RSession::new();

        let visible = session.eval("withVisible(1)");
        assert_eq!(visible.output, "$value\n[1] 1\n\n$visible\n[1] TRUE");
        assert_eq!(
            visible.typed,
            RValue::List(vec![RValue::Real(Some(1.0)), RValue::Logical(Some(true))])
        );

        let invisible = session.eval("withVisible(invisible(1))");
        assert_eq!(
            invisible.output,
            "$value\n[1] 1\n\n$visible\n[1] FALSE"
        );
        assert_eq!(
            invisible.typed,
            RValue::List(vec![RValue::Real(Some(1.0)), RValue::Logical(Some(false))])
        );
    }

    #[test]
    fn test_capture_output_evaluates_expression_under_capture() {
        let mut session = RSession::new();

        let printed = session.eval("capture.output(print(1))");
        assert_eq!(printed.output, "[1] \"[1] 1\"");
        assert_eq!(printed.typed, RValue::StringVector(vec!["[1] 1".to_string()]));

        let cat = session.eval("capture.output(cat(\"hello\"))");
        assert_eq!(cat.output, "[1] \"hello\"");
        assert_eq!(cat.typed, RValue::StringVector(vec!["hello".to_string()]));
    }

    #[test]
    fn test_stop_warning_message_and_suppression() {
        let mut session = RSession::new();

        let stopped = session.eval("stop(\"boom\")");
        assert!(matches!(stopped.typed, RValue::Error(_)));
        assert_eq!(stopped.output, "Error: boom");

        let warned = session.eval("warning(\"careful\"); 1");
        assert_eq!(warned.output, "Warning message:\ncareful \n[1] 1");

        let messaged = session.eval("message(\"hi\"); 1");
        assert_eq!(messaged.output, "hi\n[1] 1");

        let suppress_warning = session.eval("suppressWarnings(warning(\"careful\")); 1");
        assert_eq!(suppress_warning.output, "[1] 1");

        let suppress_message = session.eval("suppressMessages(message(\"hi\")); 1");
        assert_eq!(suppress_message.output, "[1] 1");
    }

    #[test]
    fn test_regexpr_reports_match_attributes() {
        let mut session = RSession::new();

        let result = session.eval("regexpr(\"a\", c(\"cat\", \"dog\"))");
        assert_eq!(
            result.output,
            "[1]  2 -1\nattr(,\"match.length\")\n[1]  1 -1\nattr(,\"index.type\")\n[1] \"chars\"\nattr(,\"useBytes\")\n[1] TRUE"
        );

        let match_length = session.eval("attr(regexpr(\"a\", c(\"cat\", \"dog\")), \"match.length\")");
        assert_eq!(match_length.output, "[1]  1 -1");

        let use_bytes = session.eval("attr(regexpr(\"a\", \"cat\"), \"useBytes\")");
        assert_eq!(use_bytes.output, "[1] TRUE");
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
    fn test_eval_logical_subset_recycles_and_preserves_na() {
        let mut session = RSession::new();
        let recycled = session.eval("(1:5)[c(TRUE, FALSE)]");
        let with_na = session.eval("(1:3)[c(TRUE, NA)]");
        let longer = session.eval("(1:3)[c(TRUE, FALSE, TRUE, TRUE)]");

        assert_eq!(recycled.output, "[1] 1 3 5");
        assert_eq!(with_na.output, "[1]  1 NA  3");
        assert_eq!(longer.output, "[1]  1  3 NA");
    }

    #[test]
    fn test_eval_numeric_subset_preserves_na_and_negative_exclusion() {
        let mut session = RSession::new();
        let positive = session.eval("(1:3)[c(1, 0, NA, 4)]");
        let integer = session.eval("(1:3)[c(1L, 0L, NA_integer_, 4L)]");
        let negative = session.eval("(1:3)[-2]");

        assert_eq!(positive.output, "[1]  1 NA NA");
        assert_eq!(integer.output, "[1]  1 NA NA");
        assert_eq!(negative.output, "[1] 1 3");
    }

    #[test]
    fn test_eval_double_bracket_subset() {
        let mut session = RSession::new();
        let list_numeric = session.eval("list(a = 10, b = 20)[[2]]");
        let list_name = session.eval("list(a = 10, b = 20)[[\"b\"]]");
        let atomic = session.eval("c(10, 20, 30)[[2]]");

        assert_eq!(list_numeric.output, "[1] 20");
        assert_eq!(list_name.output, "[1] 20");
        assert_eq!(atomic.output, "[1] 20");
    }

    #[test]
    fn test_eval_sort_unique_rev() {
        let mut session = RSession::new();
        let sorted = session.eval("sort(c(3, 1, 2))");
        let sorted_desc = session.eval("sort(c(3, 1, 2), decreasing = TRUE)");
        let unique = session.eval("unique(c(1, 1, 2, 1))");
        let reversed = session.eval("rev(c(1, 2, 3))");

        assert_eq!(sorted.output, "[1] 1 2 3");
        assert_eq!(sorted_desc.output, "[1] 3 2 1");
        assert_eq!(unique.output, "[1] 1 2");
        assert_eq!(reversed.output, "[1] 3 2 1");
    }

    #[test]
    fn test_eval_match_in_set_ops_and_extrema_indices() {
        let mut session = RSession::new();
        let matched = session.eval("match(c(2, 4), c(1, 2, 3))");
        let contains = session.eval("c(2, 4) %in% c(1, 2, 3)");
        let union = session.eval("union(c(1, 2), c(2, 3))");
        let intersect = session.eval("intersect(c(1, 2), c(2, 3))");
        let setdiff = session.eval("setdiff(c(1, 2, 3), c(2, 4))");
        let setequal = session.eval("setequal(c(1, 2), c(2, 1, 1))");
        let which_min = session.eval("which.min(c(3, 1, 2))");
        let which_max = session.eval("which.max(c(3, 1, 2))");

        assert_eq!(matched.output, "[1]  2 NA");
        assert_eq!(contains.output, "[1]  TRUE FALSE");
        assert_eq!(union.output, "[1] 1 2 3");
        assert_eq!(intersect.output, "[1] 2");
        assert_eq!(setdiff.output, "[1] 1 3");
        assert_eq!(setequal.output, "[1] TRUE");
        assert_eq!(which_min.output, "[1] 2");
        assert_eq!(which_max.output, "[1] 1");
    }

    #[test]
    fn test_eval_matrix_sequence_and_logic_helpers() {
        let mut session = RSession::new();
        let any = session.eval("any(c(FALSE, TRUE))");
        let all = session.eval("all(c(TRUE, FALSE))");
        let matrix = session.eval("matrix(1:4, nrow = 2)");
        let dim = session.eval("dim(matrix(1:4, nrow = 2))");
        let nrow = session.eval("nrow(matrix(1:4, nrow = 2))");
        let ncol = session.eval("ncol(matrix(1:4, nrow = 2))");
        let diag = session.eval("diag(3)");
        let diff = session.eval("diff(c(1, 4, 9))");
        let names = session.eval("names(setNames(c(1, 2), c(\"a\", \"b\")))");
        let seq_len = session.eval("seq_len(3)");
        let seq_along = session.eval("seq_along(c(4, 5))");

        assert_eq!(any.output, "[1] TRUE");
        assert_eq!(all.output, "[1] FALSE");
        assert_eq!(
            matrix.output,
            "     [,1] [,2]\n[1,]    1    3\n[2,]    2    4"
        );
        assert_eq!(dim.output, "[1] 2 2");
        assert_eq!(nrow.output, "[1] 2");
        assert_eq!(ncol.output, "[1] 2");
        assert_eq!(
            diag.output,
            "     [,1] [,2] [,3]\n[1,]    1    0    0\n[2,]    0    1    0\n[3,]    0    0    1"
        );
        assert_eq!(diff.output, "[1] 3 5");
        assert_eq!(names.output, "[1] \"a\" \"b\"");
        assert_eq!(seq_len.output, "[1] 1 2 3");
        assert_eq!(seq_along.output, "[1] 1 2");
    }

    #[test]
    fn test_eval_environment_file_and_predicate_helpers() {
        let mut session = RSession::new();
        let missing = session.eval("exists(\"x\")");
        let assigned = session.eval("assign(\"y\", 2)\nget(\"y\")");
        let removed = session.eval("x <- 1\nrm(\"x\")\nexists(\"x\")");
        let tempdir_exists = session.eval("file.exists(tempdir())");
        let inherits = session.eval("inherits(1, \"numeric\")");
        let to_string = session.eval("toString(c(1, 2, 3))");
        let is_vector = session.eval("is.vector(c(1, 2))");
        let is_data_frame = session.eval("is.data.frame(data.frame(a = 1))");

        assert_eq!(missing.output, "[1] FALSE");
        assert_eq!(assigned.output, "[1] 2");
        assert_eq!(removed.output, "[1] FALSE");
        assert_eq!(tempdir_exists.output, "[1] TRUE");
        assert_eq!(inherits.output, "[1] TRUE");
        assert_eq!(to_string.output, "[1] \"1, 2, 3\"");
        assert_eq!(is_vector.output, "[1] TRUE");
        assert_eq!(is_data_frame.output, "[1] TRUE");
    }

    #[test]
    fn test_eval_registered_distribution_and_cumulative_helpers() {
        let mut session = RSession::new();
        let dnorm = session.eval("dnorm(0)");
        let pnorm = session.eval("pnorm(0)");
        let qnorm = session.eval("qnorm(0.5)");
        let dpois = session.eval("dpois(2, 3)");
        let dbinom = session.eval("dbinom(2, 5, 0.5)");
        let dgamma = session.eval("dgamma(2, 3)");
        let dcauchy = session.eval("dcauchy(0)");
        let cumsum = session.eval("cumsum(c(1, 2, 3))");
        let cumprod = session.eval("cumprod(c(1, 2, 3))");

        assert!((dnorm.value - 0.3989422804014327).abs() < 1e-12);
        assert_eq!(pnorm.output, "[1] 0.5");
        assert_eq!(qnorm.output, "[1] 0");
        assert!((dpois.value - 0.22404180765538775).abs() < 1e-12);
        assert!((dbinom.value - 0.3125).abs() < 1e-12);
        assert!((dgamma.value - 0.2706705664732254).abs() < 1e-12);
        assert!((dcauchy.value - 0.3183098861837907).abs() < 1e-12);
        assert_eq!(cumsum.output, "[1] 1 3 6");
        assert_eq!(cumprod.output, "[1] 1 2 6");
    }

    #[test]
    fn test_eval_raw_string_helpers() {
        let mut session = RSession::new();
        let as_raw = session.eval("as.raw(c(65, 90))");
        let char_to_raw = session.eval("charToRaw(\"AZ\")");
        let raw_to_char = session.eval("rawToChar(as.raw(c(65, 90)))");

        assert_eq!(as_raw.output, "[1] 41 5a");
        assert_eq!(char_to_raw.output, "[1] 41 5a");
        assert_eq!(raw_to_char.output, "[1] \"AZ\"");
    }

    #[test]
    fn test_eval_replacement_and_order_predicate_helpers() {
        let mut session = RSession::new();
        let names = session.eval("x <- c(1, 2)\nnames(x) <- c(\"a\", \"b\")\nnames(x)");
        let class = session.eval("x <- 1\nclass(x) <- \"foo\"\nclass(x)");
        let unsorted = session.eval("is.unsorted(c(1, 3, 2))");
        let sorted = session.eval("is.unsorted(c(1, 2, 3))");

        assert_eq!(names.output, "[1] \"a\" \"b\"");
        assert_eq!(class.output, "[1] \"foo\"");
        assert_eq!(unsorted.output, "[1] TRUE");
        assert_eq!(sorted.output, "[1] FALSE");
    }

    #[test]
    fn test_eval_additional_distribution_helpers() {
        let mut session = RSession::new();
        let cases = [
            ("dexp(1)", "[1] 0.3678794"),
            ("pexp(1)", "[1] 0.6321206"),
            ("dbeta(0.5, 2, 3)", "[1] 1.5"),
            ("pbeta(0.5, 2, 3)", "[1] 0.6875"),
            ("qbeta(0.5, 2, 3)", "[1] 0.3857276"),
            ("dt(0, 5)", "[1] 0.3796067"),
            ("pt(0, 5)", "[1] 0.5"),
            ("qt(0.5, 5)", "[1] 0"),
            ("dchisq(2, 3)", "[1] 0.2075537"),
            ("pchisq(2, 3)", "[1] 0.4275933"),
            ("qchisq(0.5, 3)", "[1] 2.365974"),
            ("dweibull(2, 3)", "[1] 0.004025552"),
            ("pweibull(2, 3)", "[1] 0.9996645"),
            ("qweibull(0.5, 3)", "[1] 0.884997"),
            ("df(1, 5, 10)", "[1] 0.4954798"),
            ("pf(1, 5, 10)", "[1] 0.5348806"),
            ("qf(0.5, 5, 10)", "[1] 0.9319332"),
            ("dnbinom(2, 5, 0.5)", "[1] 0.1171875"),
            ("pnbinom(2, 5, 0.5)", "[1] 0.2265625"),
            ("qnbinom(0.5, 5, 0.5)", "[1] 4"),
            ("dgeom(2, 0.5)", "[1] 0.125"),
            ("pgeom(2, 0.5)", "[1] 0.875"),
            ("qgeom(0.5, 0.5)", "[1] 0"),
        ];

        for (code, expected) in cases {
            assert_eq!(session.eval(code).output, expected, "{code}");
        }
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
