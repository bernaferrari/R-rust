use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};

struct Provider {
    string: bool,
    mode: Rc<Cell<u8>>,
    hook: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        if self.string {
            SEXPTYPE::STRSXP
        } else {
            SEXPTYPE::REALSXP
        }
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(3)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        context.gc()?;
        (self.hook)();
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "extrema provider failure",
                    });
                }
                2 => panic!("extrema provider unwind"),
                3 => {
                    context.object().try_real_elt(i)?;
                }
                _ => {}
            }
        }
        if self.string {
            Ok(AltrepElement::String(context.string(
                if self.mode.get() == 4 {
                    ["3", "1", "1"][i as usize]
                } else {
                    ["alpha", "beta", "gamma"][i as usize]
                },
            )?))
        } else {
            Ok(AltrepElement::Real([3., 1., 1.][i as usize]))
        }
    }
}
fn run<'s>(factory: &SessionNodeFactory<'s>, source: &Sexp<'s>, minimum: bool) -> Sexp<'s> {
    let nil = factory.nil();
    let args = factory.pairlist_cell(source, &nil, &nil).unwrap();
    factory
        .wrap(unsafe {
            evaluate(
                nil.as_raw(),
                nil.as_raw(),
                args.as_raw(),
                nil.as_raw(),
                minimum,
            )
        })
        .unwrap()
}

#[test]
fn extrema_root_detached_input_and_lazy_selected_name_through_gc() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let argument_pointer: Rc<Cell<SEXP>> = Rc::new(Cell::new(std::ptr::null_mut()));
    let observed = argument_pointer.clone();
    let nil = factory.nil();
    let nil_pointer = nil.as_raw();
    let class = session
        .register_altrep_class(
            "extrema-detached-real",
            Provider {
                string: false,
                mode: Rc::new(Cell::new(0)),
                hook: Rc::new(move || unsafe {
                    let p = observed.get();
                    if !p.is_null() {
                        crate::sexp::accessors::SETCAR(p, nil_pointer);
                    }
                }),
            },
        )
        .unwrap();
    let names_class = session
        .register_altrep_class(
            "extrema-lazy-names",
            Provider {
                string: true,
                mode: Rc::new(Cell::new(0)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let source_node = source.allocation().unwrap().clone();
    let names = AltrepBuilder::new(names_class).build().unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            source.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            names.as_raw(),
        );
    }
    let args = factory.pairlist_cell(&source, &nil, &nil).unwrap();
    argument_pointer.set(args.as_raw());
    drop(names);
    drop(source);
    let output = factory
        .wrap(unsafe {
            evaluate(
                nil.as_raw(),
                nil.as_raw(),
                args.as_raw(),
                nil.as_raw(),
                true,
            )
        })
        .unwrap();
    assert_eq!(output.try_integer_elt(0).unwrap(), 2);
    let name = factory
        .wrap(unsafe {
            crate::sexp::attrib_core::getAttrib(
                output.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            )
        })
        .unwrap();
    assert_eq!(
        name.try_string_elt(0).unwrap().try_as_string().unwrap(),
        "beta"
    );
    session.with_active_in(|instance| unsafe {
        crate::sexp::gengc::full_gc_in(instance);
    });
    assert!(
        !source_node.is_live(),
        "finished selection must release its detached input root"
    );
    for (index, kind, number) in [
        (
            i64::from(i32::MAX) - 1,
            SEXPTYPE::INTSXP,
            f64::from(i32::MAX),
        ),
        (
            i64::from(i32::MAX),
            SEXPTYPE::REALSXP,
            f64::from(i32::MAX) + 1.,
        ),
    ] {
        let output = index_result(&factory, Some(index));
        assert_eq!(output.typeof_(), kind);
        let value = if kind == SEXPTYPE::INTSXP {
            f64::from(output.try_integer_elt(0).unwrap())
        } else {
            output.try_real_elt(0).unwrap()
        };
        assert_eq!(value, number);
    }
}

#[test]
fn extrema_provider_failures_recover_without_poisoning() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let mode = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "extrema-failure",
            Provider {
                string: false,
                mode: mode.clone(),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    for failure in [1, 2, 3] {
        mode.set(failure);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
                &factory, &source, true
            )))
            .is_err()
        );
        mode.set(0);
        assert_eq!(run(&factory, &source, false).try_integer_elt(0).unwrap(), 1);
    }
}

#[test]
fn extrema_reject_owner_revocation_during_provider_read() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let factory = session.owner_token().unwrap().node_factory();
        let fired = Rc::new(Cell::new(false));
        let observed = fired.clone();
        let class = session
            .register_altrep_class(
                "extrema-revoke",
                Provider {
                    string: false,
                    mode: Rc::new(Cell::new(0)),
                    hook: Rc::new(move || unsafe {
                        observed.set(true);
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
                &factory, &source, true
            )))
            .is_err()
        );
        assert!(fired.get());
    });
}

#[test]
fn extrema_reject_foreign_arguments_before_traversal() {
    let original = RSession::new_for_gc_tests();
    let factory = original.owner_token().unwrap().node_factory();
    let source = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 1)))
        .unwrap();
    let nil = factory.nil();
    let args = factory.pairlist_cell(&source, &nil, &nil).unwrap();
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
                    true,
                )
            }))
            .is_err()
        );
    });
}

#[test]
fn extrema_character_coercion_retains_gc_provider_and_selected_name() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let class = session
        .register_altrep_class(
            "extrema-character-coercion",
            Provider {
                string: true,
                mode: Rc::new(Cell::new(4)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let names = factory.strings(&["a", "b", "c"]).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            source.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            names.as_raw(),
        );
    }
    let output = run(&factory, &source, true);
    assert_eq!(output.try_integer_elt(0).unwrap(), 2);
    let selected = factory
        .wrap(unsafe {
            crate::sexp::attrib_core::getAttrib(
                output.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            )
        })
        .unwrap();
    assert_eq!(
        selected.try_string_elt(0).unwrap().try_as_string().unwrap(),
        "b"
    );
    assert!(!crate::sexp::altrep::is_materialized(&source));
}
