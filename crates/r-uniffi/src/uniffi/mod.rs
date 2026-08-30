//! Internal namespace for the UniFFI boundary implementation.
//!
//! Submodules are `pub(crate)` so in-crate tests can reach the internal seams
//! (`crate::uniffi::...`), while the public API is re-exported flat from the
//! crate root (`lib.rs`). The UniFFI procedural-macro surface is unchanged by
//! this namespacing: derives/exports register through `inventory` and the
//! generated bindings keep the same types, names, and methods.

pub(crate) mod cancellation;
pub(crate) mod conversion;
pub(crate) mod error;
pub(crate) mod operation;
pub(crate) mod plot;
pub(crate) mod session;
pub(crate) mod worker;

pub use conversion::{
    AndroidRuntimePaths, EvalResult, PackageInfo, ProgressUpdate, RAttribute, RComplexValue,
    RMetadata, RValue, RValueKind, ResourceLimits, RuntimeInfo, android_runtime_paths,
};
pub use error::RError;
pub use operation::OperationStatus;
pub use plot::PlotResult;
pub use session::RSession;
pub use worker::SessionCallback;
