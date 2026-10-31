use super::*;

#[test]
fn checked_besselk_reused_workspace_survives_warning_gc_and_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let seen = Rc::new(Cell::new(0));
        let count = seen.clone();
        let nested_owner = weak.clone();
        let _warnings = WarningScope::new(move |message| {
            assert!(message.starts_with("bessel_k(1): ncalc (=-2)"));
            count.set(count.get() + 1);
            let charged = crate::sexp::owner::with_runtime(&nested_owner, |access| {
                access.with_arena(|a| a.workspace_bytes())
            })
            .unwrap()
            .unwrap();
            assert!(charged >= baseline + 3 * size_of::<f64>());
            unsafe { crate::sexp::gengc::full_gc() };
            let f = nested_owner.node_factory().unwrap();
            let nested = native_k(
                &f,
                &three(
                    &f,
                    &reals(&f, &[1.0]),
                    &reals(&f, &[0.0]),
                    &reals(&f, &[1.0]),
                ),
                false,
            );
            assert!((nested.try_real_elt(0).unwrap() - 0.42102443824070834).abs() < 1e-14);
        });
        let result = native_k(
            &f,
            &three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[0.0, 2.0, 0.0, 2.0]),
                &reals(&f, &[1.0, 1.0, 3.0, 3.0]),
            ),
            false,
        );
        assert_eq!(
            result.try_real_elt(0).unwrap(),
            result.try_real_elt(2).unwrap()
        );
        assert_eq!(
            result.try_real_elt(1).unwrap(),
            result.try_real_elt(3).unwrap()
        );
        assert_eq!(seen.get(), 2);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
    });
}

#[test]
fn checked_besselk_subnormal_ncalc_warning_is_catchable() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let callback = weak.clone();
        let _warnings = WarningScope::new(move |message| {
            assert_eq!(
                message,
                "bessel_k(9.99989e-321): ncalc (=-1) != nb (=2); alpha=0. Arg. out of range?"
            );
            seen.set(seen.get() + 1);
            let charged = crate::sexp::owner::with_runtime(&callback, |access| {
                access.with_arena(|a| a.workspace_bytes())
            })
            .unwrap()
            .unwrap();
            assert!(charged >= baseline + 2 * size_of::<f64>());
            unsafe { crate::sexp::gengc::full_gc() };
        });
        let result = native_k(
            &f,
            &three(
                &f,
                &reals(&f, &[1e-320]),
                &reals(&f, &[1.0]),
                &reals(&f, &[1.0]),
            ),
            false,
        );
        assert_eq!(result.try_real_elt(0).unwrap(), 0.0);
        assert_eq!(count.get(), 1);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
    });
}

