use crate::sexp::{
    RSession, SEXPTYPE,
    ffi::{EdgeField, NodeBody},
    heap::ReferenceChild,
    object::{Sexp, SexpError, SexpResult},
    owner::{RuntimeAccess, with_runtime},
};

fn replace_rest(cell: &Sexp<'_>, rest: &Sexp<'_>) {
    let allocation = cell.allocation().unwrap();
    let heap = allocation.heap_identity();
    let mut header = heap.node_snapshot(allocation).unwrap();
    let NodeBody::List(body) = &mut header.data else {
        panic!("expected list cell")
    };
    body.cdrval = rest.link_in(&heap).unwrap();
    heap.replace_node(allocation, header).unwrap();
}

#[cfg(not(miri))]
fn cycle_contract(name: &str, target: SEXPTYPE) {
    if std::env::var("RPORT_PAIRLIST_CYCLE_CHILD").as_deref() == Ok(name) {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let factory = owner.node_factory().unwrap();
            let scalar = factory.strings(&["retained"]).unwrap();
            let cell = factory
                .pairlist_cell(&scalar, &factory.nil(), &factory.nil())
                .unwrap();
            let allocation = cell.allocation().unwrap();
            let heap = allocation.heap_identity();
            heap.set_edge(
                allocation,
                EdgeField::ListCdr,
                ReferenceChild::Node(allocation),
            )
            .unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::coercePairList(cell.as_raw(), target)
            }));
            replace_rest(&cell, &factory.nil());
            let error = result.unwrap_err();
            let error = error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap();
            assert!(
                error.message.contains("cyclic pairlist"),
                "{}",
                error.message
            );
        });
        return;
    }
    // Malformed graph regressions must fail rather than hang the test suite.
    // A child process keeps the unchanged raw-counting baseline bounded.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env("RPORT_PAIRLIST_CYCLE_CHILD", name)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "cycle admission child failed: {status}");
            return;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("pairlist coercion did not reject a cycle within ten seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(not(miri))]
#[test]
fn pairlist_string_cycle_is_rejected_before_counting() {
    cycle_contract(
        "mainutils::coerce::pairlist_tests::pairlist_string_cycle_is_rejected_before_counting",
        SEXPTYPE::STRSXP,
    );
}

#[cfg(not(miri))]
#[test]
fn pairlist_list_cycle_is_rejected_before_counting() {
    cycle_contract(
        "mainutils::coerce::pairlist_tests::pairlist_list_cycle_is_rejected_before_counting",
        SEXPTYPE::VECSXP,
    );
}

#[test]
fn pairlist_public_coercion_preserves_gnu_strings_names_and_language_cells() {
    let mut session = RSession::new_for_gc_tests();
    for expression in [
        "identical(as.character(pairlist(a='foo',b=1:2,c=list(3L),d=quote(x+y),e=NULL)),c('foo','1:2','list(3)','x + y','NULL'))",
        "identical(as.list(pairlist(a='foo',b=1:2,c=list(3L),d=quote(x+y),e=NULL)),list(a='foo',b=1:2,c=list(3L),d=quote(x+y),e=NULL))",
        "identical(as.character(quote(x+y)),c('+','x','y'))",
        "identical(as.list(quote(x+y)),list(as.name('+'),as.name('x'),as.name('y')))",
        "identical(as.character(NULL),character()) && identical(as.list(NULL),list())",
    ] {
        let value = session.eval_code_with_output_capture(expression).0.unwrap();
        assert_eq!(value.try_logical_elt(0).unwrap(), 1, "{expression}");
    }
}

#[test]
fn pairlist_and_list_simple_deparse_follow_independent_gnu_controls() {
    let mut session = RSession::new_for_gc_tests();
    let script = include_str!("fixtures/simple-deparse.R");
    let result = session.eval_code_with_output_capture(script).0.unwrap();
    assert_eq!(result.try_logical_elt(0).unwrap(), 1);
}

struct NoNative;
impl super::pairlist::Native for NoNative {
    fn scalar(
        &mut self,
        _: &RuntimeAccess,
        _: &Sexp<'static>,
        _: SEXPTYPE,
    ) -> SexpResult<super::pairlist::Scalar> {
        panic!("unexpected scalar callback before checked admission")
    }
    fn deparse(&mut self, _: &RuntimeAccess, _: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
        panic!("unexpected deparse before checked admission")
    }
    fn names(&mut self, _: &RuntimeAccess, _: &Sexp<'static>, _: &Sexp<'static>) -> SexpResult<()> {
        panic!("unexpected names callback")
    }
}

#[test]
fn pairlist_checked_graph_rejects_cycles_tails_and_tags_before_native_calls() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = owner.node_factory().unwrap();
        let scalar = factory.strings(&["cell"]).unwrap();
        let cell = factory
            .pairlist_cell(&scalar, &scalar, &factory.nil())
            .unwrap();
        for target in [SEXPTYPE::STRSXP, SEXPTYPE::VECSXP] {
            let error = with_runtime(&owner, |access| {
                super::pairlist::coerce(cell.clone(), target, access, &mut NoNative)
            })
            .unwrap()
            .unwrap_err();
            assert!(error.to_string().contains("improper pairlist"));
        }
        replace_rest(&cell, &cell);
        for target in [SEXPTYPE::STRSXP, SEXPTYPE::VECSXP] {
            let error = with_runtime(&owner, |access| {
                super::pairlist::coerce(cell.clone(), target, access, &mut NoNative)
            })
            .unwrap()
            .unwrap_err();
            assert!(error.to_string().contains("cyclic pairlist"));
        }
        replace_rest(&cell, &factory.nil());
        let invalid = factory
            .pairlist_cell(&scalar, &factory.nil(), &scalar)
            .unwrap();
        let error = with_runtime(&owner, |access| {
            super::pairlist::coerce(invalid, SEXPTYPE::VECSXP, access, &mut NoNative)
        })
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("invalid pairlist tag"));
    });
}

