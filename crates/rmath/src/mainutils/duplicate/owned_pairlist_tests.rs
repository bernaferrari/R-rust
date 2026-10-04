//! Original pairlist edges remain owned when callbacks detach their source cells.

use super::*;
use crate::sexp::{heap::CheckedNode, object::Sexp, session::RSession};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn fixture(session: &RSession, kind: SEXPTYPE) -> (Sexp<'_>, Vec<CheckedNode>, Vec<CheckedNode>) {
    fixture_with_attributes(session, kind, true)
}

fn fixture_with_attributes(
    session: &RSession,
    kind: SEXPTYPE,
    include_attributes: bool,
) -> (Sexp<'_>, Vec<CheckedNode>, Vec<CheckedNode>) {
    let factory = session.owner_token().unwrap().node_factory();
    let mut head = factory.nil();
    let mut cars = Vec::new();
    let mut tails = Vec::new();
    for index in (0..3).rev() {
        let value = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(10 + index) })
            .unwrap();
        cars.push(value.allocation().unwrap().clone());
        let tag = factory
            .wrap(unsafe {
                crate::sexp::symbol::Rf_install(
                    [c"alpha", c"beta", c"gamma"][index as usize].as_ptr(),
                )
            })
            .unwrap();
        let attribute_value = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(70 + index) })
            .unwrap();
        let attributes = factory
            .pairlist_cell(&attribute_value, &factory.nil(), &tag)
            .unwrap();
        head = factory.pairlist_cell(&value, &head, &tag).unwrap();
        unsafe {
            if include_attributes {
                SET_ATTRIB(head.as_raw(), attributes.as_raw());
            }
            SET_OBJECT(head.as_raw(), 1);
            SET_S4_OBJECT(head.as_raw());
        }
        if index != 0 {
            tails.push(head.allocation().unwrap().clone());
        }
    }
    unsafe { SET_TYPEOF(head.as_raw(), kind.as_c_int()) };
    (head, cars, tails)
}

fn force_next_gc(session: &RSession) {
    session.with_active_in(|owner| unsafe {
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    });
}

fn typed_failure(operation: impl FnOnce(), expected: &str) {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .expect_err("malformed or revoked copy must fail");
    let error = payload
        .downcast_ref::<crate::sexp::context::RError>()
        .expect("failure is an R error rather than a runtime-access or borrow panic");
    assert!(error.message.contains(expected), "{}", error.message);
}

fn assert_values(copy: &Sexp<'_>) {
    let mut cell = copy.clone();
    for index in 0..3 {
        assert_eq!(
            cell.try_car().unwrap().try_integer_elt(0).unwrap(),
            10 + index
        );
        cell = cell.try_cdr().unwrap();
    }
    assert!(cell.is_nil());
}

#[test]
fn owning_pairlist_gnu_neighbors_preserve_attributes_tags_and_head_flags() {
    // Independently checked against pinned GNU bac583951... duplicate.c through
    // native LIST/LANG/DOTS × deep/shallow × attributes/no-attributes cases.
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    for kind in [SEXPTYPE::LISTSXP, SEXPTYPE::LANGSXP, SEXPTYPE::DOTSXP] {
        for deep in [false, true] {
            for attributes in [false, true] {
                let (source, _, _) = fixture_with_attributes(&session, kind, attributes);
                let before = crate::sexp::protect::R_ProtectCount();
                let copy = factory
                    .wrap(unsafe {
                        if deep {
                            duplicate(source.as_raw())
                        } else {
                            shallow_duplicate(source.as_raw())
                        }
                    })
                    .unwrap();
                assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
                assert_values(&copy);
                let mut original = source;
                let mut copied = copy;
                for index in 0..3 {
                    assert_eq!(
                        copied.typeof_(),
                        if index == 0 { kind } else { SEXPTYPE::LISTSXP }
                    );
                    assert_eq!(copied.try_tag().unwrap(), original.try_tag().unwrap());
                    assert_eq!(
                        copied.try_car().unwrap() == original.try_car().unwrap(),
                        !deep
                    );
                    unsafe {
                        assert_eq!(OBJECT(copied.as_raw()) != 0, index == 0 || attributes);
                        assert_eq!(IS_S4_OBJECT(copied.as_raw()) != 0, index == 0 || attributes);
                    }
                    let copied_attributes = copied.try_attrib().unwrap();
                    let original_attributes = original.try_attrib().unwrap();
                    if attributes {
                        assert_ne!(copied_attributes, original_attributes);
                        assert_eq!(
                            copied_attributes
                                .try_car()
                                .unwrap()
                                .try_integer_elt(0)
                                .unwrap(),
                            70 + index
                        );
                        assert_eq!(
                            copied_attributes.try_car().unwrap()
                                == original_attributes.try_car().unwrap(),
                            !deep
                        );
                    } else {
                        assert!(copied_attributes.is_nil());
                    }
                    original = original.try_cdr().unwrap();
                    copied = copied.try_cdr().unwrap();
                }
            }
        }
    }
}

