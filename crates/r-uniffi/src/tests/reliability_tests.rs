//! Reliability tests: initialization handshake, bounded queue, request
//! timeouts, per-operation cancellation, and operation state transitions
//! through the public + `pub(crate)` seams.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel};
use std::time::{Duration, Instant};

use super::support::wait_for_terminal_status;
use crate::uniffi::error::RError;
use crate::uniffi::operation::{OperationResult, OperationStatus};
use crate::uniffi::session::RSession;
use crate::uniffi::worker::{OpKind, QUEUE_CAPACITY, SessionCommand, WorkerHandle};

// ---------------------------------------------------------------------------
// Item 1: initialization handshake
// ---------------------------------------------------------------------------

#[test]
fn constructor_propagates_worker_init_failure() {
    let result = RSession::initialize(
        Box::new(|| Err(r_embed::RSessionError::InitFailed("boom".to_string()))),
        Duration::from_secs(5),
    );

    assert!(matches!(result, Err(RError::InitFailed(message)) if message.contains("boom")));
}

#[test]
fn constructor_fails_when_init_exceeds_handshake_deadline() {
    // Init blocks briefly (250 ms) regardless of the gate; the constructor
    // deadline is 80 ms, so the constructor must fail first, on time, and
    // without waiting for the 5 s bounded shutdown join.
    let result = RSession::initialize(
        Box::new(|| {
            std::thread::sleep(Duration::from_millis(250));
            r_embed::RSession::new()
        }),
        Duration::from_millis(80),
    );
    assert!(
        matches!(result, Err(RError::InitFailed(message)) if message.contains("did not complete")),
        "expected handshake timeout error"
    );
    // Give the detached worker time to finish its doomed init and exit.
    std::thread::sleep(Duration::from_millis(300));
}

#[test]
fn shutdown_waits_for_worker_thread_to_join() {
    let (commands_tx, commands_rx) = sync_channel(1);
    let (exited_tx, exited_rx) = channel();
    let thread_finished = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&thread_finished);
    let join = std::thread::spawn(move || {
        assert!(matches!(commands_rx.recv(), Ok(SessionCommand::Shutdown)));
        exited_tx.send(()).expect("exit signal receiver");
        // Signal before actually returning: shutdown must join, not merely
        // observe the signal and detach the still-running thread.
        std::thread::sleep(Duration::from_millis(50));
        finished.store(true, Ordering::Release);
    });
    let mut worker = WorkerHandle::new(commands_tx, exited_rx, join);

    worker.shutdown();

    assert!(thread_finished.load(Ordering::Acquire));
}

// ---------------------------------------------------------------------------
// Item 3: bounded command queue
// ---------------------------------------------------------------------------

#[test]
fn queue_full_when_worker_is_blocked() {
    let session = std::sync::Arc::new(RSession::new().expect("session"));
    let blocker = session.clone();
    let blocker_thread = std::thread::spawn(move || blocker.eval("repeat { 1 + 1 }".to_string()));
    std::thread::sleep(Duration::from_millis(50)); // blocker occupies the worker

    // Fill the bounded queue; the worker is busy and cannot drain it.
    let mut ids = Vec::new();
    for _ in 0..QUEUE_CAPACITY {
        ids.push(session.eval_async("1".to_string()).expect("queued eval"));
    }
    assert!(
        matches!(session.eval_async("1".to_string()), Err(RError::QueueFull)),
        "expected QueueFull once the queue is saturated"
    );

    // Unblock: cancels the blocker and every queued operation.
    session.cancel();
    let err = blocker_thread
        .join()
        .expect("worker thread should not panic")
        .expect_err("blocker should be cancelled");
    assert!(matches!(err, RError::Cancelled));

    for id in ids {
        assert!(
            matches!(
                wait_for_terminal_status(&session, id),
                OperationStatus::Cancelled
            ),
            "queued operation {id} should resolve to Cancelled"
        );
    }
    assert_eq!(
        session.eval("1 + 1".to_string()).expect("recovered"),
        "[1] 2"
    );
}

// ---------------------------------------------------------------------------
// Item 4: request timeouts
// ---------------------------------------------------------------------------

