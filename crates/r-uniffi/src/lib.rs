//! r-uniffi: UniFFI bindings for the R interpreter.
//!
//! This crate provides high-level, safe bindings to the R interpreter
//! for use from Kotlin (Android), Swift (iOS), and Python.
//!
//! # Reliability architecture
//!
//! * **Initialization handshake** — [`RSession::new`] spawns the interpreter
//!   worker and blocks (at most 30 s) until the worker reports its session
//!   ready; init failure/timeout is a constructor error (`RError::InitFailed`).
//! * **Retained worker join** — the worker's `JoinHandle` is retained; drop
//!   and `shutdown_worker` signal shutdown and join with a bounded 5 s wait,
//!   detaching only if the interpreter is wedged.
//! * **Bounded command queue** — requests travel over a 64-slot queue;
//!   overflow fails fast with `RError::QueueFull` instead of growing without
//!   bound.
//! * **Request deadlines** — synchronous requests wait at most 120 s
//!   (per-call override on the internal seam) and time out with
//!   `RError::Timeout`, which also cancels the affected operation.
//! * **Per-operation cancellation** — every operation owns a private
//!   cancellation token (`uniffi::cancellation`); there is no shared token
//!   and no reset-around-requests.
//! * **Callbacks off the interpreter thread** — host callbacks fire on a
//!   dedicated dispatcher thread, never on the interpreter thread
//!   (`uniffi::worker::CallbackDispatcher` documents the policy).
//! * **Operation state machine** — `Queued → Running → Succeeded | Failed |
//!   Cancelled`, with FIFO retention of the last 100 completed async
//!   operations (`uniffi::operation`), `take_result` consumption, and
//!   `Expired` tombstones for evicted results.

mod uniffi;

pub use crate::uniffi::{
    AndroidRuntimePaths, EvalResult, OperationStatus, PackageInfo, PlotResult, ProgressUpdate,
    RAttribute, RComplexValue, RError, RMetadata, RSession, RValue, RValueKind, ResourceLimits,
    RuntimeInfo, SessionCallback, android_runtime_paths,
};

#[cfg(test)]
mod tests;

::uniffi::setup_scaffolding!("rport");
