use crate::sexp::{
    RSession,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::{Rcomplex, SEXPTYPE},
    object::SexpResult,
    owner::with_runtime,
};
use std::{cell::Cell, rc::Rc};

struct FirstFlag {
    kind: SEXPTYPE,
    calls: Rc<Cell<usize>>,
    len: i64,
}
impl AltrepClass for FirstFlag {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.len)
    }
    fn element<'s>(&self, ctx: &AltrepContext<'s>, index: i64) -> SexpResult<AltrepElement<'s>> {
        assert_eq!(index, 0, "GNU flag coercion reads only the first element");
        self.calls.set(self.calls.get() + 1);
        let value = match self.kind {
            SEXPTYPE::LGLSXP => AltrepElement::Logical(0),
            SEXPTYPE::INTSXP => AltrepElement::Integer(0),
            SEXPTYPE::REALSXP => AltrepElement::Real(0.),
            SEXPTYPE::CPLXSXP => AltrepElement::Complex(Rcomplex { r: 0., i: 0. }),
            SEXPTYPE::RAWSXP => AltrepElement::Raw(0),
            SEXPTYPE::STRSXP => AltrepElement::String(ctx.string("FALSE")?),
            _ => unreachable!(),
        };
        ctx.gc()?;
        Ok(value)
    }
}
#[test]
fn derivative_hessian_coercion_reads_only_first_provider_element() {
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
        SEXPTYPE::STRSXP,
    ] {
        let session = RSession::new_for_gc_tests();
        let calls = Rc::new(Cell::new(0));
        let (authority, value) = session.with_active(|| {
            let class = session
                .register_altrep_class(
                    "first_hessian_flag",
                    FirstFlag {
                        kind,
                        calls: calls.clone(),
                        len: 2,
                    },
                )
                .unwrap();
            let value = AltrepBuilder::new(class)
                .build()
                .unwrap()
                .into_owned()
                .unwrap();
            (session.owner_token().unwrap().weak_owner().unwrap(), value)
        });
        let flag = with_runtime(&authority, |access| {
            super::derivative_hessian_flag(access, &value)
        })
        .unwrap()
        .unwrap();
        assert!(!flag, "kind={kind:?}");
        assert_eq!(calls.get(), 1, "kind={kind:?}");
    }
}

