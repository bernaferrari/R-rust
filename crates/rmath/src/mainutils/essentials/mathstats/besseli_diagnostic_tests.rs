use super::*;
#[test]
fn checked_besseli_diagnostic_precision_revocation_stops_reflection() {
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
        assert_eq!(message, "bessel_i(1,nu=500.25): precision lost in result");
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
                &reals(&f, &[-500.25]),
                &reals(&f, &[1.0]),
            );
            native_i(&f, &arguments, false)
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
fn checked_besseli_diagnostic_unwind_releases_workspace() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = three(
            &f,
            &reals(&f, &[1.0]),
            &reals(&f, &[500.0]),
            &reals(&f, &[1.0]),
        );
        drop(native_i(
            &f,
            &three(
                &f,
                &reals(&f, &[1.0]),
                &reals(&f, &[0.0]),
                &reals(&f, &[1.0]),
            ),
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
                catch_unwind(AssertUnwindSafe(|| native_i(&f, &arguments, false))).unwrap_err();
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
        let z = native_i(&f, &arguments, false);
        assert_eq!(z.try_real_elt(0).unwrap(), 0.0);
    });
}

#[test]
fn checked_besseli_diagnostic_status_messages_and_legacy_controls() {
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
            crate::special::bessel_i::bessel_i_positive_status(1.0, 500.0, 1.0, &mut work)
                .is_some()
        );
        assert_eq!(
            crate::special::bessel_i::bessel_i_positive_status(1.0, 0.0, 3.0, &mut [0.0])
                .unwrap()
                .ncalc,
            -1
        );
        assert!(
            messages.borrow().is_empty(),
            "status computation never invokes handlers"
        );
        assert_eq!(
            crate::special::bessel_i::bessel_i_ex(1.0, 500.0, 1.0, &mut work),
            0.0
        );
        assert_eq!(
            &*messages.borrow(),
            &["full precision may not have been achieved in 'bessel_i'\n"]
        );
        messages.borrow_mut().clear();
        for (n, scale, expected) in [
            (500.0, 1.0, "bessel_i(1,nu=500): precision lost in result"),
            (-500.0, 2.0, "bessel_i(1,nu=500): precision lost in result"),
            (
                0.0,
                0.0,
                "bessel_i(1): ncalc (=-1) != nb (=1); alpha=0. Arg. out of range?",
            ),
            (
                0.0,
                3.0,
                "bessel_i(1): ncalc (=-1) != nb (=1); alpha=0. Arg. out of range?",
            ),
        ] {
            let z = native_i(
                &f,
                &three(
                    &f,
                    &reals(&f, &[1.0]),
                    &reals(&f, &[n]),
                    &reals(&f, &[scale]),
                ),
                false,
            );
            assert_eq!(
                z.try_real_elt(0).unwrap(),
                0.0,
                "port retains defined invalid-scale zero"
            );
            assert_eq!(&*messages.borrow(), &[expected]);
            messages.borrow_mut().clear();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before + 64 * 1024, 0)))
            .unwrap();
        // No GNU cap is claimed: preserve the existing bounded defensive port
        // callback sequence, including reflected I->K, under a small unchanged cap.
        for order in [1e9 + 1.0, -2e9 - 0.25] {
            let legacy = crate::special::bessel_i::bessel_i_ex(1.0, order, 1.0, &mut []);
            assert!(legacy.is_nan());
            let expected = messages.borrow().clone();
            messages.borrow_mut().clear();
            let z = native_i(
                &f,
                &three(
                    &f,
                    &reals(&f, &[1.0]),
                    &reals(&f, &[order]),
                    &reals(&f, &[1.0]),
                ),
                false,
            );
            assert!(z.try_real_elt(0).unwrap().is_nan());
            assert_eq!(*messages.borrow(), expected);
            messages.borrow_mut().clear();
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    });
}