#[test]
fn checked_besselk_ncalc_warning_stops_after_owner_revocation() {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let original = pin.as_ptr();
    let ledger = unsafe { (*original).heap_identity.byte_ledger() };
    let before = ledger.workspace_bytes_for_test();
    let seen = Rc::new(Cell::new(0));
    let calls = seen.clone();
    let revoke = facade.clone();
    let _warnings = WarningScope::new(move |message| {
        assert_eq!(
            message,
            "bessel_k(9.99989e-321): ncalc (=-1) != nb (=2); alpha=0. Arg. out of range?"
        );
        calls.set(calls.get() + 1);
        drop(revoke.borrow_mut().take());
    });
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let arguments = three(
                &f,
                &reals(&f, &[1e-320]),
                &reals(&f, &[1.0]),
                &reals(&f, &[1.0]),
            );
            native_k(&f, &arguments, false)
        })
    }))
    .unwrap_err();
    assert_eq!(seen.get(), 1);
    assert!(!weak.is_live());
    assert_eq!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    assert_eq!(ledger.workspace_bytes_for_test(), before);
}
fn three(f: &SessionNodeFactory<'_>, a: &Sexp<'_>, b: &Sexp<'_>, c: &Sexp<'_>) -> Sexp<'static> {
    let tail = f.pairlist_cell(c, &f.nil(), &f.nil()).unwrap();
    let tail = f.pairlist_cell(b, &tail, &f.nil()).unwrap();
    f.pairlist_cell(a, &tail, &f.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
fn native_k(f: &SessionNodeFactory<'_>, arguments: &Sexp<'_>, public: bool) -> Sexp<'static> {
    let raw = unsafe {
        if public {
            super::super::do_besselK(
                f.nil().as_raw(),
                f.nil().as_raw(),
                arguments.as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            )
        } else {
            super::super::do_besselK_internal(
                f.nil().as_raw(),
                f.nil().as_raw(),
                arguments.as_raw(),
                f.nil().as_raw(),
            )
        }
    };
    f.wrap(raw).unwrap().into_owned().unwrap()
}
#[test]
fn checked_besselk_vector_scaling_warning_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let z = native_k(
            &f,
            &three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[0.0, 1.0]),
                &reals(&f, &[1.0, 2.0, crate::sexp::ffi::NA_REAL]),
            ),
            false,
        );
        assert_eq!(z.len(), 3);
        assert!((z.try_real_elt(0).unwrap() - 0.42102443824070834).abs() < 1e-14);
        assert!((z.try_real_elt(1).unwrap() - 1.6361534862632581).abs() < 1e-14);
        assert!(crate::sexp::ffi::R_IsNA(z.try_real_elt(2).unwrap()));
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let callback = weak.clone();
        let _warnings = WarningScope::new(move |_| {
            seen.set(seen.get() + 1);
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = callback.node_factory().unwrap();
            let z = native_k(
                &f,
                &three(
                    &f,
                    &reals(&f, &[1.0]),
                    &reals(&f, &[0.0]),
                    &reals(&f, &[1.0]),
                ),
                false,
            );
            assert!((z.try_real_elt(0).unwrap() - 0.42102443824070834).abs() < 1e-14);
        });
        let z = native_k(
            &f,
            &three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[2e9]),
                &reals(&f, &[1.0]),
            ),
            false,
        );
        assert!(z.try_real_elt(0).unwrap().is_nan());
        assert_eq!(count.get(), 1);
    });
}
struct ScalingProvider {
    calls: Rc<Cell<usize>>,
    action: Rc<dyn Fn()>,
}
impl AltrepClass for ScalingProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        c.gc()?;
        (self.action)();
        Ok(AltrepElement::Real(1.0 + i as f64))
    }
}
#[test]
fn checked_besselk_third_materialization_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let weak = session.owner_token().unwrap().weak_owner().unwrap();
    let calls = Rc::new(Cell::new(0));
    let callback = weak.clone();
    let class = session
        .register_altrep_class(
            "besselk.scaling",
            ScalingProvider {
                calls: calls.clone(),
                action: Rc::new(move || {
                    let f = callback.node_factory().unwrap();
                    let z = native_k(
                        &f,
                        &three(
                            &f,
                            &reals(&f, &[1.0]),
                            &reals(&f, &[0.0]),
                            &reals(&f, &[1.0]),
                        ),
                        false,
                    );
                    assert!((z.try_real_elt(0).unwrap() - 0.42102443824070834).abs() < 1e-14);
                }),
            },
        )
        .unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let third = AltrepBuilder::new(class).build().unwrap();
        let z = native_k(
            &f,
            &three(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0]), &third),
            false,
        );
        assert_eq!(calls.get(), 2);
        assert!((z.try_real_elt(0).unwrap() - 0.42102443824070834).abs() < 1e-14);
        assert!((z.try_real_elt(1).unwrap() - 1.1444630798068949).abs() < 1e-14);
    });
}
#[test]
fn checked_besselk_public_scaling_denial_and_retry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = three(
            &f,
            &reals(&f, &[1.0]),
            &reals(&f, &[0.0]),
            &reals(&f, &[0.0, 1.0]),
        );
        drop(native_k(&f, &arguments, true));
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        let blocker = reals(&f, &[0.0; 256]);
        let cap = owner
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(cap, 0)))
            .unwrap();
        let error = catch_unwind(AssertUnwindSafe(|| native_k(&f, &arguments, true))).unwrap_err();
        let ordinary = catch_unwind(AssertUnwindSafe(|| unsafe {
            crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, 1)
        }))
        .unwrap_err();
        let error = error
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("canonical RError, not a generic panic");
        let ordinary = ordinary
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("ordinary allocation RError");
        assert_eq!(error.message, ordinary.message);
        assert_eq!(
            error.message,
            "R allocation failed: memory or node budget exceeded"
        );
        drop(blocker);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
        let ordinary = unsafe { crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, 1) };
        let ordinary = f.wrap(ordinary).unwrap().into_owned().unwrap();
        assert_eq!(ordinary.len(), 1);
        drop(ordinary);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let recovered = catch_unwind(AssertUnwindSafe(|| native_k(&f, &arguments, true)));
        let z = recovered.unwrap();
        assert_eq!(z.len(), 2);
        assert!((z.try_real_elt(1).unwrap() - 1.1444630798068949).abs() < 1e-14);
        drop(z);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}
