use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};
struct Provider {
    kind: SEXPTYPE,
    length: i64,
    mode: Rc<Cell<u8>>,
    reads: Rc<Cell<usize>>,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.reads.set(self.reads.get() + 1);
        context.gc()?;
        (self.callback)();
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "character provider failure",
                    });
                }
                2 => panic!("character provider unwind"),
                3 => {
                    if self.kind == SEXPTYPE::INTSXP {
                        context.object().try_integer_elt(i)?;
                    } else {
                        context.object().try_real_elt(i)?;
                    }
                }
                _ => {}
            }
        }
        Ok(if self.kind == SEXPTYPE::INTSXP {
            AltrepElement::Integer([1, 2, NA_INTEGER, 3][i as usize])
        } else {
            AltrepElement::Real(if self.mode.get() == 4 {
                700000001.1234567 + i as f64
            } else {
                [1.23456789012345, -0., NA_REAL, f64::NAN][i as usize]
            })
        })
    }
}

#[test]
fn character_coercion_roots_gc_provider_and_replacement_owner() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "character-owner-gc",
            Provider {
                kind: SEXPTYPE::INTSXP,
                length: 4,
                mode: Rc::new(Cell::new(0)),
                reads: reads.clone(),
                callback: Rc::new(|| {
                    let replacement = RSession::new_for_gc_tests();
                    replacement.owner_token().unwrap().full_gc().unwrap();
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let result = factory
        .wrap(unsafe { coerceToString(source.as_raw()) })
        .unwrap();
    for (i, v) in [Some("1"), Some("2"), None, Some("3")]
        .into_iter()
        .enumerate()
    {
        assert_character(&result, i as _, v);
    }
    assert_eq!(reads.get(), 4);
    factory.require_active().unwrap();
}
#[test]
fn character_coercion_rejects_owner_revocation_before_publication() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let class = session
            .register_altrep_class(
                "character-owner-revoke",
                Provider {
                    kind: SEXPTYPE::INTSXP,
                    length: 4,
                    mode: Rc::new(Cell::new(0)),
                    reads: Rc::new(Cell::new(0)),
                    callback: Rc::new(move || unsafe {
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                coerceToString(source.as_raw())
            }))
            .is_err()
        );
    });
}

#[test]
fn character_coercion_rejects_foreign_source() {
    let original = RSession::new_for_gc_tests();
    let source = original
        .owner_token()
        .unwrap()
        .node_factory()
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                coerceToString(source.as_raw())
            }))
            .is_err()
        )
    });
}

#[test]
fn character_coercion_failure_reentry_and_denial_recover_without_workspace_leaks() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "character-failure",
            Provider {
                kind: SEXPTYPE::INTSXP,
                length: 4,
                mode: mode.clone(),
                reads: reads.clone(),
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    for _ in 0..2 {
        for fail in [1, 2, 3] {
            mode.set(fail);
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                coerceToString(source.as_raw())
            }))
            .unwrap_err();
            if fail != 2 {
                assert!(
                    failure.is::<crate::sexp::context::RError>(),
                    "provider errors must remain typed R errors"
                );
            }
            mode.set(0);
            let out = f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap();
            assert_character(&out, 0, Some("1"));
            drop(out);
            owner.full_gc().unwrap();
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }
    let class = session
        .register_altrep_class(
            "character-denial",
            Provider {
                kind: SEXPTYPE::INTSXP,
                length: 4096,
                mode,
                reads: reads.clone(),
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let big = AltrepBuilder::new(class).build().unwrap();
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let before = reads.get();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 4096, usize::MAX)))
        .unwrap();
    let denial = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        coerceToString(big.as_raw())
    }))
    .unwrap_err();
    let denial = denial
        .downcast_ref::<crate::sexp::context::RError>()
        .expect("allocation denial must retain its typed R error payload");
    assert_eq!(
        denial.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert_eq!(reads.get(), before, "output denial precedes provider reads");
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
    let out = f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap();
    assert_character(&out, 0, Some("1"));
}

