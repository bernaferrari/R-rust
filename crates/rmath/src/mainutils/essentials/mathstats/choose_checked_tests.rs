use crate::sexp::{
    RSession,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::SEXPTYPE,
    memory::ArenaBudget,
    object::{SessionNodeFactory, Sexp, SexpMut, SexpResult},
};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
type Hook = Rc<dyn Fn(&str)>;
thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
fn warning(message: &str) {
    let hook = HOOK.with(|slot| slot.borrow().clone());
    if let Some(hook) = hook {
        hook(message);
    }
}
struct WarningScope;
impl WarningScope {
    fn new(hook: impl Fn(&str) + 'static) -> Self {
        HOOK.with(|slot| *slot.borrow_mut() = Some(Rc::new(hook)));
        crate::nmath::error::set_warning_hook(Some(warning));
        Self
    }
}
impl Drop for WarningScope {
    fn drop(&mut self) {
        HOOK.with(|slot| slot.borrow_mut().take());
        crate::nmath::error::set_warning_hook(Some(crate::mainutils::errors::nmath_warning_hook));
    }
}
fn reals(f: &SessionNodeFactory<'_>, values: &[f64]) -> Sexp<'static> {
    let value = f
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, values.len() as i64)
                .map(|v| v.as_raw())
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
fn args(f: &SessionNodeFactory<'_>, a: &Sexp<'_>, b: &Sexp<'_>) -> Sexp<'static> {
    let tail = f.pairlist_cell(b, &f.nil(), &f.nil()).unwrap();
    f.pairlist_cell(a, &tail, &f.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
fn native(f: &SessionNodeFactory<'_>, arguments: &Sexp<'_>, log: bool) -> Sexp<'static> {
    let raw = unsafe {
        let handler = if log {
            super::do_lchoose
        } else {
            super::do_choose
        };
        handler(
            f.nil().as_raw(),
            f.nil().as_raw(),
            arguments.as_raw(),
            f.nil().as_raw(),
        )
    };
    f.wrap(raw).unwrap().into_owned().unwrap()
}
struct Provider {
    calls: Rc<Cell<usize>>,
    action: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        context.gc()?;
        (self.action)();
        Ok(AltrepElement::Real(5.0 + i as f64))
    }
}
#[test]
fn checked_choose_materializes_before_warnings_and_survives_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let calls = Rc::new(Cell::new(0));
    let nested = Rc::new(Cell::new(false));
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let action_owner = weak.clone();
    let action_nested = nested.clone();
    let class = session
        .register_altrep_class(
            "choose.collect",
            Provider {
                calls: calls.clone(),
                action: Rc::new(move || {
                    // Nested native entry explicitly activates the original owner.
                    let pin = action_owner.pin().unwrap();
                    unsafe {
                        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
                            if !action_nested.replace(true) {
                                let f = action_owner.node_factory().unwrap();
                                let result = native(
                                    &f,
                                    &args(&f, &reals(&f, &[5.0]), &reals(&f, &[2.0])),
                                    false,
                                );
                                assert_eq!(result.try_real_elt(0), Ok(10.0));
                            }
                        });
                    }
                }),
            },
        )
        .unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let input = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let arguments = args(&f, &input, &reals(&f, &[1.2, 2.6]));
        let observed = Rc::new(RefCell::new(Vec::new()));
        let messages = observed.clone();
        let callback_calls = calls.clone();
        let callback_args = arguments.clone();
        let callback_owner = weak.clone();
        let _warnings = WarningScope::new(move |message| {
            assert_eq!(
                callback_calls.get(),
                2,
                "both provider elements precede warnings"
            );
            messages.borrow_mut().push(message.to_owned());
            let f = callback_owner.node_factory().unwrap();
            unsafe {
                crate::sexp::accessors::SETCAR(callback_args.as_raw(), f.nil().as_raw());
                crate::sexp::gengc::full_gc();
            }
            let result = native(&f, &args(&f, &reals(&f, &[6.0]), &reals(&f, &[2.0])), false);
            assert_eq!(result.try_real_elt(0), Ok(15.0));
            let other = RSession::construct_detached(RSession::new_for_gc_tests);
            other.with_active(|| unsafe {
                crate::sexp::gengc::full_gc();
            });
            crate::nmath::error::set_warning_hook(Some(warning));
        });
        let output = catch_unwind(AssertUnwindSafe(|| native(&f, &arguments, false)))
            .unwrap_or_else(|payload| {
                if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
                    panic!("GC/reentry fixture: {}", error.message);
                }
                std::panic::resume_unwind(payload)
            });
        assert_eq!(output.try_real_elt(0), Ok(5.0));
        assert_eq!(output.try_real_elt(1), Ok(20.0));
        assert_eq!(
            &*observed.borrow(),
            &[
                "'k' (1.20) must be integer, rounded to 1",
                "'k' (2.60) must be integer, rounded to 3"
            ]
        );
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(output.try_real_elt(1), Ok(20.0));
    });
}
#[test]
fn checked_choose_output_denial_precedes_warning_and_retry_recovers() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = args(&f, &reals(&f, &[5.0, 6.0]), &reals(&f, &[1.2, 2.6]));
        let count = Rc::new(Cell::new(0));
        let observed = count.clone();
        let _warnings = WarningScope::new(move |_| observed.set(observed.get() + 1));
        // Initialize existing runtime protection/coercion bookkeeping before
        // isolating the fresh admitted output allocation.
        let warm = args(&f, &reals(&f, &[5.0]), &reals(&f, &[2.0]));
        drop(native(&f, &warm, false));
        drop(warm);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        let error = catch_unwind(AssertUnwindSafe(|| native(&f, &arguments, false))).unwrap_err();
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some(),
            "controlled allocation error"
        );
        assert_eq!(count.get(), 0);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let result = native(&f, &arguments, false);
        assert_eq!(result.try_real_elt(1), Ok(20.0));
        assert_eq!(count.get(), 2);
        drop(result);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    });
}
#[test]
fn checked_choose_warning_unwind_reclaims_partial_output() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = args(&f, &reals(&f, &[5.0, 6.0]), &reals(&f, &[1.2, 2.6]));
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        {
            let _warnings = WarningScope::new(|_| std::panic::panic_any(715_u32));
            let error =
                catch_unwind(AssertUnwindSafe(|| native(&f, &arguments, true))).unwrap_err();
            assert_eq!(error.downcast_ref::<u32>(), Some(&715));
        }
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        let _warnings = WarningScope::new(|_| {});
        let result = native(&f, &arguments, true);
        assert!((result.try_real_elt(0).unwrap() - 5.0_f64.ln()).abs() < 1e-14);
    });
}
#[test]
fn checked_choose_warning_revocation_cannot_publish_or_charge_replacement() {
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
            .with_arena(|a| a.total_bytes_allocated())
            .unwrap();
        after_revoke.replace(Some((next, bytes)));
    });
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let arguments = args(&f, &reals(&f, &[5.0]), &reals(&f, &[1.2]));
            native(&f, &arguments, false)
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
                .with_arena(|a| a.total_bytes_allocated())
                .unwrap(),
            *before
        );
    });
}

