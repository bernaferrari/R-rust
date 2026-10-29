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
fn integer(f: &SessionNodeFactory<'_>, length: i64) -> Sexp<'static> {
    let value = f
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::INTSXP, length)
                .map(|v| v.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    if length > 0 {
        value.try_set_integer_elt(0, 5).unwrap();
    }
    value.freeze()
}
fn real(f: &SessionNodeFactory<'_>, x: f64) -> Sexp<'static> {
    let value = f
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, 1)
                .map(|v| v.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    value.try_set_real_elt(0, x).unwrap();
    value.freeze()
}
fn set_class(f: &SessionNodeFactory<'_>, input: &Sexp<'_>, classes: &Sexp<'_>) {
    let tag = f
        .wrap(unsafe { crate::attrib_core::R_ClassSymbol() })
        .unwrap();
    let attributes = f.pairlist_cell(classes, &f.nil(), &tag).unwrap();
    unsafe {
        crate::sexp::accessors::SET_ATTRIB(input.as_raw(), attributes.as_raw());
        crate::sexp::accessors::SET_OBJECT(input.as_raw(), 1);
    }
}
fn native(f: &SessionNodeFactory<'_>, a: &Sexp<'_>, b: &Sexp<'_>) -> Sexp<'static> {
    let tail = f.pairlist_cell(b, &f.nil(), &f.nil()).unwrap();
    let arguments = f.pairlist_cell(a, &tail, &f.nil()).unwrap();
    f.wrap(unsafe {
        super::do_choose(
            f.nil().as_raw(),
            f.nil().as_raw(),
            arguments.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
    .into_owned()
    .unwrap()
}
struct Classes {
    action: Rc<dyn Fn()>,
    calls: Rc<Cell<usize>>,
}
impl AltrepClass for Classes {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, context: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(context.data1()?.len())
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let child = context.data1()?.try_string_elt(i)?.into_owned()?;
        context.gc()?;
        (self.action)();
        Ok(AltrepElement::String(child))
    }
}
#[test]
fn choose_factor_class_remains_rooted_across_detach_gc_and_reentry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let target = Rc::new(RefCell::new(None::<Sexp<'static>>));
    let callback_target = target.clone();
    let callback_owner = weak.clone();
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "choose.factor.collect",
            Classes {
                calls: calls.clone(),
                action: Rc::new(move || {
                    let pin = callback_owner.pin().unwrap();
                    unsafe {
                        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
                            let f = callback_owner.node_factory().unwrap();
                            let input = callback_target.borrow().as_ref().unwrap().clone();
                            crate::sexp::accessors::SET_ATTRIB(input.as_raw(), f.nil().as_raw());
                            crate::sexp::accessors::SET_OBJECT(input.as_raw(), 0);
                            crate::sexp::gengc::full_gc();
                            assert_eq!(
                                native(&f, &real(&f, 6.0), &real(&f, 2.0)).try_real_elt(0),
                                Ok(15.0)
                            );
                        });
                    }
                }),
            },
        )
        .unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let data = f.strings(&["foo", "factor"]).unwrap();
        let child = data
            .try_string_elt(1)
            .unwrap()
            .allocation()
            .unwrap()
            .clone();
        let classes = AltrepBuilder::new(class)
            .data1(data)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let input = integer(&f, 1);
        set_class(&f, &input, &classes);
        target.replace(Some(input.clone()));
        let error =
            catch_unwind(AssertUnwindSafe(|| native(&f, &input, &real(&f, 1.2)))).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "non-numeric argument to mathematical function"
        );
        assert_eq!(calls.get(), 2);
        drop(classes);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert!(
            !child.is_live(),
            "detached class graph is not retained after rejection"
        );
    });
}
#[test]
fn choose_factor_dense_scan_needs_no_payload_workspace_under_budget() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let input = integer(&f, 0);
        set_class(&f, &input, &f.strings(&["foo", "factor"]).unwrap());
        let b = real(&f, 1.2);
        let scan = || {
            crate::sexp::owner::with_runtime(&weak, |access| {
                super::choose_factor::has_factor(access, &input, &b)
            })
            .unwrap()
            .unwrap()
        };
        assert!(scan());
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        assert!(scan());
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let ordinary = integer(&f, 1);
        set_class(&f, &ordinary, &f.strings(&["foo"]).unwrap());
        assert!(
            !crate::sexp::owner::with_runtime(&weak, |access| super::choose_factor::has_factor(
                access, &ordinary, &b
            ))
            .unwrap()
            .unwrap()
        );
        assert_eq!(
            native(&f, &ordinary, &real(&f, 1.0)).try_real_elt(0),
            Ok(5.0)
        );
    });
}
#[test]
fn choose_factor_class_unwind_releases_roots_and_can_retry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let panic = Rc::new(Cell::new(true));
    let callback_panic = panic.clone();
    let class = session
        .register_altrep_class(
            "choose.factor.unwind",
            Classes {
                calls: Rc::new(Cell::new(0)),
                action: Rc::new(move || {
                    if callback_panic.get() {
                        std::panic::panic_any(291_u32);
                    }
                }),
            },
        )
        .unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let classes = AltrepBuilder::new(class)
            .data1(f.strings(&["factor"]).unwrap())
            .build()
            .unwrap();
        let input = integer(&f, 1);
        set_class(&f, &input, &classes);
        let b = real(&f, 1.2);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let error = catch_unwind(AssertUnwindSafe(|| native(&f, &input, &b))).unwrap_err();
        assert_eq!(error.downcast_ref::<u32>(), Some(&291));
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        panic.set(false);
        assert!(
            crate::sexp::owner::with_runtime(&weak, |access| super::choose_factor::has_factor(
                access, &input, &b
            ))
            .unwrap()
            .unwrap()
        );
    });
}
#[test]
fn choose_factor_class_revocation_cannot_continue_in_replacement() {
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
    let callback_facade = facade.clone();
    let replacement = Rc::new(RefCell::new(None));
    let callback_replacement = replacement.clone();
    let calls = Rc::new(Cell::new(0));
    let class = facade
        .borrow()
        .as_ref()
        .unwrap()
        .register_altrep_class(
            "choose.factor.revoke",
            Classes {
                calls: calls.clone(),
                action: Rc::new(move || {
                    drop(callback_facade.borrow_mut().take());
                    let other = RSession::construct_detached(RSession::new_for_gc_tests);
                    let bytes = other.with_active(|| {
                        other
                            .owner_token()
                            .unwrap()
                            .with_arena(|a| a.total_bytes_allocated())
                            .unwrap()
                    });
                    callback_replacement.replace(Some((other, bytes)));
                }),
            },
        )
        .unwrap()
        .into_owned()
        .unwrap();
    let error = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(original, || {
            let f = weak.node_factory().unwrap();
            let classes = AltrepBuilder::new(class)
                .data1(f.strings(&["factor"]).unwrap())
                .build()
                .unwrap();
            let input = integer(&f, 1);
            set_class(&f, &input, &classes);
            native(&f, &input, &real(&f, 1.2))
        })
    }))
    .unwrap_err();
    assert!(!weak.is_live());
    assert_eq!(calls.get(), 1);
    assert_eq!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    let replacement = replacement.borrow();
    let (other, before) = replacement.as_ref().unwrap();
    other.with_active(|| {
        assert_eq!(
            other
                .owner_token()
                .unwrap()
                .with_arena(|a| a.total_bytes_allocated())
                .unwrap(),
            *before
        )
    });
}

