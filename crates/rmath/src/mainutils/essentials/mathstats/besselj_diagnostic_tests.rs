use super::*;
#[test]
fn checked_besselj_diagnostic_precision_revocation_stops_reflection() {
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
    let _warnings = WarningScope::new(move |message| {
        seen.set(seen.get() + 1);
        if seen.get() != 1 {
            return;
        }
        assert_eq!(message, "bessel_j(1,nu=500.25): precision lost in result");
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
            let arguments = args(&f, &reals(&f, &[1.0]), &reals(&f, &[-500.25]));
            native_j(&f, &arguments, false)
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
fn checked_besselj_diagnostic_unwind_releases_workspace() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = args(&f, &reals(&f, &[1.0]), &reals(&f, &[500.0]));
        drop(native_j(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0])),
            false,
        ));
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        {
            let _warnings = WarningScope::new(|_| std::panic::panic_any(42_u32));
            let error =
                catch_unwind(AssertUnwindSafe(|| native_j(&f, &arguments, false))).unwrap_err();
            assert_eq!(error.downcast_ref::<u32>(), Some(&42));
        }
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), before.1);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
        let _warnings = WarningScope::new(|_| {});
        let z = native_j(&f, &arguments, false);
        assert_eq!(z.try_real_elt(0).unwrap(), 0.0);
    });
}

#[test]
fn checked_besselj_diagnostic_messages_cap_and_legacy_callbacks() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let messages = Rc::new(RefCell::new(Vec::new()));
        let seen = messages.clone();
        let _warnings =
            WarningScope::new(move |message| seen.borrow_mut().push(message.to_owned()));
        let mut work = [0.0; 501];
        assert!(
            crate::special::bessel_j::bessel_j_positive_status(1.0, 500.0, &mut work).is_some()
        );
        assert!(
            crate::special::bessel_j::bessel_j_positive_status(f64::INFINITY, 500.0, &mut work)
                .unwrap()
                .large_x
        );
        assert!(
            messages.borrow().is_empty(),
            "status API never invokes hooks"
        );
        assert_eq!(
            crate::special::bessel_j::bessel_j_ex(1.0, 500.0, &mut work),
            0.0
        );
        assert_eq!(
            &*messages.borrow(),
            &["bessel_j(1,nu=500): precision lost in result\n"]
        );
        messages.borrow_mut().clear();
        for (n, expected) in [
            (500.0, "bessel_j(1,nu=500): precision lost in result"),
            (-500.0, "bessel_j(1,nu=500): precision lost in result"),
            (
                1e7 + 1.0,
                "besselJ(x, nu): nu=1e+07 > 1e7; too large for bessel_j() algorithm",
            ),
        ] {
            let z = native_j(&f, &args(&f, &reals(&f, &[1.0]), &reals(&f, &[n])), false);
            if n.abs() > 1e7 {
                assert!(z.try_real_elt(0).unwrap().is_nan());
            } else {
                assert_eq!(z.try_real_elt(0).unwrap(), 0.0);
            }
            assert_eq!(&*messages.borrow(), &[expected]);
            messages.borrow_mut().clear();
        }
        // A tiny unchanged cap proves cap return admits no large work array,
        // including the Y->J reflected route.
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let cap = before + 64 * 1024;
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(cap, 0)))
            .unwrap();
        let z = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[-1e7 - 0.25])),
            false,
        );
        assert!(z.try_real_elt(0).unwrap().is_nan());
        assert_eq!(
            &*messages.borrow(),
            &[
                "besselY(x, nu): nu=1e+07 too large for bessel_y() algorithm",
                "besselJ(x, nu): nu=1e+07 > 1e7; too large for bessel_j() algorithm"
            ]
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    });
}

#[test]
fn checked_besselj_diagnostic_large_x_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let before = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let _warnings = WarningScope::new(move |message| {
            assert_eq!(message, "value out of range in 'J_bessel'\n");
            seen.set(seen.get() + 1);
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = weak.node_factory().unwrap();
            let z = native_j(&f, &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0])), false);
            assert!((z.try_real_elt(0).unwrap() - 0.7651976865579666).abs() < 1e-14);
        });
        let z = native_j(
            &f,
            &args(&f, &reals(&f, &[f64::INFINITY]), &reals(&f, &[0.0])),
            false,
        );
        assert_eq!(z.try_real_elt(0).unwrap(), 0.0);
        assert_eq!(calls.get(), 1);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), before);
    });
}
