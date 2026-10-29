use super::*;
#[test]
fn checked_bessely_reflection_stops_after_revocation() {
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
            let arguments = args(&f, &reals(&f, &[5.0]), &reals(&f, &[-2_000_000_000.25]));
            native_y(&f, &arguments, false)
        })
    }))
    .unwrap_err();
    assert!(!weak.is_live());
    assert_eq!(
        calls.get(),
        1,
        "no reflected callback after original-owner revocation"
    );
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
fn checked_bessely_reflection_workspace_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let before = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let callback = weak.clone();
        let _warnings = WarningScope::new(move |_| {
            seen.set(seen.get() + 1);
            let bytes = crate::sexp::owner::with_runtime(&callback, |access| {
                access.with_arena(|a| a.workspace_bytes())
            })
            .unwrap()
            .unwrap();
            assert!(bytes >= before + 501 * size_of::<f64>());
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = callback.node_factory().unwrap();
            let z = native_y(&f, &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0])), false);
            assert!((z.try_real_elt(0).unwrap() - 0.08825696421567694).abs() < 1e-14);
        });
        let z = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[-500.25])),
            false,
        );
        assert_eq!(z.try_real_elt(0).unwrap(), f64::NEG_INFINITY);
        assert_eq!(calls.get(), 2);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), before);
    });
}
#[test]
fn bessely_reflection_rounding_zero_missing_and_order() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let _quiet = WarningScope::new(|_| {});
        for x in [-0.0, 0.0, 1.0, 10.0, 1e25] {
            for order in [
                -0.0_f64,
                0.0,
                -2.0,
                -1.5,
                -0.5,
                -0.25,
                0.25,
                f64::from_bits((-1.0_f64).to_bits() + 1),
                -500.25,
            ] {
                let expected = crate::special::bessel_y::bessel_y(x, order);
                let z = native_y(&f, &args(&f, &reals(&f, &[x]), &reals(&f, &[order])), false)
                    .try_real_elt(0)
                    .unwrap();
                assert!(
                    z.to_bits() == expected.to_bits() || (z.is_nan() && expected.is_nan()),
                    "x={x:?},nu={order:?},expected={expected:?},actual={z:?}"
                );
            }
        }
        let messages = Rc::new(RefCell::new(Vec::new()));
        let seen = messages.clone();
        let _warnings =
            WarningScope::new(move |message| seen.borrow_mut().push(message.to_owned()));
        for (x, order, na) in [
            (f64::NAN, -2_000_000_000.25, false),
            (crate::sexp::ffi::NA_REAL, f64::NAN, true),
            (1.0, crate::sexp::ffi::NA_REAL, true),
            (1.0, f64::NAN, false),
        ] {
            let z = native_y(&f, &args(&f, &reals(&f, &[x]), &reals(&f, &[order])), false)
                .try_real_elt(0)
                .unwrap();
            assert!(z.is_nan());
            assert_eq!(crate::sexp::ffi::R_IsNA(z), na);
            assert!(messages.borrow().is_empty());
        }
        let z = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[-2_000_000_000.25])),
            false,
        );
        assert!(z.try_real_elt(0).unwrap().is_nan());
        {
            let messages = messages.borrow();
            assert_eq!(messages.len(), 2);
            assert!(messages[0].starts_with("besselY("));
            assert!(messages[1].starts_with("besselJ("));
        }
        for (order, prefix) in [
            (-2_000_000_000.5, "besselJ("),
            (-2_000_000_000.0, "besselY("),
        ] {
            messages.borrow_mut().clear();
            let z = native_y(
                &f,
                &args(&f, &reals(&f, &[1.0]), &reals(&f, &[order])),
                false,
            );
            assert!(z.try_real_elt(0).unwrap().is_nan());
            let messages = messages.borrow();
            assert_eq!(messages.len(), 1);
            assert!(messages[0].starts_with(prefix));
        }
    });
}
