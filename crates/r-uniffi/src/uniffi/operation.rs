//! Operation table: state machine, retention, and per-operation tokens.
//!
//! # State machine
//!
//! ```text
//!            register()          mark_running()          complete()
//!   (new) ──────────────► Queued ─────────► Running ──────────────► Succeeded
//!                             │                  │                    Failed
//!              cancel_operation()/cancel_all()     │                    Cancelled
//!                             │                     │
//!                             └────► Cancelling ────┘
//!
//!   Succeeded | Failed | Cancelled
//!        │ retained async ops: FIFO-evicted once RETAINED_COMPLETED newer
//!        │ operations completed, or consumed via take_result()
//!        ▼
//!      Expired (tombstone; bounded by EXPIRED_TOMBSTONE_CAP)
//! ```
//!
//! Synchronous requests register unretained operations: they are removed as
//! soon as they complete and never enter the retention window.

use std::collections::{HashMap, HashSet, VecDeque};

use r_embed::CancellationToken;

use super::conversion::EvalResult;
use super::plot::PlotResult;

/// Completed async operations retained for `take_result` (FIFO eviction).
pub(crate) const RETAINED_COMPLETED: usize = 100;

/// Upper bound on `Expired` tombstones kept so `operation_status` can
/// distinguish "evicted" from "never existed" without growing without bound.
const EXPIRED_TOMBSTONE_CAP: usize = 4096;

/// Final status of an operation, exported across the UniFFI boundary.
#[derive(Debug, Clone, uniffi::Enum)]
#[allow(clippy::large_enum_variant)]
pub enum OperationStatus {
    Queued,
    Running,
    /// Cancellation has been requested and the worker has not acknowledged
    /// the terminal cancelled outcome yet.
    Cancelling,
    Succeeded {
        result: OperationResult,
    },
    Failed {
        error: String,
    },
    Cancelled,
    /// The operation completed but its result is no longer retained:
    /// it was FIFO-evicted after [`RETAINED_COMPLETED`] newer completions,
    /// or already consumed by `take_result`.
    Expired,
    /// No operation with this id was ever registered.
    Unknown,
}

/// Typed payload retained for an asynchronous operation.
///
/// Keeping evaluation and render outputs in the operation table makes
/// callbacks optional notifications rather than the only way to recover a
/// result. Hosts can always consume the exact operation they submitted.
#[derive(Debug, Clone, uniffi::Enum)]
#[allow(clippy::large_enum_variant)]
pub enum OperationResult {
    Eval { result: EvalResult },
    Render { result: PlotResult },
}

/// Terminal outcome recorded by the worker.
#[allow(clippy::large_enum_variant)]
pub(crate) enum OpOutcome {
    Succeeded(OperationResult),
    Failed(String),
    Cancelled,
}
#[allow(clippy::large_enum_variant)]
pub(crate) enum OpState {
    Queued,
    Running,
    Cancelling,
    Done(OpOutcome),
}

struct Operation {
    state: OpState,
    token: CancellationToken,
    /// Retained (async) operations keep their terminal outcome for
    /// `take_result` / `operation_status`; unretained (synchronous) ones are
    /// forgotten at completion.
    retained: bool,
}

/// Shared, mutex-guarded registry of all live and retained operations.
pub(crate) struct OperationTable {
    entries: HashMap<u64, Operation>,
    completed_fifo: VecDeque<u64>,
    expired: VecDeque<u64>,
    expired_set: HashSet<u64>,
    retention: usize,
}

impl OperationTable {
    pub(crate) fn new(retention: usize) -> Self {
        Self {
            entries: HashMap::new(),
            completed_fifo: VecDeque::new(),
            expired: VecDeque::new(),
            expired_set: HashSet::new(),
            retention,
        }
    }

    pub(crate) fn register(&mut self, id: u64, token: CancellationToken, retained: bool) {
        self.entries.insert(
            id,
            Operation {
                state: OpState::Queued,
                token,
                retained,
            },
        );
    }

    /// Only the worker thread moves operations out of `Queued`.
    pub(crate) fn mark_running(&mut self, id: u64) {
        if let Some(operation) = self.entries.get_mut(&id)
            && matches!(operation.state, OpState::Queued)
        {
            operation.state = if operation.token.is_cancelled() {
                OpState::Cancelling
            } else {
                OpState::Running
            };
        }
    }

