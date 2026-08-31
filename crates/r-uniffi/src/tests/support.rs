//! Shared test helpers: recording callback + package fixture.
#![allow(dead_code)]

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::uniffi::conversion::{EvalResult, RValueKind};
use crate::uniffi::plot::PlotResult;
use crate::uniffi::worker::SessionCallback;

pub(crate) fn make_test_package(root_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "{root_name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    let bundled = root.join("bundled-library");
    let pkg = bundled.join("tiny");
    let r_dir = pkg.join("R");
    std::fs::create_dir_all(&r_dir).expect("package R dir");
    std::fs::write(
        pkg.join("DESCRIPTION"),
        concat!(
            "Package: tiny\n",
            "Version: 0.0.1\n",
            "Title: Tiny Test Package\n",
            "Description: Tiny package for Android runtime tests\n",
            "License: MIT\n",
            "Depends: R (>= 4.0.0)\n",
            "Imports: base\n",
            "Suggests: testthat\n",
            "NeedsCompilation: no\n",
        ),
    )
    .expect("description");
    std::fs::write(r_dir.join("tiny.R"), "tiny_value <- function() 42L\n").expect("R source");
    (root, pkg)
}

#[derive(Debug, Clone)]
pub(crate) enum CallbackEvent {
    EvalComplete {
        operation_id: u64,
        output: String,
        kind: RValueKind,
    },
    PlotReady {
        operation_id: u64,
        width: u32,
        height: u32,
        bytes: usize,
    },
    Output {
        operation_id: u64,
        line: String,
    },
    Error {
        operation_id: u64,
        message: String,
    },
}

pub(crate) type CallbackEvents = Arc<(Mutex<Vec<CallbackEvent>>, Condvar)>;

pub(crate) struct RecordingCallback {
    events: CallbackEvents,
}

impl RecordingCallback {
    pub(crate) fn new() -> (Self, CallbackEvents) {
        let events = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        (
            Self {
                events: events.clone(),
            },
            events,
        )
    }

    fn push(&self, event: CallbackEvent) {
        let (lock, ready) = &*self.events;
        lock.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        ready.notify_all();
    }
}

impl SessionCallback for RecordingCallback {
    fn on_progress(&self, _operation_id: u64, _update: crate::uniffi::conversion::ProgressUpdate) {}

    fn on_output(&self, operation_id: u64, line: String) {
        self.push(CallbackEvent::Output { operation_id, line });
    }

    fn on_plot_ready(&self, operation_id: u64, plot: PlotResult) {
        self.push(CallbackEvent::PlotReady {
            operation_id,
            width: plot.width,
            height: plot.height,
            bytes: plot.png_bytes.len(),
        });
    }

    fn on_eval_complete(&self, operation_id: u64, result: EvalResult) {
        self.push(CallbackEvent::EvalComplete {
            operation_id,
            output: result.output,
            kind: result.value.kind,
        });
    }

    fn on_error(&self, operation_id: u64, error: String) {
        self.push(CallbackEvent::Error {
            operation_id,
            message: error,
        });
    }
}

pub(crate) fn wait_for_callback(
    events: &CallbackEvents,
    matches: impl Fn(&CallbackEvent) -> bool,
) -> CallbackEvent {
    let deadline = Instant::now() + Duration::from_secs(3);
    let (lock, ready) = &**events;
    let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());

    loop {
        if let Some(index) = guard.iter().position(&matches) {
            return guard.remove(index);
        }

        let now = Instant::now();
        if now >= deadline {
            panic!("timed out waiting for callback; observed events: {guard:?}");
        }
        let remaining = deadline.saturating_duration_since(now);
        let (next_guard, timeout) = ready
            .wait_timeout(guard, remaining)
            .unwrap_or_else(|e| e.into_inner());
        guard = next_guard;
        if timeout.timed_out() {
            panic!("timed out waiting for callback; observed events: {guard:?}");
        }
    }
}

/// Poll `operation_status` until the operation reaches a terminal state.
pub(crate) fn wait_for_terminal_status(
    session: &crate::uniffi::session::RSession,
    op_id: u64,
) -> crate::uniffi::operation::OperationStatus {
    use crate::uniffi::operation::OperationStatus;
    use std::time::Duration;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = session.operation_status(op_id);
        if matches!(
            status,
            OperationStatus::Succeeded { .. }
                | OperationStatus::Failed { .. }
                | OperationStatus::Cancelled
                | OperationStatus::Expired
        ) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "operation {op_id} did not reach a terminal state"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