use crate::sexp::{memory::ArenaBudget, object::Sexp};
fn character(session: &RSession, bytes: &[u8]) -> Sexp<'static> {
    session.with_active(|| {
        session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|a| Some(a.alloc_charsxp(bytes)))
            .unwrap()
            .into_owned()
            .unwrap()
    })
}
#[test]
fn derivative_hessian_coercion_empty_providers_are_not_read() {
    let session = RSession::new_for_gc_tests();
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
        SEXPTYPE::STRSXP,
    ] {
        let calls = Rc::new(Cell::new(0));
        let (authority, value) = session.with_active(|| {
            let class = session
                .register_altrep_class(
                    &format!("empty_hessian_flag_{kind:?}"),
                    FirstFlag {
                        kind,
                        calls: calls.clone(),
                        len: 0,
                    },
                )
                .unwrap();
            (
                session.owner_token().unwrap().weak_owner().unwrap(),
                AltrepBuilder::new(class)
                    .build()
                    .unwrap()
                    .into_owned()
                    .unwrap(),
            )
        });
        assert!(
            with_runtime(&authority, |access| super::derivative_hessian_flag(
                access, &value
            ))
            .unwrap()
            .unwrap()
        );
        assert_eq!(calls.get(), 0, "kind={kind:?}");
    }
}
#[test]
fn derivative_hessian_character_policy_at_zero_headroom() {
    let session = RSession::new_for_gc_tests();
    let cases: [(&[u8], bool); 12] = [
        (b"F", false),
        (b"False", false),
        (b"FALSE", false),
        (b"false", false),
        (b"f", true),
        (b"FaLsE", true),
        (b" FALSE", true),
        (b"FALSE ", true),
        (b"\xffFALSE", true),
        (b"FALSE\xff", true),
        (b"FALSE\0hidden", false),
        (b"", true),
    ];
    let mut values = cases
        .iter()
        .map(|(bytes, flag)| (character(&session, bytes), *flag))
        .collect::<Vec<_>>();
    let na = session.with_active(|| unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::globals::R_NaString())
            .unwrap()
            .into_owned()
            .unwrap()
    });
    values.push((na, true));
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let authority = owner.weak_owner().unwrap();
        let held = owner
            .with_arena(|a| a.reserve_workspace(128).unwrap())
            .unwrap();
        let (old, before) = owner
            .with_arena(|a| {
                let old = a.budget();
                let before = (a.total_bytes_allocated(), a.workspace_bytes());
                a.set_budget(ArenaBudget::new(before.0 + before.1, 0));
                (old, before)
            })
            .unwrap();
        with_runtime(&authority, |access| {
            for (value, expected) in &values {
                assert_eq!(
                    super::derivative_hessian_flag(access, value).unwrap(),
                    *expected
                );
            }
        })
        .unwrap();
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
        owner.with_arena(|a| a.set_budget(old)).unwrap();
        drop(held);
    });
}
#[test]
fn derivative_hessian_long_character_uses_no_r_scratch() {
    let session = RSession::new_for_gc_tests();
    let mut bytes = b"FALSE".to_vec();
    bytes.resize(100_000, b'x');
    let value = character(&session, &bytes);
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let authority = owner.weak_owner().unwrap();
        let before = owner
            .with_arena(|a| {
                let before = (a.total_bytes_allocated(), a.workspace_bytes());
                a.set_budget(ArenaBudget::new(before.0 + before.1, 0));
                before
            })
            .unwrap();
        assert!(
            with_runtime(&authority, |access| super::derivative_hessian_flag(
                access, &value
            ))
            .unwrap()
            .unwrap()
        );
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

use crate::sexp::{
    heap::CheckedNode, memory::WorkspaceReservation, object::SexpMut, owner::WeakOwner,
};
use std::{cell::RefCell, rc::Weak};
struct TransitionFlag {
    kind: SEXPTYPE,
    action: u8,
    authority: WeakOwner,
    sessions: Weak<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
    parent: Rc<RefCell<Option<CheckedNode>>>,
    input: Rc<RefCell<Option<CheckedNode>>>,
    replacement: Rc<RefCell<Option<(Sexp<'static>, WorkspaceReservation)>>>,
}
impl AltrepClass for TransitionFlag {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(&self, ctx: &AltrepContext<'s>, index: i64) -> SexpResult<AltrepElement<'s>> {
        assert_eq!(index, 0);
        self.calls.set(self.calls.get() + 1);
        let value = if self.kind == SEXPTYPE::STRSXP {
            AltrepElement::String(ctx.string("FALSE")?)
        } else {
            AltrepElement::Logical(0)
        };
        let root = self.parent.borrow().as_ref().unwrap().root_lease().unwrap();
        let parent = ctx.object().node_factory()?.domain().rooted_value(&root)?;
        let nil = parent.node_factory()?.domain().nil();
        SexpMut::try_from_checked(parent)?.try_set_pairlist_car(&nil)?;
        ctx.gc()?;
        assert!(self.input.borrow().as_ref().unwrap().is_live());
        if self.action == 1 {
            std::panic::panic_any(917_u32);
        }
        if self.action == 2 {
            let sessions = self.sessions.upgrade().unwrap();
            let mut slot = sessions.borrow_mut();
            slot.as_mut().unwrap().close();
            let next = RSession::new_for_gc_tests();
            let (root, ticket) = next.with_active(|| {
                let owner = next.owner_token().unwrap();
                let root = owner
                    .node_factory()
                    .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 1)))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let mut root = SexpMut::try_from_checked(root).unwrap();
                root.try_set_real_elt(0, 42.).unwrap();
                (
                    root.freeze(),
                    owner
                        .with_arena(|a| a.reserve_workspace(256).unwrap())
                        .unwrap(),
                )
            });
            *self.replacement.borrow_mut() = Some((root, ticket));
            *slot = Some(next);
        }
        if self.action == 3 {
            let flag = ctx
                .object()
                .node_factory()?
                .domain()
                .logical(false)
                .into_owned()?;
            assert!(!with_runtime(&self.authority, |access| {
                super::derivative_hessian_flag(access, &flag)
            })??);
        }
        Ok(value)
    }
}
fn transition_case(action: u8) {
    for kind in [SEXPTYPE::LGLSXP, SEXPTYPE::STRSXP] {
        let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let calls = Rc::new(Cell::new(0));
        let parent_node = Rc::new(RefCell::new(None));
        let input_node = Rc::new(RefCell::new(None));
        let replacement = Rc::new(RefCell::new(None));
        let (authority, parent, input, held, ledger) = {
            let borrowed = sessions.borrow();
            let session = borrowed.as_ref().unwrap();
            session.with_active(|| {
                let owner = session.owner_token().unwrap();
                let authority = owner.weak_owner().unwrap();
                let class = session
                    .register_altrep_class(
                        "transition_hessian_flag",
                        TransitionFlag {
                            kind,
                            action,
                            authority: authority.clone(),
                            sessions: Rc::downgrade(&sessions),
                            calls: calls.clone(),
                            parent: parent_node.clone(),
                            input: input_node.clone(),
                            replacement: replacement.clone(),
                        },
                    )
                    .unwrap();
                let value = AltrepBuilder::new(class)
                    .build()
                    .unwrap()
                    .into_owned()
                    .unwrap();
                *input_node.borrow_mut() = Some(value.allocation().unwrap().clone());
                let factory = owner.node_factory();
                let nil = factory.domain().nil();
                let parent = factory
                    .pairlist_cell(&value, &nil, &nil)
                    .unwrap()
                    .into_owned()
                    .unwrap();
                *parent_node.borrow_mut() = Some(parent.allocation().unwrap().clone());
                let input = parent.try_car().unwrap().into_owned().unwrap();
                let held = owner
                    .with_arena(|a| a.reserve_workspace(128).unwrap())
                    .unwrap();
                let ledger = owner
                    .with_arena(|a| a.heap_identity().byte_ledger())
                    .unwrap();
                (authority, parent, input, held, ledger)
            })
        };
        let pin = authority.pin().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_runtime(&authority, |access| {
                super::derivative_hessian_flag(access, &input)
            })
        }));
        assert_eq!(calls.get(), 1);
        assert!(parent.try_car().unwrap().is_nil());
        assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
        assert_eq!(ledger.workspace_bytes_for_test(), 128);
        drop(held);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        if action == 1 {
            assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 917);
        } else if action == 2 {
            let result = result.expect("revocation must return an owned error");
            assert!(result.is_err() || result.as_ref().is_ok_and(|inner| inner.is_err()));
            assert!(authority.node_factory().is_err());
            let keep = replacement.borrow();
            let (root, ticket) = keep.as_ref().unwrap();
            assert_eq!(root.try_real_elt(0).unwrap(), 42.);
            assert_eq!(ticket.bytes(), 256);
            let sessions = sessions.borrow();
            let next = sessions.as_ref().unwrap();
            assert_eq!(
                next.with_active(|| next
                    .owner_token()
                    .unwrap()
                    .with_arena(|a| a.workspace_bytes())
                    .unwrap()),
                256
            );
        } else {
            assert!(!result.unwrap().unwrap().unwrap());
        }
    }
}
#[test]
fn derivative_hessian_coercion_detached_sources_survive_collection() {
    transition_case(0);
}
#[test]
fn derivative_hessian_coercion_provider_panic_is_preserved() {
    transition_case(1);
}
#[test]
fn derivative_hessian_coercion_owner_replacement_rejects_publication() {
    transition_case(2);
}
#[test]
fn derivative_hessian_coercion_nested_callbacks_keep_original_owner() {
    transition_case(3);
}

#[test]
fn derivative_hessian_coercion_rejects_foreign_input() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let value = character(&right, b"foreign-hessian-owner");
    let authority = left.with_active(|| left.owner_token().unwrap().weak_owner().unwrap());
    let result = left
        .with_active(|| {
            with_runtime(&authority, |access| {
                super::derivative_hessian_flag(access, &value)
            })
        })
        .unwrap();
    assert!(matches!(
        result,
        Err(crate::sexp::object::SexpError::UnownedPointer { .. })
    ));
}