#[test]
fn owning_pairlist_rejects_tail_cycles_and_malformed_cdr_before_allocating() {
    let session = RSession::new_for_gc_tests();
    let (source, _, _) = fixture(&session, SEXPTYPE::LISTSXP);
    let factory = source.node_factory().unwrap();
    let last = source.try_cdr().unwrap().try_cdr().unwrap();
    unsafe {
        SETCDR(last.as_raw(), source.as_raw());
    }
    let before = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| arena.node_count())
        .unwrap();
    let protection_count = crate::sexp::protect::R_ProtectCount();
    typed_failure(
        || unsafe {
            duplicate(source.as_raw());
        },
        "cyclic pairlist tail",
    );
    assert_eq!(
        session
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.node_count())
            .unwrap(),
        before
    );
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection_count);
    unsafe {
        SETCDR(last.as_raw(), factory.nil().as_raw());
    }
    let scalar = source.try_car().unwrap();
    unsafe {
        SETCDR(last.as_raw(), scalar.as_raw());
    }
    typed_failure(
        || unsafe {
            shallow_duplicate(source.as_raw());
        },
        "pairlist tail",
    );
    assert_eq!(
        session
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.node_count())
            .unwrap(),
        before
    );
    unsafe {
        SETCDR(last.as_raw(), factory.nil().as_raw());
    }
    assert_values(&factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap());
}

#[test]
fn owning_pairlist_rejects_recursive_child_graph_and_cleans_operation_state() {
    let session = RSession::new_for_gc_tests();
    let (source, _, _) = fixture_with_attributes(&session, SEXPTYPE::LISTSXP, false);
    let factory = source.node_factory().unwrap();
    let scalar = source.try_car().unwrap();
    unsafe {
        SETCAR(source.as_raw(), source.as_raw());
    }
    typed_failure(
        || unsafe {
            duplicate(source.as_raw());
        },
        "cyclic object graph",
    );
    let shallow = factory
        .wrap(unsafe { shallow_duplicate(source.as_raw()) })
        .unwrap();
    assert_eq!(shallow.try_car().unwrap(), source);
    unsafe {
        SETCAR(source.as_raw(), scalar.as_raw());
    }
    let vector = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
        .unwrap();
    unsafe {
        SET_VECTOR_ELT(vector.as_raw(), 0, vector.as_raw());
        SETCAR(source.as_raw(), vector.as_raw());
    }
    typed_failure(
        || unsafe {
            duplicate(source.as_raw());
        },
        "cyclic object graph",
    );
    unsafe {
        SET_VECTOR_ELT(vector.as_raw(), 0, factory.nil().as_raw());
        SETCAR(source.as_raw(), scalar.as_raw());
    }
    assert_values(&factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap());
}

#[test]
fn owning_pairlist_callback_can_independently_duplicate_same_source() {
    let session = RSession::new_for_gc_tests();
    let (source, _, _) = fixture(&session, SEXPTYPE::DOTSXP);
    let factory = source.node_factory().unwrap();
    let raw = source.as_raw();
    let nested = Rc::new(RefCell::new(None));
    let result = nested.clone();
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(observed.get() + 1) != 0 {
            return;
        }
        let owner = unsafe { crate::sexp::owner::OwnerToken::current().unwrap() };
        unsafe {
            (*owner.as_ptr()).memory_state.gc_force_gap = 0;
        }
        let copy = owner
            .sexp(unsafe { duplicate(raw) })
            .unwrap()
            .into_owned()
            .unwrap();
        *result.borrow_mut() = Some(copy);
        crate::sexp::gengc::full_gc();
    }));
    force_next_gc(&session);
    let outer = factory.wrap(unsafe { duplicate(raw) }).unwrap();
    assert!(notifications.get() > 0);
    let inner = nested.borrow_mut().take().unwrap();
    assert_ne!(outer, inner);
    assert_values(&outer);
    assert_values(&inner);
    assert_eq!(outer.typeof_(), SEXPTYPE::DOTSXP);
    assert_eq!(inner.typeof_(), SEXPTYPE::DOTSXP);
}