#[test]
fn sync_request_times_out_and_unwinds() {
    let session = std::sync::Arc::new(RSession::new().expect("session"));
    let blocker = session.clone();
    let blocker_thread = std::thread::spawn(move || blocker.eval("repeat { 1 + 1 }".to_string()));
    std::thread::sleep(Duration::from_millis(50)); // blocker occupies the worker

    let started = Instant::now();
    let result = session.request_with_timeout(
        |reply| OpKind::Eval {
            code: "1 + 1".to_string(),
            reply: Some(reply),
        },
        Duration::from_millis(50),
    );
    let elapsed = started.elapsed();

    assert!(
        matches!(result, Err(RError::Timeout { after_ms }) if after_ms == 50),
        "expected Timeout, got {result:?}"
    );
    assert!(elapsed >= Duration::from_millis(50));
    assert!(elapsed < Duration::from_secs(5), "timeout must not hang");

    // Cancel the blocker and confirm the session recovers.
    session.cancel();
    let err = blocker_thread
        .join()
        .expect("worker thread should not panic")
        .expect_err("blocker should be cancelled");
    assert!(matches!(err, RError::Cancelled));
    assert_eq!(
        session.eval("2 + 2".to_string()).expect("recovered"),
        "[1] 4"
    );
}

// ---------------------------------------------------------------------------
// Item 5 + 7: per-operation cancellation and the state machine
// ---------------------------------------------------------------------------

#[test]
fn cancel_operation_is_per_operation() {
    let session = RSession::new().expect("session");

    // Occupy the worker so queue order (and therefore states) is
    // deterministic: blocker runs while the other two queue behind it.
    let blocker = session
        .eval_async("repeat { 1 + 1 }".to_string())
        .expect("blocker");
    std::thread::sleep(Duration::from_millis(50));
    assert!(matches!(
        session.operation_status(blocker),
        OperationStatus::Running
    ));

    let cancelled_op = session.eval_async("1 + 1".to_string()).expect("op");
    let survivor = session.eval_async("2 + 2".to_string()).expect("survivor");
    assert!(matches!(
        session.operation_status(cancelled_op),
        OperationStatus::Queued
    ));

    // Cancel ONE queued operation; the other queued operation must be
    // untouched.
    session.cancel_operation(cancelled_op).expect("cancel op");
    assert!(matches!(
        session.operation_status(cancelled_op),
        OperationStatus::Cancelling
    ));

    // Cancel the blocker so the worker can drain the queue.
    session.cancel_operation(blocker).expect("cancel blocker");
    assert!(matches!(
        wait_for_terminal_status(&session, blocker),
        OperationStatus::Cancelled
    ));

    // The pre-cancelled operation lands in Cancelled...
    assert!(matches!(
        wait_for_terminal_status(&session, cancelled_op),
        OperationStatus::Cancelled
    ));
    // ...while its queued sibling completes normally: cancelling one
    // operation did not leak into the other.
    assert!(matches!(
        wait_for_terminal_status(&session, survivor),
        OperationStatus::Succeeded { .. }
    ));

    // take_result consumes: Succeeded first, then the consumption of the
    // result reports Expired exactly once (the tombstone is consumed too),
    // after which the id is Unknown.
    assert!(matches!(
        session.take_result(survivor),
        OperationStatus::Succeeded { .. }
    ));
    assert!(matches!(
        session.take_result(survivor),
        OperationStatus::Expired
    ));
    assert!(matches!(
        session.operation_status(survivor),
        OperationStatus::Unknown
    ));

    // Unknown ids error on cancel and report Unknown status.
    assert!(matches!(
        session.cancel_operation(9_999),
        Err(RError::InvalidInput(_))
    ));
    assert!(matches!(
        session.operation_status(9_999),
        OperationStatus::Unknown
    ));
}

#[test]
fn cancelled_queued_render_does_not_poison_render_rerun() {
    let session = RSession::new().expect("session");
    let blocker = session
        .eval_async("repeat { 1 + 1 }".to_string())
        .expect("blocker");
    std::thread::sleep(Duration::from_millis(50));

    let cancelled_render = session
        .render_async("plot(1, 1)".to_string(), 120, 100)
        .expect("queued render");
    session
        .cancel_operation(cancelled_render)
        .expect("cancel queued render");
    assert!(matches!(
        session.operation_status(cancelled_render),
        OperationStatus::Cancelling
    ));

    session.cancel_operation(blocker).expect("cancel blocker");
    assert!(matches!(
        wait_for_terminal_status(&session, blocker),
        OperationStatus::Cancelled
    ));
    assert!(matches!(
        wait_for_terminal_status(&session, cancelled_render),
        OperationStatus::Cancelled
    ));

    let rerun = session
        .render_async("plot(1, 1)".to_string(), 120, 100)
        .expect("render rerun");
    match wait_for_terminal_status(&session, rerun) {
        OperationStatus::Succeeded {
            result: OperationResult::Render { result },
        } => {
            assert_eq!((result.width, result.height), (120, 100));
            assert!(result.png_bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        }
        status => panic!("expected typed render result, got {status:?}"),
    }
}
