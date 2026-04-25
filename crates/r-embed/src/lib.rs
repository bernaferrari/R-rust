//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter into Rust
//! applications. This crate is the safe boundary used by desktop hosts and
//! UniFFI bindings: it exposes owned Rust values and delegates runtime work to
//! rmath's per-session interpreter, never to process-global `SEXP` state.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, RenderPlot};
use std::sync::{Arc, atomic::AtomicBool};

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

/// Cooperative cancellation handle for an embedded evaluation.
///
/// The token is cheap to clone and can be cancelled from another thread. It is
/// scoped to explicit evaluations that receive it; cancelling one token does
/// not affect other sessions.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.inner.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.inner.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn flag(&self) -> Arc<AtomicBool> {
        self.inner.clone()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
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
        self.eval_result_with_cancel(code, None)
    }

    /// Evaluate an R expression with a cooperative cancellation token.
    pub fn eval_result_cancellable(
        &mut self,
        code: &str,
        cancellation: &CancellationToken,
    ) -> Result<EvalOutput, RSessionError> {
        self.eval_result_with_cancel(code, Some(cancellation.flag()))
    }

    /// Configure Android app-private R runtime paths.
    pub fn configure_android_paths(
        &mut self,
        app_files_dir: &str,
        cache_dir: &str,
        bundled_library_dir: Option<&str>,
    ) -> Result<(), RSessionError> {
        self.inner
            .configure_paths(app_files_dir, cache_dir, bundled_library_dir)
            .map_err(RSessionError::InitFailed)
    }

    fn eval_result_with_cancel(
        &mut self,
        code: &str,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<EvalOutput, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        self.inner.set_cancellation_flag(cancellation);
        let result = self.inner.eval(code);
        self.inner.set_cancellation_flag(None);

        if let Some(message) = result.output.strip_prefix("Error: ") {
            if message == "operation cancelled" {
                Err(RSessionError::EvalError("operation cancelled".to_string()))
            } else {
                Err(RSessionError::EvalError(message.to_string()))
            }
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
    fn configure_android_paths_reaches_embedded_runtime() {
        let root = std::env::temp_dir().join(format!(
            "rport-embed-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");

        let mut session = RSession::new().expect("session");
        session
            .configure_android_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("path config");

        let result = session.eval_result(".libPaths()").expect("lib paths");
        assert_eq!(
            result.value,
            RValue::StringVector(vec![
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned(),
                bundled.to_string_lossy().into_owned()
            ])
        );

        let _ = std::fs::remove_dir_all(root);
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

    #[test]
    fn eval_observes_pre_cancelled_token() {
        let mut session = RSession::new().expect("session");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let err = session
            .eval_result_cancellable("repeat { 1 + 1 }", &cancellation)
            .expect_err("cancelled");
        assert!(err.to_string().contains("operation cancelled"));
    }

    #[test]
    fn eval_can_be_cancelled_from_another_thread() {
        let cancellation = CancellationToken::new();
        let worker_flag = cancellation.clone();
        let worker = std::thread::spawn(move || {
            let mut session = RSession::new().expect("session");
            session.eval_result_cancellable("repeat { 1 + 1 }", &worker_flag)
        });

        std::thread::sleep(std::time::Duration::from_millis(10));
        cancellation.cancel();

        let err = worker
            .join()
            .expect("worker should not panic")
            .expect_err("eval should be cancelled");
        assert!(err.to_string().contains("operation cancelled"));
    }

    #[test]
    fn cancellation_does_not_poison_sessions() {
        let mut cancelled_session = RSession::new().expect("cancelled session");
        let mut other_session = RSession::new().expect("other session");

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let err = cancelled_session
            .eval_result_cancellable("repeat { 1 + 1 }", &cancellation)
            .expect_err("cancelled");
        assert!(err.to_string().contains("operation cancelled"));

        assert_eq!(other_session.eval("1 + 1").unwrap(), "[1] 2");
        assert_eq!(cancelled_session.eval("2 + 2").unwrap(), "[1] 4");
    }
}