    /// Record the terminal outcome. Unretained operations are removed
    /// immediately; retained ones enter the FIFO retention window and evict
    /// the oldest completion beyond capacity into an `Expired` tombstone.
    pub(crate) fn complete(&mut self, id: u64, outcome: OpOutcome) {
        let Some(operation) = self.entries.get_mut(&id) else {
            return;
        };
        operation.state = OpState::Done(outcome);
        if !operation.retained {
            self.entries.remove(&id);
            return;
        }
        self.completed_fifo.push_back(id);
        while self.completed_fifo.len() > self.retention {
            let Some(evicted) = self.completed_fifo.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
            self.tombstone(evicted);
        }
    }

    /// Drop an operation without a tombstone (synchronous completions and
    /// enqueue failures).
    pub(crate) fn forget(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    pub(crate) fn status(&self, id: u64) -> OperationStatus {
        match self.entries.get(&id) {
            Some(operation) => state_status(&operation.state),
            None => self.status_without_entry(id),
        }
    }

    /// Consume a completed entry, returning its terminal status. Queued and
    /// running operations have nothing to take yet and are left untouched; a
    /// consumed retained operation (or evicted tombstone) reports `Expired`
    /// once and is gone afterwards.
    pub(crate) fn take_result(&mut self, id: u64) -> OperationStatus {
        let Some(operation) = self.entries.get(&id) else {
            return self.take_tombstone(id);
        };
        if !matches!(operation.state, OpState::Done(_)) {
            return state_status(&operation.state);
        }

        let operation = self.entries.remove(&id).expect("entry checked above");
        self.completed_fifo.retain(|completed| *completed != id);
        self.tombstone(id);
        match operation.state {
            OpState::Done(outcome) => status_from_outcome(outcome),
            OpState::Queued => OperationStatus::Queued,
            OpState::Running => OperationStatus::Running,
            OpState::Cancelling => OperationStatus::Cancelling,
        }
    }

    fn take_tombstone(&mut self, id: u64) -> OperationStatus {
        if self.expired_set.remove(&id) {
            self.expired.retain(|expired| *expired != id);
            OperationStatus::Expired
        } else {
            OperationStatus::Unknown
        }
    }

    /// Request cancellation of every active operation. Returns how many were
    /// still queued or running.
    pub(crate) fn cancel_all(&mut self) -> usize {
        let mut cancelled = 0;
        for operation in self.entries.values_mut() {
            if matches!(operation.state, OpState::Queued | OpState::Running) {
                operation.token.cancel();
                operation.state = OpState::Cancelling;
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Request cancellation of one operation. Returns false when the id is
    /// unknown or the operation already finished.
    pub(crate) fn cancel_operation(&mut self, id: u64) -> bool {
        let Some(operation) = self.entries.get_mut(&id) else {
            return false;
        };
        if matches!(operation.state, OpState::Queued | OpState::Running) {
            operation.token.cancel();
            operation.state = OpState::Cancelling;
            true
        } else {
            false
        }
    }

    /// True when the id was ever registered (live entry or tombstone).
    pub(crate) fn is_known(&self, id: u64) -> bool {
        self.entries.contains_key(&id) || self.expired_set.contains(&id)
    }

    fn tombstone(&mut self, id: u64) {
        self.expired.push_back(id);
        self.expired_set.insert(id);
        while self.expired.len() > EXPIRED_TOMBSTONE_CAP {
            match self.expired.pop_front() {
                Some(oldest) => {
                    self.expired_set.remove(&oldest);
                }
                None => break,
            }
        }
    }

    fn status_without_entry(&self, id: u64) -> OperationStatus {
        if self.expired_set.contains(&id) {
            OperationStatus::Expired
        } else {
            OperationStatus::Unknown
        }
    }
}

fn state_status(state: &OpState) -> OperationStatus {
    match state {
        OpState::Queued => OperationStatus::Queued,
        OpState::Running => OperationStatus::Running,
        OpState::Cancelling => OperationStatus::Cancelling,
        OpState::Done(OpOutcome::Succeeded(result)) => OperationStatus::Succeeded {
            result: result.clone(),
        },
        OpState::Done(OpOutcome::Failed(error)) => OperationStatus::Failed {
            error: error.clone(),
        },
        OpState::Done(OpOutcome::Cancelled) => OperationStatus::Cancelled,
    }
}

fn status_from_outcome(outcome: OpOutcome) -> OperationStatus {
    match outcome {
        OpOutcome::Succeeded(result) => OperationStatus::Succeeded { result },
        OpOutcome::Failed(error) => OperationStatus::Failed { error },
        OpOutcome::Cancelled => OperationStatus::Cancelled,
    }
}