#[test]
fn owning_pairlist_public_copy_owns_sole_source_through_flags_and_trace() {
    let session = RSession::new_for_gc_tests();
    let source = fixture(&session, SEXPTYPE::LANGSXP).0.into_owned().unwrap();
    let factory = source.node_factory().unwrap();
    unsafe {
        SET_RTRACE(source.as_raw(), 1);
    }
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let source_token = source_node.clone();
    let source_holder = Rc::new(RefCell::new(Some(source)));
    let holder = source_holder.clone();
    let raw = source_holder.borrow().as_ref().unwrap().as_raw();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        unsafe {
            (*crate::sexp::owner::OwnerToken::current().unwrap().as_ptr())
                .memory_state
                .gc_force_gap = 0;
        }
        drop(holder.borrow_mut().take());
        let mut header = heap.node_snapshot(&source_token).unwrap();
        header.sxpinfo.set_obj(false);
        header.sxpinfo.set_gp(0);
        header.sxpinfo.set_trace(false);
        heap.replace_node(&source_token, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(
            source_token.is_live(),
            "the operation owns the source after its last external root is dropped"
        );
    }));
    force_next_gc(&session);
    let copy = factory.wrap(unsafe { duplicate(raw) }).unwrap();
    assert!(notified.get());
    assert!(source_holder.borrow().is_none());
    assert_values(&copy);
    unsafe {
        assert_eq!(OBJECT(copy.as_raw()), 1);
        assert_eq!(IS_S4_OBJECT(copy.as_raw()), 1);
        assert_eq!(RTRACE(copy.as_raw()), 1);
    }
    crate::sexp::gengc::full_gc();
    assert!(
        !source_node.is_live(),
        "the source lease ends after publication"
    );
}

#[test]
fn owning_pairlist_live_callback_panic_keeps_payload_and_releases_protection() {
    let session = RSession::new_for_gc_tests();
    let (source, _, _) = fixture(&session, SEXPTYPE::LISTSXP);
    let factory = source.node_factory().unwrap();
    let callbacks = Rc::new(Cell::new(0));
    let observed = callbacks.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        std::panic::panic_any(855_u32);
    }));
    force_next_gc(&session);
    let before = crate::sexp::protect::R_ProtectCount();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        duplicate(source.as_raw())
    }))
    .expect_err("callback panics must preserve their payload");
    assert_eq!(panic.downcast_ref::<u32>(), Some(&855));
    assert_eq!(callbacks.get(), 1);
    assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    session.with_active_in(|owner| unsafe {
        (*owner).gc_state.callbacks.clear();
        (*owner).memory_state.gc_force_gap = 0;
        assert!(!(*owner).gc_state.in_progress);
    });
    assert_values(&factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap());
}

#[test]
fn owning_pairlist_callback_can_drop_original_session_without_publishing_copy() {
    let session = RSession::new_for_gc_tests();
    let source = fixture(&session, SEXPTYPE::LANGSXP).0.into_owned().unwrap();
    let original = source.runtime_owner.as_ref().unwrap().clone();
    let session_holder = Rc::new(RefCell::new(Some(session)));
    let holder = session_holder.clone();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        drop(holder.borrow_mut().take());
    }));
    force_next_gc(session_holder.borrow().as_ref().unwrap());
    typed_failure(
        || unsafe {
            duplicate(source.as_raw());
        },
        "owner could not retain its root",
    );
    assert!(notified.get());
    assert!(session_holder.borrow().is_none());
    assert!(original.pin().is_err());
    // No incidental original runtime pin hides teardown in this fixture.
    let replacement = RSession::new_for_gc_tests();
    let (other, _, _) = fixture(&replacement, SEXPTYPE::LISTSXP);
    assert_values(
        &other
            .node_factory()
            .unwrap()
            .wrap(unsafe { duplicate(other.as_raw()) })
            .unwrap(),
    );
}

