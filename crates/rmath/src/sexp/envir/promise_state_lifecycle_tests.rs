//! Minimal managed-heap acceptance for the production promise state boundary.
#![forbid(unsafe_code)]

use super::promise_state::{Admission, Evaluation};
use crate::sexp::{RSession, SEXPTYPE, object::Sexp};

fn promise(session: &RSession) -> Sexp<'static> {
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        factory
            .promise(&factory.domain().logical(true), &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap()
    })
}

fn state(value: &Sexp<'_>) -> u16 {
    let node = value.allocation().unwrap();
    node.heap_identity()
        .node_snapshot(node)
        .unwrap()
        .sxpinfo
        .gp()
}

fn admit(value: &Sexp<'_>) -> Evaluation<'static> {
    match Admission::begin(value.clone().into_owned().unwrap()).unwrap() {
        Admission::Ready(evaluation) => evaluation,
        _ => panic!("fresh promise must admit evaluation"),
    }
}

#[test]
fn promise_state_only_owned_guard_retains_promise_through_full_gc_and_unwind() {
    let session = RSession::new_for_gc_tests();
    let value = promise(&session);
    let node = value.allocation().unwrap().clone();
    let guard = admit(&value);
    drop(value);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(node.is_live());
    assert_eq!(
        node.heap_identity()
            .node_snapshot(&node)
            .unwrap()
            .sxpinfo
            .gp(),
        1
    );
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = guard;
        std::panic::panic_any(137_u64);
    }));
    assert_eq!(*outcome.unwrap_err().downcast::<u64>().unwrap(), 137);
    assert_eq!(
        node.heap_identity()
            .node_snapshot(&node)
            .unwrap()
            .sxpinfo
            .gp(),
        2
    );
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(
        !node.is_live(),
        "the unwound guard must release its sole root"
    );
}

#[test]
fn promise_state_restart_warning_admission_prevents_reentry_before_evaluation() {
    let session = RSession::new_for_gc_tests();
    let value = promise(&session);
    drop(admit(&value));
    assert_eq!(state(&value), 2);
    let Admission::Restart(restart) = Admission::begin(value.clone()).unwrap() else {
        panic!("interrupted promise must restart");
    };
    assert!(matches!(
        Admission::begin(value.clone()).unwrap(),
        Admission::Evaluating
    ));
    drop(restart);
    assert_eq!(
        state(&value),
        1,
        "an aborted restart warning stays evaluating"
    );
}

#[test]
fn promise_state_publication_rejects_foreign_value_without_caching_then_retries() {
    let original = RSession::new_for_gc_tests();
    let value = promise(&original);
    let replacement = RSession::new_for_gc_tests();
    let foreign = promise(&replacement);
    let factory = original.owner_token().unwrap().node_factory();
    let guard = admit(&value);
    assert!(
        guard
            .publish(&factory.domain(), &foreign, &factory.nil())
            .is_err()
    );
    assert_eq!(state(&value), 2);
    assert_eq!(value.try_prvalue().unwrap(), factory.unbound());
    let Admission::Restart(restart) = Admission::begin(value.clone()).unwrap() else {
        panic!("failed publication must permit checked retry");
    };
    let result = original.with_active(|| {
        factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap()
    });
    restart
        .enter()
        .publish(&factory.domain(), &result, &factory.nil())
        .unwrap();
    assert_eq!(state(&value), 0);
    assert_eq!(value.try_prvalue().unwrap(), result);
    assert_eq!(value.try_prenv().unwrap(), factory.nil());
    let node = result.allocation().unwrap();
    assert_eq!(
        node.heap_identity()
            .node_snapshot(node)
            .unwrap()
            .sxpinfo
            .named(),
        2
    );
}

#[test]
fn promise_state_cleanup_after_owner_close_retains_only_original_physical_domain() {
    let mut original = RSession::new_for_gc_tests();
    let value = promise(&original);
    let guard = admit(&value);
    original.close();
    drop(original);
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| drop(guard));
    assert_eq!(state(&value), 2);
    assert!(replacement.is_active());
}

#[test]
fn promise_state_closed_original_domain_cannot_publish_a_cached_value() {
    let mut original = RSession::new_for_gc_tests();
    let value = promise(&original);
    let factory = original
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap()
        .node_factory()
        .unwrap();
    let domain = factory.domain();
    let guard = admit(&value);
    let truth = domain.logical(true);
    original.close();
    assert!(guard.publish(&domain, &truth, &factory.nil()).is_err());
    assert_eq!(state(&value), 2);
    assert_eq!(value.try_prvalue().unwrap(), factory.unbound());
}

#[test]
fn promise_state_full_gp_interrupted_identity_survives_collection() {
    let session = RSession::new_for_gc_tests();
    let value = promise(&session);
    let node = value.allocation().unwrap();
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).unwrap();
    header.sxpinfo.set_gp(0x100);
    heap.replace_node(node, header).unwrap();
    session.owner_token().unwrap().full_gc().unwrap();
    let Admission::Restart(restart) = Admission::begin(value.clone()).unwrap() else {
        panic!("full nonzero gp must be interrupted even with low bits clear");
    };
    assert_eq!(state(&value), 1);
    drop(restart.enter());
    assert_eq!(state(&value), 2);
}
