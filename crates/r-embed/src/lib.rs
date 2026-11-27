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

// The canonical public session lives in r_runtime::host exactly as the
// pure-Rust foundation plan pins — one core session, one public host
// session. This crate is the stable re-export boundary embedders depend on.
pub use r_runtime::host::{
    AndroidRuntimePaths, EvalOutput, HostCancellationToken as CancellationToken, RArenaStats,
    RAttribute, RComplexValue, RMetadata, RPackageInfo, RResourceLimits, RRuntimeInfo,
    RSessionError, RValue, RuntimePaths,
};
pub use r_runtime::host::pure_session::{
    PureResourceLimits, PureRuntimeInfo, PureSession,
};
pub use r_runtime::host::InteractiveOutput;
pub use r_runtime::{MAX_BROWSER_FILE_BYTES, MAX_BROWSER_FILE_PATH_BYTES};

#[cfg(feature = "vello-gpu")]
pub use r_device_vello_gpu::{GpuError, GpuRenderer};