#[test]
fn owning_pairlist_detached_tail_collects_cells_but_keeps_saved_values() {
    let session = RSession::new_for_gc_tests();
    let (source, cars, tails) = fixture(&session, SEXPTYPE::LANGSXP);
    let factory = source.node_factory().unwrap();
    let replacement = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(999) })
        .unwrap();
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let replacement_link = factory.link(&replacement).unwrap();
    let nil_link = factory.link(&factory.nil()).unwrap();
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(observed.get() + 1) != 0 {
            return;
        }
        unsafe {
            let owner = crate::sexp::owner::OwnerToken::current().unwrap();
            (*owner.as_ptr()).memory_state.gc_force_gap = 0;
        }
        let mut header = heap.node_snapshot(&source_node).unwrap();
        header.data.list_mut().carval = replacement_link;
        header.data.list_mut().cdrval = nil_link;
        header.data.list_mut().tagval = nil_link;
        header.attrib = nil_link;
        header.sxpinfo.set_obj(false);
        header.sxpinfo.set_gp(0);
        heap.replace_node(&source_node, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(
            tails.iter().all(|cell| !cell.is_live()),
            "detached source tail cells should be reclaimed"
        );
        assert!(
            cars.iter().all(CheckedNode::is_live),
            "the saved values need their own physical leases before callbacks"
        );
    }));
    session.with_active_in(|owner| unsafe {
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    });
    let protect_count = crate::sexp::protect::R_ProtectCount();
    let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(notifications.get() > 0);
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protect_count);
    assert_eq!(copy.typeof_(), SEXPTYPE::LANGSXP);
    let mut cell = copy;
    for index in 0..3 {
        assert_eq!(
            cell.try_car().unwrap().try_integer_elt(0).unwrap(),
            10 + index
        );
        assert!(
            cell.try_tag_name_eq(
                [b"alpha".as_slice(), b"beta".as_slice(), b"gamma".as_slice()][index as usize]
            )
            .unwrap()
        );
        assert_eq!(
            cell.try_attrib()
                .unwrap()
                .try_car()
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            70 + index
        );
        unsafe {
            assert_eq!(OBJECT(cell.as_raw()), 1);
            assert_eq!(IS_S4_OBJECT(cell.as_raw()), 1);
        }
        cell = cell.try_cdr().unwrap();
    }
    assert!(cell.is_nil());
    drop(replacement);
}

#[test]
fn owning_pairlist_rejects_reused_tail_generation_before_reading_payload() {
    let session = RSession::new_for_gc_tests();
    let (source, _, _) = fixture(&session, SEXPTYPE::LISTSXP);
    let tail = source.try_cdr().unwrap();
    let stale_node = tail.allocation().unwrap().clone();
    let old_link = stale_node.link().unwrap();
    let pointer = tail.as_raw();
    // This deliberate allocator-seam violation leaves the parent's checked
    // edge pointing at its old generation. Ordinary GC is covered separately.
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.free_node(pointer));
    });
    let reused = session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_node(SEXPTYPE::REALSXP))
    });
    assert_eq!(pointer, reused, "exercise actual physical address reuse");
    let replacement = session.sexp(reused).unwrap();
    assert!(replacement.is_live());
    assert!(!stale_node.is_live());
    assert_ne!(replacement.allocation().unwrap().link().unwrap(), old_link);
    let before = crate::sexp::protect::R_ProtectCount();
    assert!(matches!(
        snapshot_pairlist(&source),
        Err(SexpError::StaleAllocation)
    ));
    typed_failure(
        || unsafe {
            duplicate(source.as_raw());
        },
        "has been reclaimed",
    );
    assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    // Restore the intentionally invalid edge before the fixture can collect.
    let factory = source.node_factory().unwrap();
    unsafe {
        SETCDR(source.as_raw(), factory.nil().as_raw());
    }
    let recovered = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert_eq!(recovered.try_car().unwrap().try_integer_elt(0).unwrap(), 10);
    assert!(recovered.try_cdr().unwrap().is_nil());
}

struct CollectingPairlistValue {
    value: i32,
    reads: Rc<RefCell<Vec<i32>>>,
}
impl crate::sexp::altrep::AltrepClass for CollectingPairlistValue {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &crate::sexp::altrep::AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        context: &crate::sexp::altrep::AltrepContext<'s>,
        _: i64,
    ) -> SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
        self.reads.borrow_mut().push(self.value);
        context.gc()?;
        Ok(crate::sexp::altrep::AltrepElement::Integer(self.value))
    }
}