#[test]
fn pairlist_checked_graph_rejects_a_foreign_runtime_before_native_calls() {
    let original = RSession::new_for_gc_tests();
    let value = original.with_active(|| {
        let factory = original.owner_token().unwrap().node_factory();
        let scalar = factory.strings(&["original"]).unwrap();
        factory
            .pairlist_cell(&scalar, &factory.nil(), &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap().weak_owner().unwrap();
        let error = with_runtime(&owner, |access| {
            super::pairlist::coerce(value, SEXPTYPE::VECSXP, access, &mut NoNative)
        })
        .unwrap()
        .unwrap_err();
        assert!(matches!(error, SexpError::UnownedPointer { .. }));
    });
}

#[test]
fn pairlist_checked_list_result_retains_children_after_source_drop_and_full_gc() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = owner.node_factory().unwrap();
        let scalar = factory.strings(&["retained list value"]).unwrap();
        let identity = scalar.allocation().unwrap().clone();
        let input = factory
            .pairlist_cell(&scalar, &factory.nil(), &factory.nil())
            .unwrap();
        let output = with_runtime(&owner, |access| {
            super::pairlist::coerce(input, SEXPTYPE::VECSXP, access, &mut NoNative)
        })
        .unwrap()
        .unwrap();
        drop(scalar);
        with_runtime(&owner, |access| access.with_native(|token| token.full_gc()))
            .unwrap()
            .unwrap();
        assert!(identity.is_live());
        assert_eq!(
            output
                .try_vector_elt(0)
                .unwrap()
                .try_string_value_elt(0)
                .unwrap()
                .as_deref(),
            Some("retained list value")
        );
    });
}

