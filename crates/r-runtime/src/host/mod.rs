#![warn(unused_unsafe)]

pub(crate) mod packages;
mod values;
pub mod pure_session;
pub mod value_snapshot;

pub mod facade;

pub use facade::{EvalOutput, HostCancellationToken, RSessionError, RuntimePaths};
#[cfg(feature = "renderplot-device")]
pub use facade::InteractiveOutput;
pub use facade::AndroidRuntimePaths;
pub use packages::RPackageInfo;
pub use values::*;
