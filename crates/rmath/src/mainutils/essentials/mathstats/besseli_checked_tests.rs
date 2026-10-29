use super::*;
fn three(f: &SessionNodeFactory<'_>, a: &Sexp<'_>, b: &Sexp<'_>, c: &Sexp<'_>) -> Sexp<'static> {
    let tail = f.pairlist_cell(c, &f.nil(), &f.nil()).unwrap();
    let tail = f.pairlist_cell(b, &tail, &f.nil()).unwrap();
    f.pairlist_cell(a, &tail, &f.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
fn native_i(f: &SessionNodeFactory<'_>, arguments: &Sexp<'_>, public: bool) -> Sexp<'static> {
    let raw = unsafe {
        if public {
            super::super::do_besselI(
                f.nil().as_raw(),
                f.nil().as_raw(),
                arguments.as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            )
        } else {
            super::super::do_besselI_internal(
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
fn checked_besseli_diagnostic_vector_scaling_warning_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let z = native_i(
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
        assert!((z.try_real_elt(0).unwrap() - 1.26606587775200818).abs() < 1e-14);
        assert!((z.try_real_elt(1).unwrap() - 0.20791041534970844).abs() < 1e-14);
        assert!(crate::sexp::ffi::R_IsNA(z.try_real_elt(2).unwrap()));
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let callback = weak.clone();
        let before_workspace = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let _warnings = WarningScope::new(move |message| {
            assert_eq!(message, "bessel_i(1,nu=500): precision lost in result");
            let charged = crate::sexp::owner::with_runtime(&callback, |access| {
                access.with_arena(|a| a.workspace_bytes())
            })
            .unwrap()
            .unwrap();
            assert!(
                charged >= before_workspace + 501 * size_of::<f64>(),
                "precision callback retains admitted private work array"
            );
            seen.set(seen.get() + 1);
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = callback.node_factory().unwrap();
            let z = native_i(
                &f,
                &three(
                    &f,
                    &reals(&f, &[1.0]),
                    &reals(&f, &[0.0]),
                    &reals(&f, &[1.0]),
                ),
                false,
            );
            assert!((z.try_real_elt(0).unwrap() - 1.26606587775200818).abs() < 1e-14);
        });
        let z = native_i(
            &f,
            &three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[500.0]),
                &reals(&f, &[1.0]),
            ),
            false,
        );
        assert_eq!(z.try_real_elt(0).unwrap(), 0.0);
        assert_eq!(count.get(), 1);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            before_workspace
        );
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
fn checked_besseli_third_materialization_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let weak = session.owner_token().unwrap().weak_owner().unwrap();
    let calls = Rc::new(Cell::new(0));
    let callback = weak.clone();
    let class = session
        .register_altrep_class(
            "besseli.scaling",
            ScalingProvider {
                calls: calls.clone(),
                action: Rc::new(move || {
                    let f = callback.node_factory().unwrap();
                    let z = native_i(
                        &f,
                        &three(
                            &f,
                            &reals(&f, &[1.0]),
                            &reals(&f, &[0.0]),
                            &reals(&f, &[1.0]),
                        ),
                        false,
                    );
                    assert!((z.try_real_elt(0).unwrap() - 1.26606587775200818).abs() < 1e-14);
                }),
            },
        )
        .unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let third = AltrepBuilder::new(class).build().unwrap();
        let z = native_i(
            &f,
            &three(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0]), &third),
            false,
        );
        assert_eq!(calls.get(), 2);
        assert!((z.try_real_elt(0).unwrap() - 1.26606587775200818).abs() < 1e-14);
        assert!((z.try_real_elt(1).unwrap() - 0.46575960759364043).abs() < 1e-14);
    });
}
#[test]
fn checked_besseli_public_scaling_denial_and_retry() {
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
        drop(native_i(&f, &arguments, true));
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
        let error = catch_unwind(AssertUnwindSafe(|| native_i(&f, &arguments, true))).unwrap_err();
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
        let recovered = catch_unwind(AssertUnwindSafe(|| native_i(&f, &arguments, true)));
        let z = recovered.unwrap();
        assert_eq!(z.len(), 2);
        assert!((z.try_real_elt(1).unwrap() - 0.46575960759364043).abs() < 1e-14);
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
fn checked_besseli_warning_revocation_cannot_publish_or_charge_replacement() {
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
                &reals(&f, &[500.0]),
                &reals(&f, &[1.0]),
            );
            native_i(&f, &arguments, false)
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
fn checked_besseli_public_scaling_revocation_stops_before_output() {
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
            "besseli.public.revoke",
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
            native_i(&f, &arguments, true)
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
fn checked_besseli_rejects_foreign_entry_roots_before_traversal() {
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
                        super::super::do_besselI(call, f.nil().as_raw(), args, rho)
                    } else {
                        super::super::do_besselI_internal(call, f.nil().as_raw(), args, rho)
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
#[test]
fn checked_besseli_negative_reflection_stops_callbacks_after_revocation() {
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
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let _warnings = WarningScope::new(move |_| {
        seen.set(seen.get() + 1);
        if seen.get() != 1 {
            return;
        }
        drop(facade_callback.borrow_mut().take());
        let next = RSession::new_for_gc_tests();
        let bytes = next
            .owner_token()
            .unwrap()
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        after_revoke.replace(Some((next, bytes)));
        crate::nmath::error::set_warning_hook(Some(warning));
    });
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let arguments = three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[-2_000_000_000.5]),
                &reals(&f, &[1.0]),
            );
            native_i(&f, &arguments, false)
        })
    }))
    .unwrap_err();
    assert!(!weak.is_live());
    assert_eq!(calls.get(), 1, "no callback after owner revocation");
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
fn besseli_reflection_rounding_signed_zero_and_missing_neighbors() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let _quiet = WarningScope::new(|_| {});
        for x in [-0.0, 0.0, 1.0, 1000.0] {
            for order in [
                -0.0_f64,
                0.0,
                -2.0,
                -1.5,
                -0.5,
                0.5,
                2.0,
                f64::from_bits((-1.0_f64).to_bits() + 1),
            ] {
                for scale in [1.0, 2.0] {
                    let expected = crate::special::bessel_i::bessel_i(x, order, scale);
                    let z = native_i(
                        &f,
                        &three(
                            &f,
                            &reals(&f, &[x]),
                            &reals(&f, &[order]),
                            &reals(&f, &[scale]),
                        ),
                        false,
                    )
                    .try_real_elt(0)
                    .unwrap();
                    assert!(
                        z.to_bits() == expected.to_bits() || (z.is_nan() && expected.is_nan()),
                        "x={x:?},nu={order:?},scale={scale},expected={expected:?},actual={z:?}"
                    );
                }
            }
        }
        let messages = Rc::new(RefCell::new(Vec::new()));
        let seen = messages.clone();
        let _warnings =
            WarningScope::new(move |message| seen.borrow_mut().push(message.to_owned()));
        for (x, order, scale, na) in [
            (f64::NAN, -2_000_000_000.5, 1.0, false),
            (crate::sexp::ffi::NA_REAL, f64::NAN, 1.0, true),
            (1.0, -2_000_000_000.5, f64::NAN, false),
            (1.0, -2_000_000_000.5, crate::sexp::ffi::NA_REAL, true),
        ] {
            let z = native_i(
                &f,
                &three(
                    &f,
                    &reals(&f, &[x]),
                    &reals(&f, &[order]),
                    &reals(&f, &[scale]),
                ),
                false,
            )
            .try_real_elt(0)
            .unwrap();
            assert!(z.is_nan());
            assert_eq!(crate::sexp::ffi::R_IsNA(z), na);
            assert!(messages.borrow().is_empty());
        }
        let z = native_i(
            &f,
            &three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[-2_000_000_000.5]),
                &reals(&f, &[1.0]),
            ),
            false,
        );
        assert!(z.try_real_elt(0).unwrap().is_nan());
        let messages = messages.borrow();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].starts_with("besselI("));
        assert!(messages[1].starts_with("besselK("));
    });
}

#[path = "besseli_diagnostic_tests.rs"]
mod diagnostic_tests;
