//! Real allocation-triggered GC in the restart-warning signaling phase.
use super::{force_promise_result, promise_state::Admission};
use crate::sexp::RSession;
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn warning_callback(revoke: bool) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let input = facade.borrow().as_ref().unwrap().with_active(|| {
        let facade = facade.borrow();
        let factory = facade
            .as_ref()
            .unwrap()
            .owner_token()
            .unwrap()
            .node_factory();
        factory
            .promise(&factory.domain().logical(true), &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let Admission::Ready(guard) = Admission::begin(input.clone()).unwrap() else {
        panic!("fresh test promise must admit evaluation");
    };
    drop(guard);
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let pointer = pin.as_ptr();
    let unbound = weak.node_factory().unwrap().unbound();
    let observed = Rc::new(Cell::new(0));
    let callback_observed = observed.clone();
    let callback_facade = Rc::downgrade(&facade);
    let callback_input = input.clone();
    let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
        // The physical pin and owning input outlive actual warning callbacks.
        crate::sexp::session::with_instance_active(pointer, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                callback_observed.set(callback_observed.get() + 1);
                (*pointer).memory_state.gc_force_gap = 0;
                let node = callback_input.allocation().unwrap();
                assert_eq!(
                    node.heap_identity()
                        .node_snapshot(node)
                        .unwrap()
                        .sxpinfo
                        .gp(),
                    1
                );
                if revoke {
                    drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                }
                std::panic::panic_any(137_u64);
            }));
            (*pointer).memory_state.gc_force_gap = 1;
            (*pointer).memory_state.gc_force_wait = 1;
            force_promise_result(input.clone())
        })
    }));
    assert_eq!(
        observed.get(),
        1,
        "actual restart-warning allocation must collect"
    );
    // This pinned original memory remains available for passive cleanup checks.
    assert_eq!(unsafe { (*pointer).memory_state.in_gc }, 0);
    let node = input.allocation().unwrap();
    assert_eq!(
        node.heap_identity()
            .node_snapshot(node)
            .unwrap()
            .sxpinfo
            .gp(),
        1
    );
    assert_eq!(input.try_prvalue().unwrap(), unbound);
    if revoke {
        assert!(facade.borrow().is_none());
        assert!(pin.require_live().is_err());
        assert!(
            outcome
                .expect("revoked warning must become a checked failed force")
                .is_err()
        );
    } else {
        assert!(pin.require_live().is_ok());
        assert_eq!(*outcome.unwrap_err().downcast::<u64>().unwrap(), 137);
    }
}

#[test]
fn promise_warning_live_gc_unwind_preserves_exact_panic_and_warning_phase_state() {
    warning_callback(false);
}

#[test]
fn promise_warning_revoked_gc_unwind_denies_error_publication() {
    warning_callback(true);
}