#[test]
fn checked_choose_altrep_revocation_stops_before_scalar_warnings() {
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
            "choose.revoke",
            Provider {
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
            let arguments = args(&f, &input, &reals(&f, &[1.2, 2.6]));
            native(&f, &arguments, false)
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
fn checked_choose_leaves_shared_nmath_callers_without_new_rounding_callbacks() {
    let callbacks = Rc::new(Cell::new(0));
    let observed = callbacks.clone();
    let _warning = WarningScope::new(move |_| observed.set(observed.get() + 1));
    assert_eq!(crate::nmath::special::choose::choose(5.0, 1.2), 5.0);
    assert_eq!(crate::nmath::special::choose::Rf_choose(5.0, 1.2), 5.0);
    assert!((crate::nmath::special::choose::lchoose(5.0, 1.2) - 5.0_f64.ln()).abs() < 1e-14);
    assert!(
        (crate::nmath::special::choose::Rf_lfastchoose(5.0, 1.2, std::ptr::null_mut())
            - 5.0_f64.ln())
        .abs()
            < 1e-14
    );
    assert!(crate::special::polygamma::psigamma(1.0, 101.0).is_nan());
    assert_eq!(callbacks.get(), 0);
}

#[test]
fn checked_psigamma_limit_warning_reentry_and_missing_controls() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let f = weak.node_factory().unwrap();
        let arguments = args(
            &f,
            &reals(&f, &[1.0]),
            &reals(
                &f,
                &[101.0, 101.5, 100.5, f64::NAN, crate::sexp::ffi::NA_REAL],
            ),
        );
        let messages = Rc::new(RefCell::new(Vec::new()));
        let seen = messages.clone();
        let callback_owner = weak.clone();
        let _warnings = WarningScope::new(move |message| {
            seen.borrow_mut().push(message.to_owned());
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = callback_owner.node_factory().unwrap();
            assert_eq!(
                native(&f, &args(&f, &reals(&f, &[5.0]), &reals(&f, &[2.0])), false)
                    .try_real_elt(0),
                Ok(10.0)
            );
        });
        let raw = unsafe {
            super::do_psigamma(
                f.nil().as_raw(),
                f.nil().as_raw(),
                arguments.as_raw(),
                f.nil().as_raw(),
            )
        };
        let result = f.wrap(raw).unwrap().into_owned().unwrap();
        assert!(result.try_real_elt(0).unwrap().is_nan());
        assert!(result.try_real_elt(1).unwrap().is_nan());
        assert!(result.try_real_elt(2).unwrap().is_finite());
        assert!(result.try_real_elt(3).unwrap().is_nan());
        assert!(crate::sexp::ffi::R_IsNA(result.try_real_elt(4).unwrap()));
        // The aggregate warning uses the runtime warning bridge, while scalar
        // warnings use the nmath hook installed above.
        assert_eq!(
            &*messages.borrow(),
            &["deriv = 101 > 100 (= n_max)", "deriv = 102 > 100 (= n_max)"]
        );
    });
}

