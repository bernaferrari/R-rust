//! Real duplication callbacks must observe owned original edges, not incidental roots.

use super::*;
use crate::sexp::{heap::CheckedNode, object::SexpMut, session::RSession};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn force_next_gc(session: &RSession) {
    session.with_active_in(|owner| unsafe {
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    });
}

fn disable_forced_gc() {
    unsafe {
        (*crate::sexp::owner::OwnerToken::current().unwrap().as_ptr())
            .memory_state
            .gc_force_gap = 0;
    }
}

#[test]
fn owning_graph_vector_snapshot_retains_detached_slots_and_attributes() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 3) })
        .unwrap();
    let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
    let mut children: Vec<CheckedNode> = Vec::new();
    for index in 0..3 {
        let child = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(31 + index) })
            .unwrap();
        children.push(child.allocation().unwrap().clone());
        mutation.try_set_vector_elt(index as _, child).unwrap();
    }
    drop(mutation);
    let attribute_value = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(71) })
        .unwrap();
    let attribute_node = attribute_value.allocation().unwrap().clone();
    let attributes = factory
        .pairlist_cell(&attribute_value, &factory.nil(), &factory.nil())
        .unwrap();
    unsafe {
        SET_ATTRIB(source.as_raw(), attributes.as_raw());
    }
    drop(attributes);
    drop(attribute_value);
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        disable_forced_gc();
        let payload = heap.payload_lease(&source_node).unwrap();
        for index in 0..3 {
            payload.set_reference_elt(index, nil).unwrap();
        }
        let mut header = heap.node_snapshot(&source_node).unwrap();
        header.attrib = nil;
        heap.replace_node(&source_node, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(
            children.iter().all(CheckedNode::is_live),
            "saved vector entries need actual roots before callbacks"
        );
        assert!(
            attribute_node.is_live(),
            "saved attribute values need actual roots before callbacks"
        );
    }));
    force_next_gc(&session);
    let protection = crate::sexp::protect::R_ProtectCount();
    let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(notified.get());
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection);
    for index in 0..3 {
        assert_eq!(
            copy.try_vector_elt(index)
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            31 + index as i32
        );
        assert!(source.try_vector_elt(index).unwrap().is_nil());
    }
    assert_eq!(
        copy.try_attrib()
            .unwrap()
            .try_car()
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        71
    );
    assert!(source.try_attrib().unwrap().is_nil());
}

#[test]
fn owning_graph_atomic_snapshot_retains_original_attributes_and_values() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::INTSXP, 3) })
        .unwrap();
    let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
    for index in 0..3 {
        mutation
            .try_set_integer_elt(index, 41 + index as i32)
            .unwrap();
    }
    drop(mutation);
    let value = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(81) })
        .unwrap();
    let value_node = value.allocation().unwrap().clone();
    let attributes = factory
        .pairlist_cell(&value, &factory.nil(), &factory.nil())
        .unwrap();
    unsafe {
        SET_ATTRIB(source.as_raw(), attributes.as_raw());
    }
    drop(attributes);
    drop(value);
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        disable_forced_gc();
        let payload = heap.payload_lease(&source_node).unwrap();
        for index in 0..3 {
            payload.set_integer_elt(index, 999).unwrap();
        }
        let mut header = heap.node_snapshot(&source_node).unwrap();
        header.attrib = nil;
        heap.replace_node(&source_node, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(
            value_node.is_live(),
            "original attributes must remain owned through data allocation"
        );
    }));
    force_next_gc(&session);
    let protection = crate::sexp::protect::R_ProtectCount();
    let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(notified.get());
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection);
    for index in 0..3 {
        assert_eq!(copy.try_integer_elt(index).unwrap(), 41 + index as i32);
    }
    assert_eq!(
        copy.try_attrib()
            .unwrap()
            .try_car()
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        81
    );
}

