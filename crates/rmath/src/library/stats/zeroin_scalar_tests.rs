use super::{brent_root, ScalarCallbackKind, ZeroinCtx};
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::SEXPTYPE,
    object::SexpResult,
    RSession,
};
use core::ffi::c_int;
use std::{cell::Cell, rc::Rc};

struct ScalarProvider {
    kind: SEXPTYPE,
    calls: Rc<Cell<usize>>,
    revoke: bool,
    instance: *mut crate::sexp::instance::RInstance,
}

impl AltrepClass for ScalarProvider {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        context.gc()?;
        if self.revoke {
            unsafe { crate::sexp::instance::revoke_instance_availability(self.instance) };
        }
        Ok(if self.kind == SEXPTYPE::INTSXP {
            AltrepElement::Integer(7)
        } else {
            AltrepElement::Real(7.5)
        })
    }
}

#[test]
fn scalar_callback_roots_altrep_and_rejects_owner_revocation() {
    for kind in [SEXPTYPE::INTSXP, SEXPTYPE::REALSXP] {
        for revoke in [false, true] {
            let mut session = RSession::new_for_gc_tests();
            let (function, _, _) =
                session.eval_code_with_output_capture("function(x) callback_value");
            let function = function.unwrap().into_owned().unwrap();
            session.with_active_in(|instance| {
                let calls = Rc::new(Cell::new(0));
                let class = session
                    .register_altrep_class(
                        "optimizer-scalar-test",
                        ScalarProvider {
                            kind,
                            calls: calls.clone(),
                            revoke,
                            instance,
                        },
                    )
                    .unwrap();
                let value = AltrepBuilder::new(class).build().unwrap();
                let env = unsafe { crate::sexp::globals::R_GlobalEnv() };
                unsafe {
                    let name = crate::sexp::symbol::install_str("callback_value");
                    crate::sexp::envir::defineVar(name, value.as_raw(), env);
                }
                let context =
                    unsafe { ZeroinCtx::new(function.as_raw(), env, ScalarCallbackKind::Optimize) };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    context.evaluate(0.5)
                }));
                assert_eq!(calls.get(), 1);
                if revoke {
                    assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
                } else {
                    assert_eq!(
                        result.unwrap(),
                        if kind == SEXPTYPE::INTSXP { 7.0 } else { 7.5 }
                    );
                }
            });
        }
    }
}

struct NestedProvider {
    other: Rc<RSession>,
    calls: Rc<Cell<usize>>,
}
impl AltrepClass for NestedProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        self.other
            .with_active(|| self.other.owner_token().unwrap().full_gc().unwrap());
        context.gc()?;
        Ok(AltrepElement::Real(3.25))
    }
}

#[test]
fn scalar_callback_rechecks_owner_after_nested_session() {
    let mut session = RSession::new_for_gc_tests();
    let (function, _, _) = session.eval_code_with_output_capture("function(x) callback_value");
    let function = function.unwrap().into_owned().unwrap();
    let other = Rc::new(RSession::new_for_gc_tests());
    session.with_active(|| {
        let calls = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class(
                "optimizer-nested-session",
                NestedProvider {
                    other,
                    calls: calls.clone(),
                },
            )
            .unwrap();
        let value = AltrepBuilder::new(class).build().unwrap();
        let env = unsafe { crate::sexp::globals::R_GlobalEnv() };
        unsafe {
            let name = crate::sexp::symbol::install_str("callback_value");
            crate::sexp::envir::defineVar(name, value.as_raw(), env);
        }
        let context = unsafe { ZeroinCtx::new(function.as_raw(), env, ScalarCallbackKind::Zeroin) };
        assert_eq!(context.evaluate(0.5), 3.25);
        assert_eq!(calls.get(), 1);
    });
}

#[test]
fn compatibility_entry_points_reject_non_numeric_scalar() {
    use crate::sexp::object::SexpMut;
    for root in [false, true] {
        let mut session = RSession::new_for_gc_tests();
        let (function, _, _) = session.eval_code_with_output_capture("function(x) TRUE");
        let function = function.unwrap().into_owned().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let interval = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 2)))
                .unwrap();
            let mut interval = SexpMut::try_from_checked(interval).unwrap();
            interval.try_set_real_elt(0, -1.0).unwrap();
            interval.try_set_real_elt(1, 1.0).unwrap();
            let interval = interval.freeze();
            let nil = factory.nil();
            let tail = factory.pairlist_cell(&interval, &nil, &nil).unwrap();
            let args = factory.pairlist_cell(&function, &tail, &nil).unwrap();
            let env = unsafe { crate::sexp::globals::R_GlobalEnv() };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                if root {
                    super::do_uniroot(nil.as_raw(), nil.as_raw(), args.as_raw(), env)
                } else {
                    super::do_optimize(nil.as_raw(), nil.as_raw(), args.as_raw(), env)
                }
            }));
            let error = result
                .unwrap_err()
                .downcast::<crate::sexp::context::RError>()
                .unwrap();
            let expected = if root {
                "invalid function value in 'zeroin'"
            } else {
                "invalid function value in 'optimize'"
            };
            assert_eq!(error.message, expected);
        });
    }
}

#[test]
fn brent_root_keeps_gnu_callback_order_and_reporting() {
    // GNU R 4.6.2 oracle: .External2(stats:::C_zeroin2, f, 0, 2, -2, 2, 1e-8, maxiter)
    // where f records its inputs and returns x*x - 2.
    const CALLBACKS: [u64; 7] = [
        0x3ff0_0000_0000_0000,
        0x3ff5_5555_5555_5555,
        0x3ff6_b46b_46b4_6b46,
        0x3ff6_a009_72dc_45af,
        0x3ff6_a09e_256f_2c19,
        0x3ff6_a09e_667f_3c87,
        0x3ff6_a09e_6527_a396,
    ];
    for (max_iterations, root, iterations, precision, calls) in [
        (1000, 0x3ff6_a09e_667f_3c87, 7, 0x3e35_798f_1000_0000, 7),
        (1, 0x3ff5_5555_5555_5555, -1, 0x3fe5_5555_5555_5556, 2),
    ] {
        let mut recorded = Vec::new();
        let (estimate, count, width) = brent_root(0.0, 2.0, -2.0, 2.0, 1e-8, max_iterations, |x| {
            recorded.push(x.to_bits());
            x * x - 2.0
        });
        assert_eq!(estimate.to_bits(), root);
        assert_eq!(count, iterations);
        assert_eq!(width.to_bits(), precision);
        assert_eq!(recorded, CALLBACKS[..calls]);
    }

    for (fa, fb, expected, max_iterations) in [(0.0, 2.0, 0.0, 1000), (-2.0, 0.0, 2.0, c_int::MAX)]
    {
        let result = brent_root(0.0, 2.0, fa, fb, 1e-8, max_iterations, |_: f64| {
            panic!("endpoint root should not evaluate the callback")
        });
        assert_eq!(result, (expected, 0, 0.0));
    }
}
