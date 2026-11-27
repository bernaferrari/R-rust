//! The public host session facade: owned-value evaluation results, error
//! adaptation, cancellation, value handles, and interactive graphics.
//!
//! These types and methods are the canonical public embedding surface —
//! what `r-embed` re-exports — merged onto the one host `RSession` so the
//! layering collapses to core session + public session exactly as the
//! pure-Rust foundation plan pins.

use super::RValue;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide counter assigning each session a unique id.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate the next session identity. Wraps at u64 exhaustion — the
/// facade never exposes failure for what is unreachable in practice.
pub(crate) fn next_session_id() -> u64 {
    NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

/// Errors that can occur during R session operations.
#[derive(Debug)]
pub enum RSessionError {
    /// Session initialization failed.
    InitFailed(String),
    /// Evaluation produced an R error.
    EvalError(String),
    /// Evaluation errored after producing output and possibly drawing.
    EvalErrorWithOutput {
        message: String,
        output: String,
        png: Option<Vec<u8>>,
    },
    /// Graphics rendering failed.
    RenderError(String),
}

impl std::fmt::Display for RSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RSessionError::InitFailed(message) => {
                write!(f, "Failed to initialize R session: {message}")
            }
            RSessionError::EvalError(message) => write!(f, "Evaluation error: {message}"),
            RSessionError::EvalErrorWithOutput { message, .. } => {
                write!(f, "Evaluation error: {message}")
            }
            RSessionError::RenderError(message) => write!(f, "Render error: {message}"),
        }
    }
}

impl std::error::Error for RSessionError {}

/// Owned result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutput {
    pub output: String,
    pub value: RValue,
}

/// The combined result of one interactive evaluation — pure data with
/// no feature-dependent content, so embedders pattern-match one shape
/// regardless of whether the slim build rasterizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveOutput {
    pub output: String,
    pub png: Option<Vec<u8>>,
}

/// Derived runtime paths for app-private embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub app_files_dir: String,
    pub cache_dir: String,
    pub bundled_library_dir: Option<String>,
}

impl RuntimePaths {
    pub fn new(
        app_files_dir: impl Into<String>,
        cache_dir: impl Into<String>,
        bundled_library_dir: Option<impl Into<String>>,
    ) -> Self {
        Self {
            app_files_dir: app_files_dir.into(),
            cache_dir: cache_dir.into(),
            bundled_library_dir: bundled_library_dir.map(Into::into),
        }
    }

    pub fn user_library_dir(&self) -> String {
        std::path::PathBuf::from(&self.app_files_dir)
            .join("R")
            .join("library")
            .to_string_lossy()
            .into_owned()
    }

    pub fn temp_dir(&self) -> String {
        std::path::PathBuf::from(&self.cache_dir)
            .join("Rtmp")
            .to_string_lossy()
            .into_owned()
    }

    pub fn library_paths(&self) -> Vec<String> {
        let mut paths = vec![self.user_library_dir()];
        if let Some(path) = &self.bundled_library_dir {
            paths.push(path.clone());
        }
        paths
    }
}

/// Back-compatible name from the Android-first embedding era.
pub type AndroidRuntimePaths = RuntimePaths;

/// Cooperative cancellation handle for an embedded evaluation.
///
/// The token is cheap to clone and can be cancelled from another thread. It is
/// scoped to explicit evaluations that receive it; cancelling one token does
/// not affect other sessions.
#[derive(Debug, Clone)]
pub struct HostCancellationToken {
    inner: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl HostCancellationToken {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.inner.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The shared cancellation cell for the pure world's evaluator —
    /// crate-visible exactly as the migration boundary needs.
    pub(crate) fn shared_flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.inner.clone()
    }

    pub fn reset(&self) {
        self.inner.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(std::sync::atomic::Ordering::Relaxed)
    }

}

impl Default for HostCancellationToken {
    fn default() -> Self {
        Self::new()
    }
}
