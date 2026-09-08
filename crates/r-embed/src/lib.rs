//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter into Rust
//! applications. This crate is the safe boundary used by desktop hosts and
//! UniFFI bindings: it exposes owned Rust values and delegates runtime work to
//! rmath's per-session interpreter, never to process-global `SEXP` state.

//!
//! ```
//! use r_embed::{RSession, RValue};
//! let mut session = RSession::new()?;
//! let snapshot = session.eval_result("c('a', 'b')")?;
//! session.eval("gc(); x <- 42")?;
//! assert_eq!(snapshot.value, RValue::StringVector(vec![Some("a".into()), Some("b".into())]));
//! # Ok::<(), r_embed::RSessionError>(())
//! ```

mod packages;
mod session;

pub use rmath::android::{
    RArenaStats, RAttribute, RComplexValue, RMetadata, RResourceLimits, RRuntimeInfo, RValue,
};

pub use packages::RPackageInfo;
pub use session::{
    AndroidRuntimePaths, CancellationToken, EvalOutput, InteractiveOutput, RSession, ReadGuard,
    ValueHandle, WriteGuard,
};

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

#[cfg(feature = "vello-gpu")]
pub use r_device_vello_gpu::{GpuError, GpuRenderer};
