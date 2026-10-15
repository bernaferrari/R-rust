//! Android embedding layer for the crates R interpreter.
//!
//! This module provides a clean, safe API surface for embedding the R
//! interpreter in Android applications via UniFFI (or any FFI framework).
//!
//! # Design Principles
//!
//! - **No raw pointers cross the FFI boundary.** All inputs/outputs are
//!   owned Rust types (strings, numbers, Vec) or opaque handles.
//! - **Thread-confined sessions.** Each `RSession` owns its own arena and
//!   should be created on the worker thread that uses it; hosts can run
//!   multiple sessions in parallel by giving each tab its own worker.
//! - **Minimal surface.** Only the operations needed for Android embedding
//!   are exposed — eval, print, math, ALTREP.
//! - **Zero-cost.** The safe wrappers compile down to the same operations
//!   as the internal code.

#[cfg(test)]
#[path = "focused_console_ownership_tests.rs"]
mod focused_console_ownership_tests;
#[cfg(test)]
#[path = "output_contract_tests.rs"]
mod output_contract_tests;
pub(crate) mod result_budget;
pub use crate::mainutils::paths::RuntimePathPolicy;
use crate::sexp::RSession as CoreRSession;
use crate::sexp::builder;
#[cfg(test)]
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::object::{Sexp, SexpAttribute, SexpComplex, SexpMetadata, SexpValue};
use crate::sexp::output;
use crate::sexp::session::CancellationToken;
pub use crate::sexp::session::RetainedValueId;
pub use crate::sexp::session::TopLevelEvaluationMode;

mod resources;
pub use resources::{RMemoryUsage, RWorkspaceReservation};
mod session;
mod values;

pub use session::*;
pub use values::*;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_more;