#[test]
fn owning_graph_attribute_spine_snapshot_survives_car_and_tail_replacement() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::RAWSXP, 1) })
        .unwrap();
    let mut attributes = factory.nil();
    let mut values = Vec::new();
    let mut tail_cells = Vec::new();
    for index in (0..3).rev() {
        let value = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(91 + index) })
            .unwrap();
        values.push(value.allocation().unwrap().clone());
        let tag = factory
            .wrap(unsafe {
                crate::sexp::symbol::Rf_install([c"one", c"two", c"three"][index as usize].as_ptr())
            })
            .unwrap();
        attributes = factory.pairlist_cell(&value, &attributes, &tag).unwrap();
        if index != 0 {
            tail_cells.push(attributes.allocation().unwrap().clone());
        }
    }
    unsafe {
        SET_ATTRIB(source.as_raw(), attributes.as_raw());
    }
    let attribute_head = attributes.allocation().unwrap().clone();
    drop(attributes);
    let heap = attribute_head.heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        disable_forced_gc();
        let mut header = heap.node_snapshot(&attribute_head).unwrap();
        header.data.list_mut().carval = nil;
        header.data.list_mut().cdrval = nil;
        header.data.list_mut().tagval = nil;
        heap.replace_node(&attribute_head, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(
            tail_cells.iter().all(|cell| !cell.is_live()),
            "snapshot metadata does not root detached source tail cells"
        );
        assert!(
            values.iter().all(CheckedNode::is_live),
            "saved original attribute children are independently rooted"
        );
    }));
    force_next_gc(&session);
    let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(notified.get());
    let mut cell = copy.try_attrib().unwrap();
    for index in 0..3 {
        assert_eq!(
            cell.try_car().unwrap().try_integer_elt(0).unwrap(),
            91 + index
        );
        assert!(
            cell.try_tag_name_eq(
                [b"one".as_slice(), b"two".as_slice(), b"three".as_slice()][index as usize]
            )
            .unwrap()
        );
        cell = cell.try_cdr().unwrap();
    }
    assert!(cell.is_nil());
}

#[test]
fn owning_graph_strings_snapshot_keeps_original_uninterned_children() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 3) })
        .unwrap();
    let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
    let mut children = Vec::new();
    for (index, text) in ["old-a", "old-b", "old-c"].iter().enumerate() {
        let child = factory.character(text).unwrap();
        children.push(child.allocation().unwrap().clone());
        mutation.try_set_string_elt(index as _, child).unwrap();
    }
    drop(mutation);
    let replacement = factory.character("replacement").unwrap();
    let replacement_link = factory.link(&replacement).unwrap();
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        disable_forced_gc();
        let payload = heap.payload_lease(&source_node).unwrap();
        for index in 0..3 {
            payload.set_reference_elt(index, replacement_link).unwrap();
        }
        crate::sexp::gengc::full_gc();
        assert!(children.iter().all(CheckedNode::is_live));
    }));
    force_next_gc(&session);
    let copy = factory
        .wrap(unsafe { shallow_duplicate(source.as_raw()) })
        .unwrap();
    assert!(notified.get());
    for (index, text) in ["old-a", "old-b", "old-c"].iter().enumerate() {
        assert_eq!(
            copy.try_string_value_elt(index as _).unwrap().as_deref(),
            Some(*text)
        );
        assert_eq!(
            source.try_string_value_elt(index as _).unwrap().as_deref(),
            Some("replacement")
        );
    }
}

#[test]
fn owning_graph_logical_singletons_produce_mutable_copies() {
    // Pinned GNU exported R_TrueValue/R_FalseValue/R_LogicalNAValue: both
    // duplicate modes produce distinct mutable logical vectors (6 assertions).
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    for expected in [true, false] {
        let source = factory.domain().logical(expected);
        for deep in [true, false] {
            let copy = factory
                .wrap(unsafe {
                    if deep {
                        duplicate(source.as_raw())
                    } else {
                        shallow_duplicate(source.as_raw())
                    }
                })
                .unwrap();
            assert_ne!(copy, source);
            assert_eq!(copy.try_logical_elt(0).unwrap(), i32::from(expected));
            let mut mutation = SexpMut::try_from_checked(copy).unwrap();
            mutation
                .try_set_logical_elt(0, i32::from(!expected))
                .unwrap();
            assert_eq!(source.try_logical_elt(0).unwrap(), i32::from(expected));
        }
    }
    let nil = factory.nil();
    assert_eq!(unsafe { duplicate(nil.as_raw()) }, nil.as_raw());
}

