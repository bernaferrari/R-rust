use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};
struct Provider {
    length: i64,
    mode: Rc<Cell<u8>>,
    reads: Rc<Cell<usize>>,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
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
                        reason: "logical provider failure",
                    });
                }
                2 => panic!("logical provider unwind"),
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

#[test]
fn logical_coercion_roots_gc_provider_and_replacement_owner() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "logical-owner-gc",
            Provider {
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
        .wrap(unsafe { coerceToLogical(source.as_raw()) })
        .unwrap();
    for (i, v) in [1, 1, NA_LOGICAL, NA_LOGICAL].into_iter().enumerate() {
        assert_eq!(result.try_logical_elt(i as _).unwrap(), v);
    }
    assert_eq!(reads.get(), 4);
    factory.require_active().unwrap();
}
#[test]
fn logical_coercion_rejects_owner_revocation_before_publication() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let class = session
            .register_altrep_class(
                "logical-owner-revoke",
                Provider {
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
                coerceToLogical(source.as_raw())
            }))
            .is_err()
        );
    });
}

#[test]
fn logical_coercion_rejects_foreign_source() {
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
                coerceToLogical(source.as_raw())
            }))
            .is_err()
        )
    });
}

#[test]
fn logical_coercion_failure_reentry_and_denial_recover_without_workspace_leaks() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "logical-failure",
            Provider {
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
                coerceToLogical(source.as_raw())
            }))
            .unwrap_err();
            if fail != 2 {
                assert!(
                    failure.is::<crate::sexp::context::RError>(),
                    "provider errors must remain typed R errors"
                );
            }
            mode.set(0);
            let out = f.wrap(unsafe { coerceToLogical(source.as_raw()) }).unwrap();
            assert_eq!(out.try_logical_elt(0).unwrap(), 1);
            drop(out);
            owner.full_gc().unwrap();
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }
    let class = session
        .register_altrep_class(
            "logical-denial",
            Provider {
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
        coerceToLogical(big.as_raw())
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
    let out = f.wrap(unsafe { coerceToLogical(source.as_raw()) }).unwrap();
    assert_eq!(out.try_logical_elt(0).unwrap(), 1);
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
            SEXPTYPE::INTSXP => AltrepElement::Integer([0, -3, NA_INTEGER][i as usize]),
            SEXPTYPE::CPLXSXP => AltrepElement::Complex(Rcomplex {
                r: [0., 0., NA_REAL][i as usize],
                i: [0., 2., 0.][i as usize],
            }),
            SEXPTYPE::STRSXP => {
                AltrepElement::String(c.string(["false", "True", "other"][i as usize])?)
            }
            SEXPTYPE::RAWSXP => AltrepElement::Raw([0, 2, 255][i as usize]),
            _ => unreachable!(),
        })
    }
}
#[test]
fn logical_coercion_copies_all_provider_types_without_materializing() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    for (kind, expected) in [
        (SEXPTYPE::INTSXP, [0, 1, NA_LOGICAL]),
        (SEXPTYPE::CPLXSXP, [0, 1, NA_LOGICAL]),
        (SEXPTYPE::STRSXP, [0, 1, NA_LOGICAL]),
        (SEXPTYPE::RAWSXP, [0, 1, 1]),
    ] {
        let class = session
            .register_altrep_class(
                match kind {
                    SEXPTYPE::INTSXP => "logical-typed-integer",
                    SEXPTYPE::CPLXSXP => "logical-typed-complex",
                    SEXPTYPE::STRSXP => "logical-typed-string",
                    _ => "logical-typed-raw",
                },
                TypedProvider(kind),
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        let out = f.wrap(unsafe { coerceToLogical(source.as_raw()) }).unwrap();
        for (i, v) in expected.into_iter().enumerate() {
            assert_eq!(out.try_logical_elt(i as _).unwrap(), v);
        }
        assert!(!crate::sexp::altrep::is_materialized(&source));
    }
}
