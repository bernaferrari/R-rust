use super::*;
use crate::sexp::session::RSession;

fn promise(session: &RSession, allocating: bool) -> Sexp<'static> {
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let value = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(37))
            .unwrap();
        let env = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let code = if allocating {
            let name = factory
                .wrap(crate::sexp::symbol::Rf_install(c"c".as_ptr()))
                .unwrap();
            let args = factory
                .pairlist_cell(&value, &factory.nil(), &factory.nil())
                .unwrap();
            factory
                .allocate(|arena| {
                    let call = arena.alloc_node(SEXPTYPE::LANGSXP);
                    (*call).data = crate::sexp::ffi::NodeBody::List(crate::sexp::ffi::Listsxp {
                        carval: factory.domain().link(&name).unwrap(),
                        cdrval: factory.domain().link(&args).unwrap(),
                        tagval: factory.domain().link(&factory.nil()).unwrap(),
                    });
                    Some(call)
                })
                .unwrap()
        } else {
            value
        };
        factory.promise(&code, &env).unwrap().into_owned().unwrap()
    })
}

#[test]
fn owned_promise_fresh_and_cached_force_have_same_owning_identity() {
    let session = RSession::new_for_gc_tests();
    let input = promise(&session, false);
    session.with_active(|| unsafe {
        let fresh = force_promise_result(input.clone()).unwrap().unwrap();
        let cached = force_promise_result(input).unwrap().unwrap();
        assert_eq!(fresh, cached);
        assert!(
            fresh.into_owned().is_ok(),
            "fresh force must return an actual owning value"
        );
    });
}

#[test]
fn owned_promise_result_is_the_only_root_after_input_release_and_full_gc() {
    let session = RSession::new_for_gc_tests();
    let input = promise(&session, false);
    let result = session.with_active(|| unsafe { force_promise_result(input).unwrap().unwrap() });
    let node = crate::sexp::memory::checked_projection(result.as_raw())
        .unwrap()
        .1;
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(
        node.is_live(),
        "only the returned value may keep the result alive"
    );
    assert_eq!(result.try_integer_elt(0).unwrap(), 37);
}

#[test]
fn owned_promise_foreign_runtime_cannot_authenticate_fresh_or_cached_return() {
    let original = RSession::new_for_gc_tests();
    let fresh = promise(&original, false);
    let cached = promise(&original, false);
    original.with_active(|| unsafe {
        force_promise_result(cached.clone()).unwrap().unwrap();
    });
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| unsafe {
        assert!(force_promise_result(fresh.clone()).is_err());
        assert!(force_promise_result(cached).is_err());
    });
    assert_eq!(
        fresh.try_prvalue().unwrap().as_raw(),
        original.with_active(|| unsafe { R_UnboundValue() })
    );
}

fn callback_outcome(revoke: bool) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let input = promise(facade.borrow().as_ref().unwrap(), true);
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let observed = Rc::new(Cell::new(0));
    let callback_observed = observed.clone();
    let callback_facade = Rc::downgrade(&facade);
    let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                callback_observed.set(callback_observed.get() + 1);
                (*instance).memory_state.gc_force_gap = 0;
                if revoke {
                    drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                } else {
                    std::panic::panic_any(137_u64);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            force_promise_result(input)
        })
    }));
    assert_eq!(
        observed.get(),
        1,
        "actual evaluator allocation GC callback must execute"
    );
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    if revoke {
        assert!(facade.borrow().is_none());
        assert!(pin.require_live().is_err());
        assert!(
            outcome
                .expect("revocation becomes a typed failed force")
                .is_err()
        );
    } else {
        assert!(pin.require_live().is_ok());
        assert_eq!(*outcome.unwrap_err().downcast::<u64>().unwrap(), 137);
    }
}

#[test]
fn owned_promise_revocation_during_collecting_eval_denies_publication() {
    callback_outcome(true);
}

#[test]
fn owned_promise_live_collecting_eval_preserves_exact_panic_payload() {
    callback_outcome(false);
}

#[test]
fn owned_promise_closed_owner_cannot_be_replaced_by_current_runtime() {
    let mut original = RSession::new_for_gc_tests();
    let fresh = promise(&original, false);
    let cached = promise(&original, false);
    original.with_active(|| unsafe {
        force_promise_result(cached.clone()).unwrap().unwrap();
    });
    original.close();
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| unsafe {
        assert!(force_promise_result(fresh).is_err());
        assert!(force_promise_result(cached).is_err());
    });
    assert!(replacement.is_active());
}
