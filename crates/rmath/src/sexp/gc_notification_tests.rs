use super::*;
use crate::sexp::session::RSession;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[test]
fn callbacks_can_register_and_collect_without_lending_gc_state() {
    let session = RSession::new_for_gc_tests();
    let calls = Arc::new(AtomicUsize::new(0));
    let added_calls = Arc::new(AtomicUsize::new(0));
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let callback_calls = calls.clone();
    let callback_added = added_calls.clone();
    let callback_snapshots = snapshots.clone();
    session.with_active(|| {
        reset_gc_stats();
        register_gc_callback(Box::new(move |stats| {
            if callback_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                let added = callback_added.clone();
                register_gc_callback(Box::new(move |_| {
                    added.fetch_add(1, Ordering::SeqCst);
                }));
                full_gc();
                assert_eq!(get_gc_stats().collections, 2);
            }
            callback_snapshots.lock().unwrap().push(stats.collections);
        }));
        full_gc();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(added_calls.load(Ordering::SeqCst), 0);
        full_gc();
    });
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(added_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*snapshots.lock().unwrap(), vec![1, 3]);
}

#[test]
fn notification_guard_resets_after_callback_panic() {
    let session = RSession::new_for_gc_tests();
    let panic_once = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    session.with_active(|| {
        register_gc_callback(Box::new(move |_| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            if panic_once.swap(false, Ordering::SeqCst) {
                panic!("injected notification panic");
            }
        }));
        assert!(std::panic::catch_unwind(full_gc).is_err());
        full_gc();
    });
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