struct TypedProvider(SEXPTYPE);
impl AltrepClass for TypedProvider {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(3)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        c.gc()?;
        Ok(match self.0 {
            SEXPTYPE::LGLSXP => AltrepElement::Logical([0, 1, NA_LOGICAL][i as usize]),
            SEXPTYPE::REALSXP => AltrepElement::Real([0., 3.125, NA_REAL][i as usize]),
            SEXPTYPE::CPLXSXP => AltrepElement::Complex(Rcomplex {
                r: [0., 3., NA_REAL][i as usize],
                i: [0., 2., 0.][i as usize],
            }),
            SEXPTYPE::RAWSXP => AltrepElement::Raw([0, 2, 255][i as usize]),
            _ => unreachable!(),
        })
    }
}
#[test]
fn character_coercion_copies_all_provider_types_without_materializing() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    for (kind, want) in [
        (SEXPTYPE::LGLSXP, [Some("FALSE"), Some("TRUE"), None]),
        (SEXPTYPE::REALSXP, [Some("0"), Some("3.125"), None]),
        (SEXPTYPE::CPLXSXP, [Some("0+0i"), Some("3+2i"), None]),
        (SEXPTYPE::RAWSXP, [Some("00"), Some("02"), Some("ff")]),
    ] {
        let cls = s
            .register_altrep_class(
                match kind {
                    SEXPTYPE::LGLSXP => "character-logical",
                    SEXPTYPE::REALSXP => "character-real",
                    SEXPTYPE::CPLXSXP => "character-complex",
                    _ => "character-raw",
                },
                TypedProvider(kind),
            )
            .unwrap();
        let source = AltrepBuilder::new(cls).build().unwrap();
        let out = f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap();
        for (i, v) in want.into_iter().enumerate() {
            assert_character(&out, i as _, v);
        }
        assert!(!crate::sexp::altrep::is_materialized(&source));
    }
}
fn assert_character(value: &Sexp<'_>, i: i64, want: Option<&str>) {
    let x = value.try_string_elt(i).unwrap();
    if let Some(want) = want {
        assert!(x.try_char_eq(want.as_bytes()).unwrap());
    } else {
        assert!(x.is_na_string());
    }
}
fn print_state(owner: crate::sexp::owner::OwnerToken<'_>) -> [i32; 4] {
    assert!(!unsafe { (*owner.as_ptr()).eval_state.character_print_active });
    let p = unsafe { (*owner.as_ptr()).eval_state.format_print };
    [p.digits, p.scipen, p.na_width, p.na_width_noquote]
}
#[test]
fn character_digits_context_preserves_options_and_restores_on_errors_denial_and_revocation() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let three = f.wrap(unsafe { Rf_ScalarInteger(3) }).unwrap();
    unsafe {
        crate::mainutils::options::SetOptionByName("digits", three.as_raw());
    }
    let mode = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = seen.clone();
    let cls = session
        .register_altrep_class(
            "character-digits",
            Provider {
                kind: SEXPTYPE::REALSXP,
                length: 4,
                mode: mode.clone(),
                reads: reads.clone(),
                callback: Rc::new(move || {
                    observed
                        .borrow_mut()
                        .push(unsafe { crate::mainutils::options::GetOptionDigits() })
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(cls.clone()).build().unwrap();
    let before = print_state(owner);
    let out = f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap();
    assert_character(&out, 0, Some("1.23456789012345"));
    assert_character(&out, 1, Some("0"));
    assert_character(&out, 2, None);
    assert_character(&out, 3, Some("NaN"));
    assert_eq!(
        *seen.borrow(),
        vec![3; 4],
        "pinned GNU ALTREP callback observes original option"
    );
    assert_eq!(unsafe { crate::mainutils::options::GetOptionDigits() }, 3);
    assert_eq!(print_state(owner), before);
    drop(out);
    unsafe {
        (*owner.as_ptr()).eval_state.format_print.digits = 11;
        (*owner.as_ptr()).eval_state.format_print.scipen = 12;
    }
    let before = print_state(owner);
    let out = f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap();
    assert_character(&out, 0, Some("1.23456789012345"));
    assert_eq!(print_state(owner), before);
    drop(out);
    for failure in [1, 2, 3] {
        mode.set(failure);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            coerceToString(source.as_raw())
        }))
        .unwrap_err();
        if failure != 2 {
            assert!(error.is::<crate::sexp::context::RError>());
        }
        mode.set(0);
        assert_eq!(print_state(owner), before);
        assert_eq!(unsafe { crate::mainutils::options::GetOptionDigits() }, 3);
        let out = f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap();
        assert_character(&out, 0, Some("1.23456789012345"));
    }
    // Allow the output header but deny its first new CHARSXP after provider read.
    mode.set(4);
    owner.full_gc().unwrap();
    let (resident, nodes) = owner
        .with_arena(|a| (a.total_bytes_allocated(), a.node_count()))
        .unwrap();
    let count = reads.get();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(usize::MAX, nodes + 1)))
        .unwrap();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        coerceToString(source.as_raw())
    }))
    .unwrap_err();
    assert!(error.is::<crate::sexp::context::RError>());
    assert!(reads.get() > count);
    assert_eq!(print_state(owner), before);
    assert_eq!(unsafe { crate::mainutils::options::GetOptionDigits() }, 3);
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (
                a.total_bytes_allocated(),
                a.node_count(),
                a.workspace_bytes()
            ))
            .unwrap(),
        (resident, nodes, 0)
    );
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
    mode.set(0);
    assert_character(
        &f.wrap(unsafe { coerceToString(source.as_raw()) }).unwrap(),
        0,
        Some("1.23456789012345"),
    );
    session.with_active_in(|instance| {
        let revoke = session
            .register_altrep_class(
                "character-digits-revoke",
                Provider {
                    kind: SEXPTYPE::REALSXP,
                    length: 4,
                    mode: Rc::new(Cell::new(0)),
                    reads: Rc::new(Cell::new(0)),
                    callback: Rc::new(move || unsafe {
                        crate::sexp::instance::revoke_instance_availability(instance)
                    }),
                },
            )
            .unwrap();
        let value = AltrepBuilder::new(revoke).build().unwrap();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            coerceToString(value.as_raw())
        }))
        .unwrap_err();
        assert!(error.is::<crate::sexp::context::RError>());
        assert_eq!(print_state(owner), before);
        assert_eq!(
            unsafe { (*instance).options.get("digits").unwrap().as_raw() },
            three.as_raw()
        );
    });
}

