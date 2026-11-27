//! The new-world-backed host session — the migration target for the
//! facade's evaluation core.
//!
//! `PureSession` wraps `value::Session` (the generational-heap evaluator)
//! behind the facade's `RResult` contract, so consumers can cut over
//! without changing what they consume: owned typed values, separated
//! stdout/stderr, and the chronological console transcript. Capabilities
//! the old world still owns — cancellation tokens, resource limits,
//! packages, graphics — arrive slice by slice; the evaluation core is
//! already byte-exact against the GNU oracle across the conformance
//! corpus.

use crate::value::session::Session;

use super::value_snapshot::snapshot_value;
use super::values::RResult;
use super::values::RValue;

/// A host session whose evaluation runs entirely on the new-world heap —
/// no SEXP, no slab allocator, no panic-based control flow.
pub struct PureSession {
    session: Session,
}

impl Default for PureSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PureSession {
    pub fn new() -> Self {
        PureSession {
            session: Session::new(),
        }
    }

    /// Parse and evaluate R source into the facade's result contract:
    /// the typed snapshot of the final value, the numeric scalar view,
    /// and the three output streams exactly as Rscript separates them.
    pub fn eval(&mut self, code: &str) -> RResult {
        let full = self.session.eval_full(code);
        let typed = match (&full.error, full.value) {
            (Some(message), _) => RValue::Error(message.clone()),
            (None, Some(value)) => snapshot_value(&self.session.heap, &self.session.interner, value),
            (None, None) => RValue::Null,
        };
        let value = typed.numeric_scalar_value();
        RResult {
            value,
            typed,
            output: full.output,
            stdout: full.stdout,
            stderr: full.stderr,
        }
    }

    /// Read-only access to the underlying evaluator session — migration
    /// instrumentation inspects heap state through here.
    pub fn inner(&self) -> &Session {
        &self.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_eval_matches_script_semantics() {
        let mut session = PureSession::new();
        let result = session.eval("1 + 2");
        assert_eq!(result.stdout, "[1] 3\n");
        assert_eq!(result.typed, RValue::Real(Some(3.0)));
        assert_eq!(result.value, 3.0);
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn multi_expression_scripts_accumulate_output() {
        let mut session = PureSession::new();
        let result = session.eval("x <- 10\nx\nprint(x * 2)");
        assert_eq!(result.stdout, "[1] 10\n[1] 20\n");
        assert_eq!(result.typed, RValue::Real(Some(20.0)));
    }

    #[test]
    fn errors_render_gnu_style_and_preserve_prior_output() {
        let mut session = PureSession::new();
        let result = session.eval("print(1)\nstop('boom')\nprint(2)");
        assert_eq!(result.stdout, "[1] 1\n");
        assert!(result.stderr.starts_with("Error: boom"));
        assert_eq!(result.typed, RValue::Error("boom".to_string()));
    }

    #[test]
    fn unbound_variables_render_gnu_message() {
        let mut session = PureSession::new();
        let result = session.eval("no_such_object");
        assert_eq!(
            result.typed,
            RValue::Error("object 'no_such_object' not found".to_string())
        );
    }

    #[test]
    fn warnings_separate_onto_stderr() {
        let mut session = PureSession::new();
        let result = session.eval("warning('careful')\n1");
        assert!(result.stderr.contains("careful"), "stderr: {}", result.stderr);
        assert_eq!(result.stdout, "[1] 1\n");
    }

    #[test]
    fn vectors_snapshot_through_facade_types() {
        let mut session = PureSession::new();
        let result = session.eval("c(1L, NA, 3L)");
        assert_eq!(
            result.typed,
            RValue::IntegerVector(vec![Some(1), None, Some(3)])
        );
    }

    #[test]
    fn null_evaluates_to_null() {
        let mut session = PureSession::new();
        let result = session.eval("NULL");
        assert_eq!(result.typed, RValue::Null);
    }
}