#[test]
fn owning_graph_scalar_types_preserve_na_bits_attributes_and_metadata() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
    ] {
        let source = factory.wrap(unsafe { Rf_allocVector3(kind, 2) }).unwrap();
        let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
        match kind {
            SEXPTYPE::LGLSXP => {
                mutation.try_set_logical_elt(0, 1).unwrap();
                mutation.try_set_logical_elt(1, i32::MIN).unwrap();
            }
            SEXPTYPE::INTSXP => {
                mutation.try_set_integer_elt(0, 42).unwrap();
                mutation.try_set_integer_elt(1, i32::MIN).unwrap();
            }
            SEXPTYPE::REALSXP => {
                mutation.try_set_real_elt(0, -0.0).unwrap();
                mutation
                    .try_set_real_elt(1, f64::from_bits(0x7ff00000000007a2))
                    .unwrap();
            }
            SEXPTYPE::CPLXSXP => {
                mutation
                    .try_set_complex_elt(0, Rcomplex { r: -0.0, i: 2.5 })
                    .unwrap();
                mutation
                    .try_set_complex_elt(
                        1,
                        Rcomplex {
                            r: f64::from_bits(0x7ff00000000007a2),
                            i: f64::INFINITY,
                        },
                    )
                    .unwrap();
            }
            SEXPTYPE::RAWSXP => {
                mutation.try_set_raw_elt(0, 0).unwrap();
                mutation.try_set_raw_elt(1, 255).unwrap();
            }
            _ => unreachable!(),
        }
        drop(mutation);
        let child = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(61) })
            .unwrap();
        let attributes = factory
            .pairlist_cell(&child, &factory.nil(), &factory.nil())
            .unwrap();
        unsafe {
            SET_ATTRIB(source.as_raw(), attributes.as_raw());
            SET_OBJECT(source.as_raw(), 1);
            SET_S4_OBJECT(source.as_raw());
            SET_TRUELENGTH(source.as_raw(), 77);
        }
        for deep in [true, false] {
            let copy = factory
                .wrap(unsafe {
                    if deep {
                        duplicate(source.as_raw())
                    } else {
                        shallow_duplicate(source.as_raw())
                    }
                })
                .unwrap();
            assert_ne!(copy, source);
            assert_eq!(copy.typeof_(), kind);
            match kind {
                SEXPTYPE::LGLSXP => {
                    assert_eq!(copy.try_logical_elt(0).unwrap(), 1);
                    assert_eq!(copy.try_logical_elt(1).unwrap(), i32::MIN);
                }
                SEXPTYPE::INTSXP => {
                    assert_eq!(copy.try_integer_elt(0).unwrap(), 42);
                    assert_eq!(copy.try_integer_elt(1).unwrap(), i32::MIN);
                }
                SEXPTYPE::REALSXP => {
                    for index in 0..2 {
                        assert_eq!(
                            copy.try_real_elt(index).unwrap().to_bits(),
                            source.try_real_elt(index).unwrap().to_bits()
                        );
                    }
                }
                SEXPTYPE::CPLXSXP => {
                    for index in 0..2 {
                        let copied = copy.try_complex_elt(index).unwrap();
                        let original = source.try_complex_elt(index).unwrap();
                        assert_eq!(copied.r.to_bits(), original.r.to_bits());
                        assert_eq!(copied.i.to_bits(), original.i.to_bits());
                    }
                }
                SEXPTYPE::RAWSXP => {
                    assert_eq!(copy.try_raw_elt(0).unwrap(), 0);
                    assert_eq!(copy.try_raw_elt(1).unwrap(), 255);
                }
                _ => unreachable!(),
            }
            let copied_attributes = copy.try_attrib().unwrap();
            assert_ne!(copied_attributes, attributes);
            assert_eq!(copied_attributes.try_car().unwrap() == child, !deep);
            unsafe {
                assert_eq!(OBJECT(copy.as_raw()), 1);
                assert_eq!(IS_S4_OBJECT(copy.as_raw()), 1);
                assert_eq!(TRUELENGTH(copy.as_raw()), 77);
            }
        }
    }
}

