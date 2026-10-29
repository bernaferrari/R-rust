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
                        reason: "raw provider failure",
                    });
                }
                2 => panic!("raw provider unwind"),
                3 => {
                    if self.kind == SEXPTYPE::INTSXP {
                        context.object().try_integer_elt(i)?;
                    } else {
                        context.object().try_raw_elt(i)?;
                    }
                }
                _ => {}
            }
        }
        Ok(if self.kind == SEXPTYPE::INTSXP {
            AltrepElement::Integer([1, 2, 3, 4][i as usize])
        } else {
            AltrepElement::Raw([1, 0, 255, 3][i as usize])
        })
    }
}

#[test]
fn raw_coercion_roots_gc_provider_and_replacement_owner() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "raw-owner-gc",
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
        .wrap(unsafe { coerceToRaw(source.as_raw()) })
        .unwrap();
    for (i, v) in [1u8, 2, 3, 4].into_iter().enumerate() {
        assert_eq!(result.try_raw_elt(i as _).unwrap(), v);
    }
    assert_eq!(reads.get(), 4);
    factory.require_active().unwrap();
}
#[test]
fn raw_coercion_rejects_owner_revocation_before_publication() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let class = session
            .register_altrep_class(
                "raw-owner-revoke",
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
                coerceToRaw(source.as_raw())
            }))
            .is_err()
        );
    });
}

#[test]
fn raw_coercion_rejects_foreign_source() {
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
                coerceToRaw(source.as_raw())
            }))
            .is_err()
        )
    });
}

#[test]
fn raw_coercion_failure_reentry_and_denial_recover_without_workspace_leaks() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "raw-failure",
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
                coerceToRaw(source.as_raw())
            }))
            .unwrap_err();
            if fail != 2 {
                assert!(
                    failure.is::<crate::sexp::context::RError>(),
                    "provider errors must remain typed R errors"
                );
            }
            mode.set(0);
            let out = f.wrap(unsafe { coerceToRaw(source.as_raw()) }).unwrap();
            assert_eq!(out.try_raw_elt(0).unwrap(), 1);
            drop(out);
            owner.full_gc().unwrap();
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }
    let class = session
        .register_altrep_class(
            "raw-denial",
            Provider {
                kind: SEXPTYPE::INTSXP,
                length: 65536,
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
        coerceToRaw(big.as_raw())
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
    let out = f.wrap(unsafe { coerceToRaw(source.as_raw()) }).unwrap();
    assert_eq!(out.try_raw_elt(0).unwrap(), 1);
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
            SEXPTYPE::REALSXP => AltrepElement::Real([0., 255.9, 256.][i as usize]),
            SEXPTYPE::CPLXSXP => AltrepElement::Complex(Rcomplex {
                r: [0., 3., NA_REAL][i as usize],
                i: [0., 2., 0.][i as usize],
            }),
            SEXPTYPE::STRSXP => AltrepElement::String(c.string(["0", "255", "bad"][i as usize])?),
            _ => unreachable!(),
        })
    }
}
#[test]
fn raw_coercion_copies_all_provider_types_without_materializing() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    for (kind, expected) in [
        (SEXPTYPE::LGLSXP, [0u8, 1, 0]),
        (SEXPTYPE::REALSXP, [0, 255, 0]),
        (SEXPTYPE::CPLXSXP, [0, 3, 0]),
        (SEXPTYPE::STRSXP, [0, 255, 0]),
    ] {
        let class = s
            .register_altrep_class(
                match kind {
                    SEXPTYPE::LGLSXP => "raw-typed-logical",
                    SEXPTYPE::REALSXP => "raw-typed-real",
                    SEXPTYPE::CPLXSXP => "raw-typed-complex",
                    _ => "raw-typed-string",
                },
                TypedProvider(kind),
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        let out = f.wrap(unsafe { coerceToRaw(source.as_raw()) }).unwrap();
        for (i, v) in expected.into_iter().enumerate() {
            assert_eq!(out.try_raw_elt(i as _).unwrap(), v);
        }
        assert!(!crate::sexp::altrep::is_materialized(&source));
    }
}

