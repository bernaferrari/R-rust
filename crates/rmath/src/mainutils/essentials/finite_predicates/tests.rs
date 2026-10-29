use super::*;
use crate::sexp::{
    altrep::{self, AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};

struct Provider {
    mode: Rc<Cell<u8>>,
    reads: Rc<Cell<usize>>,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(4)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.reads.set(self.reads.get() + 1);
        context.gc()?;
        (self.callback)();
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "predicate provider failure",
                    });
                }
                2 => panic!("predicate provider unwind"),
                3 => {
                    context.object().try_real_elt(i)?;
                }
                _ => {}
            }
        }
        Ok(AltrepElement::Real(
            [1., f64::INFINITY, crate::sexp::ffi::NA_REAL, f64::NAN][i as usize],
        ))
    }
}

fn run<'s>(factory: &SessionNodeFactory<'s>, source: &Sexp<'s>, kind: Predicate) -> Sexp<'s> {
    let nil = factory.nil();
    let args = factory.pairlist_cell(source, &nil, &nil).unwrap();
    factory
        .wrap(unsafe {
            evaluate(
                nil.as_raw(),
                nil.as_raw(),
                args.as_raw(),
                nil.as_raw(),
                kind,
            )
        })
        .unwrap()
}

#[test]
fn predicates_keep_gc_providers_lazy_and_preserve_original_owner() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let mode = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let reentered = Rc::new(Cell::new(false));
    let observed = reentered.clone();
    let class = session
        .register_altrep_class(
            "finite-predicate-gc",
            Provider {
                mode,
                reads: reads.clone(),
                callback: Rc::new(move || {
                    if observed.replace(true) {
                        return;
                    }
                    let replacement = RSession::new_for_gc_tests();
                    replacement.with_active_in(|instance| unsafe {
                        crate::sexp::gengc::full_gc_in(instance);
                    });
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    for (kind, expected) in [
        (Predicate::Finite, [1, 0, 0, 0]),
        (Predicate::Infinite, [0, 1, 0, 0]),
        (Predicate::Nan, [0, 0, 0, 1]),
    ] {
        let result = run(&factory, &source, kind);
        for (i, want) in expected.into_iter().enumerate() {
            assert_eq!(result.try_logical_elt(i as _).unwrap(), want);
        }
        factory.require_active().unwrap();
    }
    assert!(
        reentered.get(),
        "a real replacement session must execute during a provider read"
    );
    assert_eq!(reads.get(), 12);
    assert!(!altrep::is_materialized(&source));
}

#[test]
fn predicates_propagate_provider_failure_unwind_and_reentry_without_poisoning() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let mode = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "finite-predicate-failure",
            Provider {
                mode: mode.clone(),
                reads: Rc::new(Cell::new(0)),
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    for failure in [1, 2, 3] {
        mode.set(failure);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
                &factory,
                &source,
                Predicate::Finite
            )))
            .is_err()
        );
        mode.set(0);
        assert_eq!(
            run(&factory, &source, Predicate::Finite)
                .try_logical_elt(0)
                .unwrap(),
            1
        );
        factory.require_active().unwrap();
    }
}

#[test]
fn predicates_reject_callback_revocation_before_publishing_result() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let factory = session.owner_token().unwrap().node_factory();
        let fired = Rc::new(Cell::new(false));
        let observed = fired.clone();
        let class = session
            .register_altrep_class(
                "finite-predicate-revoke",
                Provider {
                    mode: Rc::new(Cell::new(0)),
                    reads: Rc::new(Cell::new(0)),
                    callback: Rc::new(move || unsafe {
                        observed.set(true);
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
                &factory,
                &source,
                Predicate::Infinite
            )))
            .is_err()
        );
        assert!(fired.get());
    });
}

#[test]
fn predicates_reject_foreign_arguments_before_dispatch() {
    let original = RSession::new_for_gc_tests();
    let factory = original.owner_token().unwrap().node_factory();
    let value = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
        .unwrap();
    let args = factory
        .pairlist_cell(&value, &factory.nil(), &factory.nil())
        .unwrap();
    let other = RSession::new_for_gc_tests();
    other.with_active_in(|_| {
        let nil = other.owner_token().unwrap().node_factory().nil();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                evaluate(
                    nil.as_raw(),
                    nil.as_raw(),
                    args.as_raw(),
                    nil.as_raw(),
                    Predicate::Nan,
                )
            }))
            .is_err()
        );
    });
}

#[test]
fn predicates_root_shape_snapshots_across_provider_mutation_and_gc() {
    use crate::sexp::{
        accessors::STRING_ELT,
        attrib_core::{R_NamesSymbol, getAttrib, setAttrib},
    };
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source_pointer = Rc::new(Cell::new(std::ptr::null_mut()));
    let pointer = source_pointer.clone();
    let new_names = factory.strings(&["new", "new", "new", "new"]).unwrap();
    let names_pointer = new_names.as_raw();
    let class = session
        .register_altrep_class(
            "predicate-shape-snapshot",
            Provider {
                mode: Rc::new(Cell::new(0)),
                reads: Rc::new(Cell::new(0)),
                callback: Rc::new(move || unsafe {
                    setAttrib(pointer.get(), R_NamesSymbol(), names_pointer);
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    source_pointer.set(source.as_raw());
    for kind in [Predicate::Finite, Predicate::Infinite, Predicate::Nan] {
        let old_names = factory.strings(&["old", "old", "old", "old"]).unwrap();
        unsafe {
            setAttrib(source.as_raw(), R_NamesSymbol(), old_names.as_raw());
        }
        drop(old_names);
        let result = run(&factory, &source, kind);
        let names = factory
            .wrap(unsafe { getAttrib(result.as_raw(), R_NamesSymbol()) })
            .unwrap();
        let expected = if matches!(kind, Predicate::Nan) {
            "new"
        } else {
            "old"
        };
        let first = factory
            .wrap(unsafe { STRING_ELT(names.as_raw(), 0) })
            .unwrap();
        assert_eq!(first.try_as_string().unwrap(), expected);
    }
}