#[test]
fn owning_graph_vector_copies_repeated_children_per_occurrence_and_keeps_identity_kinds() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let child = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(52) })
        .unwrap();
    let environment = session.global_env().unwrap().into_owned().unwrap();
    for kind in [SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP] {
        let source = factory.wrap(unsafe { Rf_allocVector3(kind, 3) }).unwrap();
        let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
        mutation.try_set_vector_elt(0, child.clone()).unwrap();
        mutation.try_set_vector_elt(1, child.clone()).unwrap();
        mutation.try_set_vector_elt(2, environment.clone()).unwrap();
        drop(mutation);
        let deep = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
        assert_ne!(deep.try_vector_elt(0).unwrap(), child);
        assert_ne!(
            deep.try_vector_elt(0).unwrap(),
            deep.try_vector_elt(1).unwrap()
        );
        assert_eq!(deep.try_vector_elt(2).unwrap(), environment);
        unsafe {
            SET_NAMED(child.as_raw(), 7);
        }
        let original_named = unsafe { NAMED(child.as_raw()) };
        assert!(original_named > 2);
        let shallow = factory
            .wrap(unsafe { shallow_duplicate(source.as_raw()) })
            .unwrap();
        assert_eq!(shallow.try_vector_elt(0).unwrap(), child);
        assert_eq!(shallow.try_vector_elt(1).unwrap(), child);
        assert_eq!(shallow.try_vector_elt(2).unwrap(), environment);
        unsafe {
            assert_eq!(NAMED(child.as_raw()), original_named);
        }
    }
}

struct CollectingValues {
    kind: SEXPTYPE,
    created: Rc<RefCell<Vec<CheckedNode>>>,
}
impl crate::sexp::altrep::AltrepClass for CollectingValues {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &crate::sexp::altrep::AltrepContext<'_>) -> SexpResult<i64> {
        Ok(3)
    }
    fn element<'source>(
        &self,
        context: &crate::sexp::altrep::AltrepContext<'source>,
        index: i64,
    ) -> SexpResult<crate::sexp::altrep::AltrepElement<'source>> {
        context.gc()?;
        use crate::sexp::altrep::AltrepElement;
        match self.kind {
            SEXPTYPE::STRSXP => {
                let value = context.string(&format!("fresh-{index}"))?;
                self.created.borrow_mut().push(value.allocation()?.clone());
                Ok(AltrepElement::String(value))
            }
            SEXPTYPE::VECSXP => {
                let value = context.alloc_vector(SEXPTYPE::INTSXP, 1)?;
                let mut mutation = SexpMut::try_from_checked(value)?;
                mutation.try_set_integer_elt(0, 101 + index as i32)?;
                let value = mutation.freeze();
                self.created.borrow_mut().push(value.allocation()?.clone());
                Ok(AltrepElement::List(value))
            }
            SEXPTYPE::REALSXP => Ok(AltrepElement::Real(0.5 + index as f64)),
            _ => unreachable!(),
        }
    }
}