fn wrapper<'s>(
    f: &crate::sexp::object::SessionNodeFactory<'s>,
    source: &crate::sexp::object::Sexp<'_>,
) -> crate::sexp::object::Sexp<'s> {
    let nil = f.nil();
    let args = f.pairlist_cell(source, &nil, &nil).unwrap();
    f.wrap(unsafe {
        crate::mainutils::essentials::do_as_raw(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    })
    .unwrap()
}
fn attributed<'s>(
    f: &crate::sexp::object::SessionNodeFactory<'s>,
    source: &crate::sexp::object::Sexp<'_>,
) {
    let key = f.wrap(unsafe { Rf_install(c"raw.note".as_ptr()) }).unwrap();
    let val = f.wrap(unsafe { Rf_ScalarInteger(81) }).unwrap();
    let attr = f.pairlist_cell(&val, &f.nil(), &key).unwrap();
    unsafe {
        crate::sexp::accessors::SET_ATTRIB(source.as_raw(), attr.as_raw());
    }
}
#[test]
fn raw_wrapper_copies_attributed_source_across_gc_and_preserves_errors_and_budget() {
    use crate::sexp::{memory::ArenaBudget, object::SexpMut};
    let s = RSession::new_for_gc_tests();
    let o = s.owner_token().unwrap();
    let f = o.node_factory();
    let mode = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let replaced = Rc::new(Cell::new(false));
    let seen = replaced.clone();
    let class = s
        .register_altrep_class(
            "raw-wrapper-copy",
            Provider {
                kind: SEXPTYPE::RAWSXP,
                length: 4,
                mode: mode.clone(),
                reads: reads.clone(),
                callback: Rc::new(move || {
                    if !seen.replace(true) {
                        let other = RSession::new_for_gc_tests();
                        other.owner_token().unwrap().full_gc().unwrap();
                    }
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class.clone()).build().unwrap();
    attributed(&f, &source);
    let attrs = source.try_attrib().unwrap();
    let output = wrapper(&f, &source);
    assert!(output.try_attrib().unwrap().is_nil());
    assert_eq!(source.try_attrib().unwrap().as_raw(), attrs.as_raw());
    assert!(replaced.get());
    assert_eq!(reads.get(), 4);
    for (i, v) in [1u8, 0, 255, 3].into_iter().enumerate() {
        assert_eq!(output.try_raw_elt(i as _).unwrap(), v);
    }
    let mut output = SexpMut::try_from_checked(output).unwrap();
    output.try_set_raw_elt(0, 77).unwrap();
    let output = output.freeze();
    assert_eq!(source.try_raw_elt(0).unwrap(), 1);
    let mut original = SexpMut::try_from_checked(source.clone()).unwrap();
    original.try_set_raw_elt(0, 99).unwrap();
    assert_eq!(output.try_raw_elt(0).unwrap(), 77);
    drop((original, output, attrs, source));
    for fail in [1, 2, 3] {
        let value = AltrepBuilder::new(class.clone()).build().unwrap();
        attributed(&f, &value);
        mode.set(fail);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wrapper(&f, &value)))
            .unwrap_err();
        if fail != 2 {
            assert!(error.is::<crate::sexp::context::RError>());
        }
        mode.set(0);
        assert_eq!(wrapper(&f, &value).try_raw_elt(0).unwrap(), 1);
    }
    let small = AltrepBuilder::new(class).build().unwrap();
    attributed(&f, &small);
    let bigclass = s
        .register_altrep_class(
            "raw-wrapper-denial",
            Provider {
                kind: SEXPTYPE::RAWSXP,
                length: 65536,
                mode,
                reads: reads.clone(),
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let big = AltrepBuilder::new(bigclass).build().unwrap();
    attributed(&f, &big);
    // Prebuild the argument carrier so denial concerns output admission.
    let nil = f.nil();
    let args = f.pairlist_cell(&big, &nil, &nil).unwrap();
    o.full_gc().unwrap();
    let resident = o.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let count = reads.get();
    let cap = resident + 4096;
    o.with_arena(|a| a.set_budget(ArenaBudget::new(cap, usize::MAX)))
        .unwrap();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::mainutils::essentials::do_as_raw(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    }))
    .unwrap_err();
    let error = error
        .downcast_ref::<crate::sexp::context::RError>()
        .unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert_eq!(reads.get(), count);
    o.full_gc().unwrap();
    assert_eq!(
        o.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
    assert_eq!(o.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
    assert_eq!(wrapper(&f, &small).try_raw_elt(0).unwrap(), 1);
    assert_eq!(o.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
}
#[test]
fn raw_wrapper_rejects_foreign_arguments_and_callback_revocation() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let source = f.wrap(unsafe { Rf_ScalarInteger(1) }).unwrap();
    let nil = f.nil();
    let args = f.pairlist_cell(&source, &nil, &nil).unwrap();
    {
        let other = RSession::new_for_gc_tests();
        other.with_active(|| {
            let foreignnil = other.owner_token().unwrap().node_factory().nil();
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                crate::mainutils::essentials::do_as_raw(
                    foreignnil.as_raw(),
                    foreignnil.as_raw(),
                    args.as_raw(),
                    foreignnil.as_raw(),
                )
            }))
            .unwrap_err();
            assert!(error.is::<crate::sexp::context::RError>());
        });
    }
    s.with_active_in(|instance| {
        let class = s
            .register_altrep_class(
                "raw-wrapper-revoke",
                Provider {
                    kind: SEXPTYPE::RAWSXP,
                    length: 4,
                    mode: Rc::new(Cell::new(0)),
                    reads: Rc::new(Cell::new(0)),
                    callback: Rc::new(move || unsafe {
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let value = AltrepBuilder::new(class).build().unwrap();
        attributed(&f, &value);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wrapper(&f, &value)))
            .unwrap_err();
        assert!(error.is::<crate::sexp::context::RError>());
    });
}
