//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter into Rust
//! applications. This crate is the safe boundary used by desktop hosts and
//! UniFFI bindings: it exposes owned Rust values and delegates runtime work to
//! rmath's per-session interpreter, never to process-global `SEXP` state.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, RenderPlot};

pub use rmath::android::RValue;

use thiserror::Error;

/// Errors that can occur during R session operations.
#[derive(Debug, Error)]
pub enum RSessionError {
    #[error("Failed to initialize R session: {0}")]
    InitFailed(String),
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
}

/// An embedded R session.
///
/// This provides a handle to an R interpreter instance that can
/// evaluate expressions and render plots. Internally uses the rmath
/// crate for the interpreter backend.
pub struct RSession {
    active: bool,
    inner: rmath::android::RSession,
}

/// Owned result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutput {
    pub output: String,
    pub value: RValue,
}

impl RSession {
    /// Create a new R session.
    ///
    /// Initializes an isolated rmath session with its own arena, protection
    /// stack, environments, RNG state, and output capture.
    pub fn new() -> Result<Self, RSessionError> {
        Ok(RSession {
            active: true,
            inner: rmath::android::RSession::new(),
        })
    }

    /// Evaluate an R expression, returning the output as a string.
    ///
    /// Parses the code string using rmath's parser and evaluates it
    /// in the global environment. The result is formatted as a string
    /// using rmath's output subsystem.
    pub fn eval(&mut self, code: &str) -> Result<String, RSessionError> {
        self.eval_result(code).map(|result| result.output)
    }

    /// Evaluate an R expression, returning both display output and an owned
    /// typed value.
    pub fn eval_result(&mut self, code: &str) -> Result<EvalOutput, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        let result = self.inner.eval(code);
        if let Some(message) = result.output.strip_prefix("Error: ") {
            Err(RSessionError::EvalError(message.to_string()))
        } else {
            Ok(EvalOutput {
                output: result.output,
                value: result.typed,
            })
        }
    }

    /// Render an R expression as a plot, returning pixel data.
    ///
    /// Currently returns empty pixel data as the graphics engine
    /// has not yet been implemented.
    pub fn render_with_dimensions(
        &mut self,
        _code: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RSessionError> {
        if !self.active {
            return Err(RSessionError::RenderError("Session closed".into()));
        }
        let mut renderer = AndroidHeadlessRenderer::new(width, height);
        renderer.clear(Color::WHITE);
        Ok(renderer.finish())
    }

    /// Close the session.
    pub fn close(&mut self) {
        if self.active {
            self.inner.close();
            self.active = false;
        }
    }
}

impl Default for RSession {
    fn default() -> Self {
        Self::new().expect("failed to create R session")
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_uses_isolated_session_state() {
        let mut left = RSession::new().expect("left session");
        let mut right = RSession::new().expect("right session");

        assert_eq!(left.eval("x <- 11\nx").unwrap(), "[1] 11");
        assert_eq!(right.eval("x <- 29\nx").unwrap(), "[1] 29");
        assert_eq!(left.eval("x").unwrap(), "[1] 11");
        assert_eq!(right.eval("x").unwrap(), "[1] 29");
    }

    #[test]
    fn eval_result_returns_owned_typed_value() {
        let mut session = RSession::new().expect("session");
        let result = session.eval_result("c(1, 2, 3)").expect("eval");
        assert_eq!(result.output, "[1] 1 2 3");
        assert_eq!(
            result.value,
            RValue::RealVector(vec![Some(1.0), Some(2.0), Some(3.0)])
        );
    }

    #[test]
    fn eval_reports_errors_without_panicking() {
        let mut session = RSession::new().expect("session");
        let err = session
            .eval("unknown_symbol")
            .expect_err("undefined symbol");
        let message = err.to_string();
        assert!(message.contains("object '"));
        assert!(message.contains("not found"));
    }

    #[test]
    fn close_makes_eval_fail() {
        let mut session = RSession::new().expect("session");
        session.close();
        assert!(session.eval("1 + 1").is_err());
    }
}
