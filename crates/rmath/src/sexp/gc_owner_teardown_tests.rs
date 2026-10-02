use super::*;
use crate::sexp::gengc::{
    collect_with_environment_protects, full_gc, register_gc_callback, run_pending_gc_if_quiescent,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

thread_local! {
    static AMBIENT_OWNER: RefCell<Option<RSession>> = const { RefCell::new(None) };
    static TEARDOWN_FINALIZERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn assert_installations_cleared() {
    assert!(super::super::instance::current_instance_ptr().is_none());
    assert!(unsafe { rmath_nmath::rng::swap_rng(None) }.is_none());
    let mut temporary = rmath_nmath::MathState::default();
    let previous = rmath_nmath::state::replace_state(&mut temporary);
    rmath_nmath::state::restore_state(None);
    assert!(previous.is_none());
}

#[test]
fn safe_gc_callback_can_destroy_ambient_owner_without_later_notification() {
    let session = RSession::new_for_gc_tests();
    let observer = unsafe { super::super::instance::instance_liveness(session.instance_ptr()) };
    let later_calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = later_calls.clone();
    register_gc_callback(Box::new(|_| {
        let owner = AMBIENT_OWNER.with(|slot| slot.borrow_mut().take());
        drop(owner);
    }));
    register_gc_callback(Box::new(move |_| {
        callback_calls.fetch_add(1, Ordering::SeqCst);
    }));
    AMBIENT_OWNER.with(|slot| {
        assert!(slot.borrow_mut().replace(session).is_none());
    });
    // The collecting session is owned by TLS, with no borrow protecting it.
    // Both the callback and this public collection call use safe Rust only.
    full_gc();
    assert!(!observer.is_live());
    assert_eq!(later_calls.load(Ordering::SeqCst), 0);
    assert_installations_cleared();
}

#[test]
fn destroying_ambient_owner_then_panicking_clears_activation_on_unwind() {
    let session = RSession::new_for_gc_tests();
    let observer = unsafe { super::super::instance::instance_liveness(session.instance_ptr()) };
    register_gc_callback(Box::new(|_| {
        let owner = AMBIENT_OWNER.with(|slot| slot.borrow_mut().take());
        drop(owner);
        panic!("injected panic after owner teardown");
    }));
    AMBIENT_OWNER.with(|slot| {
        assert!(slot.borrow_mut().replace(session).is_none());
    });
    assert!(catch_unwind(full_gc).is_err());
    assert!(!observer.is_live());
    assert_installations_cleared();
    let fresh = RSession::new_for_gc_tests();
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    register_gc_callback(Box::new(move |_| {
        callback_calls.fetch_add(1, Ordering::SeqCst);
    }));
    fresh.gc();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn collection_cleanup_skips_retired_owner_for_full_minor_and_pending_gc() {
    unsafe extern "C" fn unexpected_finalizer(_: *mut std::ffi::c_void) {
        TEARDOWN_FINALIZERS.set(TEARDOWN_FINALIZERS.get() + 1);
    }
    for mode in 0..3 {
        TEARDOWN_FINALIZERS.set(0);
        let session = RSession::new_for_gc_tests();
        let owner = session.instance_ptr();
        let observer = unsafe { super::super::instance::instance_liveness(owner) };
        // An environment binding makes the explicit collection preamble add a
        // real temporary protection entry, whose cleanup must skip a dead owner.
        let value = session
            .sexp(unsafe { super::super::constructors::Rf_ScalarInteger(39) })
            .unwrap();
        assert!(session.define_var("protected_binding", value));
        // Only fixture setup uses raw runtime state. The triggering collection
        // and the callback that destroys its owner are public safe Rust calls.
        unsafe {
            (*owner).gc_state.gc_pending = true;
            (*owner).memory_state.pending_finalizers.push(
                crate::mainutils::memory_main::PendingFinalizer::C {
                    obj: R_NilValue(),
                    fun: unexpected_finalizer,
                    ready: true,
                    onexit: false,
                },
            );
        }
        let later_calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = later_calls.clone();
        register_gc_callback(Box::new(|_| {
            let owner = AMBIENT_OWNER.with(|slot| slot.borrow_mut().take());
            drop(owner);
        }));
        register_gc_callback(Box::new(move |_| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
        }));
        AMBIENT_OWNER.with(|slot| {
            assert!(slot.borrow_mut().replace(session).is_none());
        });
        match mode {
            0 => {
                collect_with_environment_protects(true);
            }
            1 => {
                collect_with_environment_protects(false);
            }
            _ => run_pending_gc_if_quiescent(),
        }
        assert!(!observer.is_live(), "mode {mode} did not collect");
        assert_eq!(later_calls.load(Ordering::SeqCst), 0, "mode {mode}");
        assert_eq!(
            TEARDOWN_FINALIZERS.get(),
            0,
            "mode {mode} ran a finalizer after teardown"
        );
        assert_installations_cleared();
    }
}
