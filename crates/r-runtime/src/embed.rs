//! The stable embedding boundary exactly as the foundation plan pins it —
//! one canonical public session surface over the runtime's host layer.
//! Embedders (r-host-cli, r-uniffi, r-wasm) consume this module rather
//! than the runtime internals, so the internals may evolve freely.
//! This is the r-embed facade merged into r-runtime exactly as the plan
//! specifies: the facade was pure re-export with no logic of its own.

pub use crate::host::{
    AndroidRuntimePaths, EvalOutput, HostCancellationToken as CancellationToken, RArenaStats,
    RAttribute, RComplexValue, RMetadata, RPackageInfo, RResourceLimits, RRuntimeInfo,
    RSessionError, RValue, RuntimePaths,
};
pub use crate::host::pure_session::{PureResourceLimits, PureRuntimeInfo, PureSession};
#[cfg(feature = "renderplot-device")]
pub use crate::host::InteractiveOutput;
pub use crate::{MAX_BROWSER_FILE_BYTES, MAX_BROWSER_FILE_PATH_BYTES};