use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct DetachingString {
    facade: Weak<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
    second: crate::sexp::heap::CheckedNode,
    action: u8,
    kind: SEXPTYPE,
}
impl AltrepClass for DetachingString {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let character = context.data2()?;
        let head = context.data1()?;
        let nil = head.try_cdr()?.try_cdr()?;
        let node = head.allocation()?;
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(node).unwrap();
        let NodeBody::List(body) = &mut header.data else {
            panic!("expected source cell")
        };
        body.carval = nil.link_in(&heap)?;
        body.cdrval = nil.link_in(&heap)?;
        heap.replace_node(node, header).unwrap();
        context.set_data2(nil)?;
        // The later value has no remaining source edge or parsed-expression root.
        context.gc()?;
        assert!(
            self.second.is_live(),
            "selected later child survives full GC"
        );
        if self.action == 2 {
            self.facade
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }
        if self.action != 0 {
            std::panic::panic_any(479_u32);
        }
        Ok(match self.kind {
            SEXPTYPE::INTSXP => AltrepElement::Integer(101),
            SEXPTYPE::LGLSXP => AltrepElement::Logical(1),
            SEXPTYPE::REALSXP => AltrepElement::Real(101.0),
            SEXPTYPE::CPLXSXP => AltrepElement::Complex(crate::sexp::Rcomplex { r: 101.0, i: 0.0 }),
            SEXPTYPE::RAWSXP => AltrepElement::Raw(101),
            _ => AltrepElement::String(character),
        })
    }
}

fn detaching_provider(action: u8) {
    detaching_provider_kind(action, SEXPTYPE::STRSXP);
}

fn detaching_provider_kind(action: u8, kind: SEXPTYPE) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let (owner, input) = {
        let borrow = facade.borrow();
        let session = borrow.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let factory = owner.node_factory().unwrap();
            let second = factory
                .strings(&[if kind != SEXPTYPE::STRSXP {
                    "202"
                } else {
                    "later"
                }])
                .unwrap();
            let class = session
                .register_altrep_class(
                    "pairlist.detaching.string",
                    DetachingString {
                        facade: Rc::downgrade(&facade),
                        calls: calls.clone(),
                        second: second.allocation().unwrap().clone(),
                        action,
                        kind,
                    },
                )
                .unwrap()
                .into_owned()
                .unwrap();
            let first = factory.character("first").unwrap();
            let first = AltrepBuilder::new(class).data2(first).build().unwrap();
            let tail = factory
                .pairlist_cell(&second, &factory.nil(), &factory.nil())
                .unwrap();
            let input = factory
                .pairlist_cell(&first, &tail, &factory.nil())
                .unwrap();
            crate::sexp::altrep::set_data1(&first, input.clone()).unwrap();
            (owner, input.into_owned().unwrap())
        })
    };
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            with_runtime(&owner, |access| {
                if kind != SEXPTYPE::STRSXP {
                    access.with_native(|owner| {
                        owner
                            .sexp(super::coercePairList(input.as_raw(), kind))?
                            .into_owned()
                    })
                } else {
                    super::pairlist::coerce(input, kind, access, &mut NoNative)
                }
            })
        })
    }));
    assert_eq!(calls.get(), 1);
    assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
    match action {
        0 if kind != SEXPTYPE::STRSXP => {
            let output = result.unwrap().unwrap().unwrap();
            match kind {
                SEXPTYPE::INTSXP => {
                    assert_eq!(output.try_integer_elt(0).unwrap(), 101);
                    assert_eq!(output.try_integer_elt(1).unwrap(), 202);
                }
                SEXPTYPE::LGLSXP => {
                    assert_eq!(output.try_logical_elt(0).unwrap(), 1);
                    assert_eq!(output.try_logical_elt(1).unwrap(), crate::sexp::NA_LOGICAL);
                }
                SEXPTYPE::REALSXP => {
                    assert_eq!(output.try_real_elt(0).unwrap(), 101.0);
                    assert_eq!(output.try_real_elt(1).unwrap(), 202.0);
                }
                SEXPTYPE::CPLXSXP => {
                    assert_eq!(output.try_complex_elt(0).unwrap().r, 101.0);
                    assert_eq!(output.try_complex_elt(1).unwrap().r, 202.0);
                    assert_eq!(output.try_complex_elt(1).unwrap().i, 0.0);
                }
                SEXPTYPE::RAWSXP => {
                    assert_eq!(output.try_raw_elt(0).unwrap(), 101);
                    assert_eq!(output.try_raw_elt(1).unwrap(), 202);
                }
                _ => unreachable!(),
            }
        }
        0 => {
            let output = result.unwrap().unwrap().unwrap();
            assert_eq!(
                output.try_string_value_elt(0).unwrap().as_deref(),
                Some("first")
            );
            assert_eq!(
                output.try_string_value_elt(1).unwrap().as_deref(),
                Some("later")
            );
        }
        1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 479),
        _ => {
            if kind == SEXPTYPE::STRSXP {
                assert!(matches!(result.unwrap(), Err(SexpError::RootUnavailable)));
            } else {
                let error = result.unwrap_err();
                let error = error
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap();
                assert_eq!(error.message, SexpError::RootUnavailable.to_string());
            }
            let mut replacement = RSession::new_for_gc_tests();
            assert_eq!(
                replacement
                    .eval_code_with_output_capture("1L+1L")
                    .0
                    .unwrap()
                    .try_integer_elt(0)
                    .unwrap(),
                2
            );
        }
    }
}