#[test]
fn checked_besselk_warning_revocation_cannot_publish_or_charge_replacement() {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let original = pin.as_ptr();
    let replacement = Rc::new(RefCell::new(None));
    let after_revoke = replacement.clone();
    let facade_callback = facade.clone();
    let _warnings = WarningScope::new(move |_| {
        drop(facade_callback.borrow_mut().take());
        let next = RSession::new_for_gc_tests();
        let bytes = next
            .owner_token()
            .unwrap()
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        after_revoke.replace(Some((next, bytes)));
    });
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let arguments = three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[2e9]),
                &reals(&f, &[1.0]),
            );
            native_k(&f, &arguments, false)
        })
    }))
    .unwrap_err();
    assert!(!weak.is_live());
    assert_eq!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    let replacement = replacement.borrow();
    let (session, before) = replacement.as_ref().unwrap();
    session.with_active(|| {
        assert_eq!(
            session
                .owner_token()
                .unwrap()
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            *before
        );
    });
}
#[test]
fn checked_besselk_public_scaling_revocation_stops_before_output() {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let original = pin.as_ptr();
    let calls = Rc::new(Cell::new(0));
    let callback_facade = facade.clone();
    let class = facade
        .borrow()
        .as_ref()
        .unwrap()
        .register_altrep_class(
            "besselk.public.revoke",
            ScalingProvider {
                calls: calls.clone(),
                action: Rc::new(move || {
                    drop(callback_facade.borrow_mut().take());
                }),
            },
        )
        .unwrap()
        .into_owned()
        .unwrap();
    let warnings = Rc::new(Cell::new(0));
    let observed = warnings.clone();
    let _warnings = WarningScope::new(move |_| observed.set(observed.get() + 1));
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            let arguments = three(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0]), &input);
            native_k(&f, &arguments, true)
        })
    }))
    .unwrap_err();
    assert!(!weak.is_live());
    assert_eq!(calls.get(), 1);
    assert_eq!(warnings.get(), 0);
    assert_eq!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
}

#[test]
fn checked_besselk_rejects_foreign_entry_roots_before_traversal() {
    let original = RSession::new_for_gc_tests();
    let other = RSession::construct_detached(RSession::new_for_gc_tests);
    let foreign = other.with_active(|| {
        let f = other.owner_token().unwrap().node_factory();
        three(
            &f,
            &reals(&f, &[1.0]),
            &reals(&f, &[0.0]),
            &reals(&f, &[1.0]),
        )
    });
    original.with_active(|| {
        let f = original.owner_token().unwrap().node_factory();
        let own = three(
            &f,
            &reals(&f, &[1.0]),
            &reals(&f, &[0.0]),
            &reals(&f, &[1.0]),
        );
        for public in [false, true] {
            for field in 0..3 {
                let call = if field == 0 {
                    foreign.as_raw()
                } else {
                    f.nil().as_raw()
                };
                let args = if field == 1 {
                    foreign.as_raw()
                } else {
                    own.as_raw()
                };
                let rho = if field == 2 {
                    foreign.as_raw()
                } else {
                    unsafe { crate::sexp::globals::R_GlobalEnv() }
                };
                let error = catch_unwind(AssertUnwindSafe(|| unsafe {
                    if public {
                        super::super::do_besselK(call, f.nil().as_raw(), args, rho)
                    } else {
                        super::super::do_besselK_internal(call, f.nil().as_raw(), args, rho)
                    }
                }))
                .unwrap_err();
                assert!(
                    error
                        .downcast_ref::<crate::sexp::context::RError>()
                        .is_some(),
                    "controlled owner failure before argument reads"
                );
            }
        }
    });
}
