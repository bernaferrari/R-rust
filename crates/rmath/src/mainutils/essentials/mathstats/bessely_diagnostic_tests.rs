use super::*;
#[test]
fn checked_bessely_diagnostic_precision_revocation_stops_reflection() {
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
        assert_eq!(message, "bessel_y(1,nu=500.25): precision lost in result");
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
fn checked_bessely_diagnostic_messages_and_legacy_callbacks() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let messages = Rc::new(RefCell::new(Vec::new()));
        let seen = messages.clone();
        let _warnings =
            WarningScope::new(move |message| seen.borrow_mut().push(message.to_owned()));
        let z = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[500.0])),
            false,
        );
        assert_eq!(z.try_real_elt(0).unwrap(), f64::NEG_INFINITY);
        assert_eq!(
            &*messages.borrow(),
            &["bessel_y(1,nu=500): precision lost in result"]
        );
        messages.borrow_mut().clear();
        let mut work = [0.0; 501];
        assert_eq!(
            crate::special::bessel_y::bessel_y(1.0, 500.0),
            f64::NEG_INFINITY
        );
        assert_eq!(
            crate::special::bessel_y::bessel_y_ex(1.0, 500.0, &mut work),
            f64::NEG_INFINITY
        );
        assert!(
            crate::special::bessel_y::bessel_y_positive_status(1.0, 500.0, &mut work).is_some()
        );
        assert_eq!(
            &*messages.borrow(),
            &[
                "bessel_y(1,nu=500): precision lost in result\n",
                "bessel_y(1,nu=500): precision lost in result\n"
            ]
        );
        messages.borrow_mut().clear();
        let z = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[1e8 + 1.0])),
            false,
        );
        assert!(z.try_real_elt(0).unwrap().is_nan());
        assert_eq!(
            &*messages.borrow(),
            &["besselY(x, nu): nu=1e+08 too large for bessel_y() algorithm"]
        );
        messages.borrow_mut().clear();
        let z = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[-500.25])),
            false,
        );
        assert_eq!(z.try_real_elt(0).unwrap(), f64::NEG_INFINITY);
        assert_eq!(messages.borrow().len(), 2);
        assert_eq!(
            messages.borrow()[0],
            "bessel_y(1,nu=500.25): precision lost in result"
        );
        assert!(messages.borrow()[1].contains("bessel_j"));
        messages.borrow_mut().clear();
        let z = native_j(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[-500.25])),
            false,
        );
        assert_eq!(z.try_real_elt(0).unwrap(), f64::INFINITY);
        assert_eq!(messages.borrow().len(), 2);
        assert!(messages.borrow()[0].contains("bessel_j"));
        assert_eq!(
            messages.borrow()[1],
            "bessel_y(1,nu=500.25): precision lost in result"
        );
    });
}
#[test]
fn checked_bessely_diagnostic_unwind_releases_workspace() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = args(&f, &reals(&f, &[1.0]), &reals(&f, &[500.0]));
        drop(native_y(
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
                catch_unwind(AssertUnwindSafe(|| native_y(&f, &arguments, false))).unwrap_err();
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
        let z = native_y(&f, &arguments, false);
        assert_eq!(z.try_real_elt(0).unwrap(), f64::NEG_INFINITY);
    });
}