#[test]
fn pairlist_checked_snapshot_retains_later_values_after_provider_detachment_and_gc() {
    detaching_provider(0);
}
#[test]
fn pairlist_checked_snapshot_preserves_live_provider_unwind_and_gc_cleanup() {
    detaching_provider(1);
}
#[test]
fn pairlist_checked_snapshot_rejects_revoked_owner_before_provider_unwind() {
    detaching_provider(2);
}

#[test]
fn pairlist_atomic_snapshot_retains_later_value_after_provider_detachment_and_gc() {
    detaching_provider_kind(0, SEXPTYPE::INTSXP);
}

#[test]
fn pairlist_atomic_snapshot_covers_every_scalar_target_after_detachment_and_gc() {
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
    ] {
        detaching_provider_kind(0, kind);
    }
}

#[test]
fn pairlist_atomic_snapshot_preserves_live_provider_unwind_and_gc_cleanup() {
    detaching_provider_kind(1, SEXPTYPE::INTSXP);
}

#[test]
fn pairlist_atomic_snapshot_rejects_revoked_owner_before_provider_unwind() {
    detaching_provider_kind(2, SEXPTYPE::INTSXP);
}

#[test]
fn pairlist_atomic_public_values_warnings_and_admission_match_pinned_gnu() {
    let mut session = RSession::new_for_gc_tests();
    let result = session
        .eval_code_with_output_capture(include_str!("fixtures/pairlist-atomic.R"))
        .0
        .unwrap();
    assert_eq!(result.try_logical_elt(0).unwrap(), 1);
}

#[test]
fn pairlist_atomic_checked_admission_rejects_invalid_children_and_graphs_before_native_calls() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = owner.node_factory().unwrap();
        for child in [
            factory.nil(),
            factory.strings(&["first", "second"]).unwrap(),
        ] {
            let input = factory
                .pairlist_cell(&child, &factory.nil(), &factory.nil())
                .unwrap();
            let error = with_runtime(&owner, |access| {
                super::pairlist::coerce(input, SEXPTYPE::INTSXP, access, &mut NoNative)
            })
            .unwrap()
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                "'pairlist' object cannot be coerced to type 'integer'"
            );
        }
        let scalar = factory.strings(&["202"]).unwrap();
        let cell = factory
            .pairlist_cell(&scalar, &scalar, &factory.nil())
            .unwrap();
        let error = with_runtime(&owner, |access| {
            super::pairlist::coerce(cell.clone(), SEXPTYPE::INTSXP, access, &mut NoNative)
        })
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("improper pairlist"));
        replace_rest(&cell, &cell);
        let error = with_runtime(&owner, |access| {
            super::pairlist::coerce(cell.clone(), SEXPTYPE::INTSXP, access, &mut NoNative)
        })
        .unwrap()
        .unwrap_err();
        replace_rest(&cell, &factory.nil());
        assert!(error.to_string().contains("cyclic pairlist"));
        let error = with_runtime(&owner, |access| {
            super::pairlist::coerce(cell, SEXPTYPE::ENVSXP, access, &mut NoNative)
        })
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("unsupported checked pairlist"));
    });
}
