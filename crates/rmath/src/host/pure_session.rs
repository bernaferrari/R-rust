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

    /// Evaluate and return output exactly as the facade's string eval —
    /// the fire-and-forget shape the CLI's history plumbing consumes.
    pub fn eval_output(&mut self, code: &str) -> Result<String, String> {
        let full = self.session.eval_full(code);
        match full.error {
            Some(message) => Err(message),
            None => Ok(full.output),
        }
    }

    /// The facade's EvalOutput shape — output transcript plus the typed
    /// final value exactly as `eval_result_cancellable` answers it. The
    /// pure world has no cancellation machinery yet; Ctrl-C during
    /// evaluation is the documented follow-up slice.
    pub fn eval_result(
        &mut self,
        code: &str,
    ) -> Result<super::facade::EvalOutput, super::facade::RSessionError> {
        let result = self.eval(code);
        match result.typed {
            RValue::Error(message) => Err(super::facade::RSessionError::EvalError(message)),
            value => Ok(super::facade::EvalOutput {
                output: result.output,
                value,
            }),
        }
    }

    /// Evaluate with the host's cancellation token observing SIGINT —
    /// the facade's cancellable entry point exactly. The token cell rides
    /// the heap for the evaluation's duration and clears afterward so a
    /// later non-cancellable eval never inherits a stale watch.
    pub fn eval_result_cancellable(
        &mut self,
        code: &str,
        cancellation: &super::facade::HostCancellationToken,
    ) -> Result<super::facade::EvalOutput, super::facade::RSessionError> {
        self.session.heap.cancellation = Some(cancellation.shared_flag());
        let result = self.eval_result(code);
        self.session.heap.cancellation = None;
        result
    }

    /// Whether `code` is a syntactically complete R input — incomplete
    /// means the parser reached end-of-input mid-construct exactly as
    /// GNU's continuation-prompt discipline detects.
    pub fn is_input_complete(&mut self, code: &str) -> Result<bool, String> {
        use crate::value::parser::parse_native_expressions;
        // Parse against the scratch state of the live heap: incomplete
        // input never mutates the environment, and a complete input's
        // parse artifacts collect with the next evaluation exactly as
        // GNU's parser allocations do.
        match parse_native_expressions(code, &mut self.session.heap, &mut self.session.interner) {
            Ok(_) => Ok(true),
            Err(error) => {
                let text = format!("{error}");
                if text.contains("end of input") {
                    Ok(false)
                } else {
                    // A complete-but-invalid input is complete exactly as
                    // GNU's REPL treats it — the syntax error surfaces at
                    // evaluation, not as a continuation prompt.
                    Ok(true)
                }
            }
        }
    }

    /// The user-visible binding names of the global environment — the
    /// completion pool exactly as the facade's enumeration answers it.
    pub fn global_binding_names(&self) -> Vec<String> {
        crate::value::env::frame_symbols(&self.session.heap, self.session.global_env)
            .iter()
            .map(|sym| self.session.interner.symbol_name(*sym).to_string())
            .collect()
    }

    /// Configure the embedder's resource limits — the facade's
    /// contract exactly: eval-depth ceiling (zero for the GNU 500),
    /// wall-clock execution deadline derived from the millisecond
        /// budget, memory caps pending the heap's admission control.
    pub fn set_resource_limits(&mut self, limits: super::values::RResourceLimits) {
        self.session.heap.max_eval_depth = limits.max_eval_depth as usize;
        self.session.heap.eval_deadline = if limits.max_execution_time_ms == 0 {
            None
        } else {
            Some(
                std::time::Instant::now()
                    + std::time::Duration::from_millis(limits.max_execution_time_ms),
            )
        };
    }

    /// Live heap occupancy for the embedder's memory pressure
    /// instrumentation — node count and payload bytes exactly as the
    /// facade's memory usage surface reports them.
    pub fn heap_live_nodes(&self) -> usize {
        self.session.heap.live_count()
    }

    /// Process-capability opt-in — a no-op in the pure world, where the
    /// capability surface has no legacy process state to unlock.
    pub fn enable_host_process_capabilities(&mut self) {}

    /// Close the session — the pure world's heap is plain owned memory,
    /// so closing is a no-op beyond dropping.
    pub fn close(&mut self) {}
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
    fn pre_cancelled_token_aborts_immediately() {
        let mut session = PureSession::new();
        let token = crate::host::facade::HostCancellationToken::new();
        token.cancel();
        let result = session.eval_result_cancellable("1 + 1", &token);
        match result {
            Err(crate::host::facade::RSessionError::EvalError(message)) => {
                assert_eq!(message, "operation cancelled");
            }
            other => panic!("expected cancellation, got {other:?}"),
        }
    }

    #[test]
    fn cancellation_mid_loop_unwinds_evaluation() {
        let mut session = PureSession::new();
        let token = crate::host::facade::HostCancellationToken::new();
        // The canceller thread flips the shared cell directly — the same
        // atomic the SIGINT handler drives through token.cancel().
        let cell = token.shared_flag();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            cell.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        // An unbounded loop would never finish without cancellation —
        // completion with the cancellation error is the proof.
        let result = session.eval_result_cancellable("while (TRUE) 1", &token);
        handle.join().expect("canceller thread");
        match result {
            Err(crate::host::facade::RSessionError::EvalError(message)) => {
                assert_eq!(message, "operation cancelled");
            }
            other => panic!("expected cancellation, got {other:?}"),
        }
    }

    #[test]
    fn uncancelled_token_lets_evaluation_finish() {
        let mut session = PureSession::new();
        let token = crate::host::facade::HostCancellationToken::new();
        let result = session.eval_result_cancellable("40 + 2", &token);
        assert!(result.is_ok(), "evaluation completes: {result:?}");
    }

    #[test]
    fn depth_limit_catches_infinite_recursion() {
        let mut session = PureSession::new();
        let result = session.eval("f <- function() f()\nf()");
        match result.typed {
            RValue::Error(message) => {
                assert!(
                    message.contains("nested too deeply"),
                    "depth error text: {message}"
                );
            }
            other => panic!("expected depth error, got {other:?}"),
        }
    }

    #[test]
    fn shallow_recursion_stays_within_the_default_ceiling() {
        let mut session = PureSession::new();
        // Depth-20 self-recursion through the fibonacci shape — far inside
        // the 500 default exactly as ordinary consumer code is.
        let result = session.eval(
            "fib <- function(n) if (n < 2) n else fib(n - 1) + fib(n - 2)\nfib(10)",
        );
        assert_eq!(result.typed, RValue::Real(Some(55.0)));
    }

    #[test]
    fn execution_deadline_aborts_unbounded_loops() {
        let mut session = PureSession::new();
        session.set_resource_limits(crate::host::values::RResourceLimits {
            max_eval_depth: 0,
            max_execution_time_ms: 50,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        });
        // The deadline recomputes at configuration time; an unbounded loop
        // exceeds it within milliseconds exactly as a runaway script does.
        let result = session.eval("while (TRUE) 1");
        match result.typed {
            RValue::Error(message) => {
                assert!(
                    message.contains("time limit"),
                    "deadline error text: {message}"
                );
            }
            other => panic!("expected deadline error, got {other:?}"),
        }
    }

    #[test]
    fn heap_occupancy_reports_live_nodes() {
        let mut session = PureSession::new();
        session.eval("x <- 1:100");
        assert!(session.heap_live_nodes() > 0);
    }

    #[test]
    fn null_evaluates_to_null() {
        let mut session = PureSession::new();
        let result = session.eval("NULL");
        assert_eq!(result.typed, RValue::Null);
    }
}