fn native_psi(f: &SessionNodeFactory<'_>, arguments: &Sexp<'_>, _: bool) -> Sexp<'static> {
    let raw = unsafe {
        super::do_psigamma(
            f.nil().as_raw(),
            f.nil().as_raw(),
            arguments.as_raw(),
            f.nil().as_raw(),
        )
    };
    f.wrap(raw).unwrap().into_owned().unwrap()
}
#[test]
fn checked_psigamma_output_denial_precedes_warning_and_retry_recovers() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = args(&f, &reals(&f, &[5.0, 6.0]), &reals(&f, &[101.0, 102.0]));
        let count = Rc::new(Cell::new(0));
        let observed = count.clone();
        let _warnings = WarningScope::new(move |_| observed.set(observed.get() + 1));
        // Initialize existing runtime protection/coercion bookkeeping before
        // isolating the fresh admitted output allocation.
        let warm = args(&f, &reals(&f, &[5.0]), &reals(&f, &[101.0]));
        drop(native_psi(&f, &warm, false));
        drop(warm);
        count.set(0);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        let error =
            catch_unwind(AssertUnwindSafe(|| native_psi(&f, &arguments, false))).unwrap_err();
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some(),
            "controlled allocation error"
        );
        assert_eq!(count.get(), 0);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let result = native_psi(&f, &arguments, false);
        assert!(result.try_real_elt(1).unwrap().is_nan());
        assert_eq!(count.get(), 2);
        drop(result);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    });
}
#[test]
fn checked_psigamma_warning_unwind_reclaims_partial_output() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let arguments = args(&f, &reals(&f, &[5.0, 6.0]), &reals(&f, &[101.0, 102.0]));
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        {
            let _warnings = WarningScope::new(|_| std::panic::panic_any(715_u32));
            let error =
                catch_unwind(AssertUnwindSafe(|| native_psi(&f, &arguments, true))).unwrap_err();
            assert_eq!(error.downcast_ref::<u32>(), Some(&715));
        }
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        let _warnings = WarningScope::new(|_| {});
        let result = native_psi(&f, &arguments, true);
        assert!(result.try_real_elt(0).unwrap().is_nan());
    });
}
#[test]
fn checked_psigamma_warning_revocation_cannot_publish_or_charge_replacement() {
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
            .with_arena(|a| a.total_bytes_allocated())
            .unwrap();
        after_revoke.replace(Some((next, bytes)));
    });
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let arguments = args(&f, &reals(&f, &[5.0]), &reals(&f, &[101.0]));
            native_psi(&f, &arguments, false)
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
                .with_arena(|a| a.total_bytes_allocated())
                .unwrap(),
            *before
        );
    });
}