#[test]
fn owning_graph_altrep_callbacks_keep_fresh_values_and_original_public_attributes() {
    for kind in [SEXPTYPE::STRSXP, SEXPTYPE::VECSXP, SEXPTYPE::REALSXP] {
        let session = RSession::new_for_gc_tests();
        let factory = session.owner_token().unwrap().node_factory();
        let created = Rc::new(RefCell::new(Vec::new()));
        let class = session
            .register_altrep_class(
                "collecting-copy",
                CollectingValues {
                    kind,
                    created: created.clone(),
                },
            )
            .unwrap();
        let source = crate::sexp::altrep::AltrepBuilder::new(class)
            .build()
            .unwrap();
        let value = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(89) })
            .unwrap();
        let value_node = value.allocation().unwrap().clone();
        let attributes = factory
            .pairlist_cell(&value, &factory.nil(), &factory.nil())
            .unwrap();
        let attribute_node = attributes.allocation().unwrap().clone();
        let prefix = source.try_attrib().unwrap();
        unsafe {
            SETCDR(prefix.as_raw(), attributes.as_raw());
        }
        let prefix_node = prefix.allocation().unwrap().clone();
        drop(prefix);
        drop(attributes);
        drop(value);
        let heap = prefix_node.heap_identity();
        let nil = factory.link(&factory.nil()).unwrap();
        let notified = Rc::new(Cell::new(false));
        let observed = notified.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if observed.replace(true) {
                return;
            }
            let mut prefix = heap.node_snapshot(&prefix_node).unwrap();
            prefix.data.list_mut().cdrval = nil;
            heap.replace_node(&prefix_node, prefix).unwrap();
            let mut attribute = heap.node_snapshot(&attribute_node).unwrap();
            attribute.data.list_mut().carval = nil;
            heap.replace_node(&attribute_node, attribute).unwrap();
            crate::sexp::gengc::full_gc();
            assert!(
                value_node.is_live(),
                "public attribute children are owned before provider callbacks"
            );
        }));
        let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
        assert!(notified.get());
        assert!(!copy.header().sxpinfo.alt());
        assert!(!unsafe { crate::sexp::altrep::has_extension_raw(copy.as_raw()) });
        assert_eq!(
            copy.try_attrib()
                .unwrap()
                .try_car()
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            89
        );
        assert!(!crate::sexp::altrep::is_materialized(&source));
        for index in 0..3 {
            match kind {
                SEXPTYPE::STRSXP => assert_eq!(
                    copy.try_string_value_elt(index).unwrap(),
                    Some(format!("fresh-{index}"))
                ),
                SEXPTYPE::VECSXP => assert_eq!(
                    copy.try_vector_elt(index)
                        .unwrap()
                        .try_integer_elt(0)
                        .unwrap(),
                    101 + index as i32
                ),
                SEXPTYPE::REALSXP => {
                    assert_eq!(copy.try_real_elt(index).unwrap(), 0.5 + index as f64)
                }
                _ => unreachable!(),
            }
        }
        crate::sexp::gengc::full_gc();
        if kind == SEXPTYPE::STRSXP {
            assert!(created.borrow().iter().all(CheckedNode::is_live));
        }
        drop(copy);
        crate::sexp::gengc::full_gc();
        assert!(
            created.borrow().iter().all(|node| !node.is_live()),
            "safe provider reads leave no hidden parent roots after the copy dies"
        );
    }
}

#[test]
fn owning_graph_traced_public_copy_retains_sole_source_and_original_flags() {
    let session = RSession::new_for_gc_tests();
    let source = session
        .owner_token()
        .unwrap()
        .node_factory()
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::REALSXP, 1) })
        .unwrap()
        .into_owned()
        .unwrap();
    let factory = source.node_factory().unwrap();
    let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
    mutation.try_set_real_elt(0, 5.5).unwrap();
    drop(mutation);
    unsafe {
        SET_OBJECT(source.as_raw(), 1);
        SET_S4_OBJECT(source.as_raw());
        SET_RTRACE(source.as_raw(), 1);
    }
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let token = source_node.clone();
    let source_holder = Rc::new(RefCell::new(Some(source)));
    let holder = source_holder.clone();
    let raw = source_holder.borrow().as_ref().unwrap().as_raw();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        disable_forced_gc();
        drop(holder.borrow_mut().take());
        let mut header = heap.node_snapshot(&token).unwrap();
        header.sxpinfo.set_obj(false);
        header.sxpinfo.set_gp(0);
        header.sxpinfo.set_trace(false);
        heap.replace_node(&token, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(
            token.is_live(),
            "original source has an operation lease after its sole external root dies"
        );
    }));
    let capture = crate::sexp::output::OutputCaptureGuard::start();
    force_next_gc(&session);
    let copy = factory.wrap(unsafe { duplicate(raw) }).unwrap();
    let output = capture.finish();
    assert!(notified.get());
    assert!(source_holder.borrow().is_none());
    assert_eq!(copy.try_real_elt(0).unwrap(), 5.5);
    unsafe {
        assert_eq!(OBJECT(copy.as_raw()), 1);
        assert_eq!(IS_S4_OBJECT(copy.as_raw()), 1);
        assert_eq!(RTRACE(copy.as_raw()), 1);
    }
    if cfg!(feature = "memory-profiling") {
        assert!(
            output.stdout.starts_with("tracemem["),
            "real GNU-style trace was emitted: {output:?}"
        );
    }
    crate::sexp::gengc::full_gc();
    assert!(!source_node.is_live());
}

