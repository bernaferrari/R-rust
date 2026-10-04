#![allow(unsafe_code)]
use super::*;
use std::cell::{Cell, RefCell};

struct CounterProvider {
    callbacks: Rc<Cell<usize>>,
    kind: SEXPTYPE,
}
impl CounterProvider {
    fn called(&self) {
        self.callbacks.set(self.callbacks.get() + 1);
    }
}
impl AltrepClass for CounterProvider {
    fn vector_type(&self) -> SEXPTYPE {
        self.called();
        self.kind
    }
    fn cache_in_data2(&self) -> bool {
        self.called();
        false
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        self.called();
        Ok(3)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.called();
        Ok(AltrepElement::Integer(40 + i as i32))
    }
}

#[test]
fn safe_provider_element_rejects_actual_arena_lend_before_callback() {
    let session = RSession::new_for_gc_tests();
    let callbacks = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "lend.counter",
            CounterProvider {
                callbacks: callbacks.clone(),
                kind: SEXPTYPE::INTSXP,
            },
        )
        .unwrap();
    let value = AltrepBuilder::new(class).build().unwrap();
    callbacks.set(0);
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let count = arena.node_count();
            let result = value.try_integer_elt(1);
            assert_eq!(callbacks.get(), 0, "provider ran during an arena lend");
            assert!(result.is_err());
            assert_eq!(arena.node_count(), count);
        });
    });
    assert_eq!(value.try_integer_elt(1).unwrap(), 41);
    assert_eq!(callbacks.get(), 1);
}

#[test]
fn safe_provider_registration_rejects_lend_before_configuration_callback() {
    let session = RSession::new_for_gc_tests();
    let callbacks = Rc::new(Cell::new(0));
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let count = arena.node_count();
            // Unsupported kind stops the unchanged implementation immediately
            // after its callback, avoiding any allocator or interpreter action.
            let result = session.register_altrep_class(
                "lend.unsupported",
                CounterProvider {
                    callbacks: callbacks.clone(),
                    kind: SEXPTYPE::CLOSXP,
                },
            );
            assert!(result.is_err());
            assert_eq!(callbacks.get(), 0, "configuration ran during an arena lend");
            assert_eq!(arena.node_count(), count);
        });
    });
}

#[test]
fn safe_provider_construction_and_materialization_reject_lends_and_retry() {
    let session = RSession::new_for_gc_tests();
    let callbacks = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "lend.retry",
            CounterProvider {
                callbacks: callbacks.clone(),
                kind: SEXPTYPE::INTSXP,
            },
        )
        .unwrap();
    let value = AltrepBuilder::new(class.clone()).build().unwrap();
    callbacks.set(0);
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let count = arena.node_count();
            assert_eq!(value.len(), 3);
            assert!(data1(&value).unwrap().is_nil());
            assert!(data2(&value).unwrap().is_nil());
            assert!(force_materialization(&value).is_err());
            assert!(AltrepBuilder::new(class.clone()).build().is_err());
            assert!(
                session
                    .register_altrep_class(
                        "lend.valid",
                        CounterProvider {
                            callbacks: callbacks.clone(),
                            kind: SEXPTYPE::INTSXP,
                        }
                    )
                    .is_err()
            );
            assert_eq!(callbacks.get(), 0);
            assert_eq!(arena.node_count(), count);
        });
    });
    assert_eq!(AltrepBuilder::new(class).build().unwrap().len(), 3);
    assert_eq!(callbacks.get(), 1);
    force_materialization(&value).unwrap();
    assert_eq!(callbacks.get(), 4);
    assert_eq!(value.try_integer_elt(2).unwrap(), 42);
}

#[test]
fn safe_provider_admission_keeps_sealed_builtin_reads_and_expansion_loan_safe() {
    let session = RSession::new_for_gc_tests();
    let value = new_sequence(
        session.owner_token().unwrap(),
        SEXPTYPE::INTSXP,
        3.0,
        2.0,
        8,
    )
    .unwrap();
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let count = arena.node_count();
            assert_eq!(value.try_integer_elt(7).unwrap(), 17);
            force_materialization(&value).unwrap();
            assert_eq!(value.try_integer_elt(7).unwrap(), 17);
            assert_eq!(arena.node_count(), count);
        });
    });
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
}

struct CallbackProvider(Rc<dyn Fn()>);
impl AltrepClass for CallbackProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        (self.0)();
        Ok(AltrepElement::Integer(71))
    }
}

#[test]
fn safe_provider_live_panic_preserves_exact_payload_and_releases_operation() {
    let session = RSession::new_for_gc_tests();
    let foreign = RSession::new_for_gc_tests();
    let fail = Rc::new(Cell::new(true));
    let retry = fail.clone();
    let class = session
        .register_altrep_class(
            "lend.panic",
            CallbackProvider(Rc::new(move || {
                if retry.replace(false) {
                    // The fixture retains the foreign facade while changing
                    // only ambient identity; the provider gate owns the
                    // original runtime and must preserve this live panic.
                    unsafe {
                        crate::sexp::instance::set_current_instance(
                            foreign.owner_token().unwrap().as_ptr(),
                        );
                    }
                    std::panic::panic_any(0x1234_5678_u64);
                }
            })),
        )
        .unwrap();
    let value = AltrepBuilder::new(class).build().unwrap();
    let payload =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| value.try_integer_elt(0)))
            .unwrap_err();
    assert_eq!(*payload.downcast::<u64>().unwrap(), 0x1234_5678_u64);
    assert_eq!(value.try_integer_elt(0).unwrap(), 71);
}

fn closed_provider(panic: bool) {
    let session = RSession::new_for_gc_tests();
    let facade = Rc::new(RefCell::new(None::<RSession>));
    let callback_facade = facade.clone();
    let class = session
        .register_altrep_class(
            "lend.closed",
            CallbackProvider(Rc::new(move || {
                callback_facade.borrow_mut().as_mut().unwrap().close();
                if panic {
                    std::panic::panic_any(92_u64);
                }
            })),
        )
        .unwrap();
    let value = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    *facade.borrow_mut() = Some(session);
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| value.try_integer_elt(0)));
    assert!(
        outcome.unwrap().is_err(),
        "closed runtime published provider output or panic"
    );
    assert!(value.try_integer_elt(0).is_err());
    drop(facade.borrow_mut().take());
}

#[test]
fn safe_provider_revocation_refuses_normal_publication() {
    closed_provider(false);
}

#[test]
fn safe_provider_revocation_refuses_unwind_continuation() {
    closed_provider(true);
}