#[test]
fn checked_psigamma_altrep_revocation_stops_before_scalar_warnings() {
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
            "choose.revoke",
            Provider {
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
            let arguments = args(&f, &input, &reals(&f, &[101.0, 102.0]));
            native_psi(&f, &arguments, false)
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

fn native_j(f: &SessionNodeFactory<'_>, arguments: &Sexp<'_>, _: bool) -> Sexp<'static> {
    let raw = unsafe {
        super::do_besselJ(
            f.nil().as_raw(),
            f.nil().as_raw(),
            arguments.as_raw(),
            f.nil().as_raw(),
        )
    };
    f.wrap(raw).unwrap().into_owned().unwrap()
}
#[test]
fn checked_besselj_vectors_and_warning_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let result = native_j(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0, 1.0, 2.0])),
            false,
        );
        assert_eq!(result.len(), 3);
        for (i, x) in [0.7651976865579666, 0.44005058574493355, 0.11490348493190049]
            .into_iter()
            .enumerate()
        {
            assert!((result.try_real_elt(i as i64).unwrap() - x).abs() < 1e-14);
        }
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let callback_owner = weak.clone();
        let _warnings = WarningScope::new(move |_| {
            seen.set(seen.get() + 1);
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = callback_owner.node_factory().unwrap();
            let z = native_j(&f, &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0])), false);
            assert!((z.try_real_elt(0).unwrap() - 0.7651976865579666).abs() < 1e-14);
        });
        let result = native_j(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[500.0])),
            false,
        );
        assert_eq!(result.try_real_elt(0).unwrap(), 0.0);
        assert_eq!(calls.get(), 1);
    });
}
#[test]
fn checked_besselj_warning_revocation_cannot_publish_or_charge_replacement() {
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
            let arguments = args(&f, &reals(&f, &[5.0]), &reals(&f, &[500.0]));
            native_j(&f, &arguments, false)
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
fn checked_besselj_altrep_revocation_stops_before_scalar_warnings() {
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
            "choose.revoke",
            Provider {
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
            let arguments = args(&f, &input, &reals(&f, &[0.0, 1.0]));
            native_j(&f, &arguments, false)
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

fn native_y(f: &SessionNodeFactory<'_>, arguments: &Sexp<'_>, _: bool) -> Sexp<'static> {
    let raw = unsafe {
        super::do_besselY(
            f.nil().as_raw(),
            f.nil().as_raw(),
            arguments.as_raw(),
            f.nil().as_raw(),
        )
    };
    f.wrap(raw).unwrap().into_owned().unwrap()
}
#[test]
fn checked_bessely_vectors_and_warning_gc_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = weak.node_factory().unwrap();
        let result = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0, 1.0, 2.0])),
            false,
        );
        assert_eq!(result.len(), 3);
        for (i, x) in [
            0.08825696421567694,
            -0.7812128213002889,
            -1.6506826068162548,
        ]
        .into_iter()
        .enumerate()
        {
            assert!((result.try_real_elt(i as i64).unwrap() - x).abs() < 1e-14);
        }
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let callback_owner = weak.clone();
        let _warnings = WarningScope::new(move |_| {
            seen.set(seen.get() + 1);
            unsafe {
                crate::sexp::gengc::full_gc();
            }
            let f = callback_owner.node_factory().unwrap();
            let z = native_y(&f, &args(&f, &reals(&f, &[1.0]), &reals(&f, &[0.0])), false);
            assert!((z.try_real_elt(0).unwrap() - 0.08825696421567694).abs() < 1e-14);
        });
        let result = native_y(
            &f,
            &args(&f, &reals(&f, &[1.0]), &reals(&f, &[-500.5])),
            false,
        );
        assert_eq!(result.try_real_elt(0).unwrap(), 0.0);
        assert_eq!(calls.get(), 1);
    });
}
#[test]
fn checked_bessely_warning_revocation_cannot_publish_or_charge_replacement() {
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
            let arguments = args(&f, &reals(&f, &[5.0]), &reals(&f, &[-500.5]));
            native_y(&f, &arguments, false)
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
fn checked_bessely_altrep_revocation_stops_before_scalar_warnings() {
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
            "choose.revoke",
            Provider {
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
            let arguments = args(&f, &input, &reals(&f, &[0.0, 1.0]));
            native_y(&f, &arguments, false)
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

#[path = "besselk_checked_tests.rs"]
mod besselk_checked_tests;

#[path = "besseli_checked_tests.rs"]
mod besseli_checked_tests;

#[path = "besselj_reflection_tests.rs"]
mod besselj_reflection_tests;

#[path = "bessely_reflection_tests.rs"]
mod bessely_reflection_tests;

#[path = "bessely_diagnostic_tests.rs"]
mod bessely_diagnostic_tests;

#[path = "besselj_diagnostic_tests.rs"]
mod besselj_diagnostic_tests;