#[test]
fn owning_graph_live_callback_panic_preserves_exact_payload_and_cleanup() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) })
        .unwrap();
    let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
    mutation
        .try_set_string_elt(0, factory.character("before").unwrap())
        .unwrap();
    drop(mutation);
    crate::sexp::gengc::register_gc_callback(Box::new(|_| std::panic::panic_any(917_u32)));
    force_next_gc(&session);
    let protection = crate::sexp::protect::R_ProtectCount();
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        duplicate(source.as_raw())
    }))
    .expect_err("the callback panics");
    assert_eq!(payload.downcast_ref::<u32>(), Some(&917));
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection);
    session.with_active_in(|owner| unsafe {
        (*owner).gc_state.callbacks.clear();
        (*owner).memory_state.gc_force_gap = 0;
        assert!(!(*owner).gc_state.in_progress);
    });
    let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert_eq!(
        copy.try_string_value_elt(0).unwrap().as_deref(),
        Some("before")
    );
}

#[test]
fn owning_graph_revoked_original_cannot_publish_copy_or_adopt_replacement() {
    let session = RSession::new_for_gc_tests();
    let source = session
        .owner_token()
        .unwrap()
        .node_factory()
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
        .unwrap()
        .into_owned()
        .unwrap();
    let original = source.runtime_owner.as_ref().unwrap().clone();
    let holder = Rc::new(RefCell::new(Some(session)));
    let callback_holder = holder.clone();
    let replacement = Rc::new(RefCell::new(None));
    let installed = replacement.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if callback_holder.borrow().is_none() {
            return;
        }
        drop(callback_holder.borrow_mut().take());
        *installed.borrow_mut() = Some(RSession::new_for_gc_tests());
    }));
    force_next_gc(holder.borrow().as_ref().unwrap());
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        duplicate(source.as_raw())
    }))
    .expect_err("original owner is revoked");
    let error = payload
        .downcast_ref::<crate::sexp::context::RError>()
        .expect("revocation is a typed R error");
    assert!(
        error.message.contains("owner could not retain its root"),
        "{}",
        error.message
    );
    assert!(holder.borrow().is_none());
    assert!(original.pin().is_err());
    let replacement = replacement.borrow();
    let replacement = replacement.as_ref().unwrap();
    let factory = replacement.owner_token().unwrap().node_factory();
    assert!(factory.wrap(source.as_raw()).is_err());
    replacement.with_active(|| {
        let input = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(13) })
            .unwrap();
        assert_eq!(
            factory
                .wrap(unsafe { duplicate(input.as_raw()) })
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            13
        );
    });
}

#[test]
fn owning_graph_empty_vectors_and_invalid_string_edges_are_checked_before_copy_allocation() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
        SEXPTYPE::STRSXP,
        SEXPTYPE::VECSXP,
        SEXPTYPE::EXPRSXP,
    ] {
        let source = factory.wrap(unsafe { Rf_allocVector3(kind, 0) }).unwrap();
        let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
        assert_eq!(copy.typeof_(), kind);
        assert_eq!(copy.len(), 0);
        assert_ne!(copy, source);
    }
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) })
        .unwrap();
    let invalid_child = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(41) })
        .unwrap();
    let node = source.allocation().unwrap();
    node.heap_identity()
        .payload_lease(node)
        .unwrap()
        .set_reference_elt(0, factory.link(&invalid_child).unwrap())
        .unwrap();
    let before = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| arena.node_count())
        .unwrap();
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        duplicate(source.as_raw())
    }))
    .expect_err("a malformed string edge is rejected");
    let error = payload
        .downcast_ref::<crate::sexp::context::RError>()
        .expect("failure is typed");
    assert!(
        error.message.contains("character element"),
        "{}",
        error.message
    );
    assert_eq!(
        session
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.node_count())
            .unwrap(),
        before
    );
}
