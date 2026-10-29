use super::*;
use crate::sexp::{
    RSession, Sexp, SexpMut,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn reals(session: &RSession, values: &[f64]) -> Sexp<'static> {
    let owner = session.owner_token().unwrap();
    let value = owner
        .node_factory()
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, values.len() as i64)
                .map(|s| s.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, &x) in values.iter().enumerate() {
        value.try_set_real_elt(i as i64, x).unwrap();
    }
    value.freeze()
}
#[test]
fn registered_flags_cover_all_math_arities_and_empty_short_circuit() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let input = reals(&session, &[1.0]);
    let zero = reals(&session, &[0.0]);
    let five = reals(&session, &[5.0]);
    let eight = reals(&session, &[8.0]);
    let four = reals(&session, &[4.0]);
    let flag = session.with_active(|| unsafe {
        owner
            .sexp(Rf_mkString(c"0".as_ptr()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let x = input.as_raw();
    let z = zero.as_raw();
    let f = flag.as_raw();
    for (name, args, expected) in [
        ("dexp", vec![x, x, f], 0.36787944117144233),
        ("pexp", vec![x, x, f, z], 0.36787944117144233),
        ("dnorm", vec![x, z, x, f], 0.24197072451914337),
        ("pnorm", vec![x, z, x, f, z], 0.15865525393145705),
        (
            "dhyper",
            vec![x, five.as_raw(), eight.as_raw(), four.as_raw(), f],
            0.39160839160839161,
        ),
        (
            "phyper",
            vec![x, five.as_raw(), eight.as_raw(), four.as_raw(), f, z],
            0.51048951048951052,
        ),
    ] {
        let result = session.with_active(|| {
            let routine = lookup_call(name).unwrap();
            let raw = unsafe { routine.invoke_call(&args).unwrap() };
            owner.sexp(raw).unwrap().into_owned().unwrap()
        });
        assert!(
            (result.try_real_elt(0).unwrap() - expected).abs() < 1e-14,
            "{name}"
        );
    }
    let empty = reals(&session, &[]);
    let bad = session.with_active(|| unsafe {
        owner
            .sexp(Rf_mkString(c"bad".as_ptr()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let result = session.with_active(|| {
        let routine = lookup_call("pnorm").unwrap();
        let raw = unsafe {
            routine
                .invoke_call(&[empty.as_raw(), z, x, bad.as_raw(), bad.as_raw()])
                .unwrap()
        };
        owner.sexp(raw).unwrap().into_owned().unwrap()
    });
    assert!(result.is_empty());
}
struct Plain;
impl AltrepClass for Plain {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(
        &self,
        _: &AltrepContext<'s>,
        i: i64,
    ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
        Ok(AltrepElement::Real(i as f64))
    }
}
struct Flag {
    facade: Rc<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
}
impl AltrepClass for Flag {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        _: i64,
    ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        context.gc()?;
        let input = context.data1()?;
        assert!(crate::sexp::altrep::is_materialized(&input));
        crate::sexp::altrep::force_materialization(&input)?;
        if let Some(session) = self.facade.borrow_mut().as_mut() {
            session.close();
        }
        Ok(AltrepElement::Real(0.0))
    }
}
#[test]
fn registered_flag_providers_collect_revoke_and_are_skipped_for_empty_data() {
    for (revoke, empty) in [(false, false), (true, false), (true, true)] {
        let session = RSession::new_for_gc_tests();
        let weak = session.owner_token().unwrap().weak_owner().unwrap();
        let input = if empty {
            reals(&session, &[])
        } else {
            AltrepBuilder::new(session.register_altrep_class("dpq.input", Plain).unwrap())
                .build()
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let facade = Rc::new(RefCell::new(None));
        let calls = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class(
                "dpq.flag",
                Flag {
                    facade: facade.clone(),
                    calls: calls.clone(),
                },
            )
            .unwrap();
        let flag = AltrepBuilder::new(class)
            .data1(input.clone())
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let zero = reals(&session, &[0.0]);
        let one = reals(&session, &[1.0]);
        if revoke {
            *facade.borrow_mut() = Some(session);
        }
        let published = Cell::new(false);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::sexp::owner::with_runtime(&weak, |access| {
                access.with_native(|owner| {
                    let routine = lookup_call("pnorm").unwrap();
                    let raw = unsafe {
                        routine
                            .invoke_call(&[
                                input.as_raw(),
                                zero.as_raw(),
                                one.as_raw(),
                                flag.as_raw(),
                                zero.as_raw(),
                            ])
                            .unwrap()
                    };
                    published.set(true);
                    owner.sexp(raw)?.into_owned()
                })
            })
        }));
        if empty {
            let result = outcome.unwrap().unwrap().unwrap();
            assert!(result.is_empty());
            assert_eq!(calls.get(), 0);
            assert!(published.get());
            drop(facade.borrow_mut().take());
        } else if revoke {
            assert!(calls.get() > 0);
            assert!(!published.get());
            match outcome {
                Err(_) => {}
                Ok(value) => assert!(value.is_err()),
            };
            // Materialized owned data remains physically readable; provider
            // execution through the revoked owner must fail.
            assert_eq!(input.try_real_elt(0), Ok(0.0));
            assert!(flag.try_real_elt(0).is_err());
            drop(facade.borrow_mut().take());
        } else {
            let result = outcome.unwrap().unwrap().unwrap();
            assert!(calls.get() > 0);
            assert_eq!(result.try_real_elt(0), Ok(0.5));
            assert!((result.try_real_elt(1).unwrap() - 0.15865525393145705).abs() < 1e-14);
        }
    }
}