#[test]
fn character_print_context_follows_gnu_callback_defaults_and_live_decimal_mark() {
    struct ContextProvider(Rc<dyn Fn()>);
    impl AltrepClass for ContextProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            i: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            context.gc()?;
            (self.0)();
            Ok(AltrepElement::Real([1.23456789012345, 1e-8][i as usize]))
        }
    }
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let digits = factory.wrap(unsafe { Rf_ScalarInteger(3) }).unwrap();
    let scipen = factory.wrap(unsafe { Rf_ScalarInteger(0) }).unwrap();
    let changed_digits = factory.wrap(unsafe { Rf_ScalarInteger(2) }).unwrap();
    let changed_scipen = factory.wrap(unsafe { Rf_ScalarInteger(999) }).unwrap();
    let comma = factory.wrap(unsafe { Rf_mkString(c",".as_ptr()) }).unwrap();
    let dot = factory.wrap(unsafe { Rf_mkString(c".".as_ptr()) }).unwrap();
    let nested = factory
        .wrap(unsafe { Rf_ScalarReal(9.87654321098765) })
        .unwrap();
    let print_args = factory
        .wrap(unsafe { Rf_cons(nested.as_raw(), R_NilValue()) })
        .unwrap();
    for (mode, first, second) in [
        (0, "1.23456789012345", "1e-08"),
        (1, "1,23456789012345", "1e-08"),
        (2, "1.23", "1e-08"),
        (3, "1.23", "1e-08"),
        (4, "1,23456789012345", "0,00000001"),
    ] {
        unsafe {
            crate::mainutils::options::SetOptionByName("digits", digits.as_raw());
            crate::mainutils::options::SetOptionByName(
                "scipen",
                if mode == 4 {
                    changed_scipen.as_raw()
                } else {
                    scipen.as_raw()
                },
            );
            crate::mainutils::options::SetOptionByName(
                "OutDec",
                if mode == 4 {
                    comma.as_raw()
                } else {
                    dot.as_raw()
                },
            );
        }
        // The checked roots above outlive every provider invocation; capture
        // their stable addresses without extending a session borrow to static.
        let changed_digits = changed_digits.as_raw();
        let changed_scipen = changed_scipen.as_raw();
        let comma = comma.as_raw();
        let nested = nested.as_raw();
        let print_args = print_args.as_raw();
        let callback = Rc::new(move || unsafe {
            match mode {
                1 => {
                    crate::mainutils::options::SetOptionByName("digits", changed_digits);
                    crate::mainutils::options::SetOptionByName("scipen", changed_scipen);
                    crate::mainutils::options::SetOptionByName("OutDec", comma);
                }
                2 => {
                    crate::mainutils::print::do_printdefault(
                        R_NilValue(),
                        R_NilValue(),
                        print_args,
                        R_GlobalEnv(),
                    );
                }
                3 => {
                    coerceToString(nested);
                }
                _ => {}
            }
        });
        let class = session
            .register_altrep_class(
                &format!("character-context-{mode}"),
                ContextProvider(callback),
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        let before = print_state(owner);
        let output = factory
            .wrap(unsafe { coerceToString(source.as_raw()) })
            .unwrap();
        assert_character(&output, 0, Some(first));
        assert_character(&output, 1, Some(second));
        assert_eq!(print_state(owner), before);
        assert!(!unsafe { (*owner.as_ptr()).eval_state.character_print_active });
    }
}
