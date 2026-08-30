//! Unit tests for the operation state machine (`OperationTable`).

use crate::uniffi::cancellation::new_token;
use crate::uniffi::conversion::null_eval_result;
use crate::uniffi::operation::{OpOutcome, OperationStatus, OperationTable, RETAINED_COMPLETED};

#[test]
fn operations_transition_through_the_state_machine() {
    let mut table = OperationTable::new(RETAINED_COMPLETED);
    table.register(1, new_token(), true);

    assert!(matches!(table.status(1), OperationStatus::Queued));

    table.mark_running(1);
    assert!(matches!(table.status(1), OperationStatus::Running));

    table.complete(1, OpOutcome::Succeeded(null_eval_result("done")));
    match table.status(1) {
        OperationStatus::Succeeded { result } => assert_eq!(result.output, "done"),
        other => panic!("expected Succeeded, got {other:?}"),
    }

    // take_result consumes; the id reports Expired afterwards.
    assert!(matches!(
        table.take_result(1),
        OperationStatus::Succeeded { .. }
    ));
    assert!(matches!(table.status(1), OperationStatus::Expired));
    assert!(matches!(table.take_result(1), OperationStatus::Expired));
    assert!(matches!(table.take_result(1), OperationStatus::Unknown));

    // Never-registered ids report Unknown.
    assert!(matches!(table.status(42), OperationStatus::Unknown));
}

#[test]
fn failed_and_cancelled_outcomes_are_distinguished() {
    let mut table = OperationTable::new(RETAINED_COMPLETED);
    table.register(1, new_token(), true);
    table.register(2, new_token(), true);

    table.complete(1, OpOutcome::Failed("boom".to_string()));
    match table.status(1) {
        OperationStatus::Failed { error } => assert_eq!(error, "boom"),
        other => panic!("expected Failed, got {other:?}"),
    }

    table.complete(2, OpOutcome::Cancelled);
    assert!(matches!(table.status(2), OperationStatus::Cancelled));

    // Terminal operations are no longer cancellable.
    assert!(!table.cancel_operation(1));
    assert_eq!(table.cancel_all(), 0);
}

#[test]
fn cancellation_hits_only_the_target_operation() {
    let mut table = OperationTable::new(RETAINED_COMPLETED);
    let token_a = new_token();
    let token_b = new_token();
    table.register(1, token_a.clone(), true);
    table.register(2, token_b.clone(), true);

    assert!(table.cancel_operation(1));
    assert!(token_a.is_cancelled());
    assert!(!token_b.is_cancelled());

    // cancel_all reaches every active operation exactly once.
    assert_eq!(table.cancel_all(), 2);
    assert!(token_b.is_cancelled());
}

#[test]
fn retention_fifo_eviction_marks_expired() {
    let mut table = OperationTable::new(2);
    for id in 1..=3 {
        table.register(id, new_token(), true);
        table.complete(id, OpOutcome::Succeeded(null_eval_result("ok")));
    }

    // Capacity 2: the oldest completion (id 1) was evicted into an Expired
    // tombstone; ids 2 and 3 are still retained.
    assert!(matches!(table.status(1), OperationStatus::Expired));
    assert!(matches!(table.status(2), OperationStatus::Succeeded { .. }));
    assert!(matches!(table.status(3), OperationStatus::Succeeded { .. }));

    // Completing another operation evicts the next-oldest (id 2).
    table.register(4, new_token(), true);
    table.complete(4, OpOutcome::Succeeded(null_eval_result("ok")));
    assert!(matches!(table.status(2), OperationStatus::Expired));
    assert!(matches!(table.status(3), OperationStatus::Succeeded { .. }));
    assert!(matches!(table.status(4), OperationStatus::Succeeded { .. }));
}

#[test]
fn synchronous_operations_are_not_retained() {
    let mut table = OperationTable::new(RETAINED_COMPLETED);
    let token = new_token();
    table.register(1, token.clone(), false);

    assert!(matches!(table.status(1), OperationStatus::Queued));

    table.complete(1, OpOutcome::Succeeded(null_eval_result("ok")));

    // Removed without a tombstone: the result was handed to the synchronous
    // caller and must not occupy the retention window.
    assert!(matches!(table.status(1), OperationStatus::Unknown));
    assert!(!table.is_known(1));
}