#[test]
fn choose_factor_attribute_cycles_and_malformed_tails_reject_and_recover() {
    #[cfg(not(miri))]
    {
        // Keep a future traversal regression from hanging the native suite.
        const NAME: &str = "mainutils::essentials::mathstats::math1::choose_factor_tests::choose_factor_attribute_cycles_and_malformed_tails_reject_and_recover";
        if std::env::var("RPORT_CHOOSE_CYCLE_CHILD").as_deref() != Ok("1") {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([NAME, "--exact", "--nocapture", "--test-threads=1"])
                .env("RPORT_CHOOSE_CYCLE_CHILD", "1")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("attribute cycle did not reject within ten seconds");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    use crate::sexp::{ffi::EdgeField, heap::ReferenceChild};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    session.with_active(|| {
        let f = owner.node_factory();
        let input = integer(&f, 1);
        let b = real(&f, 1.0);
        let tag = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"note".as_ptr()) })
            .unwrap();
        let first = f
            .pairlist_cell(&b, &f.nil(), &tag)
            .unwrap()
            .into_owned()
            .unwrap();
        let second = f
            .pairlist_cell(&b, &f.nil(), &tag)
            .unwrap()
            .into_owned()
            .unwrap();
        let tail = f
            .pairlist_cell(&b, &f.nil(), &tag)
            .unwrap()
            .into_owned()
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_ATTRIB(input.as_raw(), first.as_raw());
        }
        let link = |from: &Sexp<'_>, to: &Sexp<'_>| {
            let node = from.allocation().unwrap();
            let child = to.allocation().unwrap();
            node.heap_identity()
                .set_edge(node, EdgeField::ListCdr, ReferenceChild::Node(child))
                .unwrap();
        };
        let clear = |from: &Sexp<'_>| {
            let mut value = SexpMut::try_from_checked(from.clone()).unwrap();
            value.try_set_pairlist_cdr(&f.nil()).unwrap();
        };
        let scan = || {
            crate::sexp::owner::with_runtime(&weak, |access| {
                super::choose_factor::has_factor(access, &input, &b)
            })
            .unwrap()
        };
        // Warm metadata roots over the longest valid chain, then prove the
        // actual validation (including errors) adds no payload workspace.
        link(&first, &second);
        link(&second, &tail);
        assert!(!scan().unwrap());
        clear(&first);
        clear(&second);
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        link(&first, &first);
        assert!(
            scan()
                .unwrap_err()
                .to_string()
                .contains("cyclic choose attribute list")
        );
        clear(&first);
        assert!(!scan().unwrap());
        link(&first, &second);
        link(&second, &tail);
        link(&tail, &second);
        assert!(
            scan()
                .unwrap_err()
                .to_string()
                .contains("cyclic choose attribute list")
        );
        clear(&tail);
        assert!(!scan().unwrap());
        link(&tail, &b);
        assert!(
            scan()
                .unwrap_err()
                .to_string()
                .contains("malformed choose attribute list")
        );
        clear(&tail);
        assert!(!scan().unwrap());
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(native(&f, &input, &b).try_real_elt(0), Ok(5.0));
    });
}
