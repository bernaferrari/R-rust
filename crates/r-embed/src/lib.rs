//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter into Rust
//! applications. This crate is the safe boundary used by desktop hosts and
//! UniFFI bindings: it exposes owned Rust values and delegates runtime work to
//! rmath's per-session interpreter, never to process-global `SEXP` state.

mod packages;
mod plot;
mod session;

pub use rmath::android::{
    RArenaStats, RAttribute, RComplexValue, RMetadata, RResourceLimits, RRuntimeInfo, RValue,
};

pub use packages::RPackageInfo;
pub use session::{AndroidRuntimePaths, CancellationToken, EvalOutput, ReadGuard, RSession, ValueHandle, WriteGuard};

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