#[test]
fn owning_pairlist_collecting_providers_keep_source_order_after_detachment() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let reads = Rc::new(RefCell::new(Vec::new()));
    let mut source = factory.nil();
    for value in (10..13).rev() {
        let class = session
            .register_altrep_class(
                &format!("pairlist-{value}"),
                CollectingPairlistValue {
                    value,
                    reads: reads.clone(),
                },
            )
            .unwrap();
        let lazy = crate::sexp::altrep::AltrepBuilder::new(class)
            .build()
            .unwrap();
        source = factory
            .pairlist_cell(&lazy, &source, &factory.nil())
            .unwrap();
    }
    let shallow = factory
        .wrap(unsafe { shallow_duplicate(source.as_raw()) })
        .unwrap();
    assert!(
        reads.borrow().is_empty(),
        "shallow copying must not evaluate providers"
    );
    assert_eq!(shallow.try_car().unwrap(), source.try_car().unwrap());
    drop(shallow);
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(observed.get() + 1) == 0 {
            let mut header = heap.node_snapshot(&source_node).unwrap();
            header.data.list_mut().cdrval = nil;
            heap.replace_node(&source_node, header).unwrap();
            crate::sexp::gengc::full_gc();
        }
    }));
    let before = crate::sexp::protect::R_ProtectCount();
    let copied = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(
        notifications.get() >= 3,
        "each provider performs actual collection"
    );
    assert_eq!(*reads.borrow(), [10, 11, 12]);
    assert_values(&copied);
    assert!(source.try_cdr().unwrap().is_nil());
    assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
}
fn sized_fixture(session: &RSession, count: i32) -> (Sexp<'_>, Vec<CheckedNode>, Vec<CheckedNode>) {
    let factory = session.owner_token().unwrap().node_factory();
    let mut source = factory.nil();
    let mut values = Vec::new();
    let mut tails = Vec::new();
    for value in (0..count).rev() {
        let scalar = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(value) })
            .unwrap();
        values.push(scalar.allocation().unwrap().clone());
        source = factory
            .pairlist_cell(&scalar, &source, &factory.nil())
            .unwrap();
        if value != 0 {
            tails.push(source.allocation().unwrap().clone());
        }
    }
    (source, values, tails)
}

fn assert_sized_values(copy: &Sexp<'_>, count: i32) {
    let mut cell = copy.clone();
    for value in 0..count {
        assert_eq!(cell.try_car().unwrap().try_integer_elt(0).unwrap(), value);
        cell = cell.try_cdr().unwrap();
    }
    assert!(cell.is_nil());
}

#[test]
fn owning_pairlist_thirty_two_detached_cells_preserve_saved_graph() {
    let session = RSession::new_for_gc_tests();
    let (source, values, tails) = sized_fixture(&session, 32);
    let factory = source.node_factory().unwrap();
    let replacement = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(999) })
        .unwrap();
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let replacement_link = factory.link(&replacement).unwrap();
    let nil = factory.link(&factory.nil()).unwrap();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        unsafe {
            (*crate::sexp::owner::OwnerToken::current().unwrap().as_ptr())
                .memory_state
                .gc_force_gap = 0;
        }
        let mut header = heap.node_snapshot(&source_node).unwrap();
        header.data.list_mut().carval = replacement_link;
        header.data.list_mut().cdrval = nil;
        heap.replace_node(&source_node, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert_eq!(tails.len(), 31);
        assert!(tails.iter().all(|cell| !cell.is_live()));
        assert_eq!(values.len(), 32);
        assert!(values.iter().all(CheckedNode::is_live));
    }));
    force_next_gc(&session);
    let before = crate::sexp::protect::R_ProtectCount();
    let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(notified.get());
    assert_sized_values(&copy, 32);
    assert!(source.try_cdr().unwrap().is_nil());
    assert_eq!(source.try_car().unwrap().try_integer_elt(0).unwrap(), 999);
    assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
}

#[test]
fn owning_pairlist_thirty_two_and_sixty_four_allocate_exactly_linear_nodes() {
    let session = RSession::new_for_gc_tests();
    for count in [32, 64] {
        let (source, _, _) = sized_fixture(&session, count);
        let factory = source.node_factory().unwrap();
        // Header snapshots and owning leases allocate Rust bookkeeping only.
        assert_eq!(snapshot_pairlist(&source).unwrap().len(), count as usize);
        for deep in [false, true] {
            let before = session
                .owner_token()
                .unwrap()
                .with_arena(|arena| arena.node_count())
                .unwrap();
            let copied = factory
                .wrap(unsafe {
                    if deep {
                        duplicate(source.as_raw())
                    } else {
                        shallow_duplicate(source.as_raw())
                    }
                })
                .unwrap();
            let after = session
                .owner_token()
                .unwrap()
                .with_arena(|arena| arena.node_count())
                .unwrap();
            assert_eq!(after - before, count as usize * if deep { 2 } else { 1 });
            assert_sized_values(&copied, count);
            assert_eq!(
                copied.try_car().unwrap() == source.try_car().unwrap(),
                !deep
            );
        }
    }
}
